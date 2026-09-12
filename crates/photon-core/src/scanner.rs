use crate::{
    Result,
    library::{KnownItem, Library, NewItem, WatchedFolder},
    media::MediaKind,
    metadata::read_image_meta,
};
use std::{
    collections::HashMap,
    fs::Metadata,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::UNIX_EPOCH,
};
use walkdir::{DirEntry, WalkDir};

const BATCH: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub added: u64,
    pub changed: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub offline: bool,
    pub added: u64,
    pub changed: u64,
    pub unchanged: u64,
    pub marked_missing: u64,
    pub purged: u64,
    pub cancelled: bool,
}

/// Per-scan settings.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    /// Directories never walked, e.g. photon's own cache and database folders.
    pub excluded: Vec<PathBuf>,
    /// Checked before each entry; once set, the scan stops without marking anything missing.
    pub cancel: Arc<AtomicBool>,
}

/// Brings the library in line with what is on disk under `watched`.
/// Never modifies files; only reads directory listings, metadata and EXIF.
///
/// `scan_id` marks the folders seen by this scan, so folders from older scans can be
/// pruned. It must increase monotonically per watched folder; use [`crate::now_ms`].
/// `scan_watched` must not run concurrently on the same or overlapping watched folders.
///
/// `options.excluded` lists directories never walked. `options.cancel`, checked before each
/// entry, lets a scan be stopped early: it returns with `cancelled: true` and never marks,
/// purges or prunes anything, since anything unreached is unknown, not missing. It also
/// leaves the watched folder's online/offline state unchanged, since deciding that needs
/// the empty-root guard below, which needs a complete walk.
pub fn scan_watched(
    lib: &Library,
    watched: &WatchedFolder,
    scan_id: i64,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    // Online/offline is decided once, below, after the empty-root guard, so the folder
    // doesn't flicker online and back.

    let mut known = lib.known_items(watched.id)?;
    let mut folder_ids: HashMap<PathBuf, i64> = HashMap::new();

    let WalkOutcome {
        report,
        seen,
        incomplete_prefixes,
        skip_mark_purge,
        cancelled,
    } = walk_tree(
        lib,
        watched.id,
        root,
        None,
        &mut known,
        &mut folder_ids,
        scan_id,
        options,
        progress,
    )?;

    if cancelled {
        // We stopped early, so everything we didn't reach is unknown, not missing. Leave
        // online/offline as it was too: that decision needs the empty-root guard below,
        // which needs a complete walk, so a cancelled scan of an unmounted mount point
        // must not get marked online.
        progress(&seen);
        return Ok(ScanReport {
            cancelled: true,
            ..report
        });
    }

    if skip_mark_purge {
        // We couldn't tell what happened to the rest of the tree; don't guess.
        lib.set_watched_online(watched.id, true)?;
        progress(&seen);
        return Ok(report);
    }

    // Drop anything under a subtree we couldn't fully walk: it might still be there.
    known.retain(|path_str, _| {
        !incomplete_prefixes
            .iter()
            .any(|prefix| Path::new(path_str).starts_with(prefix))
    });

    // A reachable but empty root usually means an unmounted volume left its mount
    // point behind, not that every known file vanished at once.
    if seen.files_seen == 0 && known.values().any(|k| !k.missing) {
        lib.set_watched_online(watched.id, false)?;
        progress(&seen);
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    lib.set_watched_online(watched.id, true)?;

    // Anything left in `known` was not found on this (reachable) scan: soft-delete it,
    // or purge it if it was already missing last time.
    let (marked, purged) = finish_mark_purge(lib, known)?;
    lib.prune_folders(watched.id, scan_id)?;

    progress(&seen);
    Ok(ScanReport {
        marked_missing: marked,
        purged,
        ..report
    })
}

/// Brings one directory and everything beneath it in line with what is on disk.
///
/// Everything this touches is restricted to that subtree: the walk, the known-items set it
/// diffs against, what it marks or purges, and which folders it prunes. `scan_watched`'s
/// diff compares against every item under the watched root, so aiming that logic at one
/// directory would make the rest of the library look deleted.
///
/// Differences from [`scan_watched`], both deliberate:
/// - it never changes the watched folder's online flag, except when the watched root itself
///   has gone, which it reports exactly as `scan_watched` does;
/// - it has no empty-directory guard. An empty root means an unmounted volume; an empty
///   subdirectory means its files really were deleted.
///
/// A `dir` that no longer exists is not an error: the nearest ancestor that still exists is
/// scanned instead, which is what makes a deleted folder disappear from the library.
pub fn scan_subtree(
    lib: &Library,
    watched: &WatchedFolder,
    dir: &Path,
    scan_id: i64,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    if !crate::paths::is_within(dir, root) {
        tracing::warn!(?dir, watched = %watched.path, "ignoring a subtree outside its watched folder");
        return Ok(ScanReport::default());
    }

    // A deleted directory is scanned through its nearest living ancestor, so the parent's
    // walk sees it gone, marks its items missing and eventually prunes it.
    let mut target = dir.to_path_buf();
    while !target.is_dir() {
        match target.parent() {
            Some(parent) if crate::paths::is_within(parent, root) => target = parent.to_path_buf(),
            _ => {
                target = root.to_path_buf();
                break;
            }
        }
    }
    if crate::paths::same_path(&target, root) {
        return scan_watched(lib, watched, scan_id, options, progress);
    }

    let target_str = target
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(target.clone()))?;
    let mut known = lib.known_items_under(watched.id, target_str)?;
    let (mut folder_ids, parent_id) = seed_ancestors(lib, watched, &target, scan_id)?;

    let outcome = walk_tree(
        lib,
        watched.id,
        &target,
        parent_id,
        &mut known,
        &mut folder_ids,
        scan_id,
        options,
        progress,
    )?;

    if outcome.cancelled {
        progress(&outcome.seen);
        return Ok(ScanReport {
            cancelled: true,
            ..outcome.report
        });
    }
    if outcome.skip_mark_purge {
        progress(&outcome.seen);
        return Ok(outcome.report);
    }

    known.retain(|path_str, _| {
        !outcome
            .incomplete_prefixes
            .iter()
            .any(|prefix| Path::new(path_str).starts_with(prefix))
    });

    let mut report = outcome.report;
    let (marked, purged) = finish_mark_purge(lib, known)?;
    lib.prune_folders_under(watched.id, scan_id, target_str)?;
    report.marked_missing = marked;
    report.purged = purged;

    progress(&outcome.seen);
    Ok(report)
}

