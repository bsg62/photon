use crate::{
    Result,
    keywords::read_keywords,
    library::{KnownItem, Library, NewItem, WatchedFolder},
    media::MediaKind,
    metadata::{EXIF_VERSION, read_image_meta},
    paths,
    picasa::{Face, FolderIni},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
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

/// How many files the walk passes between progress reports it makes on its own, apart
/// from the ones every batch flush makes. A rescan of an unchanged folder flushes nothing,
/// so without these it would report once, at the end, and the status bar would show no
/// progress for exactly the scan a person triggers most. The sink throttles what it emits,
/// so this only bounds how stale its view of the count can be.
const PROGRESS_EVERY: u64 = 64;

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
    /// Items whose faces the Picasa pass changed this scan. Like `restarred`, populated
    /// for photos the walk found unchanged: naming a face in Picasa rewrites the INI, not
    /// the photo.
    pub refaced: u64,
    /// Items whose `hidden` the Picasa pass changed this scan, following a `hidden=` line
    /// Picasa added or removed. Like `restarred`, the photo itself never changes.
    pub rehidden: u64,
    /// Photos whose Picasa-album memberships the Picasa pass changed this scan, plus Picasa
    /// albums it inserted or renamed. Like `restarred`, the photo itself never changes; a
    /// rename alone counts, because only the sidebar shows it.
    pub realbumed: u64,
    /// Unchanged files re-read because their stored metadata predates the current reader
    /// (`items.exif_version` behind `metadata::EXIF_VERSION`). The backfill for a library
    /// indexed before a camera column existed.
    pub enriched: u64,
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
    ///
    /// `refaced`, `rehidden`, `realbumed` and `enriched` count because a view can be built
    /// from what they change: the Person view from faces, the Album view and its sidebar list
    /// from Picasa's albums, every view from the hidden flag, the Tag and Search views from
    /// keywords and camera columns.
    pub fn touched_rows(&self) -> bool {
        self.added
            + self.changed
            + self.marked_missing
            + self.purged
            + self.restarred
            + self.refaced
            + self.rehidden
            + self.realbumed
            + self.enriched
            > 0
    }
}

