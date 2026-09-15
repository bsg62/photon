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
    /// Items whose `rating` the Picasa pass actually changed this scan. Populated even
    /// when every photo in the walk took the `unchanged` branch, which is the whole point:
    /// starring a photo in Picasa never changes the photo, so this is often the only
    /// non-zero field in an otherwise all-unchanged report, and callers deciding whether to
    /// refresh the grid must count it alongside `added`/`changed`/`marked_missing`/`purged`.
    pub restarred: u64,
    pub cancelled: bool,
}

impl ScanReport {
    /// Whether this scan moved any row the grid is built from, and so whether the grid has
    /// to be rebuilt.
    ///
    /// Here rather than at the caller: which fields mean "rows moved" is the scanner's
    /// knowledge, and spelled out in the app crate it had to be remembered in three places
    /// (this doc, the caller, and CLAUDE.md). A new mutation counter added to this struct
    /// and forgotten there compiles, passes every scanner test, and silently stops the grid
    /// from ever rebuilding for it - which is exactly what `restarred` did once.
    pub fn touched_rows(&self) -> bool {
        self.added + self.changed + self.marked_missing + self.purged + self.restarred > 0
    }
}

/// Where a running scan reports to.
///
/// A trait rather than a second closure because the two reports have different consumers:
/// `progress` feeds the UI's scan counter, `indexed` feeds the thumbnail queue. A caller
/// that wants only progress wraps a closure in [`progress_only`].
pub trait ScanSink {
    /// Called after every batch, and once more at the end of the scan.
    fn progress(&mut self, progress: &ScanProgress);

    /// Items just inserted or replaced. Each has `thumb_state = Pending`, so these are
    /// exactly the rows `Library::pending_thumb_ids` would find, handed over without the
    /// query: re-running it every 250ms of a scan sorted every pending row in grid order,
    /// for the whole of an import, to learn what the scanner already knew.
    fn indexed(&mut self, _ids: &[i64]) {}
}