/// Upserts the folder rows from the watched root down to `target`'s parent, so the walk can
/// attach `target` to its real parent rather than treating it as a root. Returns the ids it
/// created, and the id of `target`'s parent.
fn seed_ancestors(
    lib: &Library,
    watched: &WatchedFolder,
    target: &Path,
    scan_id: i64,
) -> Result<(HashMap<PathBuf, i64>, Option<i64>)> {
    let root = Path::new(&watched.path);
    let root_str = root
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(root.to_path_buf()))?;
    let mut ids = HashMap::new();
    let mut parent = Some(lib.upsert_folder(watched.id, None, root_str, scan_id)?);
    ids.insert(root.to_path_buf(), parent.expect("just inserted"));

    let relative = target.strip_prefix(root).unwrap_or(Path::new(""));
    let mut components: Vec<_> = relative.components().collect();
    components.pop(); // `target` itself is upserted by the walk.
    let mut current = root.to_path_buf();
    for component in components {
        current = current.join(component);
        let current_str = current
            .to_str()
            .ok_or_else(|| crate::Error::NonUtf8Path(current.clone()))?;
        let id = lib.upsert_folder(watched.id, parent, current_str, scan_id)?;
        ids.insert(current.clone(), id);
        parent = Some(id);
    }
    Ok((ids, parent))
}

