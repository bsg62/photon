use super::{Priority, ThumbCache, ThumbQueue, ThumbSize};
use crate::{Error, Result, library::Library, media::ThumbState};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

/// Worker threads for thumbnail generation: all cores but one, so the UI stays responsive.
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .max(1)
}

pub struct ThumbService {
    lib: Arc<Library>,
    cache: Arc<ThumbCache>,
    queue: Arc<ThumbQueue>,
    workers: Vec<JoinHandle<()>>,
}

/// Guarantees `queue.done()` runs exactly once per popped job, even if the job panics.
struct DoneGuard<'a>(&'a ThumbQueue);

impl Drop for DoneGuard<'_> {
    fn drop(&mut self) {
        self.0.done();
    }
}

impl ThumbService {
    pub fn start(lib: Arc<Library>, cache: Arc<ThumbCache>, workers: usize) -> Self {
        let queue = Arc::new(ThumbQueue::new());
        let workers = (0..workers.max(1))
            .map(|i| {
                let (lib, cache, queue) = (lib.clone(), cache.clone(), queue.clone());
                std::thread::Builder::new()
                    .name(format!("photon-thumb-{i}"))
                    .spawn(move || {
                        while let Some(id) = queue.pop_blocking() {
                            let _guard = DoneGuard(&queue);
                            if let Err(err) = process(&lib, &cache, id) {
                                tracing::warn!(id, %err, "thumbnail job failed");
                            }
                        }
                    })
                    .expect("failed to spawn thumbnail worker")
            })
            .collect();
        Self {
            lib,
            cache,
            queue,
            workers,
        }
    }

    /// Queues every item still waiting for thumbnails at background priority.
    pub fn enqueue_pending(&self) -> Result<usize> {
        let ids = self.lib.pending_thumb_ids()?;
        self.queue.push_many(&ids, Priority::Background);
        Ok(ids.len())
    }

    pub fn set_visible(&self, ids: &[i64]) {
        self.queue.set_visible(ids);
    }

    pub fn prioritize(&self, ids: &[i64], priority: Priority) {
        self.queue.push_many(ids, priority);
    }

    /// Returns the cached thumbnail, generating it on the calling thread if needed.
    pub fn get_or_generate(&self, id: i64, size: ThumbSize) -> Result<PathBuf> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.fingerprint(), size);
        if path.is_file() {
            return Ok(path);
        }
        process(&self.lib, &self.cache, id)?;
        if path.is_file() {
            return Ok(path);
        }
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        Err(Error::ThumbFailed(
            item.thumb_error
                .unwrap_or_else(|| "thumbnail unavailable".into()),
        ))
    }

    pub fn wait_idle(&self) {
        self.queue.wait_idle();
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn collect_garbage(&self) -> Result<usize> {
        let live = self.lib.live_fingerprints()?;
        self.cache.collect_garbage(&live)
    }
}

impl Drop for ThumbService {
    fn drop(&mut self) {
        self.queue.close();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Generates thumbnails for one item.
///
/// A decode (render) failure means the source file itself is bad: it's recorded as
/// `Failed` on the item, not returned, so the worker doesn't retry it. A cache write
/// (store) failure means the destination is unwritable (full disk, permissions): it's
/// propagated as `Err` and the item is left `Pending` so it's retried later.
///
/// State writes go through `set_thumb_state_if_unchanged` so a rescan that replaces this
/// item mid-decode (resetting it to `Pending`) can't be clobbered by a stale result.
fn process(lib: &Library, cache: &ThumbCache, id: i64) -> Result<()> {
    let Some(item) = lib.item(id)? else {
        return Ok(());
    };
    if item.missing_since.is_some() || item.thumb_state == ThumbState::Failed {
        return Ok(());
    }
    let fp = item.fingerprint();
    if !cache.is_complete(fp) {
        match cache.render(Path::new(&item.path), item.orientation) {
            Ok((preview, grid)) => cache.store(fp, &preview, &grid)?,
            Err(err) => {
                lib.set_thumb_state_if_unchanged(
                    &item,
                    ThumbState::Failed,
                    Some(&err.to_string()),
                )?;
                return Ok(());
            }
        }
    }
    if item.thumb_state != ThumbState::Ready {
        lib.set_thumb_state_if_unchanged(&item, ThumbState::Ready, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::testutil::{jpeg_bytes, new_item, seed_folder, write_file};
    use tempfile::TempDir;

    fn setup(files: &[(&str, Vec<u8>)]) -> (TempDir, Arc<Library>, Arc<ThumbCache>, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
        let photos = dir.path().join("photos");
        let (_, folder) = seed_folder(&lib, &photos);
        let items: Vec<NewItem> = files
            .iter()
            .map(|(name, bytes)| {
                let path = write_file(&photos, name, bytes);
                new_item(folder, path.to_str().unwrap(), 0)
            })
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        let cache = Arc::new(ThumbCache::new(dir.path().join("cache")));
        (dir, lib, cache, ids)
    }

    fn state(lib: &Library, id: i64) -> ThumbState {
        lib.item(id).unwrap().unwrap().thumb_state
    }

    #[test]
    fn processes_pending_items_in_background() {
        let (_dir, lib, cache, ids) =
            setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(32, 64))]);
        let service = ThumbService::start(lib.clone(), cache.clone(), 2);
        assert_eq!(service.enqueue_pending().unwrap(), 2);
        service.wait_idle();
        for id in ids {
            assert_eq!(state(&lib, id), ThumbState::Ready);
            assert!(cache.is_complete(lib.item(id).unwrap().unwrap().fingerprint()));
        }
        assert!(lib.pending_thumb_ids().unwrap().is_empty());
    }

    #[test]
    fn records_failures_instead_of_retrying() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert!(item.thumb_error.is_some());
        assert_eq!(service.enqueue_pending().unwrap(), 0);
    }

    #[test]
    fn get_or_generate_builds_on_demand() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        let path = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert!(path.is_file());
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert_eq!(
            service
                .get_or_generate(ids[0], ThumbSize::Preview)
                .unwrap()
                .extension()
                .unwrap(),
            "webp"
        );
    }

    #[test]
    fn get_or_generate_reports_failures_and_unknown_items() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib, cache, 1);
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.get_or_generate(9_999, ThumbSize::Grid),
            Err(Error::NotFound(9_999))
        ));
    }

    #[test]
    fn store_failure_leaves_item_pending() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
        let photos = dir.path().join("photos");
        let (_, folder) = seed_folder(&lib, &photos);
        let path = write_file(&photos, "a.jpg", &jpeg_bytes(64, 32));
        let ids = lib
            .insert_items(&[new_item(folder, path.to_str().unwrap(), 0)])
            .unwrap();
        // The cache root is a regular file, so `create_dir_all` inside `store` fails: a
        // stand-in for a full disk or a permissions problem, distinct from a bad source file.
        let cache_root = dir.path().join("cache");
        std::fs::write(&cache_root, b"not a directory").unwrap();
        let cache = Arc::new(ThumbCache::new(cache_root));
        let service = ThumbService::start(lib.clone(), cache, 1);

        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
    }

    #[test]
    fn collect_garbage_removes_thumbnails_of_purged_items() {
        let (_dir, lib, cache, ids) =
            setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        lib.purge_items(&[ids[1]]).unwrap();
        assert_eq!(service.collect_garbage().unwrap(), 2);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_ok());
    }
}