/// A sink that reports progress to `f` and ignores `indexed`.
///
/// A named constructor rather than a blanket `impl ScanSink for F: FnMut` because a
/// closure handed straight to a `&mut dyn ScanSink` parameter gets no signature hint, so
/// `|_| {}` failed to infer and every call site needed `|_: &ScanProgress|`. Through a
/// generic function's bound the closure's type infers as usual.
pub fn progress_only(f: impl FnMut(&ScanProgress)) -> impl ScanSink {
    struct ProgressOnly<F>(F);
    impl<F: FnMut(&ScanProgress)> ScanSink for ProgressOnly<F> {
        fn progress(&mut self, progress: &ScanProgress) {
            (self.0)(progress)
        }
    }
    ProgressOnly(f)
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
    progress: &mut dyn ScanSink,
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
        walked,
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
        progress.progress(&seen);
        return Ok(ScanReport {
            cancelled: true,
            ..report
        });
    }

    if skip_mark_purge {
        // We couldn't tell what happened to the rest of the tree; don't guess.
        lib.set_watched_online(watched.id, true)?;
        // The stars of the folders we *did* walk are a separate question, and this branch
        // has already concluded the root is live. Returning without them would leave every
        // star in the library at its last scan's value over one walkdir error, with
        // `restarred` at 0 so the grid would not refresh either. It cannot simply move above
        // this guard: the empty-root check below is what tells a live folder from an
        // unmounted volume, and an unmounted mount point reads as a folder whose INI is
        // gone - which would clear every star it has.
        let restarred = apply_picasa_stars(lib, &walked);
        progress.progress(&seen);
        return Ok(ScanReport {
            restarred,
            ..report
        });
    }

    // A reachable but empty root usually means an unmounted volume left its mount
    // point behind, not that every known file vanished at once.
    if seen.files_seen == 0 && known.values().any(|k| !k.missing) {
        lib.set_watched_online(watched.id, false)?;
        progress.progress(&seen);
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    lib.set_watched_online(watched.id, true)?;

    let restarred = apply_picasa_stars(lib, &walked);

    // Anything left in `known` was not found on this (reachable) scan: soft-delete it,
    // or purge it if it was already missing last time.
    let (marked, purged) = finish_mark_purge(lib, known, &incomplete_prefixes)?;
    lib.prune_folders(watched.id, scan_id)?;

    progress.progress(&seen);
    Ok(ScanReport {
        marked_missing: marked,
        purged,
        restarred,
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
    progress: &mut dyn ScanSink,
) -> Result<ScanReport> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    // A deleted directory is scanned through its nearest living ancestor, so the parent's
    // walk sees it gone, marks its items missing and eventually prunes it. This walk-up
    // happens on the raw, non-canonical path: `is_within` and `same_path` compare
    // component-wise and case-insensitively on macOS/Windows, but a plain `Path::is_dir`
    // check works fine either way, and canonicalizing first would fail outright on a path
    // that no longer exists.
    //
    // The climb stops at the watched root: a `dir` that isn't under it is rejected below
    // anyway, and without this guard an ineligible path would stat its way up to the
    // filesystem root first, leaving the whole safety argument resting on that single
    // downstream check. Callers hand this an existing directory's canonical path (the
    // watcher canonicalizes each event directory before mapping it to a root), so the
    // component-wise comparison agrees with `root` for every path that can reach here.
    let mut target = dir.to_path_buf();
    while !target.is_dir() {
        match target.parent() {
            Some(parent) if crate::paths::is_within(parent, root) => target = parent.to_path_buf(),
            _ => break,
        }
    }
    // Only now, with an existing directory in hand, canonicalize it so the membership and
    // delegation checks below - and the byte-exact `known_items_under` /
    // `prune_folders_under` / `strip_prefix` calls further down - agree with `root`, which
    // `add_watched_folder` stored canonicalized. Skipping this would let a `dir` that only
    // differs from `root` in case, or that contains `..`, slip past `is_within` and then
    // corrupt the folder tree once `strip_prefix` disagrees with it.
    let target = dunce::canonicalize(&target).unwrap_or(target);

    if !crate::paths::is_within(&target, root) {
        tracing::warn!(?dir, watched = %watched.path, "ignoring a subtree outside its watched folder");
        return Ok(ScanReport::default());
    }
    if crate::paths::same_path(&target, root) {
        return scan_watched(lib, watched, scan_id, options, progress);
    }
    // `is_within` compares component-wise and case-insensitively on macOS/Windows, while
    // `strip_prefix` is byte-exact; now that both `target` and `root` are canonicalized they
    // should always agree, but if they somehow didn't, silently defaulting the relative path
    // to empty would attach `target` to the wrong parent instead of failing loudly.
    let Ok(relative) = target.strip_prefix(root) else {
        tracing::warn!(?dir, ?target, watched = %watched.path, "canonicalized subtree unexpectedly does not lie under its watched folder");
        return Ok(ScanReport::default());
    };

    // `walk_tree` skips hidden directories, but exempts its own depth 0 - which here is the
    // event directory the watcher handed us, not the watched root. Nothing between the OS
    // event and the walk rejects a dot-directory, so without this the subtree scan indexes
    // photos that every `scan_watched` skips, marks missing and then purges. Only the part
    // below the root is checked: a user who explicitly watches `~/.photos` gets it scanned,
    // exactly as `scan_watched` does.
    if relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|name| name.starts_with('.'))
    {
        return Ok(ScanReport::default());
    }

    let target_str = target
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(target.clone()))?;
    let mut known = lib.known_items_under(watched.id, target_str)?;
    let (mut folder_ids, parent_id) = seed_ancestors(lib, watched, relative, scan_id)?;

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
        progress.progress(&outcome.seen);
        return Ok(ScanReport {
            cancelled: true,
            ..outcome.report
        });
    }
    // Above the `skip_mark_purge` guard, which `scan_watched` cannot do: the constraint
    // there is its empty-root check, and this function deliberately has none (see the doc
    // comment). Every non-cancelled walk applies the stars of the folders it reached.
    let restarred = apply_picasa_stars(lib, &outcome.walked);

    if outcome.skip_mark_purge {
        progress.progress(&outcome.seen);
        return Ok(ScanReport {
            restarred,
            ..outcome.report
        });
    }

    let mut report = outcome.report;
    let (marked, purged) = finish_mark_purge(lib, known, &outcome.incomplete_prefixes)?;
    lib.prune_folders_under(watched.id, scan_id, target_str)?;
    report.marked_missing = marked;
    report.purged = purged;
    report.restarred = restarred;

    progress.progress(&outcome.seen);
    Ok(report)
}