/// The result of walking a subtree: what was found, and whether the walk was complete
/// enough to safely mark or purge anything afterwards.
struct WalkOutcome {
    report: ScanReport,
    seen: ScanProgress,
    /// Subtrees we could not fully walk: anything `known` claims to live under one of
    /// these might still exist, so it must not be marked missing or purged this scan.
    incomplete_prefixes: Vec<PathBuf>,
    /// Set when an error gives no path, or points at the root itself: the caller can no
    /// longer tell which `known` entries are safe to touch, so it must skip mark/purge/prune
    /// entirely.
    skip_mark_purge: bool,
    cancelled: bool,
}

/// Walks `root`, upserting folders and files into the library and removing matches from
/// `known` as they're found. Does not mark, purge or prune anything, or touch the watched
/// folder's online state; the caller decides that from the returned [`WalkOutcome`].
#[allow(clippy::too_many_arguments)]
fn walk_tree(
    lib: &Library,
    watched_id: i64,
    root: &Path,
    root_parent_id: Option<i64>,
    known: &mut HashMap<String, KnownItem>,
    folder_ids: &mut HashMap<PathBuf, i64>,
    scan_id: i64,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<WalkOutcome> {
    let mut report = ScanReport::default();
    let mut seen = ScanProgress::default();
    let mut new_batch: Vec<NewItem> = Vec::new();
    let mut changed_batch: Vec<(i64, NewItem)> = Vec::new();
    let mut incomplete_prefixes: Vec<PathBuf> = Vec::new();
    let mut skip_mark_purge = false;
    let mut cancelled = false;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            (e.depth() == 0 || !is_hidden(e))
                && !options
                    .excluded
                    .iter()
                    .any(|x| crate::paths::is_within(e.path(), x))
        });
    for entry in walker {
        if options.cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                match err.path() {
                    Some(p) if p != root => incomplete_prefixes.push(p.to_path_buf()),
                    _ => skip_mark_purge = true,
                }
                tracing::warn!(%err, "skipping unreadable entry");
                continue;
            }
        };
        let path = entry.path();
        let Some(path_str) = path.to_str() else {
            tracing::warn!(?path, "skipping non-UTF-8 path");
            continue;
        };

        if entry.file_type().is_dir() {
            let parent = if entry.depth() == 0 {
                root_parent_id
            } else {
                path.parent().and_then(|p| folder_ids.get(p)).copied()
            };
            let id = lib.upsert_folder(watched_id, parent, path_str, scan_id)?;
            folder_ids.insert(path.to_path_buf(), id);
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(kind) = MediaKind::from_path(path) else {
            continue;
        };
        let Some(&folder_id) = path.parent().and_then(|p| folder_ids.get(p)) else {
            // Unknown parent folder: leave any existing record alone rather than treat
            // the file as missing.
            known.remove(path_str);
            continue;
        };
        let md = match entry.metadata() {
            Ok(md) => md,
            Err(err) => {
                // Couldn't stat it, but it clearly still exists: leave it untouched.
                known.remove(path_str);
                tracing::warn!(%err, ?path, "skipping file without metadata");
                continue;
            }
        };
        let (size, mtime_ms) = (md.len() as i64, mtime_ms(&md));
        seen.files_seen += 1;

        match known.remove(path_str) {
            Some(k) if k.size == size && k.mtime_ms == mtime_ms && !k.missing => {
                report.unchanged += 1
            }
            Some(k) => changed_batch.push((
                k.id,
                describe(&entry, path_str, folder_id, kind, size, mtime_ms),
            )),
            None => new_batch.push(describe(&entry, path_str, folder_id, kind, size, mtime_ms)),
        }

        if new_batch.len() >= BATCH {
            flush_new(lib, &mut new_batch, &mut report, &mut seen, progress)?;
        }
        if changed_batch.len() >= BATCH {
            flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;
        }
    }
    flush_new(lib, &mut new_batch, &mut report, &mut seen, progress)?;
    flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;

    Ok(WalkOutcome {
        report,
        seen,
        incomplete_prefixes,
        skip_mark_purge,
        cancelled,
    })
}

