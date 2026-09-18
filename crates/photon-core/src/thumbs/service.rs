use super::{Priority, ThumbCache, ThumbQueue, ThumbSize};
use crate::{
    Error, Result,
    edit::Edit,
    library::{Item, Library},
    media::ThumbState,
};
use image::DynamicImage;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The most thumbnail workers photon will run, however many cores the machine has.
///
/// Each worker holds a whole decoded image while it downscales it, and `image`'s default
/// limit allows up to 512 MiB for one decode, so the peak memory of the thumbnail pool is
/// this number times that. Without a cap it grows with the core count: a 32-core machine
/// importing a folder of large scans or panoramas could put ~16 GiB of decode buffers in
/// flight at once. Eight workers saturate the disk on any machine photon runs on, so the
/// cap costs no throughput worth having.
const MAX_WORKERS: usize = 8;

/// Worker threads for thumbnail generation: all cores but one, so the UI stays responsive,
/// and never more than [`MAX_WORKERS`].
pub fn default_workers() -> usize {
    worker_count(std::thread::available_parallelism().map_or(1, |n| n.get()))
}

fn worker_count(parallelism: usize) -> usize {
    parallelism.saturating_sub(1).clamp(1, MAX_WORKERS)
}

pub struct ThumbService {
    lib: Arc<Library>,
    cache: Arc<ThumbCache>,
    queue: Arc<ThumbQueue>,
    workers: Vec<JoinHandle<()>>,
    render: RenderFn,
}

/// Decodes a source file into (preview, grid) images. A seam so tests can inject a
/// misbehaving decoder; production always uses [`ThumbCache::render`].
type RenderFn = fn(&ThumbCache, &Path, u8, Edit) -> Result<(DynamicImage, DynamicImage)>;

fn default_render(
    cache: &ThumbCache,
    source: &Path,
    orientation: u8,
    edit: Edit,
) -> Result<(DynamicImage, DynamicImage)> {
    cache.render(source, orientation, edit)
}

/// Guarantees `queue.done(id)` runs exactly once per popped job, even if the job panics.
struct DoneGuard<'a>(&'a ThumbQueue, i64);

impl Drop for DoneGuard<'_> {
    fn drop(&mut self) {
        self.0.done(self.1);
    }
}

impl ThumbService {
    pub fn start(lib: Arc<Library>, cache: Arc<ThumbCache>, workers: usize) -> Self {
        Self::start_with(lib, cache, workers, default_render)
    }