/// Upserts the folder rows from the watched root down to the target's parent, so the walk
/// can attach the target to its real parent rather than treating it as a root. `relative` is
/// the target's path relative to the watched root (i.e. `target.strip_prefix(root)`).
/// Returns the ids it created, and the id of the target's parent.
fn seed_ancestors(
    lib: &Library,
    watched: &WatchedFolder,
    relative: &Path,
    scan_id: i64,
) -> Result<(HashMap<PathBuf, i64>, Option<i64>)> {
    let root = Path::new(&watched.path);
    let root_str = root
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(root.to_path_buf()))?;
    let mut ids = HashMap::new();
    let mut parent = Some(lib.upsert_folder(watched.id, None, root_str, scan_id)?);
    ids.insert(root.to_path_buf(), parent.expect("just inserted"));

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
    /// The directories this walk actually entered, as opposed to those `seed_ancestors`
    /// pre-inserted into `folder_ids`. The Picasa pass must only touch these: a subtree
    /// scan that rewrote its ancestors' ratings would break its own isolation contract.
    walked: Vec<(PathBuf, i64)>,
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
    progress: &mut dyn ScanSink,
) -> Result<WalkOutcome> {
    let mut report = ScanReport::default();
    let mut seen = ScanProgress::default();
    let mut new_batch: Vec<NewItem> = Vec::new();
    let mut changed_batch: Vec<(i64, NewItem)> = Vec::new();
    let mut walked: Vec<(PathBuf, i64)> = Vec::new();
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
            walked.push((path.to_path_buf(), id));
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
        walked,
        incomplete_prefixes,
        skip_mark_purge,
        cancelled,
    })
}

/// Applies each walked folder's Picasa stars to its photos. Returns how many items' ratings
/// actually changed, for [`ScanReport::restarred`].
///
/// Runs after the walk rather than inside `describe()`, which is called only for photos
/// whose size or mtime changed. Starring a photo in Picasa rewrites the folder's INI and
/// leaves the photo untouched, so on a rescan every photo takes the `unchanged` branch and
/// a star read in `describe()` would never be written.
///
/// The INI is the only authority: a photo it does not name is set to unstarred, so removing
/// a star in Picasa clears it here too. A folder that cannot be read is skipped instead,
/// because failing to read is not evidence that the stars are gone.
///
/// Infallible: a per-folder DB error (a busy database, say) is logged and skipped rather
/// than aborting the whole scan, since that would also skip `finish_mark_purge` and
/// `prune_folders` over an unrelated folder's transient failure. Nothing is lost — the next
/// scan reapplies this folder's stars.
fn apply_picasa_stars(lib: &Library, walked: &[(PathBuf, i64)]) -> u64 {
    let mut restarred = 0;
    for (dir, folder_id) in walked {
        let Some(stars) = crate::picasa::read_stars(dir) else {
            tracing::debug!(?dir, "leaving stars alone for an unreadable folder");
            continue;
        };
        match apply_folder_stars(lib, *folder_id, &stars) {
            Ok(n) => restarred += n,
            Err(err) => {
                // One folder's transient failure (a busy database, say) must not cost the
                // whole scan its mark/purge/prune. The next scan reapplies this folder's
                // stars.
                tracing::warn!(%err, ?dir, "could not apply Picasa stars for a folder");
            }
        }
    }
    restarred
}

/// Sets `folder_id`'s items' ratings from `stars`, writing only the rows whose rating
/// actually changes, and returns how many that was. Comparing before writing is what makes
/// a scan of an all-agreeing folder cost zero transactions, and what lets the caller tell a
/// real star change from a no-op scan.
fn apply_folder_stars(
    lib: &Library,
    folder_id: i64,
    stars: &std::collections::HashSet<String>,
) -> Result<u64> {
    let ratings: Vec<(i64, u8)> = lib
        .folder_item_names(folder_id)?
        .into_iter()
        .filter_map(|(id, name, current)| {
            let wanted = u8::from(stars.contains(&name));
            (current != Some(wanted as i64)).then_some((id, wanted))
        })
        .collect();
    let changed = ratings.len() as u64;
    for chunk in ratings.chunks(BATCH) {
        lib.set_ratings(chunk)?;
    }
    Ok(changed)
}