/// Soft-deletes what this walk didn't find, and purges what was already missing.
/// Both are chunked at `BATCH` rows per transaction.
fn finish_mark_purge(lib: &Library, known: HashMap<String, KnownItem>) -> Result<(u64, u64)> {
    let (mut to_mark, mut to_purge) = (Vec::new(), Vec::new());
    for k in known.into_values() {
        if k.missing {
            to_purge.push(k.id)
        } else {
            to_mark.push(k.id)
        }
    }
    let now = crate::now_ms();
    for chunk in to_mark.chunks(BATCH) {
        lib.mark_missing(chunk, now)?;
    }
    for chunk in to_purge.chunks(BATCH) {
        lib.purge_items(chunk)?;
    }
    Ok((to_mark.len() as u64, to_purge.len() as u64))
}

fn describe(
    entry: &DirEntry,
    path: &str,
    folder_id: i64,
    kind: MediaKind,
    size: i64,
    mtime_ms: i64,
) -> NewItem {
    let meta = read_image_meta(entry.path());
    NewItem {
        folder_id,
        path: path.to_string(),
        file_name: entry.file_name().to_string_lossy().into_owned(),
        kind,
        size,
        mtime_ms,
        width: meta.width,
        height: meta.height,
        orientation: meta.orientation,
        taken_at: meta.taken_at.unwrap_or(mtime_ms.div_euclid(1000)),
    }
}

fn flush_new(
    lib: &Library,
    batch: &mut Vec<NewItem>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.insert_items(batch)?;
    report.added += batch.len() as u64;
    seen.added = report.added;
    batch.clear();
    progress(seen);
    Ok(())
}

fn flush_changed(
    lib: &Library,
    batch: &mut Vec<(i64, NewItem)>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.update_items(batch)?;
    report.changed += batch.len() as u64;
    seen.changed = report.changed;
    batch.clear();
    progress(seen);
    Ok(())
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with('.'))
}