    fn start_with(
        lib: Arc<Library>,
        cache: Arc<ThumbCache>,
        workers: usize,
        render: RenderFn,
    ) -> Self {
        let queue = Arc::new(ThumbQueue::new());
        let workers = (0..workers.max(1))
            .map(|i| {
                let (lib, cache, queue) = (lib.clone(), cache.clone(), queue.clone());
                std::thread::Builder::new()
                    .name(format!("photon-thumb-{i}"))
                    .spawn(move || {
                        while let Some(id) = queue.pop_blocking() {
                            let _guard = DoneGuard(&queue, id);
                            if let Err(err) = process(&lib, &cache, id, render) {
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
            render,
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

    /// Closes the queue so every worker finishes its current job and then stops.
    /// Does not join the workers itself; `Drop` still does that.
    pub fn close(&self) {
        self.queue.close();
    }

    /// Returns the cached thumbnail, generating it on the calling thread if needed.
    ///
    /// Not the production path - `protocol.rs` uses `request`, which keeps decoding inside
    /// the worker pool - and today only the tests call this, as a synchronous harness around
    /// `process`. Kept public rather than gated to tests because `self.render` would
    /// otherwise be a field no non-test code reads; if a second caller ever appears, it
    /// should be `request` unless it genuinely wants to decode on its own thread.
    pub fn get_or_generate(&self, id: i64, size: ThumbSize) -> Result<PathBuf> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.thumb_key(), size);
        if path.is_file() {
            return Ok(path);
        }
        process(&self.lib, &self.cache, id, self.render)?;
        if path.is_file() {
            return Ok(path);
        }
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        Err(Error::ThumbFailed(
            item.thumb_error
                .unwrap_or_else(|| "thumbnail unavailable".into()),
        ))
    }

    /// Returns the cached thumbnail, or moves the item to the front of the queue and waits
    /// for a worker to build it. Concurrent requests for one item share the same decode,
    /// and CPU use stays within the worker pool. Used by the `photon://` protocol.
    pub fn request(&self, id: i64, size: ThumbSize, timeout: Duration) -> Result<PathBuf> {
        let deadline = Instant::now() + timeout;
        // Two rounds: the first may only wait out a job already running for an older
        // version of the file.
        for _ in 0..2 {
            if let Some(path) = self.cached(id, size)? {
                return Ok(path);
            }
            self.queue.push(id, Priority::Visible);
            if !self.queue.wait_for(id, deadline) {
                return Err(Error::ThumbTimeout(id));
            }
        }
        self.cached(id, size)?.ok_or(Error::ThumbUnavailable(id))
    }

    /// The cached thumbnail for `id` at `size`, or `None` if it has not been built yet.
    ///
    /// The checks `request` makes before and after waiting, in one place rather than two
    /// copies of four lines: `missing_since` was added to the copy inside the loop only, so
    /// an item that went missing *while* its thumbnail was being built reported
    /// `ThumbUnavailable` rather than `NotFound` - a 503 the tile would keep retrying
    /// instead of a 404 it can give up on.
    fn cached(&self, id: i64, size: ThumbSize) -> Result<Option<PathBuf>> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.thumb_key(), size);
        Ok(path.is_file().then_some(path))
    }

    pub fn wait_idle(&self) {
        self.queue.wait_idle();
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

const PANIC_MESSAGE: &str = "decoder panicked";

/// Generates thumbnails for one item.
///
/// A decode failure means the source file itself is bad: it's recorded as `Failed` on
/// the item, not returned, so the worker doesn't retry it. Anything else (the source
/// can't be opened because its drive is unplugged or it's being replaced, or the cache
/// is unwritable) is propagated as `Err` and the item is left `Pending` for a later retry.
///
/// A panicking decoder is contained: the item is recorded as `Failed` ("decoder
/// panicked") and `Err(ThumbFailed)` is returned, so neither a worker thread nor an
/// on-demand caller goes down with it.
///
/// State writes go through `set_thumb_state_if_unchanged` so a rescan that replaces this
/// item mid-decode (resetting it to `Pending`) can't be clobbered by a stale result.
fn process(lib: &Library, cache: &ThumbCache, id: i64, render: RenderFn) -> Result<()> {
    let Some(item) = lib.item(id)? else {
        return Ok(());
    };
    if item.missing_since.is_some() || item.thumb_state == ThumbState::Failed {
        return Ok(());
    }
    match catch_unwind(AssertUnwindSafe(|| process_item(lib, cache, &item, render))) {
        Ok(result) => result,
        Err(_) => {
            tracing::error!(id, path = %item.path, "thumbnail decoder panicked");
            lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(PANIC_MESSAGE))?;
            Err(Error::ThumbFailed(PANIC_MESSAGE.into()))
        }
    }
}

fn process_item(lib: &Library, cache: &ThumbCache, item: &Item, render: RenderFn) -> Result<()> {
    let fp = item.thumb_key();
    if !cache.is_complete(fp) {
        match render(cache, Path::new(&item.path), item.orientation, item.edit) {
            Ok((preview, grid)) => cache.store(fp, &preview, &grid)?,
            Err(err) if !is_source_defect(&err) => return Err(err),
            Err(err) => {
                lib.set_thumb_state_if_unchanged(item, ThumbState::Failed, Some(&err.to_string()))?;
                return Ok(());
            }
        }
    }
    if item.thumb_state != ThumbState::Ready {
        lib.set_thumb_state_if_unchanged(item, ThumbState::Ready, None)?;
    }
    Ok(())
}

/// Whether a render error means the source file itself is bad (so the item is `Failed`).
/// I/O errors, including those surfaced by the decoder, mean the file couldn't be read
/// right now (offline drive, permissions, mid-replace) and must not be recorded.
fn is_source_defect(err: &Error) -> bool {
    matches!(err, Error::Image(e) if !matches!(e, image::ImageError::IoError(_)))
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

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static COUNTED_RENDERS: AtomicUsize = AtomicUsize::new(0);

    fn counting_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        COUNTED_RENDERS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        cache.render(source, orientation, edit)
    }

    fn slow_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        std::thread::sleep(Duration::from_millis(500));
        cache.render(source, orientation, edit)
    }

    #[test]
    fn concurrent_requests_share_one_decode() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = Arc::new(ThumbService::start_with(lib, cache, 2, counting_render));
        let id = ids[0];
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let service = service.clone();
                std::thread::spawn(move || {
                    service.request(id, ThumbSize::Grid, Duration::from_secs(10))
                })
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().unwrap().is_file());
        }
        assert_eq!(COUNTED_RENDERS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn request_times_out_but_keeps_the_job() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, slow_render);
        assert!(matches!(
            service.request(ids[0], ThumbSize::Grid, Duration::from_millis(50)),
            Err(Error::ThumbTimeout(_))
        ));
        service.wait_idle();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
    }

    #[test]
    fn request_reports_failed_unknown_and_unreadable_items() {
        let (dir, lib, cache, ids) = setup(&[
            ("bad.jpg", b"garbage".to_vec()),
            ("gone.jpg", jpeg_bytes(8, 8)),
        ]);
        std::fs::remove_file(dir.path().join("photos").join("gone.jpg")).unwrap();
        let service = ThumbService::start(lib, cache, 1);
        let t = Duration::from_secs(10);
        assert!(matches!(
            service.request(ids[0], ThumbSize::Grid, t),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.request(ids[1], ThumbSize::Grid, t),
            Err(Error::ThumbUnavailable(_))
        ));
        assert!(matches!(
            service.request(9_999, ThumbSize::Grid, t),
            Err(Error::NotFound(9_999))
        ));
    }

    #[test]
    fn worker_count_leaves_a_core_free_and_stops_at_the_cap() {
        assert_eq!(worker_count(1), 1, "a single core still gets one worker");
        assert_eq!(worker_count(4), 3, "one core stays free for the UI");
        assert_eq!(
            worker_count(64),
            MAX_WORKERS,
            "peak decode memory is bounded by the cap, not by the core count"
        );
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
            assert!(cache.is_complete(lib.item(id).unwrap().unwrap().thumb_key()));
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
    fn an_edited_photo_gets_thumbnails_of_the_edit_under_its_own_key() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache.clone(), 1);
        let plain = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(image::image_dimensions(&plain).unwrap(), (64, 32));

        lib.set_item_edit(ids[0], Edit::new(1, None).unwrap())
            .unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
        let turned = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_ne!(turned, plain, "a different key, so a different file");
        assert_eq!(image::image_dimensions(&turned).unwrap(), (32, 64));
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
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
    fn missing_source_file_leaves_item_pending() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        // Drive unplugged / file being replaced: the source can't be opened at all.
        std::fs::remove_file(dir.path().join("photos").join("a.jpg")).unwrap();
        let service = ThumbService::start(lib.clone(), cache, 1);

        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::Io(_))
        ));
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);

        service.enqueue_pending().unwrap();
        service.wait_idle();
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
        assert_eq!(lib.pending_thumb_ids().unwrap(), ids);
    }

    #[test]
    fn only_decode_errors_mark_failed() {
        let io = || std::io::Error::other("unplugged");
        assert!(is_source_defect(&Error::Image(
            image::ImageError::Unsupported(image::error::UnsupportedError::from(
                image::error::ImageFormatHint::Unknown
            ))
        )));
        assert!(!is_source_defect(&Error::Image(
            image::ImageError::IoError(io())
        )));
        assert!(!is_source_defect(&Error::Io(io())));
    }

    fn panicking_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        if source.to_string_lossy().contains("panic") {
            panic!("simulated decoder bug");
        }
        cache.render(source, orientation, edit)
    }

    #[test]
    fn panicking_job_is_failed_and_worker_survives() {
        let (_dir, lib, cache, ids) = setup(&[
            ("a_panic.jpg", jpeg_bytes(64, 32)),
            ("b_ok.jpg", jpeg_bytes(64, 32)),
        ]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        // Grid order puts a_panic.jpg first, so the single worker hits the panic first.
        service.enqueue_pending().unwrap();
        service.wait_idle();

        let bad = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(bad.thumb_state, ThumbState::Failed);
        assert_eq!(bad.thumb_error.as_deref(), Some("decoder panicked"));
        assert_eq!(state(&lib, ids[1]), ThumbState::Ready);
    }

    #[test]
    fn get_or_generate_contains_decoder_panics() {
        let (_dir, lib, cache, ids) = setup(&[("panic.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert_eq!(state(&lib, ids[0]), ThumbState::Failed);
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