/// Soft-deletes what this walk didn't find, and purges what was already missing.
/// Both are chunked at `BATCH` rows per transaction.
///
/// `incomplete_prefixes` are the subtrees the walk could not read. Anything under one of
/// them is dropped from `known` first: not finding it is no evidence it is gone. That is a
/// precondition of marking and purging rather than of either caller, so it lives here - both
/// callers used to carry their own copy of the filter, and the failure mode of the two
/// drifting apart is silently purging photos from a directory the walk never reached.
fn finish_mark_purge(
    lib: &Library,
    mut known: HashMap<String, KnownItem>,
    incomplete_prefixes: &[PathBuf],
) -> Result<(u64, u64)> {
    known.retain(|path_str, _| {
        !incomplete_prefixes
            .iter()
            .any(|prefix| Path::new(path_str).starts_with(prefix))
    });
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
        // Always `None` here; `apply_picasa_stars` sets the real value after the walk.
        rating: meta.rating,
    }
}

fn flush_new(
    lib: &Library,
    batch: &mut Vec<NewItem>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn ScanSink,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let ids = lib.insert_items(batch)?;
    report.added += batch.len() as u64;
    seen.added = report.added;
    batch.clear();
    progress.indexed(&ids);
    progress.progress(seen);
    Ok(())
}