fn mtime_ms(md: &Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, jpeg_with_exif, png_bytes, temp_library, write_file};
    use std::fs;

    fn scan(lib: &Library, watched: &WatchedFolder, scan_id: i64) -> ScanReport {
        scan_watched(lib, watched, scan_id, &ScanOptions::default(), &mut |_| {}).unwrap()
    }

    fn key(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    /// The watched root, canonicalised the way `add_watched_folder` will store it (on macOS
    /// the temp dir lives behind the /var → /private/var symlink).
    fn photos_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let root = dir.path().join("photos");
        std::fs::create_dir_all(&root).unwrap();
        dunce::canonicalize(root).unwrap()
    }

    #[test]
    fn indexes_supported_files_and_folders() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(
            &root,
            "a.jpg",
            &jpeg_with_exif(4, 2, 6, "2024:06:15 12:30:45"),
        );
        write_file(&root, "2024/b.png", &png_bytes(3, 3));
        write_file(&root, "notes.txt", b"ignored");
        write_file(&root, ".hidden/c.jpg", &jpeg_bytes(8, 8));
        write_file(&root, ".d.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let mut last = None;
        let report = scan_watched(&lib, &watched, 1, &ScanOptions::default(), &mut |p| {
            last = Some(*p)
        })
        .unwrap();
        assert_eq!(
            (report.added, report.changed, report.offline),
            (2, 0, false)
        );
        assert_eq!(last.unwrap().files_seen, 2);

        let known = lib.known_items(watched.id).unwrap();
        assert_eq!(known.len(), 2);
        let item = lib.item(known[&key(&a)].id).unwrap().unwrap();
        assert_eq!(
            (item.orientation, item.taken_at, item.width),
            (6, 1_718_454_645, 4)
        );

        let names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["photos", "2024"]);
        let sub = &lib.folders().unwrap()[1];
        assert_eq!(sub.parent_id, Some(lib.folders().unwrap()[0].id));
    }

    #[test]
    fn capture_date_falls_back_to_mtime() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let p = write_file(&root, "a.png", &png_bytes(2, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let mtime_s = fs::metadata(&p)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let id = lib.known_items(watched.id).unwrap()[&key(&p)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().taken_at, mtime_s);
    }

    #[test]
    fn rescans_detect_unchanged_and_changed_files() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let b = write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(&root, "b.jpg", &jpeg_bytes(64, 64));
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.added, report.changed, report.unchanged), (0, 1, 1));
        let id = lib.known_items(watched.id).unwrap()[&key(&b)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().width, 64);
    }

    #[test]
    fn missing_files_are_soft_deleted_then_purged() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let gone = write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&gone)].id;

        fs::remove_dir_all(root.join("trip")).unwrap();
        let before = crate::now_ms();
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.marked_missing, report.purged), (1, 0));
        let since = lib.item(id).unwrap().unwrap().missing_since;
        assert!(
            since.is_some_and(|t| t >= before),
            "missing_since is a timestamp, got {since:?}"
        );
        assert_eq!(
            lib.folders().unwrap().len(),
            2,
            "folder kept while it holds a soft-deleted item"
        );

        let report = scan(&lib, &watched, 3);
        assert_eq!((report.marked_missing, report.purged), (0, 1));
        assert!(lib.item(id).unwrap().is_none());
        assert_eq!(lib.folders().unwrap().len(), 1, "empty folder pruned");
    }

    #[test]
    fn reappearing_files_are_restored() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        // Keeps the root non-empty so scan 2 doesn't hit the empty-root offline guard.
        write_file(&root, "keep.jpg", &jpeg_bytes(8, 8));
        let bytes = fs::read(&a).unwrap();
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;

        fs::remove_file(&a).unwrap();
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.offline, report.marked_missing), (false, 1));
        assert!(lib.item(id).unwrap().unwrap().missing_since.is_some());

        fs::write(&a, bytes).unwrap();
        let report = scan(&lib, &watched, 3);
        assert_eq!((report.changed, report.purged), (1, 0));
        assert_eq!(lib.known_items(watched.id).unwrap()[&key(&a)].id, id);
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    #[test]
    fn unreachable_folder_goes_offline_and_keeps_items() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        fs::rename(&root, dir.path().join("unplugged")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert!(report.offline);
        assert!(!lib.watched_folders().unwrap()[0].online);
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
        assert!(
            lib.known_items(watched.id)
                .unwrap()
                .values()
                .all(|k| !k.missing)
        );

        fs::rename(dir.path().join("unplugged"), &root).unwrap();
        assert!(!scan(&lib, &watched, 3).offline);
        assert!(lib.watched_folders().unwrap()[0].online);
    }

    #[test]
    fn empty_reachable_root_is_treated_as_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        fs::remove_file(root.join("a.jpg")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert!(report.offline);
        assert!(!lib.watched_folders().unwrap()[0].online);
        assert!(
            lib.known_items(watched.id)
                .unwrap()
                .values()
                .all(|k| !k.missing)
        );
    }

    #[test]
    #[cfg(unix)]
    fn unreadable_subdirectory_does_not_purge_its_contents() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let locked = root.join("locked");
        let a = write_file(&root, "locked/a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked).is_ok() {
            // Running with elevated privileges (e.g. as root): permission bits aren't
            // enforced, so this test can't exercise the unreadable-directory path.
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        scan(&lib, &watched, 2);
        scan(&lib, &watched, 3);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    #[test]
    fn excluded_subtrees_are_not_indexed() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "cache/b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let options = ScanOptions {
            excluded: vec![root.join("cache")],
            ..ScanOptions::default()
        };
        let report = scan_watched(&lib, &watched, 1, &options, &mut |_| {}).unwrap();
        assert_eq!(report.added, 1);
        assert!(lib.folders().unwrap().iter().all(|f| f.name != "cache"));
    }

    #[test]
    fn cancelled_scan_leaves_unseen_items_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let b = write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_file(&b).unwrap();

        let options = ScanOptions::default();
        options
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let report = scan_watched(&lib, &watched, 2, &options, &mut |_| {}).unwrap();

        assert!(report.cancelled);
        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&b)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    fn scan_sub(lib: &Library, watched: &WatchedFolder, dir: &Path, scan_id: i64) -> ScanReport {
        scan_subtree(
            lib,
            watched,
            dir,
            scan_id,
            &ScanOptions::default(),
            &mut |_| {},
        )
        .unwrap()
    }

    #[test]
    fn subtree_scan_leaves_everything_outside_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let keep = write_file(&root, "b/keep.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        // Delete a file in the *other* folder; scanning /a must not notice or touch it.
        fs::remove_file(&keep).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);

        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&keep)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    #[test]
    fn subtree_scan_finds_additions_and_removals_inside_it() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let gone = write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&gone)].id;

        write_file(&root, "a/two.jpg", &jpeg_bytes(8, 8));
        fs::remove_file(&gone).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);
        assert_eq!((report.added, report.marked_missing), (1, 1));
        assert!(lib.item(id).unwrap().unwrap().missing_since.is_some());

        // A second subtree scan purges it, as a full scan would.
        let report = scan_sub(&lib, &watched, &root.join("a"), 3);
        assert_eq!(report.purged, 1);
        assert!(lib.item(id).unwrap().is_none());
    }

    #[test]
    fn subtree_scan_of_a_deleted_directory_uses_its_nearest_living_ancestor() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/sub/one.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/keep.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        fs::remove_dir_all(root.join("a").join("sub")).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a").join("sub"), 2);

        assert_eq!(report.marked_missing, 1);
        assert_eq!(report.unchanged, 1, "the surviving sibling was walked");
        scan_sub(&lib, &watched, &root.join("a").join("sub"), 3);
        assert!(lib.folders().unwrap().iter().all(|f| f.name != "sub"));
    }

    #[test]
    fn subtree_scan_of_an_empty_directory_does_not_report_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let only = write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        // Emptying a subdirectory is a real deletion, not an unmounted volume.
        fs::remove_file(&only).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);
        assert!(!report.offline);
        assert_eq!(report.marked_missing, 1);
        assert!(lib.watched_folders().unwrap()[0].online);
    }

    #[test]
    fn subtree_scan_delegates_to_a_full_scan_at_the_root() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let report = scan_sub(&lib, &watched, &root, 1);
        assert_eq!(report.added, 1);
    }

    #[test]
    fn subtree_scan_refuses_a_directory_outside_the_watched_folder() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        let report = scan_sub(&lib, &watched, &outside, 2);
        assert_eq!(report, ScanReport::default());
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
    }

    #[test]
    fn subtree_scan_skips_excluded_directories_and_honours_cancel() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/cache/two.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let excluded = ScanOptions {
            excluded: vec![root.join("a").join("cache")],
            ..ScanOptions::default()
        };
        let report =
            scan_subtree(&lib, &watched, &root.join("a"), 1, &excluded, &mut |_| {}).unwrap();
        assert_eq!(report.added, 1);

        let cancelled = ScanOptions::default();
        cancelled
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let report =
            scan_subtree(&lib, &watched, &root.join("a"), 2, &cancelled, &mut |_| {}).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.marked_missing, 0);
    }

    #[test]
    fn cancelled_scan_leaves_online_state_unchanged() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        lib.set_watched_online(watched.id, false).unwrap();

        let options = ScanOptions::default();
        options
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let report = scan_watched(&lib, &watched, 2, &options, &mut |_| {}).unwrap();

        assert!(report.cancelled);
        assert!(!lib.watched_folders().unwrap()[0].online);
    }
}