/// Where a running scan reports to.
///
/// A trait rather than a second closure because the two reports have different consumers:
/// `progress` feeds the UI's scan counter, `indexed` feeds the thumbnail queue. A caller
/// that wants only progress wraps a closure in [`progress_only`].
pub trait ScanSink {
    /// Called after every batch, every [`PROGRESS_EVERY`] files the walk passes, and once
    /// more at the end of the scan.
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
        let applied = apply_picasa(lib, &walked);
        progress.progress(&seen);
        return Ok(ScanReport {
            restarred: applied.restarred,
            refaced: applied.refaced,
            rehidden: applied.rehidden,
            realbumed: applied.realbumed,
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

    let applied = apply_picasa(lib, &walked);

    // Anything left in `known` was not found on this (reachable) scan: soft-delete it,
    // or purge it if it was already missing last time.
    let (marked, purged) = finish_mark_purge(lib, known, &incomplete_prefixes)?;
    lib.prune_folders(watched.id, scan_id)?;

    progress.progress(&seen);
    Ok(ScanReport {
        marked_missing: marked,
        purged,
        restarred: applied.restarred,
        refaced: applied.refaced,
        rehidden: applied.rehidden,
        realbumed: applied.realbumed,
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
    let target = paths::canonicalize(&target).unwrap_or(target);

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
    // comment). Every non-cancelled walk applies the stars and faces of the folders it
    // reached.
    let applied = apply_picasa(lib, &outcome.walked);

    if outcome.skip_mark_purge {
        progress.progress(&outcome.seen);
        return Ok(ScanReport {
            restarred: applied.restarred,
            refaced: applied.refaced,
            rehidden: applied.rehidden,
            realbumed: applied.realbumed,
            ..outcome.report
        });
    }

    let mut report = outcome.report;
    let (marked, purged) = finish_mark_purge(lib, known, &outcome.incomplete_prefixes)?;
    lib.prune_folders_under(watched.id, scan_id, target_str)?;
    report.marked_missing = marked;
    report.purged = purged;
    report.restarred = applied.restarred;
    report.refaced = applied.refaced;
    report.rehidden = applied.rehidden;
    report.realbumed = applied.realbumed;

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
    let mut meta_batch: Vec<(i64, NewItem)> = Vec::new();
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
        if seen.files_seen.is_multiple_of(PROGRESS_EVERY) {
            progress.progress(&seen);
        }

        match known.remove(path_str) {
            Some(k) if k.size == size && k.mtime_ms == mtime_ms && !k.missing => {
                report.unchanged += 1;
                // The file is as it was, but the reader that last looked at it knew less
                // than the current one does (a camera column added since). This is the
                // only place an unchanged file is ever re-read, and it is what backfills
                // a library indexed before the column existed; a scan that skipped it
                // would leave every old photo without camera metadata for good.
                if k.exif_version < EXIF_VERSION {
                    meta_batch.push((
                        k.id,
                        describe(&entry, path_str, folder_id, kind, size, mtime_ms),
                    ));
                }
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
        if meta_batch.len() >= BATCH {
            flush_meta(lib, &mut meta_batch, &mut report)?;
        }
    }
    flush_new(lib, &mut new_batch, &mut report, &mut seen, progress)?;
    flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;
    flush_meta(lib, &mut meta_batch, &mut report)?;

    Ok(WalkOutcome {
        report,
        seen,
        walked,
        incomplete_prefixes,
        skip_mark_purge,
        cancelled,
    })
}

/// Applies each walked folder's Picasa stars, faces and contacts to its photos. Returns how
/// many items' ratings and how many items' faces actually changed, for
/// [`ScanReport::restarred`] and [`ScanReport::refaced`].
///
/// Runs after the walk rather than inside `describe()`, which is called only for photos
/// whose size or mtime changed. Starring a photo or naming a face in Picasa rewrites the
/// folder's INI and leaves the photo untouched, so on a rescan every photo takes the
/// `unchanged` branch and anything read in `describe()` would never be written.
///
/// The INI is the only authority: a photo it does not name is set to unstarred and
/// faceless, so removing a star or a face in Picasa clears it here too. A folder that
/// cannot be read is skipped instead, because failing to read is not evidence that they
/// are gone.
///
/// Infallible: a per-folder DB error (a busy database, say) is logged and skipped rather
/// than aborting the whole scan, since that would also skip `finish_mark_purge` and
/// `prune_folders` over an unrelated folder's transient failure. Nothing is lost — the next
/// scan reapplies this folder's INI.
/// What the Picasa pass changed, for the report's counters.
#[derive(Clone, Copy, Debug, Default)]
struct PicasaApplied {
    restarred: u64,
    refaced: u64,
    rehidden: u64,
    realbumed: u64,
}

fn apply_picasa(lib: &Library, walked: &[(PathBuf, i64)]) -> PicasaApplied {
    let mut applied = PicasaApplied::default();
    for (dir, folder_id) in walked {
        let Some(ini) = crate::picasa::read_folder(dir) else {
            tracing::debug!(
                ?dir,
                "leaving stars and faces alone for an unreadable folder"
            );
            continue;
        };
        match apply_folder_ini(lib, *folder_id, &ini) {
            Ok(folder) => {
                applied.restarred += folder.restarred;
                applied.refaced += folder.refaced;
                applied.rehidden += folder.rehidden;
                applied.realbumed += folder.realbumed;
            }
            Err(err) => {
                // One folder's transient failure (a busy database, say) must not cost the
                // whole scan its mark/purge/prune. The next scan reapplies this folder's
                // INI.
                tracing::warn!(%err, ?dir, "could not apply the Picasa INI for a folder");
            }
        }
    }
    applied
}

/// Contacts first, so a face written below can already resolve its name; then albums, stars,
/// faces and hidden flags from one read of the folder's item names.
fn apply_folder_ini(lib: &Library, folder_id: i64, ini: &FolderIni) -> Result<PicasaApplied> {
    lib.upsert_contacts(&ini.contacts)?;
    let names = lib.folder_item_names(folder_id)?;
    Ok(PicasaApplied {
        realbumed: apply_folder_albums(lib, folder_id, &names, ini)?,
        restarred: apply_folder_stars(lib, &names, &ini.stars)?,
        refaced: apply_folder_faces(lib, folder_id, &names, &ini.faces)?,
        rehidden: apply_folder_hidden(lib, folder_id, &names, &ini.hidden)?,
    })
}

/// Mirrors the folder's Picasa albums onto its photos, and returns how many photos'
/// memberships moved plus how many albums were inserted or renamed.
///
/// Mirrored like faces, not followed on change like `hidden=`: photon never writes an album,
/// so there is no photon-side answer to protect, and an INI that no longer lists a photo in an
/// album means Picasa took it out. A photo's photon albums are out of reach of this:
/// `set_picasa_album_items` deletes only Picasa albums' rows. Only photos that differ are
/// written, so an agreeing folder costs nothing.
fn apply_folder_albums(
    lib: &Library,
    folder_id: i64,
    names: &[(i64, String, Option<i64>)],
    ini: &FolderIni,
) -> Result<u64> {
    let referenced: HashSet<String> = ini.item_albums.values().flatten().cloned().collect();
    let (ids, upserted) = lib.upsert_picasa_albums(&ini.albums, &referenced, crate::now_ms())?;
    let current = lib.folder_picasa_albums(folder_id)?;
    let none = BTreeSet::new();
    let changes: Vec<(i64, BTreeSet<i64>)> = names
        .iter()
        .filter_map(|(id, name, _)| {
            let wanted: BTreeSet<i64> = ini
                .item_albums
                .get(name)
                .into_iter()
                .flatten()
                .filter_map(|token| ids.get(token).copied())
                .collect();
            (wanted != *current.get(id).unwrap_or(&none)).then_some((*id, wanted))
        })
        .collect();
    let moved = changes.len() as u64;
    for chunk in changes.chunks(BATCH) {
        lib.set_picasa_album_items(chunk, crate::now_ms())?;
    }
    Ok(upserted + moved)
}

/// Follows Picasa's `hidden=yes` where it changed, and returns how many photos' `hidden`
/// that moved.
///
/// The INI is followed on change, not mirrored (see `Library::apply_picasa_hidden`), with
/// one exception: the first read of a photo (`picasa_hidden` still NULL) follows a
/// `hidden=yes` - Picasa hid it, and a user coming from Picasa expects it hidden - but not
/// a missing line, so a photo the user hid in photon before this pass existed stays hidden.
/// Only rows whose INI answer differs from the recorded one are written, so an agreeing
/// folder costs nothing, as with stars and faces.
fn apply_folder_hidden(
    lib: &Library,
    folder_id: i64,
    names: &[(i64, String, Option<i64>)],
    hidden: &std::collections::HashSet<String>,
) -> Result<u64> {
    let recorded: HashMap<i64, Option<bool>> =
        lib.folder_picasa_hidden(folder_id)?.into_iter().collect();
    let changes: Vec<(i64, bool, bool)> = names
        .iter()
        .filter_map(|(id, name, _)| {
            let says = hidden.contains(name);
            match recorded.get(id).copied().flatten() {
                Some(before) if before == says => None,
                Some(_) => Some((*id, says, true)),
                None => Some((*id, says, says)),
            }
        })
        .collect();
    let mut moved = 0;
    for chunk in changes.chunks(BATCH) {
        moved += lib.apply_picasa_hidden(chunk)?;
    }
    Ok(moved)
}

/// Sets `folder_id`'s items' ratings from `stars`, writing only the rows whose rating
/// actually changes, and returns how many that was. Comparing before writing is what makes
/// a scan of an all-agreeing folder cost zero transactions, and what lets the caller tell a
/// real star change from a no-op scan.
fn apply_folder_stars(
    lib: &Library,
    names: &[(i64, String, Option<i64>)],
    stars: &std::collections::HashSet<String>,
) -> Result<u64> {
    let ratings: Vec<(i64, u8)> = names
        .iter()
        .filter_map(|(id, name, current)| {
            let wanted = u8::from(stars.contains(name));
            (*current != Some(wanted as i64)).then_some((*id, wanted))
        })
        .collect();
    let changed = ratings.len() as u64;
    for chunk in ratings.chunks(BATCH) {
        lib.set_ratings(chunk)?;
    }
    Ok(changed)
}

/// Sets each item's faces from the INI, writing only the items whose list differs from
/// what is stored, and returns how many that was. The comparison is exact, in order:
/// Picasa lists faces in a stable order, so a folder that agrees costs nothing.
fn apply_folder_faces(
    lib: &Library,
    folder_id: i64,
    names: &[(i64, String, Option<i64>)],
    faces: &HashMap<String, Vec<Face>>,
) -> Result<u64> {
    let current = lib.folder_faces(folder_id)?;
    let empty: Vec<Face> = Vec::new();
    let changes: Vec<(i64, Vec<Face>)> = names
        .iter()
        .filter_map(|(id, name, _)| {
            let wanted = faces.get(name).unwrap_or(&empty);
            let stored = current.get(id).unwrap_or(&empty);
            (wanted != stored).then(|| (*id, wanted.clone()))
        })
        .collect();
    let changed = changes.len() as u64;
    for chunk in changes.chunks(BATCH) {
        lib.set_item_faces(chunk)?;
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
        // Always `None` here; `apply_picasa` sets the real value after the walk.
        rating: meta.rating,
        camera: meta.camera,
        tags: read_keywords(entry.path()),
    }
}

/// Writes the re-read metadata of unchanged files. No `indexed` report: nothing about
/// these rows' thumbnails changed.
fn flush_meta(
    lib: &Library,
    batch: &mut Vec<(i64, NewItem)>,
    report: &mut ScanReport,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.update_item_meta(batch)?;
    report.enriched += batch.len() as u64;
    batch.clear();
    Ok(())
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
    use crate::testutil::{
        ExifSpec, avif_fixture, bmp_bytes, jpeg_bytes, jpeg_with_exif, jpeg_with_exif_spec,
        jpeg_with_iptc_keywords, png_bytes, temp_library, tiff_bytes, write_file,
    };
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
        paths::canonicalize(root).unwrap()
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

    /// The status bar's bar. A rescan of an unchanged folder flushes no batch, and the
    /// batch flushes were the only reports the walk made: the first event the UI saw was
    /// the final one, so a rescan showed no progress at all. Reverting the per-file report
    /// brings this back to exactly one call.
    #[test]
    fn an_unchanged_rescan_still_reports_progress_during_the_walk() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        for i in 0..(PROGRESS_EVERY as usize * 2 + 5) {
            write_file(&root, &format!("{i:04}.jpg"), &jpeg_bytes(4, 4));
        }
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let mut reports: Vec<u64> = Vec::new();
        let report = scan_watched(
            &lib,
            &watched,
            2,
            &ScanOptions::default(),
            &mut progress_only(|p| reports.push(p.files_seen)),
        )
        .unwrap();

        assert_eq!((report.added, report.changed), (0, 0), "nothing changed");
        assert_eq!(
            reports,
            [PROGRESS_EVERY, 2 * PROGRESS_EVERY, 2 * PROGRESS_EVERY + 5],
            "two reports on the way and the final one, not just the final one"
        );
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
    fn a_face_named_in_picasa_is_picked_up_without_the_photo_changing() {
        // The face pass is the star pass's twin: naming a face in Picasa rewrites the INI
        // and leaves the photo untouched, so it takes the `unchanged` branch and only a
        // post-walk pass can see it. Removing the face from the INI clears it again.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert!(lib.people_with_counts().unwrap().is_empty());

        write_file(
            &root,
            ".picasa.ini",
            b"[Contacts2]\nb5d3a7e4f1c2d9a8=Ada Lovelace;;\n[a.jpg]\nfaces=rect64(4000200080006000),b5d3a7e4f1c2d9a8\n",
        );
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.unchanged, report.refaced), (1, 1));
        assert!(
            report.touched_rows(),
            "the grid must rebuild for the People list"
        );
        let people = lib.people_with_counts().unwrap();
        assert_eq!(
            (people[0].name.as_str(), people[0].count),
            ("Ada Lovelace", 1)
        );

        let report = scan(&lib, &watched, 3);
        assert_eq!(report.refaced, 0, "an agreeing folder writes nothing");

        write_file(&root, ".picasa.ini", b"[a.jpg]\nbackuphash=1\n");
        let report = scan(&lib, &watched, 4);
        assert_eq!(report.refaced, 1);
        assert!(lib.people_with_counts().unwrap().is_empty());
    }

    #[test]
    fn a_picasa_album_follows_the_ini_without_the_photo_changing() {
        // Like faces: an album is an INI change, so it takes the `unchanged` branch and only
        // the post-walk pass sees it. An agreeing rescan counts nothing, or every scan of a
        // Picasa library would rebuild the grid; a rename alone must refresh, because only
        // the sidebar shows it.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(
            &root,
            ".picasa.ini",
            b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n",
        );
        let report = scan(&lib, &watched, 2);
        assert_eq!(report.unchanged, 1, "the photo itself did not change");
        assert!(report.realbumed > 0);
        assert!(report.touched_rows());
        let listed = |lib: &Library| -> Vec<(String, i64, bool)> {
            lib.albums_with_counts()
                .unwrap()
                .into_iter()
                .map(|a| (a.name, a.count, a.picasa))
                .collect()
        };
        assert_eq!(listed(&lib), vec![("Holiday".to_string(), 1, true)]);

        let report = scan(&lib, &watched, 3);
        assert_eq!(report.realbumed, 0, "an agreeing folder writes nothing");

        write_file(
            &root,
            ".picasa.ini",
            b"[.album:t]\nname=Summer\n[a.jpg]\nalbums=t\n",
        );
        let report = scan(&lib, &watched, 4);
        assert_eq!(report.realbumed, 1, "the rename, and no membership moved");
        assert!(
            report.touched_rows(),
            "a rename alone refreshes the sidebar"
        );
        assert_eq!(listed(&lib), vec![("Summer".to_string(), 1, true)]);

        write_file(
            &root,
            ".picasa.ini",
            b"[.album:t]\nname=Summer\n[a.jpg]\nbackuphash=1\n",
        );
        let report = scan(&lib, &watched, 5);
        assert_eq!(report.realbumed, 1);
        assert!(
            listed(&lib).is_empty(),
            "Picasa took the photo out, and the album is empty"
        );
    }

    #[test]
    fn a_picasa_album_spanning_two_folders_is_one_album() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b/two.jpg", &jpeg_bytes(4, 3));
        write_file(
            &root,
            "a/.picasa.ini",
            b"[.album:t]\nname=Holiday\n[one.jpg]\nalbums=t\n",
        );
        // The second folder only names the token: its name must not win.
        write_file(&root, "b/.picasa.ini", b"[two.jpg]\nalbums=T\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let albums: Vec<(String, i64)> = lib
            .albums_with_counts()
            .unwrap()
            .into_iter()
            .map(|a| (a.name, a.count))
            .collect();
        assert_eq!(albums, vec![("Holiday".to_string(), 2)]);
    }

    #[test]
    fn a_subtree_scan_applies_picasa_albums_too() {
        // The watcher's path. Passes only because `scan_subtree` shares `apply_picasa` with
        // `scan_watched`; the mistake it catches is wiring the album pass into one of them.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(
            &root,
            "sub/.picasa.ini",
            b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n",
        );
        let report = scan_sub(&lib, &watched, &root.join("sub"), 2);
        assert!(report.realbumed > 0);
        assert!(report.touched_rows());
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn an_unreadable_ini_keeps_a_photos_picasa_albums() {
        // A pin on the existing `None` branch of `apply_picasa`, for the new data: a symlinked
        // INI is refused by the reader (v0.28.2) and must read as no evidence, not as "no
        // albums". Passes before this task's code as long as the album pass sits inside
        // `apply_folder_ini`, which is the point.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(
            &root,
            ".picasa.ini",
            b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n",
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);

        let elsewhere = dir.path().join("elsewhere.ini");
        std::fs::write(&elsewhere, b"[a.jpg]\nbackuphash=1\n").unwrap();
        std::fs::remove_file(root.join(".picasa.ini")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join(".picasa.ini")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert_eq!(report.realbumed, 0);
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);
    }

    /// End to end, on both of `walk_tree`'s callers: a file that lands in a hidden folder is
    /// hidden when the scan indexes it, and a photo the user unhid inside that folder is
    /// left visible by the rescan - only new rows inherit the folder's flag.
    #[test]
    fn a_file_scanned_into_a_hidden_folder_arrives_hidden() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let sub = lib
            .folders()
            .unwrap()
            .into_iter()
            .find(|f| f.path.ends_with("sub"))
            .unwrap()
            .id;
        lib.set_folder_hidden(sub, true).unwrap();
        let first = lib.entries_for(crate::grid::GridView::Hidden, "").unwrap()[0].id;
        lib.set_hidden(&[first], false).unwrap();

        write_file(&root, "sub/b.jpg", &jpeg_bytes(4, 3));
        scan(&lib, &watched, 2);
        write_file(&root, "sub/c.jpg", &jpeg_bytes(4, 5));
        scan_sub(&lib, &watched, &root.join("sub"), 3);

        let hidden = lib.entries_for(crate::grid::GridView::Hidden, "").unwrap();
        assert_eq!(
            hidden.len(),
            2,
            "a file scanned into a hidden folder is visible"
        );
        assert!(
            !is_hidden(&lib, first),
            "a rescan re-hid a photo the user unhid"
        );
    }