fn flush_changed(
    lib: &Library,
    batch: &mut Vec<(i64, NewItem)>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn ScanSink,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.update_items(batch)?;
    // Replaced rows are reset to `Pending` by `update_items`, so they are pending
    // thumbnails just as new rows are.
    let ids: Vec<i64> = batch.iter().map(|(id, _)| *id).collect();
    report.changed += batch.len() as u64;
    seen.changed = report.changed;
    batch.clear();
    progress.indexed(&ids);
    progress.progress(seen);
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
        .map(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            // Dated before 1970 - an archive extracted by a tool that clamps, a backup
            // restored with its original timestamps. Signed, not folded to 0: every such
            // file would then compare equal to its last scan, so an edit in place that kept
            // the byte size would take the `unchanged` branch and its new dimensions, EXIF
            // and thumbnail would never be picked up.
            Err(before) => -(before.duration().as_millis() as i64),
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, jpeg_with_exif, png_bytes, temp_library, write_file};
    use std::fs;
    use std::time::Duration;

    fn scan(lib: &Library, watched: &WatchedFolder, scan_id: i64) -> ScanReport {
        scan_watched(
            lib,
            watched,
            scan_id,
            &ScanOptions::default(),
            &mut progress_only(|_| {}),
        )
        .unwrap()
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
        let report = scan_watched(
            &lib,
            &watched,
            1,
            &ScanOptions::default(),
            &mut progress_only(|p| last = Some(*p)),
        )
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

    /// A sink that keeps every id the scan reports as freshly indexed.
    #[derive(Default)]
    struct Indexed(Vec<i64>);

    impl ScanSink for Indexed {
        fn progress(&mut self, _: &ScanProgress) {}
        fn indexed(&mut self, ids: &[i64]) {
            self.0.extend_from_slice(ids);
        }
    }

    /// The thumbnail queue used to learn about new photos only by re-running the full
    /// pending query every 250ms of a scan - a grid-order sort of every pending row, for the
    /// whole of an import. The scanner already has the ids it just inserted or replaced,
    /// which are exactly the rows that query would find, so it hands them over directly.
    #[test]
    fn new_and_changed_items_are_reported_to_the_sink_as_they_are_indexed() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let mut first = Indexed::default();
        scan_watched(&lib, &watched, 1, &ScanOptions::default(), &mut first).unwrap();
        let known = lib.known_items(watched.id).unwrap();
        let mut expected: Vec<i64> = known.values().map(|k| k.id).collect();
        expected.sort();
        first.0.sort();
        assert_eq!(first.0, expected, "both new photos are reported");

        // A changed file is replaced in place and its thumbnail reset, so it is reported
        // again; the unchanged one is not.
        write_file(&root, "a.jpg", &jpeg_bytes(64, 64));
        let mut second = Indexed::default();
        scan_watched(&lib, &watched, 2, &ScanOptions::default(), &mut second).unwrap();
        assert_eq!(second.0, [known[&key(&a)].id]);
    }

    #[test]
    fn a_star_added_after_indexing_is_picked_up_without_the_photo_changing() {
        // THE test for this feature. Starring in Picasa rewrites the INI and leaves the photo
        // untouched, so the photo takes the scanner's `unchanged` branch and describe() never
        // runs for it. An implementation that reads stars in describe() passes every other test
        // here and fails this one.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 0);

        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let report = scan(&lib, &watched, 2);

        assert_eq!(report.unchanged, 1, "the photo itself did not change");
        assert_eq!(lib.starred_count().unwrap(), 1);
    }

    #[test]
    fn a_star_removed_from_the_ini_is_cleared_on_the_next_scan() {
        // The INI is the only authority (spec §5): only-ever-adding would leave an un-starred
        // photo stuck with no way to clear it short of deleting the library.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        write_file(&root, ".picasa.ini", b"[a.jpg]\nbackuphash=1\n");
        scan(&lib, &watched, 2);
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn deleting_the_ini_clears_the_folder_s_stars() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        fs::remove_file(root.join(".picasa.ini")).unwrap();
        scan(&lib, &watched, 2);
        assert_eq!(
            lib.starred_count().unwrap(),
            0,
            "a folder with no INI has no stars"
        );
    }

    #[test]
    fn a_parent_folder_s_ini_does_not_star_photos_in_a_subfolder() {
        // Picasa writes one INI per directory and its section names are bare filenames, so
        // nothing is inherited downward.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn stars_are_matched_case_insensitively_against_the_files_on_disk() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "DSC_0001.JPG", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[dsc_0001.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        assert_eq!(lib.starred_count().unwrap(), 1);
    }

    #[test]
    fn a_subtree_scan_applies_stars_too() {
        // scan_subtree is the path the file watcher uses when a folder changes, which is
        // exactly what happens when someone stars a photo in Picasa. Wiring the pass into
        // scan_watched alone would leave this broken while every other test passed.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 0);

        write_file(&root, "sub/.picasa.ini", b"[a.jpg]\nstar=yes\n");
        scan_sub(&lib, &watched, &root.join("sub"), 2);

        assert_eq!(lib.starred_count().unwrap(), 1);
    }

    #[test]
    fn a_photo_with_a_pre_epoch_mtime_is_still_seen_as_changed() {
        // Files dated before 1970 turn up in practice: archives extracted by tools that
        // clamp, backups restored with their original timestamps. `duration_since` returns
        // `Err` for all of them, and folding that to one stored value makes every such file
        // compare equal to its last scan - so an edit that keeps the byte size takes the
        // `unchanged` branch and its new dimensions, EXIF and thumbnail are never picked up.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        set_mtime(&a, UNIX_EPOCH - Duration::from_secs(400 * 86_400));
        scan(&lib, &watched, 1);

        // Edited in place, keeping its size and still dated before the epoch.
        set_mtime(&a, UNIX_EPOCH - Duration::from_secs(399 * 86_400));
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.changed, report.unchanged), (1, 0));
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_that_cannot_be_read_keeps_its_stars() {
        // Spec §7: failing to read is not evidence that the stars are gone, unlike a
        // successfully-read INI with no entry for the photo. The `Option` in read_stars's
        // return type is the only thing carrying that distinction, and this is the only test
        // that exercises the caller's side of it. It does not discriminate the describe()
        // revert (see a_star_added_after_indexing_...); the revert that does break this one
        // is replacing read_stars's `None` arm with `.unwrap_or_default()`, which fails it
        // with `left: 0, right: 1`.
        use std::os::unix::fs::PermissionsExt;

        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "sub/.picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        let sub = root.join("sub");
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&sub).is_ok() {
            // Running with elevated privileges (e.g. as root): permission bits aren't
            // enforced, so this test can't exercise the unreadable-directory path.
            fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        scan(&lib, &watched, 2);

        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            lib.starred_count().unwrap(),
            1,
            "no evidence is not evidence of no stars"
        );
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
        let report = scan_watched(&lib, &watched, 1, &options, &mut progress_only(|_| {})).unwrap();
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
        let report = scan_watched(&lib, &watched, 2, &options, &mut progress_only(|_| {})).unwrap();

        assert!(report.cancelled);
        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&b)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    fn set_mtime(path: &Path, at: std::time::SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn scan_sub(lib: &Library, watched: &WatchedFolder, dir: &Path, scan_id: i64) -> ScanReport {
        scan_subtree(
            lib,
            watched,
            dir,
            scan_id,
            &ScanOptions::default(),
            &mut progress_only(|_| {}),
        )
        .unwrap()
    }

    #[test]
    fn subtree_scan_leaves_everything_outside_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let keep = write_file(&root, "b/keep.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        // An empty sibling: `prune_folders` (the unscoped version) would delete it once it's
        // no longer the newest scan, so this is the only thing distinguishing a correct call
        // to `prune_folders_under` from an accidental call to `prune_folders`.
        std::fs::create_dir_all(root.join("c")).unwrap();
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert!(lib.folders().unwrap().iter().any(|f| f.name == "c"));

        // Delete a file in the *other* folder; scanning /a must not notice or touch it.
        fs::remove_file(&keep).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);

        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&keep)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
        assert!(
            lib.folders().unwrap().iter().any(|f| f.name == "c"),
            "an empty sibling folder must survive a subtree scan of a different subtree"
        );
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
        // Starts false so the assertion below is non-vacuous: `online` defaults to true on
        // insert, so without this a broken `same_path` match (running the subtree path with
        // `target == root` instead of delegating) would still leave it true.
        lib.set_watched_online(watched.id, false).unwrap();
        let report = scan_sub(&lib, &watched, &root, 1);
        assert_eq!(report.added, 1);
        // `scan_subtree` never sets the online flag true on any path but this delegation, so
        // this is what actually proves `scan_watched` ran, rather than `report.added == 1`
        // merely surviving a broken `same_path` match.
        assert!(lib.watched_folders().unwrap()[0].online);
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
        let report = scan_subtree(
            &lib,
            &watched,
            &root.join("a"),
            1,
            &excluded,
            &mut progress_only(|_| {}),
        )
        .unwrap();
        assert_eq!(report.added, 1);

        let cancelled = ScanOptions::default();
        cancelled
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let report = scan_subtree(
            &lib,
            &watched,
            &root.join("a"),
            2,
            &cancelled,
            &mut progress_only(|_| {}),
        )
        .unwrap();
        assert!(report.cancelled);
        assert_eq!(report.marked_missing, 0);
    }

    #[test]
    fn subtree_scan_of_a_hidden_directory_indexes_nothing() {
        // `walk_tree`'s hidden filter exempts depth 0, which for a subtree scan is the
        // watcher's event directory rather than the watched root. Without a check of its own,
        // touching a file under `.private` indexes it, the next full scan skips `.private` and
        // marks it missing, and the one after purges it: the photo flickers in and out of the
        // library, rebuilding the grid on every transition.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, ".private/c.jpg", &jpeg_bytes(8, 8));
        write_file(&root, ".private/sub/d.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        assert_eq!(
            scan_sub(&lib, &watched, &root.join(".private"), 1),
            ScanReport::default()
        );
        // A hidden component anywhere between the root and the event directory, not just the
        // event directory itself.
        assert_eq!(
            scan_sub(&lib, &watched, &root.join(".private").join("sub"), 2),
            ScanReport::default()
        );
        assert!(lib.known_items(watched.id).unwrap().is_empty());
    }

    #[test]
    fn subtree_scan_of_a_vanished_watched_root_reports_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        // The one place `scan_subtree` writes `online = false`.
        fs::rename(&root, dir.path().join("unplugged")).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);
        assert!(report.offline);
        assert!(!lib.watched_folders().unwrap()[0].online);
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
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
        let report = scan_watched(&lib, &watched, 2, &options, &mut progress_only(|_| {})).unwrap();

        assert!(report.cancelled);
        assert!(!lib.watched_folders().unwrap()[0].online);
    }
}