    fn is_hidden(lib: &Library, id: i64) -> bool {
        lib.item(id).unwrap().unwrap().hidden
    }

    #[test]
    fn a_photo_hidden_in_picasa_is_hidden_in_photon() {
        // Picasa's `hidden=yes` is the star pass's third twin: the INI changes, the photo
        // does not, so only the post-walk pass sees it - and the grid must rebuild for it.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b.jpg", &jpeg_bytes(4, 3));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.hidden_count().unwrap(), 0);

        write_file(&root, ".picasa.ini", b"[a.jpg]\nhidden=yes\n");
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.unchanged, report.rehidden), (2, 1));
        assert!(
            report.touched_rows(),
            "the grid must rebuild to drop the photo"
        );
        assert_eq!(lib.hidden_count().unwrap(), 1);

        let report = scan(&lib, &watched, 3);
        assert_eq!(report.rehidden, 0, "an agreeing folder writes nothing");

        write_file(&root, ".picasa.ini", b"[a.jpg]\nbackuphash=1\n");
        let report = scan(&lib, &watched, 4);
        assert_eq!(report.rehidden, 1, "unhidden in Picasa, unhidden in photon");
        assert_eq!(lib.hidden_count().unwrap(), 0);
    }

    #[test]
    fn an_unhide_in_photon_holds_until_picasa_changes_its_answer() {
        // photon never writes `hidden=`, so the INI keeps saying yes after the user unhides
        // the photo here. Mirroring the INI would hide it again on every scan; following
        // only its changes lets the user's answer stand until Picasa gives a new one.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nhidden=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.entries_for(crate::grid::GridView::Hidden, "").unwrap()[0].id;

        lib.set_hidden(&[id], false).unwrap();
        let report = scan(&lib, &watched, 2);
        assert_eq!(report.rehidden, 0);
        assert!(!is_hidden(&lib, id), "a rescan undid the user's unhide");

        // Picasa unhides and hides it again: a real change, so it is followed.
        write_file(&root, ".picasa.ini", b"[a.jpg]\n");
        scan(&lib, &watched, 3);
        write_file(&root, ".picasa.ini", b"[a.jpg]\nhidden=yes\n");
        scan(&lib, &watched, 4);
        assert!(is_hidden(&lib, id));
    }

    #[test]
    fn a_photo_hidden_in_photon_stays_hidden_when_picasa_never_hid_it() {
        // The first read of a photo with no `hidden=` line records the answer and leaves the
        // flag alone: the user hid it here, and Picasa has said nothing about it either way.
        // A later hide in photon of a photo Picasa has read survives rescans too.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.entries_for(crate::grid::GridView::All, "").unwrap()[0].id;
        lib.set_hidden(&[id], true).unwrap();
        // A library upgraded from before the column: the INI never read for this photo.
        rusqlite::Connection::open(dir.path().join("library.db"))
            .unwrap()
            .execute("UPDATE items SET picasa_hidden = NULL", [])
            .unwrap();

        scan(&lib, &watched, 2);
        assert!(
            is_hidden(&lib, id),
            "the first read unhid a photo the user hid"
        );
        scan(&lib, &watched, 3);
        assert!(is_hidden(&lib, id), "a rescan unhid a photo the user hid");
    }

    #[test]
    fn a_photo_indexed_before_the_camera_columns_is_re_read_on_the_next_scan() {
        // The backfill. A row with `exif_version = 0` is what every photo indexed before
        // this feature looks like after the migration; the file has not changed, so only
        // the version check can bring it back through `describe()`. Reverting that check
        // leaves `make` NULL forever, which is what the final assertion catches.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let spec = ExifSpec {
            make: Some("Canon"),
            ..ExifSpec::default()
        };
        let a = write_file(&root, "a.jpg", &jpeg_with_exif_spec(4, 2, &spec));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        assert_eq!(
            lib.item(id).unwrap().unwrap().camera.make.as_deref(),
            Some("Canon")
        );

        // A library from before the columns: the make is gone and the row is unread.
        lib.forget_metadata_for_test(id).unwrap();
        lib.set_thumb_state(id, crate::media::ThumbState::Ready, None)
            .unwrap();

        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.unchanged, report.changed, report.enriched),
            (1, 0, 1)
        );
        assert!(report.touched_rows());
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.camera.make.as_deref(), Some("Canon"));
        assert_eq!(
            item.thumb_state,
            crate::media::ThumbState::Ready,
            "a metadata re-read is not a file change: the thumbnail stays"
        );

        let report = scan(&lib, &watched, 3);
        assert_eq!(report.enriched, 0, "read once, not on every scan");
    }

    #[test]
    fn a_photo_dated_by_a_credulous_reader_is_re_dated_on_the_next_scan() {
        // A library indexed before EXIF_VERSION 2 holds whatever date the file claimed.
        // The file never changes, so only the backfill can correct the row, and only if
        // `update_item_meta` writes `taken_at`: dropping it from that UPDATE leaves the
        // year 4501 in place, which is what the assertion after the second scan catches.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(
            &root,
            "a.jpg",
            &jpeg_with_exif(4, 2, 1, "4501:01:01 00:00:00"),
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        let mtime_s = lib.item(id).unwrap().unwrap().taken_at;
        assert!(
            mtime_s < 4_000_000_000,
            "a fresh index already refuses the date and uses the mtime"
        );

        let year_4501 = crate::metadata::naive_to_unix(4501, 1, 1, 0, 0, 0);
        lib.misdate_for_test(id, year_4501).unwrap();

        let report = scan(&lib, &watched, 2);
        assert_eq!((report.unchanged, report.enriched), (1, 1));
        assert_eq!(lib.item(id).unwrap().unwrap().taken_at, mtime_s);
    }

    #[test]
    fn keywords_are_indexed_with_the_photo() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(
            &root,
            "a.jpg",
            &jpeg_with_iptc_keywords(4, 2, &[b"beach", b"summer"]),
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        assert_eq!(lib.item_tags(id).unwrap(), ["beach", "summer"]);
        assert_eq!(lib.tags_with_counts().unwrap().len(), 2);

        // Re-tagged in place: the file changes, and the keyword list follows it.
        write_file(&root, "a.jpg", &jpeg_with_iptc_keywords(8, 8, &[b"beach"]));
        scan(&lib, &watched, 2);
        assert_eq!(lib.item_tags(id).unwrap(), ["beach"]);
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

    /// The watcher's path, for the hidden flag: hiding in Picasa rewrites one folder's INI,
    /// and the report must say so or the grid never drops the photo.
    #[test]
    fn a_subtree_scan_applies_picasas_hidden_flag_too() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(&root, "sub/.picasa.ini", b"[a.jpg]\nhidden=yes\n");
        let report = scan_sub(&lib, &watched, &root.join("sub"), 2);
        assert_eq!(report.rehidden, 1);
        assert!(report.touched_rows());
        assert_eq!(lib.hidden_count().unwrap(), 1);
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

    /// TIFFs and BMPs are photos like any other: indexed by the walk and described, so
    /// their real dimensions reach the row rather than a placeholder. The two files have
    /// different shapes so a swapped or defaulted size cannot pass.
    #[test]
    fn indexes_tiff_and_bmp_files() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let tif = write_file(&root, "scan.tif", &tiff_bytes(40, 10));
        let bmp = write_file(&root, "old.bmp", &bmp_bytes(12, 24));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let report = scan(&lib, &watched, 1);
        assert_eq!(report.added, 2);

        let known = lib.known_items(watched.id).unwrap();
        let shape = |path: &Path| {
            let item = lib.item(known[&key(path)].id).unwrap().unwrap();
            (item.width, item.height)
        };
        assert_eq!(shape(&tif), (40, 10));
        assert_eq!(shape(&bmp), (12, 24));
    }

    /// An AVIF is indexed beside a JPEG with its displayed size: the grid photo at its
    /// stitched size, the rotated one turned.
    #[test]
    fn indexes_avif_files() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let grid = write_file(&root, "grid.avif", &avif_fixture("grid_10bit.avif"));
        let turned = write_file(&root, "turned.avif", &avif_fixture("irot90.avif"));
        write_file(&root, "plain.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let report = scan(&lib, &watched, 1);
        assert_eq!(report.added, 3);

        let known = lib.known_items(watched.id).unwrap();
        let shape = |path: &Path| {
            let item = lib.item(known[&key(path)].id).unwrap().unwrap();
            (item.width, item.height, item.orientation)
        };
        assert_eq!(shape(&grid), (128, 128, 1));
        assert_eq!(shape(&turned), (32, 64, 1));
    }
}
