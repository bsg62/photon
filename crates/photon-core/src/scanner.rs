use crate::{
    Result,
    library::{FolderItem, KnownItem, Library, MoveCandidate, NewItem, WatchedFolder},
    media::{MediaKind, fingerprint},
    metadata::{CameraMeta, EXIF_VERSION, read_image},
    moved, paths,
    picasa::{Face, FolderIni, IniListing},
};
use std::{
    cell::RefCell,
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
    /// Rows re-pointed to their file's new path: a photo renamed or moved on disk, found as
    /// a new file and recognised as a row whose own file is gone (`crate::moved`). Like
    /// `restarred` it can be the only non-zero field, since a followed move is neither an
    /// added row nor a changed one.
    pub moved: u64,
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
    /// keywords and camera columns. `moved` counts because a re-pointed row is in another
    /// folder, or under another name, than the grid shows it in.
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
            + self.moved
            > 0
    }
}

/// Where a running scan reports to.
///
/// A trait rather than a second closure because the reports have different consumers:
/// `progress` feeds the UI's scan counter, `indexed` feeds the thumbnail queue, `moved` the
/// thumbnail cache. A caller that wants only progress wraps a closure in [`progress_only`].
pub trait ScanSink {
    /// Called after every batch, every [`PROGRESS_EVERY`] files the walk passes, and once
    /// more at the end of the scan. A full scan also calls it once before its walk, with
    /// nothing seen, as soon as it has found its root (`scan_watched`).
    fn progress(&mut self, progress: &ScanProgress);

    /// Items just inserted, replaced or re-pointed to their file's new path. Each has
    /// `thumb_state = Pending`, so these are exactly the rows `Library::pending_thumb_ids`
    /// would find for *some* `MediaKind` - photos and videos alike, handed over without the
    /// query: re-running it every 250ms of a scan sorted every pending row in grid order,
    /// for the whole of an import, to learn what the scanner already knew. `Engine`'s
    /// consumer of this sink prioritises the ids into the photo thumbnail queue regardless
    /// of kind; a video id lands there too, and that queue's `process` simply ignores it,
    /// since videos are drained from their own queue instead (see `thumbs`).
    fn indexed(&mut self, _ids: &[i64]) {}

    /// Rows just re-pointed to their file's new path, each as its `Item::thumb_key()` before
    /// and after. The path is part of the key, so the row now names thumbnails that are not
    /// there, while the ones cached under the old key are of this very picture: with the
    /// pair a consumer can carry them over instead of rendering them again. Called before
    /// `indexed` for the same rows, so they are in place when the queue reaches the row.
    fn moved(&mut self, _keys: &[(u64, u64)]) {}
}

/// A sink that reports progress to `f` and ignores `indexed` and `moved`.
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

    // The scan's opening report, with nothing seen. The walk's own first report is
    // `PROGRESS_EVERY` photos in, a flushed batch, or its end: on a tree with many other
    // files ahead of its photos, or a cold network share, that is a long time in which
    // nothing says a scan is running. After the root has been found, not before: a root
    // that is not there is polled twice a minute, and must not announce a scan each time.
    // Only here, not in `scan_subtree`: the watcher runs one of those for every directory
    // a file changed in, most over in milliseconds, and each would flash the scan's line.
    progress.progress(&ScanProgress::default());

    let mut known = lib.known_items(watched.id)?;
    let mut folder_ids: HashMap<PathBuf, i64> = HashMap::new();
    let mut rows = FolderRows::load(lib, watched.id)?;

    let WalkOutcome {
        report,
        seen,
        walked,
        inis,
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
        &mut rows,
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
        let applied = apply_picasa(
            lib,
            &walked,
            &IniEvidence::new(&inis, &incomplete_prefixes, skip_mark_purge),
        );
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

    let applied = apply_picasa(
        lib,
        &walked,
        &IniEvidence::new(&inis, &incomplete_prefixes, skip_mark_purge),
    );

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
    //
    // The same predicate as the walk's, so a recycle bin is refused here as well. That is
    // the scan a deleted photo causes: the event is in the bin, and walked from here the bin
    // is where `flush_new` found the photo "moved to" (`passed_over`).
    if relative.components().any(|c| passed_over(c.as_os_str())) {
        return Ok(ScanReport::default());
    }

    let target_str = target
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(target.clone()))?;
    let mut known = lib.known_items_under(watched.id, target_str)?;
    let mut rows = FolderRows::load(lib, watched.id)?;
    let (mut folder_ids, parent_id) = seed_ancestors(lib, watched, relative, &mut rows, scan_id)?;

    let outcome = walk_tree(
        lib,
        watched.id,
        &target,
        parent_id,
        &mut known,
        &mut folder_ids,
        &mut rows,
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
    let applied = apply_picasa(lib, &outcome.walked, &outcome.ini_evidence());

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

/// What an INI pass did, and the folders it could not do it for.
#[derive(Debug, Default)]
pub struct IniPass {
    pub report: ScanReport,
    /// Folders photon has no row for: never scanned, so a new folder whose INI appeared with
    /// it. A walk's to index, not this pass's.
    pub needs_walk: Vec<PathBuf>,
}

/// Rereads the Picasa INI of each folder in `dirs` and applies it - stars, faces, the hidden
/// flag and albums - without walking anything: for a folder whose only change is its INI
/// (the watcher's `Changed::ini_dirs`), which is what photon's own star writes and a Picasa
/// album rename produce.
///
/// Through the scan's own `apply_picasa`, so it applies exactly the scan's rules. What a scan
/// does besides is left out on purpose - no mark or purge, no pruning, no thumbnails, no
/// metadata - since nothing but the INI changed; that is what makes photon's own echo cost
/// one INI read per folder.
pub fn refresh_picasa(
    lib: &Library,
    watched: &WatchedFolder,
    dirs: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<IniPass> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(IniPass {
            report: ScanReport {
                offline: true,
                ..ScanReport::default()
            },
            ..IniPass::default()
        });
    }
    let rows = FolderRows::load(lib, watched.id)?;
    let mut pass = IniPass::default();
    let mut walked: Vec<(PathBuf, i64)> = Vec::new();
    for dir in dirs {
        if !crate::paths::is_within(dir, root) {
            continue;
        }
        match dir.to_str().and_then(|d| rows.stored.get(d)) {
            Some(&(id, _)) => walked.push((dir.clone(), id)),
            None => pass.needs_walk.push(dir.clone()),
        }
    }
    // In chunks so a cancel (a folder's removal, a quit) is honoured between folders
    // without paying `apply_picasa`'s per-call setup once per folder.
    for chunk in walked.chunks(64) {
        if cancel.load(Ordering::Relaxed) {
            pass.report.cancelled = true;
            break;
        }
        let applied = apply_picasa(lib, chunk, &IniEvidence::none());
        pass.report.restarred += applied.restarred;
        pass.report.refaced += applied.refaced;
        pass.report.rehidden += applied.rehidden;
        pass.report.realbumed += applied.realbumed;
    }
    Ok(pass)
}

/// Upserts the folder rows from the watched root down to the target's parent, so the walk
/// can attach the target to its real parent rather than treating it as a root. `relative` is
/// the target's path relative to the watched root (i.e. `target.strip_prefix(root)`).
/// Returns the ids it created, and the id of the target's parent.
///
/// An ancestor whose row already agrees is not written, and its `seen_scan` is left alone:
/// this scan prunes only inside the target (`prune_folders_under`), so nothing reads an
/// ancestor's marker against this scan's id.
fn seed_ancestors(
    lib: &Library,
    watched: &WatchedFolder,
    relative: &Path,
    rows: &mut FolderRows,
    scan_id: i64,
) -> Result<(HashMap<PathBuf, i64>, Option<i64>)> {
    let root = Path::new(&watched.path);
    let root_str = root
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(root.to_path_buf()))?;
    let mut ids = HashMap::new();
    let (root_id, _) = rows.ensure(lib, watched.id, None, root_str, scan_id)?;
    let mut parent = Some(root_id);
    ids.insert(root.to_path_buf(), root_id);

    let mut components: Vec<_> = relative.components().collect();
    components.pop(); // `target` itself is upserted by the walk.
    let mut current = root.to_path_buf();
    for component in components {
        current = current.join(component);
        let current_str = current
            .to_str()
            .ok_or_else(|| crate::Error::NonUtf8Path(current.clone()))?;
        let (id, _) = rows.ensure(lib, watched.id, parent, current_str, scan_id)?;
        ids.insert(current.clone(), id);
        parent = Some(id);
    }
    Ok((ids, parent))
}

/// A watched folder's folder rows as the scan began, so the walk writes only the folders
/// that are new or whose parent changed.
///
/// Before this every walked folder was upserted on every scan, one transaction each - and
/// since the upsert set `seen_scan` to the new scan's id, every one of them really changed.
/// On a network share of thousands of folders that was thousands of commits for a rescan
/// that found nothing. The `seen_scan` bump the prune needs is done in bulk at the end of
/// the walk instead (`walk_tree`).
struct FolderRows {
    stored: HashMap<String, (i64, Option<i64>)>,
    /// The folders this scan made a row for: a directory it had no row at the path of. One
    /// whose row only changed parent is not among them. `apply_moves` hands a folder's name
    /// and Hide folder flag on to these alone.
    created: HashSet<i64>,
}

impl FolderRows {
    fn load(lib: &Library, watched_id: i64) -> Result<Self> {
        Ok(Self {
            stored: lib.folder_rows(watched_id)?,
            created: HashSet::new(),
        })
    }

    /// The folder's id, and whether its row already agreed - in which case nothing was
    /// written and its `seen_scan` still names an older scan.
    fn ensure(
        &mut self,
        lib: &Library,
        watched_id: i64,
        parent: Option<i64>,
        path: &str,
        scan_id: i64,
    ) -> Result<(i64, bool)> {
        if let Some(&(id, stored_parent)) = self.stored.get(path)
            && stored_parent == parent
        {
            return Ok((id, true));
        }
        let id = lib.upsert_folder(watched_id, parent, path, scan_id)?;
        if self.stored.insert(path.to_string(), (id, parent)).is_none() {
            self.created.insert(id);
        }
        Ok((id, false))
    }
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
    /// The Picasa INIs the walk's own listings saw, by directory. A walked directory with
    /// no entry here had none when it was listed.
    inis: HashMap<PathBuf, IniListing>,
    /// Subtrees we could not fully walk: anything `known` claims to live under one of
    /// these might still exist, so it must not be marked missing or purged this scan.
    incomplete_prefixes: Vec<PathBuf>,
    /// Set when an error gives no path, or points at the root itself: the caller can no
    /// longer tell which `known` entries are safe to touch, so it must skip mark/purge/prune
    /// entirely.
    skip_mark_purge: bool,
    cancelled: bool,
}

impl WalkOutcome {
    fn ini_evidence(&self) -> IniEvidence<'_> {
        IniEvidence::new(&self.inis, &self.incomplete_prefixes, self.skip_mark_purge)
    }
}

/// What the walk's listings say about each walked directory's INI, and when they cannot be
/// taken as the whole answer.
struct IniEvidence<'a> {
    listed: &'a HashMap<PathBuf, IniListing>,
    incomplete: &'a [PathBuf],
    /// False when some walk error carried no path: any directory's listing may have been
    /// cut short, and nothing says which.
    whole: bool,
}

impl<'a> IniEvidence<'a> {
    fn new(
        listed: &'a HashMap<PathBuf, IniListing>,
        incomplete: &'a [PathBuf],
        skip_mark_purge: bool,
    ) -> Self {
        Self {
            listed,
            incomplete,
            whole: !skip_mark_purge,
        }
    }

    /// What `dir`'s listing saw, or `None` when that listing may be incomplete and the
    /// directory must be listed again. An error at `p` means `p`'s own contents are unknown,
    /// and also that its parent's listing may have lost an entry: an entry whose type could
    /// not be read is reported by its own path, and that entry may be the INI. An
    /// incomplete listing taken as whole would read "no INI" and clear the folder's stars.
    fn listing_of(&self, dir: &Path) -> Option<&'a IniListing> {
        static NONE: std::sync::LazyLock<IniListing> =
            std::sync::LazyLock::new(IniListing::default);
        let cut_short = self
            .incomplete
            .iter()
            .any(|p| dir.starts_with(p) || p.parent() == Some(dir));
        if !self.whole || cut_short {
            return None;
        }
        Some(self.listed.get(dir).unwrap_or(&NONE))
    }

    /// No listing at all: every folder is found by `read_folder`, as a folder whose listing
    /// may have been cut short is. For the INI pass, which lists nothing itself.
    fn none() -> IniEvidence<'static> {
        static EMPTY: std::sync::LazyLock<HashMap<PathBuf, IniListing>> =
            std::sync::LazyLock::new(HashMap::new);
        IniEvidence {
            listed: &EMPTY,
            incomplete: &[],
            whole: false,
        }
    }
}

/// Walks `root`, upserting folders and files into the library and removing matches from
/// `known` as they're found. Does not mark, purge or prune anything, or touch the watched
/// folder's online state; the caller decides that from the returned [`WalkOutcome`].
///
/// A file at a path no row has is not always a new photo: `flush_new` re-points the row of
/// one that was renamed or moved. That row is left in `known` under its old path, which is
/// why [`finish_mark_purge`] writes by path as well as by id.
#[allow(clippy::too_many_arguments)]
fn walk_tree(
    lib: &Library,
    watched_id: i64,
    root: &Path,
    root_parent_id: Option<i64>,
    known: &mut HashMap<String, KnownItem>,
    folder_ids: &mut HashMap<PathBuf, i64>,
    rows: &mut FolderRows,
    scan_id: i64,
    options: &ScanOptions,
    progress: &mut dyn ScanSink,
) -> Result<WalkOutcome> {
    let mut report = ScanReport::default();
    let mut seen = ScanProgress::default();
    let mut new_batch: Vec<NewItem> = Vec::new();
    let mut changed_batch: Vec<(i64, NewItem)> = Vec::new();
    let mut meta_batch: Vec<(i64, NewItem)> = Vec::new();
    // One for the whole walk: it asks each watched root once whether its drive is there.
    let mut probe = moved::Probe::new(watched_id);
    let mut walked: Vec<(PathBuf, i64)> = Vec::new();
    // Walked folders whose row already agreed: bumped to this scan in bulk after the walk.
    let mut unwritten: Vec<i64> = Vec::new();
    let mut incomplete_prefixes: Vec<PathBuf> = Vec::new();
    let mut skip_mark_purge = false;
    let mut cancelled = false;
    // Filled from inside `filter_entry`, which is the only place the walk sees the INI at
    // all: `.picasa.ini` is a dot-file, and the filter drops it. Recording it there is what
    // lets the Picasa pass skip listing every folder a second time.
    let inis: RefCell<HashMap<PathBuf, IniListing>> = RefCell::default();
    let excluded: Vec<paths::Folder> = options
        .excluded
        .iter()
        .map(|x| paths::Folder::new(x))
        .collect();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() > 0
                && let Some(name) = crate::picasa::ini_name_of(e.file_name())
                && let Some(dir) = e.path().parent()
            {
                // `file_type()` is the entry's own type and does not follow a link, the
                // same answer `picasa::ini_path` reads, so a symlinked INI is still refused.
                inis.borrow_mut()
                    .entry(dir.to_path_buf())
                    .or_default()
                    .record(name, e.path().to_path_buf(), e.file_type().is_file());
            }
            // Only a directory is checked against the excluded folders: a file inside one
            // can only be reached through it, and it was pruned here first. An excluded
            // folder is a directory by definition (`ScanOptions::excluded`).
            (e.depth() == 0 || !passed_over(e.file_name()))
                && !(e.file_type().is_dir() && excluded.iter().any(|x| x.contains(e.path())))
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
            let (id, agreed) = rows.ensure(lib, watched_id, parent, path_str, scan_id)?;
            if agreed {
                unwritten.push(id);
            }
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
            flush_new(
                lib,
                &mut new_batch,
                &mut probe,
                &rows.created,
                &mut report,
                &mut seen,
                progress,
            )?;
        }
        if changed_batch.len() >= BATCH {
            flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;
        }
        if meta_batch.len() >= BATCH {
            flush_meta(lib, &mut meta_batch, &mut report)?;
        }
    }
    flush_new(
        lib,
        &mut new_batch,
        &mut probe,
        &rows.created,
        &mut report,
        &mut seen,
        progress,
    )?;
    flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;
    flush_meta(lib, &mut meta_batch, &mut report)?;
    // Here rather than in either caller, because both prune afterwards and the prune deletes
    // every empty folder whose `seen_scan` is older than this scan: a folder the walk reached
    // but did not write must be marked before that, by whichever caller walked it.
    for chunk in unwritten.chunks(BATCH) {
        lib.mark_folders_seen(chunk, scan_id)?;
    }

    Ok(WalkOutcome {
        report,
        seen,
        walked,
        inis: inis.into_inner(),
        incomplete_prefixes,
        skip_mark_purge,
        cancelled,
    })
}

/// What the Picasa pass changed, for the report's counters.
#[derive(Clone, Copy, Debug, Default)]
struct PicasaApplied {
    restarred: u64,
    refaced: u64,
    rehidden: u64,
    realbumed: u64,
}

/// Below this many walked folders the Picasa pass asks each folder for its faces and album
/// memberships rather than loading which folders have any. Those two lookups read every
/// face and every membership in the library, which a watcher's one-folder subtree scan
/// should not pay for to save itself two indexed queries.
const PRESENCE_MIN_FOLDERS: usize = 16;

/// Which folders hold any faces or Picasa-album memberships, so a folder that has none and
/// whose INI names none can skip the query that would only find nothing. `None` asks every
/// folder.
struct PicasaPresence {
    faces: Option<HashSet<i64>>,
    albums: Option<HashSet<i64>>,
}

impl PicasaPresence {
    fn load(lib: &Library, walked: usize) -> Self {
        if walked < PRESENCE_MIN_FOLDERS {
            return Self::unknown();
        }
        // A failed lookup costs only the queries it would have saved.
        Self {
            faces: lib.folders_with_faces().ok(),
            albums: lib.folders_with_picasa_albums().ok(),
        }
    }

    fn unknown() -> Self {
        Self {
            faces: None,
            albums: None,
        }
    }

    fn may_have_faces(&self, folder_id: i64) -> bool {
        self.faces.as_ref().is_none_or(|f| f.contains(&folder_id))
    }

    fn may_have_albums(&self, folder_id: i64) -> bool {
        self.albums.as_ref().is_none_or(|a| a.contains(&folder_id))
    }
}

/// Applies each walked folder's Picasa stars, faces, contacts, hidden flags and albums to its
/// photos, and returns how many items each actually changed, for [`ScanReport::restarred`]
/// and its siblings.
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
/// Each folder's INI is found from what the walk's own listing of it recorded
/// ([`IniEvidence`]), not by listing the folder again: see `picasa::read_folder_listed` for
/// why that listing is checked rather than trusted. A folder whose listing may have been cut
/// short is listed again by `read_folder`, as before.
///
/// Infallible: a per-folder DB error (a busy database, say) is logged and skipped rather
/// than aborting the whole scan, since that would also skip `finish_mark_purge` and
/// `prune_folders` over an unrelated folder's transient failure. Nothing is lost — the next
/// scan reapplies this folder's INI.
fn apply_picasa(
    lib: &Library,
    walked: &[(PathBuf, i64)],
    evidence: &IniEvidence<'_>,
) -> PicasaApplied {
    let mut applied = PicasaApplied::default();
    let presence = PicasaPresence::load(lib, walked.len());
    for (dir, folder_id) in walked {
        let ini = match evidence.listing_of(dir) {
            Some(listing) => crate::picasa::read_folder_listed(dir, listing),
            None => crate::picasa::read_folder(dir),
        };
        let Some(ini) = ini else {
            tracing::debug!(
                ?dir,
                "leaving stars and faces alone for an unreadable folder"
            );
            continue;
        };
        match apply_folder_ini(lib, *folder_id, &ini, &presence) {
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
/// faces and hidden flags from one read of the folder's items.
fn apply_folder_ini(
    lib: &Library,
    folder_id: i64,
    ini: &FolderIni,
    presence: &PicasaPresence,
) -> Result<PicasaApplied> {
    lib.upsert_contacts(&ini.contacts)?;
    let items = lib.folder_item_names(folder_id)?;
    // Where the INI names no face and the library holds none, the mirror has nothing to
    // compare, so the query that would find nothing is skipped. The same for albums.
    let faces = !ini.faces.is_empty() || presence.may_have_faces(folder_id);
    let albums = !ini.item_albums.is_empty() || presence.may_have_albums(folder_id);
    Ok(PicasaApplied {
        realbumed: apply_folder_albums(lib, folder_id, &items, ini, albums)?,
        restarred: apply_folder_stars(lib, &items, &ini.stars)?,
        refaced: if faces {
            apply_folder_faces(lib, folder_id, &items, &ini.faces)?
        } else {
            0
        },
        rehidden: apply_folder_hidden(lib, &items, &ini.hidden)?,
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
///
/// `memberships` false says the INI assigns no photo to an album and the library holds no
/// Picasa membership in this folder: there is nothing to mirror, though the INI's album
/// names are still recorded.
fn apply_folder_albums(
    lib: &Library,
    folder_id: i64,
    items: &[FolderItem],
    ini: &FolderIni,
    memberships: bool,
) -> Result<u64> {
    let referenced: HashSet<String> = ini.item_albums.values().flatten().cloned().collect();
    let (ids, upserted) =
        lib.upsert_picasa_albums(&ini.albums, &referenced, ini.modified_ms, crate::now_ms())?;
    if !memberships {
        return Ok(upserted);
    }
    let current = lib.folder_picasa_albums(folder_id)?;
    let none = BTreeSet::new();
    let changes: Vec<(i64, BTreeSet<i64>)> = items
        .iter()
        .filter_map(|item| {
            let wanted: BTreeSet<i64> = ini
                .item_albums
                .get(&item.name)
                .into_iter()
                .flatten()
                .filter_map(|token| ids.get(token).copied())
                .collect();
            (wanted != *current.get(&item.id).unwrap_or(&none)).then_some((item.id, wanted))
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
/// A hidden photo that was renamed or moved is a first read again (`Library::move_items`
/// forgets the answer its old name had), and stays hidden by the same half of the rule: the
/// line it had is under another name, or in the folder it left.
/// Only rows whose INI answer differs from the recorded one are written, so an agreeing
/// folder costs nothing, as with stars and faces.
fn apply_folder_hidden(
    lib: &Library,
    items: &[FolderItem],
    hidden: &std::collections::HashSet<String>,
) -> Result<u64> {
    let changes: Vec<(i64, bool, bool)> = items
        .iter()
        .filter_map(|item| {
            let says = hidden.contains(&item.name);
            match item.picasa_hidden {
                Some(before) if before == says => None,
                Some(_) => Some((item.id, says, true)),
                None => Some((item.id, says, says)),
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
    items: &[FolderItem],
    stars: &std::collections::HashSet<String>,
) -> Result<u64> {
    let ratings: Vec<(i64, u8)> = items
        .iter()
        .filter_map(|item| {
            let wanted = u8::from(stars.contains(&item.name));
            (item.rating != Some(wanted as i64)).then_some((item.id, wanted))
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
    items: &[FolderItem],
    faces: &HashMap<String, Vec<Face>>,
) -> Result<u64> {
    let current = lib.folder_faces(folder_id)?;
    let empty: Vec<Face> = Vec::new();
    let changes: Vec<(i64, Vec<Face>)> = items
        .iter()
        .filter_map(|item| {
            let wanted = faces.get(&item.name).unwrap_or(&empty);
            let stored = current.get(&item.id).unwrap_or(&empty);
            (wanted != stored).then(|| (item.id, wanted.clone()))
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
///
/// Each row is written only while it is still at the path this walk's list has for it
/// (`mark_missing_at`, `purge_at`), and the counts returned are the rows actually changed,
/// not the length of the list. A photo this walk followed to a new path is still in `known`
/// under its old one, which the walk never finds: by id alone it would be marked missing by
/// the scan that had just kept it - or, already missing from an earlier scan of the folder it
/// left, purged with everything on it. The same holds for a row a scan of another watched
/// folder re-pointed after this one read its list.
///
/// That is these two writes only. The walk's other writes from the same list are still by
/// id alone. `update_items` writes the path with the rest, so it would put a moved row back
/// at its old path, and clear what a changed file clears. `update_item_meta` writes no path:
/// it would leave the row where it went and put on it the metadata and keywords read from
/// the file at the old path, one with the row's own size and mtime. They are left so because
/// they are made for a file the walk *found* at the listed path: for the row to have moved
/// meanwhile, it must have been followed elsewhere after the list was read - by a scan of
/// another watched folder, or by this walk - and a file must be at its old path again by the
/// time this walk gets there.
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
    for (path, k) in known {
        if k.missing {
            to_purge.push((k.id, path))
        } else {
            to_mark.push((k.id, path))
        }
    }
    let now = crate::now_ms();
    let (mut marked, mut purged) = (0, 0);
    for chunk in to_mark.chunks(BATCH) {
        marked += lib.mark_missing_at(chunk, now)?;
    }
    for chunk in to_purge.chunks(BATCH) {
        purged += lib.purge_at(chunk)?;
    }
    Ok((marked, purged))
}

fn describe(
    entry: &DirEntry,
    path: &str,
    folder_id: i64,
    kind: MediaKind,
    size: i64,
    mtime_ms: i64,
) -> NewItem {
    let file_name = entry.file_name().to_string_lossy().into_owned();
    let dated = |taken_at: Option<i64>| taken_at.unwrap_or(mtime_ms.div_euclid(1000));
    match kind {
        MediaKind::Image => {
            let (meta, embedded) = read_image(entry.path());
            NewItem {
                folder_id,
                path: path.to_string(),
                file_name,
                kind,
                size,
                mtime_ms,
                width: meta.width,
                height: meta.height,
                orientation: meta.orientation,
                taken_at: dated(meta.taken_at),
                // Always `None` here; `apply_picasa` sets the real value after the walk.
                rating: meta.rating,
                camera: meta.camera,
                tags: embedded.keywords,
                caption: embedded.caption,
                duration_ms: None,
            }
        }
        MediaKind::Video => {
            // Rotation is resolved into the size (`VideoMeta`), so orientation is always 1.
            let meta = crate::video::read_meta(entry.path());
            NewItem {
                folder_id,
                path: path.to_string(),
                file_name,
                kind,
                size,
                mtime_ms,
                width: meta.width,
                height: meta.height,
                orientation: 1,
                taken_at: dated(meta.taken_at),
                rating: None,
                camera: CameraMeta {
                    make: meta.make,
                    model: meta.model,
                    ..CameraMeta::default()
                },
                tags: Vec::new(),
                caption: None,
                duration_ms: meta.duration_ms,
            }
        }
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

/// Writes the files the walk found at paths no row has. Most are new photos; one that is a
/// row's own file under another name or in another place re-points that row instead (spec
/// `2026-10-05-photon-follow-moved-files-design.md`), so the photo keeps its albums, its
/// edit, its hidden flag and its faces.
fn flush_new(
    lib: &Library,
    batch: &mut Vec<NewItem>,
    probe: &mut moved::Probe,
    created: &HashSet<i64>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn ScanSink,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let (moves, mut fresh) = find_moves(lib, std::mem::take(batch), probe)?;
    fresh.extend(apply_moves(lib, moves, created, report, progress)?);
    // A batch that was all moves - a renamed folder's - inserts nothing and has nothing to
    // hand the thumbnail queue.
    if !fresh.is_empty() {
        let ids = lib.insert_items(&fresh)?;
        report.added += fresh.len() as u64;
        seen.added = report.added;
        progress.indexed(&ids);
    }
    progress.progress(seen);
    Ok(())
}

/// A row, and the new file that is its file under another path.
type Move = (MoveCandidate, NewItem);

/// Splits a batch of files no row has the path of into those that are a row's moved file,
/// each with that row, and those that are new. Both come back in the batch's order.
///
/// One indexed lookup per file (`items_moved`), which for nearly every file of an import
/// finds nothing; the filesystem is asked only about a row `moved::pick` could not rule out
/// from what the library holds.
///
/// A row is claimed by one file. When two files of the batch fit one row - two copies of a
/// deleted original - the one that kept the row's file name has it and the other is new,
/// whichever the walk met first; with no name to decide, the first of the batch has it. That
/// holds within a batch only. Across batches and across scans the first file to arrive has
/// the row: its move is written before the next file is looked at, and the row's file is
/// then no longer gone.
fn find_moves(
    lib: &Library,
    batch: Vec<NewItem>,
    probe: &mut moved::Probe,
) -> Result<(Vec<Move>, Vec<NewItem>)> {
    // Every lookup below reads the library as it was before this batch wrote anything, so
    // the claims are what keeps a row from being picked twice, and what lets a second file
    // be offered another row that fits it.
    let mut claimed: HashSet<i64> = HashSet::new();
    // An error is the probe's: it could not read which drives are there. It fails the batch,
    // and with it the scan, rather than passing for "no row is this file".
    let mut pick = |item: &NewItem, candidates: &[MoveCandidate], claimed: &HashSet<i64>| {
        moved::pick(item, candidates, claimed, |c| {
            probe.gone(lib, c, Path::new(&item.path))
        })
        .map(|row| row.cloned())
    };

    // First the files that are their row's file under its own name, so that none of them
    // finds its row taken by a copy the walk happened to meet earlier.
    let mut looked = Vec::with_capacity(batch.len());
    for item in &batch {
        let candidates = lib.move_candidates(item.size, item.mtime_ms, item.kind)?;
        let row = pick(item, &candidates, &claimed)?;
        let named = row.as_ref().filter(|row| row.file_name == item.file_name);
        if let Some(row) = named {
            claimed.insert(row.id);
        }
        let named = named.is_some();
        looked.push((candidates, row, named));
    }

    // Then the rest, in the batch's order, each offered what is left.
    let (mut moves, mut fresh) = (Vec::new(), Vec::new());
    for (item, (candidates, first, named)) in batch.into_iter().zip(looked) {
        // A pick depends on the claims only through the file's own candidates. With none of
        // them claimed the first answer stands, and the filesystem is not asked a second
        // time - which is every renamed file but the contested ones.
        let contested = !named && candidates.iter().any(|c| claimed.contains(&c.id));
        let row = if contested {
            pick(&item, &candidates, &claimed)?
        } else {
            first
        };
        match row {
            Some(row) => {
                claimed.insert(row.id);
                moves.push((row, item));
            }
            None => fresh.push(item),
        }
    }
    Ok((moves, fresh))
}

/// Re-points each row to its file's new path, and returns the files whose row was no longer
/// where the lookup found it - purged or claimed since by a scan of another watched folder -
/// for the caller to insert as new. Dropped, such a file would stay out of the library until
/// some later scan.
///
/// Reports the moved rows to the sink twice over: `moved` with each row's thumbnail key
/// before and after, then `indexed`, since `move_items` leaves the row's thumbnail pending
/// under its new key.
///
/// `created` is the folders this walk made a row for (`FolderRows::created`).
fn apply_moves(
    lib: &Library,
    moves: Vec<Move>,
    created: &HashSet<i64>,
    report: &mut ScanReport,
    progress: &mut dyn ScanSink,
) -> Result<Vec<NewItem>> {
    // Nearly every batch of new files: `move_items` moves the thumbnail collector's epoch
    // even for no rows, and an import must not make that walk of the whole cache due.
    if moves.is_empty() {
        return Ok(Vec::new());
    }
    let (rows, writes): (Vec<MoveCandidate>, Vec<(i64, String, NewItem)>) = moves
        .into_iter()
        .map(|(row, item)| {
            let write = (row.id, row.path.clone(), item);
            (row, write)
        })
        .unzip();
    let written = lib.move_items(&writes)?;

    let (mut ids, mut keys, mut unmoved) = (Vec::new(), Vec::new(), Vec::new());
    // (the folder the row left, its directory, the folder it is in now, its directory), each
    // pair once and in the order the walk met them: of two folders emptied into one, the
    // first to arrive is the one whose name and flag it takes. A row renamed in place has
    // changed no folder.
    let mut folders: Vec<(i64, String, i64, PathBuf)> = Vec::new();
    let mut paired: HashSet<(i64, i64)> = HashSet::new();
    for ((row, (_, _, item)), moved) in rows.into_iter().zip(writes).zip(written) {
        if !moved {
            unmoved.push(item);
            continue;
        }
        // The only record of which row went where. A row followed to the wrong file shows
        // as nothing but a photo with another photo's albums and names, so the pair has to
        // be recoverable from a user's log; the count is in the engine's end-of-scan line.
        tracing::debug!(id = row.id, from = %row.path, to = %item.path, "followed a moved photo");
        // `Item::thumb_key()` before and after: the row's edit over the fingerprint of each
        // path. The size and time are the file's, which are the row's - that is how it was
        // found.
        let key = |path: &str| {
            row.edit
                .thumb_key(fingerprint(path, item.size, item.mtime_ms))
        };
        keys.push((key(&row.path), key(&item.path)));
        ids.push(row.id);
        if row.folder_id != item.folder_id && paired.insert((row.folder_id, item.folder_id)) {
            let now = Path::new(&item.path).parent().unwrap_or(Path::new(""));
            folders.push((
                row.folder_id,
                row.folder_path,
                item.folder_id,
                now.to_path_buf(),
            ));
        }
    }

    // Before the folders below, which can fail: the rows are re-pointed already, and an
    // error there must not leave their thumbnails neither carried over nor queued.
    report.moved += ids.len() as u64;
    progress.moved(&keys);
    progress.indexed(&ids);

    // A renamed directory is a new folder row, and the name and Hide folder flag the user
    // gave the old one follow the photos. Two conditions, each with a case behind it.
    //
    // The old directory is gone, or is the new one under another spelling (`moved::vacated`,
    // as for a file: on a case-insensitive volume the old spelling still opens a directory
    // renamed only in its case, while its row is a new one, folder rows being found by the
    // path as written). A photo moved out of a folder that is still there has left that
    // folder, not renamed it.
    //
    // And this walk made the new folder's row. A folder that was there already is one that
    // photos were moved *into*: one photo out of a hidden, named folder whose directory was
    // then deleted hid every photo its new folder held, under the old folder's name.
    //
    // The cost is every folder whose row an earlier scan made before the old directory was
    // gone: its name and Hide folder flag are not handed on. The commonest is not exotic.
    // "New folder" in the file manager, which the watcher scans two seconds later; then the
    // photos dragged in and the old folder deleted. The photos keep their own hidden flag,
    // but the folder has no name and is not hidden, so a photo added to it later is visible.
    // The others: a folder moved file by file between volumes with the watcher scanning
    // mid-move, a subtree scan of a child that seeded the parent's row (`seed_ancestors`),
    // a walk that errored after `ensure`, and a first scan cancelled before any of the
    // folder's files was flushed. Accepted knowingly: handing on to any folder with neither
    // a name nor a flag is the merge above.
    //
    // Before the caller inserts the batch's new files, so in a hidden folder they arrive
    // hidden as any new row does. (Inserted first they would be hidden all the same, by
    // `set_folder_hidden`'s own write: the order saves that write, nothing more.)
    for (from, was, to, now) in folders {
        if created.contains(&to) && moved::vacated(Path::new(&was), &now) {
            lib.inherit_folder_flags(from, to)?;
        }
    }

    Ok(unmoved)
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

/// The directories that are a drive's or a network share's recycle bin, by the names their
/// systems give them: Windows' on every drive, Synology's and QNAP's on every share. macOS's
/// `.Trashes` and a Linux desktop's `.Trash-<uid>` are dot-names, and passed over as those.
///
/// Each begins with a sign (`$`, `#`, `@`) that a folder of the user's own seldom does, and
/// that is what makes a name safe to list, matched as they are without case. Windows XP's
/// `RECYCLER` is left out for it: it is a plain word, a user's folder called "Recycler" would
/// be passed over with it, and on the upgrade that brought the rule its photos would be purged.
const RECYCLE_BINS: [&str; 3] = ["$RECYCLE.BIN", "#recycle", "@Recycle"];

/// Whether the scan passes over an entry of this name, and does not enter it when it is a
/// directory: a dot-name, or a recycle bin.
///
/// One predicate for the two places that ask: `walk_tree`'s filter, and `scan_subtree`'s
/// check of the directory the watcher handed it. Both exempt the watched root itself, so a
/// user who watches `~/.photos` gets it scanned.
///
/// A bin is passed over because of what deleting does where the bin lies inside the
/// watched root - a drive's root watched whole, or a NAS share. Explorer, and an SMB server
/// with a bin, delete by renaming the file into it, on its own volume and with its size and
/// modification time as they were. That is a moved photo by every rule `moved::pick` has, so
/// the row was re-pointed into the bin and the deleted photo stayed in its albums, its
/// person's view and the grid until the bin was emptied. (Before rows were followed it was
/// wrong another way: the photo left its albums and came back as a new one.)
///
/// The name is matched whole, without ASCII case (a share's may be spelled either way, and
/// the volume may not fold it), and at any depth, since a watched root can lie above several
/// shares. A folder whose name merely contains one - `my #recycle photos` - is a folder.
///
/// A photo an earlier photon indexed inside a bin is no longer found, so the next scan of
/// the folder above it marks it missing and the one after purges it. That is intended: it
/// was never a photo the user kept.
fn passed_over(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.starts_with('.')
            || RECYCLE_BINS
                .iter()
                .any(|bin| name.eq_ignore_ascii_case(bin))
    })
}

pub(crate) fn mtime_ms(md: &Metadata) -> i64 {
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
    use crate::edit::Edit;
    use crate::face_detect::{DETECTOR_VERSION, Detection, Rect};
    use crate::library::{Folder, Item, is_starred};
    use crate::media::{ThumbState, fingerprint};
    use crate::metadata::naive_to_unix;
    use crate::testutil::{
        ExifSpec, Mp4Spec, avif_fixture, bmp_bytes, jpeg_bytes, jpeg_with_exif,
        jpeg_with_exif_spec, jpeg_with_iptc_keywords, mp4_bytes, png_bytes, temp_library,
        tiff_bytes, write_file,
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

    #[test]
    fn indexes_a_video_with_its_size_running_time_and_date() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(
            &root,
            "IMG_0001.MOV",
            &mp4_bytes(&Mp4Spec {
                width: 1920,
                height: 1080,
                rotation: 90,
                timescale: 600,
                duration: 600 * 12,
                apple_date: Some("2024-06-15T12:30:45+0200"),
                make: Some("Apple"),
                model: Some("iPhone 15 Pro"),
                ..Mp4Spec::default()
            }),
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let id = lib
            .known_items(watched.id)
            .unwrap()
            .values()
            .next()
            .unwrap()
            .id;
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.kind, MediaKind::Video);
        assert_eq!((item.width, item.height, item.orientation), (1080, 1920, 1));
        assert_eq!(item.duration_ms, Some(12_000));
        assert_eq!(item.taken_at, naive_to_unix(2024, 6, 15, 12, 30, 45));
        assert_eq!(item.camera.model.as_deref(), Some("iPhone 15 Pro"));
    }

    #[test]
    fn a_garbage_mp4_is_still_indexed() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "broken.mp4", b"");
        write_file(&root, "junk.webm", b"\x1a\x45\xdf\xa3 and then nothing");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let known = lib.known_items(watched.id).unwrap();
        assert_eq!(known.len(), 2);
        for k in known.values() {
            let item = lib.item(k.id).unwrap().unwrap();
            assert_eq!(item.kind, MediaKind::Video);
            assert_eq!((item.width, item.height, item.duration_ms), (0, 0, None));
            assert_eq!(item.taken_at, item.mtime_ms.div_euclid(1000));
        }
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

    fn ini_pass(lib: &Library, watched: &WatchedFolder, dirs: &[PathBuf]) -> IniPass {
        refresh_picasa(lib, watched, dirs, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn an_ini_pass_applies_a_star_without_walking() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 0);

        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        // A photo that appears at the same time is a walk's business, not this pass's.
        write_file(&root, "b.jpg", &jpeg_bytes(6, 2));
        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert_eq!(lib.starred_count().unwrap(), 1);
        assert_eq!(pass.report.restarred, 1);
        assert!(pass.report.touched_rows());
        assert_eq!(lib.grid_entries().unwrap().len(), 1, "nothing was walked");
        assert!(pass.needs_walk.is_empty());
    }

    #[test]
    fn an_ini_pass_over_a_deleted_ini_clears_the_stars() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        fs::remove_file(root.join(".picasa.ini")).unwrap();
        ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    /// Review Focus 2.
    #[test]
    fn an_unknown_folder_needs_a_walk() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let new = root.join("new");
        write_file(&new, "c.jpg", &jpeg_bytes(4, 2));
        write_file(&new, ".picasa.ini", b"[c.jpg]\nstar=yes\n");
        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&new));

        assert_eq!(pass.needs_walk, [new]);
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn an_ini_pass_over_an_offline_root_reports_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_dir_all(&root).unwrap();

        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert!(pass.report.offline);
    }

    #[test]
    fn an_ini_pass_stops_when_cancelled() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");

        let pass = refresh_picasa(
            &lib,
            &watched,
            std::slice::from_ref(&root),
            &AtomicBool::new(true),
        )
        .unwrap();

        assert!(pass.report.cancelled);
        assert_eq!(lib.starred_count().unwrap(), 0);
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
            [
                0,
                PROGRESS_EVERY,
                2 * PROGRESS_EVERY,
                2 * PROGRESS_EVERY + 5
            ],
            "the opening report, two on the way and the final one, not just the final one"
        );
    }

    /// A full scan reports once before it walks, with nothing seen: the walk's own first
    /// report is `PROGRESS_EVERY` photos in, or at its end, and until then nothing told a
    /// listener a scan was running at all - the status bar was empty for a network share's
    /// first minute, and an empty library said photon had found no photos in the folder it
    /// was in the middle of reading.
    #[test]
    fn a_full_scan_reports_before_it_walks() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 4));
        write_file(&root, "b.jpg", &jpeg_bytes(4, 5));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let mut reports: Vec<ScanProgress> = Vec::new();
        scan_watched(
            &lib,
            &watched,
            1,
            &ScanOptions::default(),
            &mut progress_only(|p| reports.push(*p)),
        )
        .unwrap();

        assert_eq!(
            reports.first(),
            Some(&ScanProgress::default()),
            "{reports:?}"
        );
        assert_eq!(reports.last().unwrap().files_seen, 2, "{reports:?}");
    }

    /// Not for a root that is not there. An unplugged drive is polled every thirty seconds,
    /// and each poll would light the status bar's scan line for a scan that reads nothing.
    #[test]
    fn a_scan_of_a_root_that_is_not_there_reports_nothing() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 4));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        let mut reports = 0;
        let report = scan_watched(
            &lib,
            &watched,
            1,
            &ScanOptions::default(),
            &mut progress_only(|_| reports += 1),
        )
        .unwrap();

        assert!(report.offline);
        assert_eq!(reports, 0);
    }

    /// Nor does a subtree scan open with one. The watcher runs one for every directory a
    /// file changed in, most of them over in milliseconds: announced, each would flash the
    /// status bar's scan line. It reports as it always has, on the way and at its end.
    #[test]
    fn a_subtree_scan_does_not_open_with_a_report() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 4));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let mut reports: Vec<ScanProgress> = Vec::new();
        scan_subtree(
            &lib,
            &watched,
            &root.join("sub"),
            2,
            &ScanOptions::default(),
            &mut progress_only(|p| reports.push(*p)),
        )
        .unwrap();

        assert_eq!(reports.len(), 1, "only the closing report: {reports:?}");
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

    /// On a network share every directory listing is a round trip, and the Picasa pass used
    /// to list each walked folder again to find its INI. The walk has already seen every
    /// entry, so the pass reads what it recorded: no folder is listed a second time, whether
    /// it has a dotted INI, an old-style one, or none. Reverting `apply_picasa` to call
    /// `read_folder` for every folder lists all four here.
    #[test]
    fn the_picasa_pass_does_not_list_the_walked_folders_again() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "a/.picasa.ini", b"[one.jpg]\nstar=yes\n");
        write_file(&root, "b/two.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b/Picasa.ini", b"[two.jpg]\nstar=yes\n");
        write_file(&root, "c/three.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let before = crate::picasa::listings_for_test();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 2, "both INIs were read");
        fs::remove_file(root.join("b/Picasa.ini")).unwrap();
        scan(&lib, &watched, 2);
        assert_eq!(
            lib.starred_count().unwrap(),
            1,
            "a deleted INI still clears"
        );
        assert_eq!(crate::picasa::listings_for_test() - before, 0);
    }

    /// The walk's listing is older than the Picasa pass that uses it. A star set in photon
    /// while a long scan runs creates `.picasa.ini` after the walk passed the folder; taking
    /// the listing's "no INI" as the answer would clear that star at the end of the same
    /// scan. Here the INI appears once the walk is done, as the scan hands over the photo it
    /// indexed. Returning "no INI" without probing the names fails this.
    #[test]
    fn an_ini_written_after_the_walk_listed_its_folder_is_still_read() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let ini = root.join(".picasa.ini");
        let report = scan_after_the_walk(&lib, &watched, 1, || {
            fs::write(&ini, b"[a.jpg]\nstar=yes\n").unwrap()
        });

        assert_eq!(report.added, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);
    }

    /// The same staleness the other way round: the walk saw the INI, and it has gone by the
    /// time the pass reads it. Gone is an answer - the folder has no stars now - where
    /// treating the failed open as unreadable would keep the stars it no longer lists.
    /// Reading the listed path without falling back on `NotFound` fails this.
    #[test]
    fn an_ini_deleted_after_the_walk_listed_its_folder_clears_its_stars() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        write_file(&root, "b.jpg", &jpeg_bytes(4, 3));
        let ini = root.join(".picasa.ini");
        scan_after_the_walk(&lib, &watched, 2, || fs::remove_file(&ini).unwrap());

        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    /// A folder the walk saw with only an old-style `Picasa.ini` can gain a `.picasa.ini`
    /// before the pass reads it, and the dotted one wins, as `read_folder` would decide.
    /// Reading the listed `Picasa.ini` without probing for the dotted name fails this.
    #[test]
    fn a_dotted_ini_written_after_the_walk_wins_over_the_old_one_it_saw() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b.jpg", &jpeg_bytes(4, 3));
        write_file(&root, "Picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let ini = root.join(".picasa.ini");
        scan_after_the_walk(&lib, &watched, 1, || {
            fs::write(&ini, b"[b.jpg]\nstar=yes\n").unwrap()
        });

        let b = lib.known_items(watched.id).unwrap()[&key(&root.join("b.jpg"))].id;
        assert_eq!(lib.starred_count().unwrap(), 1);
        assert_eq!(lib.item(b).unwrap().unwrap().rating, Some(1));
    }

    /// Scans `watched`, running `after_walk` once the walk has listed every folder and before
    /// the Picasa pass reads them: the scan's last flush of new photos falls between the two.
    /// Only a scan that indexes something new reaches it.
    fn scan_after_the_walk(
        lib: &Library,
        watched: &WatchedFolder,
        scan_id: i64,
        after_walk: impl FnMut(),
    ) -> ScanReport {
        struct AfterWalk<F>(F);
        impl<F: FnMut()> ScanSink for AfterWalk<F> {
            fn progress(&mut self, _: &ScanProgress) {}
            fn indexed(&mut self, _: &[i64]) {
                (self.0)()
            }
        }
        let mut sink = AfterWalk(after_walk);
        scan_watched(lib, watched, scan_id, &ScanOptions::default(), &mut sink).unwrap()
    }

    /// A rescan that finds nothing changed used to take the writer once per folder (the
    /// folder upsert, which rewrote `seen_scan` every time) and once more per INI listing
    /// contacts. Now it takes it a fixed number of times however many folders there are:
    /// the watched folder's online flag, one chunk of `seen_scan` bumps, and the prune.
    /// Reverting the walk to `upsert_folder` for every folder fails this with one write per
    /// folder; reverting the contacts' read-before-write, with one more.
    #[test]
    fn an_unchanged_rescan_writes_a_fixed_amount_however_many_folders() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "f00/a.jpg", &jpeg_bytes(4, 2));
        write_file(
            &root,
            "f00/.picasa.ini",
            b"[Contacts2]\nb5d3a7e4f1c2d9a8=Ada;;\n\
              [a.jpg]\nstar=yes\nfaces=rect64(4000200080006000),b5d3a7e4f1c2d9a8\n",
        );
        for i in 1..40 {
            fs::create_dir_all(root.join(format!("f{i:02}/deeper"))).unwrap();
        }
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        let before = lib.writes_for_test();
        let report = scan(&lib, &watched, 2);
        assert!(!report.touched_rows());
        assert_eq!(lib.writes_for_test() - before, 3);
        assert_eq!(
            lib.folders().unwrap().len(),
            1 + 40 + 39,
            "no folder was pruned"
        );
    }

    /// The bump is what keeps the prune off a folder the walk reached but did not write: an
    /// empty folder is pruned exactly when a scan did not see it. Without the bump every
    /// empty folder that was already known disappears on the next scan.
    #[test]
    fn an_empty_folder_a_rescan_reaches_is_kept_and_one_it_does_not_is_pruned() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        fs::create_dir_all(root.join("kept")).unwrap();
        fs::create_dir_all(root.join("gone")).unwrap();
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_dir(root.join("gone")).unwrap();

        scan(&lib, &watched, 2);
        let mut names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        names.sort();
        assert_eq!(names, ["kept", "photos"]);

        // The subtree scan's walk goes through the same bump before its own prune.
        let sub = root.join("kept");
        fs::create_dir_all(sub.join("inner")).unwrap();
        scan_subtree(
            &lib,
            &watched,
            &sub,
            3,
            &ScanOptions::default(),
            &mut progress_only(|_| {}),
        )
        .unwrap();
        scan_subtree(
            &lib,
            &watched,
            &sub,
            4,
            &ScanOptions::default(),
            &mut progress_only(|_| {}),
        )
        .unwrap();
        let mut names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        names.sort();
        assert_eq!(names, ["inner", "kept", "photos"]);
    }

    /// Writes `n` folders `f00`.. under `root`, a photo in each.
    fn folders_of_one_photo(root: &Path, range: std::ops::Range<usize>) {
        for i in range {
            write_file(root, &format!("f{i:02}/a.jpg"), &jpeg_bytes(4, 2));
        }
    }

    /// The Picasa pass used to make four queries per folder: its photos' names and ratings,
    /// their Picasa albums, their faces, and their hidden answers - on every scan, for a
    /// folder with no INI and nothing in the library. Now the names, ratings and hidden
    /// answers are one query, and the face and album queries run only where the INI or the
    /// library has any. What this pins is the cost of one more such folder: one query.
    /// Reverting the merge makes it two; reverting the presence check, three.
    #[test]
    fn a_folder_with_no_picasa_data_costs_the_picasa_pass_one_query() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        folders_of_one_photo(&root, 0..PRESENCE_MIN_FOLDERS + 4);
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let before = lib.reads_for_test();
        scan(&lib, &watched, 2);
        let fewer = lib.reads_for_test() - before;

        let more = PRESENCE_MIN_FOLDERS + 4;
        folders_of_one_photo(&root, more..2 * more);
        scan(&lib, &watched, 3);
        let before = lib.reads_for_test();
        scan(&lib, &watched, 4);
        let many = lib.reads_for_test() - before;

        assert_eq!(many - fewer, more);
    }

    /// The presence check skips a folder's face and album queries only where the library
    /// has none: a folder whose INI drops the faces and albums it had must still be compared,
    /// or they stay forever. Enough folders that the check is loaded at all. Treating every
    /// folder as empty in the library fails the second half; ignoring what the INI names
    /// fails the first.
    #[test]
    fn faces_and_albums_an_ini_drops_are_cleared_when_most_folders_have_none() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        folders_of_one_photo(&root, 0..PRESENCE_MIN_FOLDERS);
        write_file(
            &root,
            "f03/.picasa.ini",
            b"[Contacts2]\nb5d3a7e4f1c2d9a8=Ada;;\n[.album:t]\nname=Holiday\n\
              [a.jpg]\nfaces=rect64(4000200080006000),b5d3a7e4f1c2d9a8\nalbums=t\n",
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let report = scan(&lib, &watched, 1);
        assert_eq!((report.refaced, report.realbumed), (1, 2));
        assert_eq!(lib.people_with_counts().unwrap().len(), 1);
        assert_eq!(lib.albums_with_counts().unwrap()[0].count, 1);

        write_file(&root, "f03/.picasa.ini", b"[a.jpg]\nbackuphash=1\n");
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.refaced, report.realbumed), (1, 1));
        assert!(lib.people_with_counts().unwrap().is_empty());
        assert!(
            lib.albums_with_counts().unwrap().is_empty(),
            "the album is empty"
        );
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

        let ini = write_file(
            &root,
            ".picasa.ini",
            b"[.album:t]\nname=Summer\n[a.jpg]\nalbums=t\n",
        );
        // Dated explicitly: a rename is only taken from a newer INI, and on a filesystem
        // with one-second timestamps this rewrite could otherwise share the first one's mtime.
        set_mtime(&ini, std::time::SystemTime::now() + Duration::from_secs(60));
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
    fn a_stale_copy_of_a_folder_does_not_take_turns_naming_its_album() {
        // `b` is a backup of `a` taken before the album was renamed in Picasa: its INI is
        // older and still says Holiday. The album must settle on the newer name whatever
        // order the folders are walked in, and a rescan must count nothing - or the grid
        // rebuilds after every scan and the sidebar shows whichever folder came last.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b/two.jpg", &jpeg_bytes(4, 3));
        let renamed = write_file(
            &root,
            "a/.picasa.ini",
            b"[.album:t]\nname=Summer\n[one.jpg]\nalbums=t\n",
        );
        let stale = write_file(
            &root,
            "b/.picasa.ini",
            b"[.album:t]\nname=Holiday\n[two.jpg]\nalbums=t\n",
        );
        set_mtime(&stale, UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        set_mtime(&renamed, UNIX_EPOCH + Duration::from_secs(1_700_000_100));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let names = |lib: &Library| -> Vec<String> {
            lib.albums_with_counts()
                .unwrap()
                .into_iter()
                .map(|a| a.name)
                .collect()
        };
        assert_eq!(names(&lib), vec!["Summer"]);

        let report = scan(&lib, &watched, 2);
        assert_eq!(report.realbumed, 0, "a rescan settles, it does not flip");
        assert_eq!(names(&lib), vec!["Summer"]);
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

    /// A rescan refreshes a folder's row in place (`upsert_folder`), and neither of
    /// `walk_tree`'s callers may take the user's name for it with the refresh.
    #[test]
    fn a_folder_alias_survives_a_scan_and_a_subtree_scan() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let sub = |lib: &Library| {
            lib.folders()
                .unwrap()
                .into_iter()
                .find(|f| f.path.ends_with("sub"))
                .unwrap()
        };
        lib.set_folder_alias(sub(&lib).id, Some("Easter")).unwrap();

        write_file(&root, "sub/b.jpg", &jpeg_bytes(4, 3));
        scan(&lib, &watched, 2);
        assert_eq!(
            sub(&lib).alias.as_deref(),
            Some("Easter"),
            "a scan cleared the alias"
        );
        write_file(&root, "sub/c.jpg", &jpeg_bytes(4, 5));
        scan_sub(&lib, &watched, &root.join("sub"), 3);
        assert_eq!(
            sub(&lib).alias.as_deref(),
            Some("Easter"),
            "a subtree scan cleared the alias"
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
    fn an_unchanged_photo_gains_its_position_from_the_backfill() {
        // A library indexed under EXIF_VERSION 3 has the position columns empty; the photo
        // is unchanged, so only the version bump brings it back through `describe()`.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let spec = ExifSpec {
            gps: Some(crate::testutil::GpsSpec {
                lat: [(48, 1), (30, 1), (0, 1)],
                lat_ref: Some("N"),
                lon: [(11, 1), (15, 1), (0, 1)],
                lon_ref: Some("E"),
            }),
            ..ExifSpec::default()
        };
        let a = write_file(&root, "a.jpg", &jpeg_with_exif_spec(4, 2, &spec));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        let here = Some(crate::metadata::Gps {
            lat: 48.5,
            lon: 11.25,
        });
        assert_eq!(lib.item(id).unwrap().unwrap().camera.gps, here);

        lib.forget_position_for_test(id).unwrap();
        assert_eq!(lib.item(id).unwrap().unwrap().camera.gps, None);
        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.unchanged, report.changed, report.enriched),
            (1, 0, 1)
        );
        assert_eq!(lib.item(id).unwrap().unwrap().camera.gps, here);
        assert_eq!(
            scan(&lib, &watched, 3).enriched,
            0,
            "read once, not on every scan"
        );
    }

    #[test]
    fn a_new_photo_is_stored_with_its_caption() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let app13 = crate::testutil::iptc_app13_datasets(&[(120, b"Grandma's 80th")]);
        let a = write_file(
            &root,
            "a.jpg",
            &crate::testutil::jpeg_with_segments(4, 2, &[(0xED, &app13)]),
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        assert_eq!(
            lib.item_caption(id).unwrap().as_deref(),
            Some("Grandma's 80th")
        );
    }

    #[test]
    fn an_unchanged_photo_gains_its_caption_from_the_backfill() {
        // A library indexed under EXIF_VERSION 2 has no caption column filled; the photo is
        // unchanged, so only the backfill can read it - once.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let app13 = crate::testutil::iptc_app13_datasets(&[(120, b"Grandma's 80th")]);
        let a = write_file(
            &root,
            "a.jpg",
            &crate::testutil::jpeg_with_segments(4, 2, &[(0xED, &app13)]),
        );
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        lib.forget_caption_for_test(id).unwrap();

        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.unchanged, report.changed, report.enriched),
            (1, 0, 1)
        );
        assert!(report.touched_rows());
        assert_eq!(
            lib.item_caption(id).unwrap().as_deref(),
            Some("Grandma's 80th")
        );
        assert_eq!(
            scan(&lib, &watched, 3).enriched,
            0,
            "read once, not on every scan"
        );
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

        // Another picture than `one.jpg`. With its bytes, and on a clock coarse enough its
        // modification time too, this would be `one.jpg` renamed - a row that is followed,
        // not an addition and a removal.
        write_file(&root, "a/two.jpg", &jpeg_bytes(8, 9));
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

    /// A drive's or a share's recycle bin is not part of the library, under each name a
    /// system gives it, in any case and at any depth. A folder whose name only contains one
    /// of them is a folder like any other, and so is one called `Recycler`: Windows XP's bin
    /// had that name, but it is a plain word a user's own folder can have, and passing it
    /// over would purge that folder's photos.
    #[test]
    fn a_recycle_bin_is_not_walked() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let img = jpeg_bytes(8, 8);
        write_file(&root, "a.jpg", &img);
        write_file(&root, "$RECYCLE.BIN/S-1-5-21/$R0A1B2C.jpg", &img);
        write_file(&root, "#recycle/b.jpg", &img);
        write_file(&root, "share/@Recycle/c.jpg", &img);
        write_file(&root, "Recycler/d.jpg", &img);
        // Another parent than the bins above, so the spelling is this directory's own on a
        // filesystem that folds case as well.
        write_file(&root, "share/#Recycle/e.jpg", &img);
        write_file(&root, "other/$Recycle.Bin/f.jpg", &img);
        write_file(&root, "my #recycle photos/g.jpg", &img);
        write_file(&root, "@Recycle 2003/h.jpg", &img);
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let report = scan(&lib, &watched, 1);

        assert_eq!(report.added, 4);
        let mut names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "@Recycle 2003",
                "Recycler",
                "my #recycle photos",
                "other",
                "photos",
                "share"
            ]
        );
    }

    /// The watched root is exempt, as it is from the dot-name rule: what the user pointed
    /// photon at is scanned, whatever it is called.
    #[test]
    fn a_watched_root_named_like_a_recycle_bin_is_scanned() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("@Recycle");
        fs::create_dir_all(&root).unwrap();
        let root = paths::canonicalize(root).unwrap();
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "sub/b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        assert_eq!(scan(&lib, &watched, 1).added, 2);
        write_file(&root, "sub/c.jpg", &jpeg_bytes(8, 8));
        assert_eq!(scan_sub(&lib, &watched, &root.join("sub"), 2).added, 1);
    }

    /// The watcher hands `scan_subtree` the directory an event came from, and deleting a
    /// photo is an event in the bin. `walk_tree` exempts its own depth 0, so the bin needs
    /// the check the dot-directories have - for the bin itself and for anything inside it.
    #[test]
    fn subtree_scan_of_a_recycle_bin_indexes_nothing() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(
            &root,
            "$RECYCLE.BIN/S-1-5-21/$R0A1B2C.jpg",
            &jpeg_bytes(8, 8),
        );
        write_file(&root, "share/#recycle/b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let bin = root.join("$RECYCLE.BIN");
        for (scan_id, dir) in [
            bin.clone(),
            bin.join("S-1-5-21"),
            root.join("share").join("#recycle"),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                scan_sub(&lib, &watched, dir, scan_id as i64 + 1),
                ScanReport::default(),
                "{dir:?}"
            );
        }
        assert!(lib.known_items(watched.id).unwrap().is_empty());
        // The bin's parent is a folder like any other, and its walk still passes the bin by.
        let report = scan_sub(&lib, &watched, &root.join("share"), 9);
        assert_eq!(report.added, 0);
        assert!(folder(&lib, "#recycle").is_none());
    }

    /// Deleting a photo in Explorer, or over SMB on a NAS, renames it into the bin of its
    /// own volume, with its size and modification time intact: to the rule that follows a
    /// moved photo, a move like any other. Walked, the bin was where the photo went, and the
    /// deleted photo stayed in its albums and its person's view until the bin was emptied.
    #[test]
    fn a_photo_deleted_into_a_recycle_bin_leaves_the_library() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        // Without it the root holds no photo once this one is deleted, which reads as an
        // unmounted volume.
        write_file(&root, "keep.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        decorate(&lib, id);

        let bin = root.join("$RECYCLE.BIN").join("S-1-5-21");
        fs::create_dir_all(&bin).unwrap();
        fs::rename(&a, bin.join("$R0A1B2C.jpg")).unwrap();
        // The watcher reports both directories. The bin's first: the order in which the row
        // is still live when the "new" file is met.
        assert_eq!(scan_sub(&lib, &watched, &bin, 2), ScanReport::default());
        let report = scan_sub(&lib, &watched, &root.join("x"), 3);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (0, 0, 1)
        );
        let row = lib.item(id).unwrap().unwrap();
        assert_eq!(row.path, key(&a));
        assert!(row.missing_since.is_some());

        let report = scan(&lib, &watched, 4);
        assert_eq!((report.moved, report.added, report.purged), (0, 0, 1));
        assert!(lib.item(id).unwrap().is_none());
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
        assert!(folder(&lib, "$RECYCLE.BIN").is_none());
    }

    /// A library an earlier photon built can hold rows inside a bin, which that photon
    /// walked. They are not found any more, so they go the way of any file that is not:
    /// missing at the next scan and purged by the one after, their folder rows with them.
    #[test]
    fn a_photo_indexed_inside_a_recycle_bin_by_an_earlier_photon_is_purged() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "keep.jpg", &jpeg_bytes(8, 7));
        let binned = write_file(&root, "#recycle/old.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        // The rows as that photon left them, written here because no scan makes them any
        // more: the file's own size and time, so a walk that reached it would find it
        // unchanged.
        let md = fs::metadata(&binned).unwrap();
        let top = lib.upsert_folder(watched.id, None, &key(&root), 1).unwrap();
        let bin = lib
            .upsert_folder(watched.id, Some(top), &key(&root.join("#recycle")), 1)
            .unwrap();
        let id = lib
            .insert_items(&[NewItem {
                size: md.len() as i64,
                mtime_ms: mtime_ms(&md),
                ..crate::testutil::new_item(bin, &key(&binned), 1)
            }])
            .unwrap()[0];

        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.added, report.unchanged, report.marked_missing),
            (1, 0, 1)
        );
        let report = scan(&lib, &watched, 3);
        assert_eq!(report.purged, 1);
        assert!(lib.item(id).unwrap().is_none());
        assert!(folder(&lib, "#recycle").is_none());
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

    // Following a moved or renamed photo (spec `2026-10-05-photon-follow-moved-files-design.md`).
    // Files are real and moved with `fs::rename`, which keeps the modification time.

    /// One detection, in the shape `library/detected_faces.rs`'s tests seed.
    fn face() -> Detection {
        Detection {
            rect: Rect {
                left: 0.1,
                top: 0.2,
                right: 0.2,
                bottom: 0.4,
            },
            landmarks: [(0.1, 0.2), (0.3, 0.4), (0.5, 0.6), (0.7, 0.8), (0.9, 1.0)],
            score: 0.9,
        }
    }

    /// Everything photon keeps on a row, put on `id`, so a test can ask what survived a move:
    /// an album, a keyword of the user's, an edit, the hidden flag and a detected face.
    fn decorate(lib: &Library, id: i64) -> i64 {
        let album = lib.create_album("Kept", 1).unwrap();
        lib.add_to_album(album.id, &[id], 1).unwrap();
        lib.add_item_tag(id, "kept").unwrap();
        lib.set_item_edit(id, Edit::new(1, None).unwrap()).unwrap();
        lib.set_hidden(&[id], true).unwrap();
        // The face last: an edit deletes a photo's detections, and the detector is handed
        // only a photo whose preview is ready.
        lib.set_face_detection(true).unwrap();
        lib.set_thumb_state(id, ThumbState::Ready, None).unwrap();
        let candidate = lib
            .face_candidates(id - 1, 1, DETECTOR_VERSION)
            .unwrap()
            .remove(0);
        assert_eq!(candidate.id, id);
        let written = lib
            .write_face_batch(&[(candidate, vec![face()])], DETECTOR_VERSION)
            .unwrap();
        assert_eq!(written, 1);
        album.id
    }

    /// The row `id` is at `path` now and still carries what `decorate` put on it.
    fn assert_followed(lib: &Library, id: i64, album: i64, path: &Path) {
        let item = lib.item(id).unwrap().expect("the row is still there");
        assert_eq!(item.path, key(path));
        assert_eq!(item.missing_since, None);
        assert_eq!(item.edit, Edit::new(1, None).unwrap());
        assert!(item.hidden, "the photo came back visible");
        assert!(lib.item_tags(id).unwrap().contains(&"kept".to_string()));
        assert!(lib.item_albums(id).unwrap().contains(&album));
        assert_eq!(lib.item_detected_faces(id).unwrap(), [face().rect]);
    }

    fn id_at(lib: &Library, watched: &WatchedFolder, path: &Path) -> i64 {
        lib.known_items(watched.id).unwrap()[&key(path)].id
    }

    fn folder(lib: &Library, name: &str) -> Option<Folder> {
        lib.folders().unwrap().into_iter().find(|f| f.name == name)
    }

    /// A second watched root beside `photos_root`'s, canonicalised the same way.
    fn second_root(dir: &tempfile::TempDir) -> PathBuf {
        let root = dir.path().join("second");
        fs::create_dir_all(&root).unwrap();
        paths::canonicalize(root).unwrap()
    }

    #[test]
    fn a_renamed_file_keeps_its_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        let album = decorate(&lib, id);

        let b = root.join("b.jpg");
        fs::rename(&a, &b).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (
                report.moved,
                report.added,
                report.marked_missing,
                report.purged
            ),
            (1, 0, 0, 0)
        );
        assert_followed(&lib, id, album, &b);
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
    }

    #[test]
    fn a_file_moved_to_another_folder_keeps_its_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        let album = decorate(&lib, id);

        let new = root.join("2024").join("a.jpg");
        fs::create_dir(root.join("2024")).unwrap();
        fs::rename(&a, &new).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 0)
        );
        assert_followed(&lib, id, album, &new);
        assert_eq!(
            lib.item(id).unwrap().unwrap().folder_id,
            folder(&lib, "2024").unwrap().id
        );
    }

    /// A folder renamed while photon was closed: every file of it is new to the one walk
    /// that finds it, and that walk's own list still holds every row at its old path.
    #[test]
    fn a_renamed_folder_keeps_every_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 6));
        write_file(&root, "trip/b.jpg", &jpeg_bytes(8, 7));
        write_file(&root, "trip/c.jpg", &jpeg_bytes(9, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let ids = |lib: &Library| -> BTreeSet<i64> {
            let known = lib.known_items(watched.id).unwrap();
            assert!(known.values().all(|k| !k.missing));
            known.values().map(|k| k.id).collect()
        };
        let before = ids(&lib);
        let id = id_at(&lib, &watched, &root.join("trip").join("a.jpg"));
        let album = decorate(&lib, id);

        fs::rename(root.join("trip"), root.join("holiday")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (3, 0, 0)
        );
        assert_eq!(ids(&lib), before);
        assert_followed(&lib, id, album, &root.join("holiday").join("a.jpg"));

        let report = scan(&lib, &watched, 3);
        assert_eq!(
            (report.unchanged, report.marked_missing, report.purged),
            (3, 0, 0)
        );
        assert_eq!(ids(&lib), before);
        assert!(
            folder(&lib, "trip").is_none(),
            "the old folder's row stayed"
        );
    }

    /// The watcher reports a move as its two directories, scanned in either order. Here the
    /// destination comes first: the row is live, not missing, when it is re-pointed.
    #[test]
    fn a_move_is_followed_when_only_the_destination_is_scanned() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        let album = decorate(&lib, id);

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&old, &new).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("y"), 2);

        assert_eq!((report.moved, report.added), (1, 0));
        assert_followed(&lib, id, album, &new);

        // The source's scan comes second, and its own list no longer holds the row.
        let report = scan_sub(&lib, &watched, &root.join("x"), 3);
        assert_eq!((report.marked_missing, report.purged), (0, 0));
        // A scan that read its list before the move was written still holds the row at its
        // old path - a scan of another watched folder can, each having its own slot. Its
        // writes are refused by that path.
        let stale = [(id, key(&old))];
        assert_eq!(lib.mark_missing_at(&stale, 5).unwrap(), 0);
        assert_eq!(lib.purge_at(&stale).unwrap(), 0);
        assert_followed(&lib, id, album, &new);
    }

    /// The other order: the source's scan has already marked the row missing. Reviving it
    /// with `update_items`, as a file that reappears at its own path is, would delete the
    /// detections with the hashes.
    #[test]
    fn a_move_is_followed_after_the_source_was_scanned_first() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        let album = decorate(&lib, id);

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&old, &new).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("x"), 2);
        assert_eq!(report.marked_missing, 1);
        assert!(lib.item(id).unwrap().unwrap().missing_since.is_some());

        let report = scan_sub(&lib, &watched, &root.join("y"), 3);
        assert_eq!((report.moved, report.added), (1, 0));
        assert_followed(&lib, id, album, &new);
    }

    /// The source scanned first, then the whole root: that walk's own list holds the row as
    /// missing at its old path, which is what a walk purges. Purged by id alone, the row it
    /// had just re-pointed went, with everything on it.
    #[test]
    fn a_row_already_missing_is_not_purged_by_the_walk_that_follows_it() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        let album = decorate(&lib, id);

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&old, &new).unwrap();
        assert_eq!(
            scan_sub(&lib, &watched, &root.join("x"), 2).marked_missing,
            1
        );

        let report = scan(&lib, &watched, 3);
        assert_eq!((report.moved, report.added, report.purged), (1, 0, 0));
        assert_followed(&lib, id, album, &new);
    }

    #[test]
    fn a_move_between_two_watched_folders_keeps_the_row() {
        let (dir, lib) = temp_library();
        let (first, second) = (photos_root(&dir), second_root(&dir));
        let old = write_file(&first, "a.jpg", &jpeg_bytes(8, 6));
        // Without it the first root is empty once the photo has left, which reads as an
        // unmounted volume.
        write_file(&first, "keep.jpg", &jpeg_bytes(8, 7));
        let from = lib.add_watched_folder(&first, &[]).unwrap();
        let to = lib.add_watched_folder(&second, &[]).unwrap();
        scan(&lib, &from, 1);
        let id = id_at(&lib, &from, &old);
        let album = decorate(&lib, id);

        let new = second.join("a.jpg");
        fs::rename(&old, &new).unwrap();
        let report = scan(&lib, &to, 2);

        assert_eq!((report.moved, report.added), (1, 0));
        assert_followed(&lib, id, album, &new);
        assert_eq!(id_at(&lib, &to, &new), id);

        let report = scan(&lib, &from, 3);
        assert_eq!((report.marked_missing, report.purged), (0, 0));
        assert_followed(&lib, id, album, &new);
    }

    /// A copy is not a move: the original is still there, so it keeps everything and the
    /// copy is a photo of its own.
    #[test]
    fn a_copy_beside_the_original_is_a_new_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        let album = decorate(&lib, id);

        let copy = root.join("b.jpg");
        fs::copy(&a, &copy).unwrap();
        // A file manager that keeps timestamps is the case; `fs::copy` does not on every
        // platform.
        set_mtime(&copy, fs::metadata(&a).unwrap().modified().unwrap());
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added, report.unchanged), (0, 1, 1));
        assert_followed(&lib, id, album, &a);
        let copied = id_at(&lib, &watched, &copy);
        assert_ne!(copied, id);
        assert!(lib.item_albums(copied).unwrap().is_empty());
    }

    /// `a.jpg` and `b.jpg` in `root`, byte-identical and with one modification time, indexed
    /// and then deleted: two rows that fit any file with those bytes and that time.
    fn deleted_twins(
        lib: &Library,
        root: &Path,
        bytes: &[u8],
        at: std::time::SystemTime,
    ) -> (WatchedFolder, i64, i64) {
        let a = write_file(root, "a.jpg", bytes);
        let b = write_file(root, "b.jpg", bytes);
        set_mtime(&a, at);
        set_mtime(&b, at);
        let watched = lib.add_watched_folder(root, &[]).unwrap();
        scan(lib, &watched, 1);
        let ids = (id_at(lib, &watched, &a), id_at(lib, &watched, &b));
        fs::remove_file(&a).unwrap();
        fs::remove_file(&b).unwrap();
        (watched, ids.0, ids.1)
    }

    #[test]
    fn two_rows_that_fit_are_not_guessed_between() {
        let bytes = jpeg_bytes(8, 6);
        let at = UNIX_EPOCH + Duration::from_secs(1_718_454_645);

        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, a, b) = deleted_twins(&lib, &root, &bytes, at);
        set_mtime(&write_file(&root, "c.jpg", &bytes), at);
        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (0, 1, 2)
        );
        for id in [a, b] {
            assert!(lib.item(id).unwrap().unwrap().missing_since.is_some());
        }

        // The file name decides, where it singles one of them out.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, a, b) = deleted_twins(&lib, &root, &bytes, at);
        let new = write_file(&root, "sub/a.jpg", &bytes);
        set_mtime(&new, at);
        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 1)
        );
        assert_eq!(lib.item(a).unwrap().unwrap().path, key(&new));
        assert!(lib.item(b).unwrap().unwrap().missing_since.is_some());
    }

    /// `n` byte-identical files with one modification time, indexed, and then the first of
    /// them moved to another folder: the report of the scan that finds it there.
    fn one_of_identical_files_moved(n: usize) -> ScanReport {
        let bytes = jpeg_bytes(8, 6);
        let at = UNIX_EPOCH + Duration::from_secs(1_718_454_645);
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        for i in 0..n {
            set_mtime(
                &write_file(&root, &format!("frames/{i:02}.jpg"), &bytes),
                at,
            );
        }
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        assert_eq!(scan(&lib, &watched, 1).added, n as u64);

        fs::create_dir(root.join("picked")).unwrap();
        fs::rename(
            root.join("frames").join("00.jpg"),
            root.join("picked").join("00.jpg"),
        )
        .unwrap();
        scan(&lib, &watched, 2)
    }

    /// Rows that cannot be told apart by what the library holds each cost a stat to rule
    /// out, and in a cluster of them every file asks about every other. Up to the cap
    /// (`Library::move_candidates`) the one whose file is gone is still found; past it the
    /// file is a new photo, and the row it was is marked missing.
    #[test]
    fn a_move_is_followed_among_identical_rows_only_up_to_the_cap() {
        let report = one_of_identical_files_moved(32);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 0)
        );
        let report = one_of_identical_files_moved(33);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (0, 1, 1)
        );
    }

    /// A row is claimed by one file of a batch, and the next file is offered what is left.
    /// Both new files are named `a.jpg`, so without the claim both pick `a.jpg`'s row: the
    /// second is refused by the write and inserted as new, and `b.jpg`'s row is left to be
    /// marked missing though exactly one file was there for it.
    #[test]
    fn a_row_is_claimed_by_one_file_and_the_next_takes_the_one_left() {
        let bytes = jpeg_bytes(8, 6);
        let at = UNIX_EPOCH + Duration::from_secs(1_718_454_645);
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, a, b) = deleted_twins(&lib, &root, &bytes, at);
        let news = [
            write_file(&root, "y/a.jpg", &bytes),
            write_file(&root, "z/a.jpg", &bytes),
        ];
        for new in &news {
            set_mtime(new, at);
        }

        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (2, 0, 0)
        );
        // Which of the two took which row is the walk's order, and not asserted.
        let now_at: BTreeSet<String> = [a, b]
            .iter()
            .map(|id| lib.item(*id).unwrap().unwrap().path)
            .collect();
        assert_eq!(now_at, news.iter().map(|p| key(p)).collect());
    }

    /// Two copies of a deleted original both fit its row, and the one that kept its name has
    /// it, whichever of them the walk meets first. A walk's order is the filesystem's, so the
    /// batch is handed to `find_moves` directly, in both orders, and then walked as well.
    #[test]
    fn of_two_files_that_fit_one_row_the_one_with_its_name_has_it() {
        let bytes = jpeg_bytes(8, 6);
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &bytes);
        let at = fs::metadata(&a).unwrap().modified().unwrap();
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let row = lib.item(id_at(&lib, &watched, &a)).unwrap().unwrap();
        fs::remove_file(&a).unwrap();
        let named = write_file(&root, "sub/a.jpg", &bytes);
        let copy = write_file(&root, "sub/copy.jpg", &bytes);
        set_mtime(&named, at);
        set_mtime(&copy, at);

        let decided = |batch: Vec<NewItem>| {
            let (moves, fresh) =
                find_moves(&lib, batch, &mut moved::Probe::new(watched.id)).unwrap();
            let moves: Vec<(i64, String)> = moves
                .into_iter()
                .map(|(row, item)| (row.id, item.file_name))
                .collect();
            let fresh: Vec<String> = fresh.into_iter().map(|item| item.file_name).collect();
            (moves, fresh)
        };
        let (named_item, copy_item) = (described_at(&row, &named), described_at(&row, &copy));
        let by_name = (
            vec![(row.id, "a.jpg".to_string())],
            vec!["copy.jpg".to_string()],
        );
        assert_eq!(
            decided(vec![copy_item.clone(), named_item.clone()]),
            by_name
        );
        assert_eq!(decided(vec![named_item, copy_item.clone()]), by_name);

        // Neither has the row's name: the first of the batch has it, and the other is new.
        let other = described_at(&row, &root.join("sub").join("other.jpg"));
        assert_eq!(
            decided(vec![copy_item, other]),
            (
                vec![(row.id, "copy.jpg".to_string())],
                vec!["other.jpg".to_string()]
            )
        );

        let report = scan(&lib, &watched, 2);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 1, 0)
        );
        assert_eq!(lib.item(row.id).unwrap().unwrap().path, key(&named));
    }

    /// Rule 3 through a scan: where names are compared without case, the old spelling still
    /// opens the renamed file, so its row's file is never "gone" - it is this very file.
    ///
    /// Run only where the filesystem is case-insensitive. Elsewhere the rename makes a plain
    /// new path and the row follows as any renamed file's does, which would pass without
    /// reaching the rule this is for.
    #[test]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows")),
        ignore = "needs a case-insensitive filesystem"
    )]
    fn a_case_only_file_rename_keeps_its_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "IMG.JPG", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        let album = decorate(&lib, id);

        fs::rename(&old, root.join("img.jpg")).unwrap();
        let new = paths::canonicalize(root.join("img.jpg")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 0)
        );
        assert_followed(&lib, id, album, &new);
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
    }

    /// The same for a directory: the old spelling still opens it, so it is never "gone", and
    /// the folder's name and Hide folder flag would be left on a row the scan then prunes.
    ///
    /// Run only where the filesystem is case-insensitive, as above: elsewhere the old
    /// directory is simply gone.
    #[test]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows")),
        ignore = "needs a case-insensitive filesystem"
    )]
    fn a_case_only_folder_rename_keeps_its_rows_its_name_and_its_hide_flag() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "Trip/a.jpg", &jpeg_bytes(8, 6));
        let b = write_file(&root, "Trip/b.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let (a, b) = (id_at(&lib, &watched, &a), id_at(&lib, &watched, &b));
        let old = folder(&lib, "Trip").unwrap().id;
        lib.set_folder_alias(old, Some("Holiday")).unwrap();
        lib.set_folder_hidden(old, true).unwrap();

        fs::rename(root.join("Trip"), root.join("trip")).unwrap();
        let new = paths::canonicalize(root.join("trip")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (2, 0, 0)
        );
        let path_of = |id: i64| lib.item(id).unwrap().unwrap().path;
        assert_eq!(path_of(a), key(&new.join("a.jpg")));
        assert_eq!(path_of(b), key(&new.join("b.jpg")));
        let renamed = lib
            .folders()
            .unwrap()
            .into_iter()
            .find(|f| f.path == key(&new))
            .expect("a folder row at the new spelling");
        assert_eq!(renamed.alias.as_deref(), Some("Holiday"));
        assert!(renamed.hidden, "the folder came back visible");
        assert!(is_hidden(&lib, a) && is_hidden(&lib, b));

        let report = scan(&lib, &watched, 3);
        assert_eq!(
            (report.unchanged, report.marked_missing, report.purged),
            (2, 0, 0)
        );
        assert!(
            lib.folders().unwrap().iter().all(|f| f.id != old),
            "the old spelling's folder row stayed"
        );
        assert_eq!(path_of(a), key(&new.join("a.jpg")));
        assert_eq!(path_of(b), key(&new.join("b.jpg")));
    }

    /// An unplugged drive's files all answer "no such file", and so do those of an unmounted
    /// volume whose mount point was left behind. Photos copied off it into another watched
    /// folder are new photos; the drive's rows keep everything for when it comes back.
    #[test]
    fn an_offline_or_empty_root_gives_no_candidates() {
        let (dir, lib) = temp_library();
        let (first, second) = (photos_root(&dir), second_root(&dir));
        let bytes = jpeg_bytes(8, 6);
        let a = write_file(&first, "a.jpg", &bytes);
        let at = fs::metadata(&a).unwrap().modified().unwrap();
        let drive = lib.add_watched_folder(&first, &[]).unwrap();
        let other = lib.add_watched_folder(&second, &[]).unwrap();
        scan(&lib, &drive, 1);
        let id = id_at(&lib, &drive, &a);
        let album = decorate(&lib, id);

        let unplugged = dir.path().join("unplugged");
        fs::rename(&first, &unplugged).unwrap();
        set_mtime(&write_file(&second, "b.jpg", &bytes), at);
        let report = scan(&lib, &other, 2);
        assert_eq!((report.moved, report.added), (0, 1));
        assert_followed(&lib, id, album, &a);

        // The mount point left behind: a directory, and empty.
        fs::rename(&unplugged, &first).unwrap();
        fs::remove_file(&a).unwrap();
        set_mtime(&write_file(&second, "c.jpg", &bytes), at);
        let report = scan(&lib, &other, 3);
        assert_eq!((report.moved, report.added), (0, 1));
        assert_followed(&lib, id, album, &a);
    }

    #[test]
    fn a_different_picture_with_the_same_size_and_time_is_not_followed() {
        // Padded after the end-of-image marker to one byte length, whatever the two encode to.
        let (mut wide, mut tall) = (jpeg_bytes(8, 6), jpeg_bytes(6, 8));
        let len = wide.len().max(tall.len());
        wide.resize(len, 0);
        tall.resize(len, 0);
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &wide);
        let at = fs::metadata(&a).unwrap().modified().unwrap();
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let old = lib.item(id_at(&lib, &watched, &a)).unwrap().unwrap();

        fs::remove_file(&a).unwrap();
        let b = write_file(&root, "b.jpg", &tall);
        set_mtime(&b, at);
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (0, 1, 1)
        );
        let new = lib.item(id_at(&lib, &watched, &b)).unwrap().unwrap();
        assert_ne!(new.id, old.id);
        // What the fixture is for: nothing but the picture's shape tells the two apart.
        assert_eq!(
            (new.size, new.mtime_ms, new.taken_at),
            (old.size, old.mtime_ms, old.taken_at)
        );
        assert_eq!(
            ((old.width, old.height), (new.width, new.height)),
            ((8, 6), (6, 8))
        );
    }

    /// A renamed directory is a new folder row, and what the user set on the old one follows
    /// the photos: the name and Hide folder, which a file added to it since takes as well.
    #[test]
    fn a_renamed_folder_keeps_its_name_and_its_hide_flag() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 6));
        let b = write_file(&root, "trip/b.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let moved = [id_at(&lib, &watched, &a), id_at(&lib, &watched, &b)];
        let trip = folder(&lib, "trip").unwrap().id;
        lib.set_folder_alias(trip, Some("Holiday")).unwrap();
        lib.set_folder_hidden(trip, true).unwrap();

        fs::rename(root.join("trip"), root.join("summer")).unwrap();
        let added = write_file(&root, "summer/new.jpg", &jpeg_bytes(9, 6));
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added), (2, 1));
        let summer = folder(&lib, "summer").unwrap();
        assert_eq!(summer.alias.as_deref(), Some("Holiday"));
        assert!(summer.hidden, "the folder came back visible");
        for id in moved {
            assert!(is_hidden(&lib, id));
        }
        assert!(
            is_hidden(&lib, id_at(&lib, &watched, &added)),
            "a file added to the renamed folder is visible"
        );
    }

    /// A folder takes a name and a Hide folder flag only as a row this walk made - which is
    /// what a renamed directory is. A folder that was there before is somewhere photos were
    /// moved *into*: handed the flag of the folder they left, every photo it already held
    /// was hidden, under the other folder's name.
    #[test]
    fn photos_moved_into_a_folder_that_was_already_there_do_not_rename_or_hide_it() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        let own = write_file(&root, "y/own.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let (id, own) = (id_at(&lib, &watched, &old), id_at(&lib, &watched, &own));
        let x = folder(&lib, "x").unwrap().id;
        lib.set_folder_alias(x, Some("Easter")).unwrap();
        lib.set_folder_hidden(x, true).unwrap();

        let new = root.join("y").join("a.jpg");
        fs::rename(&old, &new).unwrap();
        fs::remove_dir(root.join("x")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().path, key(&new));
        assert!(is_hidden(&lib, id), "its own flag");
        let y = folder(&lib, "y").unwrap();
        assert_eq!((y.alias, y.hidden), (None, false));
        assert!(!is_hidden(&lib, own), "a photo that was there all along");
    }

    /// A drive unplugged, its files renamed on another machine, and plugged back in. The
    /// watched folder's flag still says offline while the walk that brings it back runs; it
    /// is that walk's own files that say the drive is there.
    #[test]
    fn a_drive_that_comes_back_with_a_renamed_file_keeps_its_row() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        let album = decorate(&lib, id);
        // What the scan that found the root gone left behind.
        lib.set_watched_online(watched.id, false).unwrap();

        let b = root.join("b.jpg");
        fs::rename(&a, &b).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 0)
        );
        assert_followed(&lib, id, album, &b);
        assert!(lib.watched_folders().unwrap()[0].online);
    }

    /// Whether a row's drive is there is read from the library, once per walk, and that
    /// read can fail. Taken for "no drive is there", every renamed file of the walk was a new
    /// photo, and the same scan went on to mark the rows they had been as missing. The scan
    /// fails instead, before it has decided anything, and the next one follows the rename.
    #[test]
    fn a_scan_that_cannot_ask_which_drives_are_there_fails_before_it_calls_a_file_new() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        let album = decorate(&lib, id);
        let b = root.join("b.jpg");
        fs::rename(&a, &b).unwrap();

        // The read refused, as a corrupt page refuses it. From a connection of the test's
        // own: nothing in photon fails that read on request.
        let conn = rusqlite::Connection::open(lib.path()).unwrap();
        conn.execute_batch("ALTER TABLE watched_folders RENAME TO watched_elsewhere")
            .unwrap();
        let scanned = scan_watched(
            &lib,
            &watched,
            2,
            &ScanOptions::default(),
            &mut progress_only(|_| {}),
        );
        conn.execute_batch("ALTER TABLE watched_elsewhere RENAME TO watched_folders")
            .unwrap();

        assert!(scanned.is_err(), "the fixture did not fail the read");
        assert_followed(&lib, id, album, &a);
        assert_eq!(
            lib.known_items(watched.id).unwrap().len(),
            1,
            "the renamed file was given a row of its own"
        );

        let report = scan(&lib, &watched, 3);
        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (1, 0, 0)
        );
        assert_followed(&lib, id, album, &b);
    }

    /// What `apply_moves` takes for "a folder this walk made": a directory there was no row
    /// at the path of. A row that only changed parent was there before.
    #[test]
    fn only_a_folder_with_no_row_at_its_path_is_one_the_scan_made() {
        let (_dir, lib) = temp_library();
        let watched = crate::testutil::watch(&lib, "/p");
        let root = lib.upsert_folder(watched.id, None, "/p", 1).unwrap();
        let moved = lib.upsert_folder(watched.id, None, "/p/sub", 1).unwrap();
        let mut rows = FolderRows::load(&lib, watched.id).unwrap();

        let agreed = rows.ensure(&lib, watched.id, None, "/p", 2).unwrap();
        assert_eq!(agreed, (root, true));
        let reparented = rows
            .ensure(&lib, watched.id, Some(root), "/p/sub", 2)
            .unwrap();
        assert_eq!(reparented, (moved, false));
        assert!(rows.created.is_empty());

        let (new, _) = rows
            .ensure(&lib, watched.id, Some(root), "/p/new", 2)
            .unwrap();
        assert_eq!(rows.created, HashSet::from([new]));
    }

    /// The rows are re-pointed before the folder's name is handed on, and that second write
    /// can fail. By then the sink has to have been told: the rows are at their new paths
    /// whatever becomes of the scan, and told after, their thumbnails were neither carried
    /// over nor queued by it.
    #[test]
    fn a_failed_folder_inheritance_still_reports_the_rows_it_moved() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let before = lib.item(id_at(&lib, &watched, &a)).unwrap().unwrap();
        lib.set_folder_alias(folder(&lib, "trip").unwrap().id, Some("Holiday"))
            .unwrap();
        // The write of a folder's name refused, as a full disk refuses it. From a connection
        // of the test's own: nothing in photon fails that write on request.
        rusqlite::Connection::open(lib.path())
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_alias BEFORE UPDATE OF alias ON folders
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
            .unwrap();

        fs::rename(root.join("trip"), root.join("summer")).unwrap();
        let mut sink = Reports::default();
        let scanned = scan_watched(&lib, &watched, 2, &ScanOptions::default(), &mut sink);

        assert!(scanned.is_err(), "the fixture did not fail the write");
        let after = lib.item(before.id).unwrap().unwrap();
        assert_eq!(after.path, key(&root.join("summer").join("a.jpg")));
        assert_eq!(
            sink.0,
            [
                Reported::Moved(vec![(before.thumb_key(), after.thumb_key())]),
                Reported::Indexed(vec![before.id]),
            ]
        );
    }

    /// A link stands in for a case-insensitive filesystem, which no Linux runner has: the
    /// directory's old path still opens it, under its new one. It is then not "gone", and
    /// its rows, its name and its Hide folder flag follow because it is the same directory.
    #[test]
    #[cfg(unix)]
    fn a_folder_its_old_path_still_opens_keeps_its_rows_its_name_and_its_hide_flag() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 6));
        let b = write_file(&root, "trip/b.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let (a, b) = (id_at(&lib, &watched, &a), id_at(&lib, &watched, &b));
        let trip = folder(&lib, "trip").unwrap().id;
        lib.set_folder_alias(trip, Some("Holiday")).unwrap();
        lib.set_folder_hidden(trip, true).unwrap();

        let summer = root.join("summer");
        fs::rename(root.join("trip"), &summer).unwrap();
        std::os::unix::fs::symlink(&summer, root.join("trip")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!(
            (report.moved, report.added, report.marked_missing),
            (2, 0, 0)
        );
        let path_of = |id: i64| lib.item(id).unwrap().unwrap().path;
        assert_eq!(path_of(a), key(&summer.join("a.jpg")));
        assert_eq!(path_of(b), key(&summer.join("b.jpg")));
        let renamed = folder(&lib, "summer").unwrap();
        assert_eq!(renamed.alias.as_deref(), Some("Holiday"));
        assert!(renamed.hidden, "the folder came back visible");
    }

    /// One photo leaving a folder is not the folder moving: the folder is still there, with
    /// its name and its flag, and the folder the photo went to gets neither. The photo itself
    /// stays hidden, by its own flag.
    #[test]
    fn a_folder_that_still_exists_gives_nothing_to_the_one_its_photo_moved_to() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        write_file(&root, "x/keep.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        let x = folder(&lib, "x").unwrap().id;
        lib.set_folder_alias(x, Some("Easter")).unwrap();
        lib.set_folder_hidden(x, true).unwrap();

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&old, &new).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().path, key(&new));
        let y = folder(&lib, "y").unwrap();
        assert_eq!((y.alias, y.hidden), (None, false));
        let x = folder(&lib, "x").unwrap();
        assert_eq!((x.alias.as_deref(), x.hidden), (Some("Easter"), true));
        assert!(is_hidden(&lib, id));
    }

    /// Picasa's data follows only with the folder's INI: the Picasa pass mirrors the INI of
    /// the folder the photo is in now, and a photo moved alone has left its INI behind.
    #[test]
    fn a_photo_moved_without_its_ini_loses_its_picasa_star() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        write_file(&root, "x/.picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);
        assert!(is_starred(lib.item(id).unwrap().unwrap().rating));

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&old, &new).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added, report.restarred), (1, 0, 1));
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.path, key(&new));
        assert!(!is_starred(item.rating));
    }

    #[test]
    fn a_folder_moved_with_its_ini_keeps_its_stars() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let old = write_file(&root, "x/a.jpg", &jpeg_bytes(8, 6));
        write_file(&root, "x/.picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &old);

        fs::rename(root.join("x"), root.join("z")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.added, report.restarred), (1, 0, 0));
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.path, key(&root.join("z").join("a.jpg")));
        assert!(is_starred(item.rating));
    }

    /// `x/a.jpg`, hidden by a `hidden=yes` line in `x`'s INI and by nothing else, with a
    /// second photo beside it so that the folder outlives a move of the first.
    fn hidden_by_picasa(lib: &Library, root: &Path) -> (WatchedFolder, PathBuf, i64) {
        let a = write_file(root, "x/a.jpg", &jpeg_bytes(8, 6));
        write_file(root, "x/keep.jpg", &jpeg_bytes(8, 7));
        write_file(root, "x/.picasa.ini", b"[a.jpg]\nhidden=yes\n");
        let watched = lib.add_watched_folder(root, &[]).unwrap();
        assert_eq!(scan(lib, &watched, 1).rehidden, 1);
        let id = id_at(lib, &watched, &a);
        assert!(is_hidden(lib, id));
        (watched, a, id)
    }

    /// The INI's last answer for a row, as the hidden pass reads it.
    fn picasa_said(lib: &Library, id: i64) -> Option<bool> {
        let folder = lib.item(id).unwrap().unwrap().folder_id;
        let items = lib.folder_item_names(folder).unwrap();
        items.iter().find(|i| i.id == id).unwrap().picasa_hidden
    }

    /// The INI lists a photo under its file name. Renamed, the photo has no line there, and
    /// with the old name's "yes" still on its row the hidden pass read that as Picasa having
    /// un-hidden it: the photo came back visible, by a rename.
    #[test]
    fn a_photo_hidden_by_picasa_stays_hidden_when_it_is_renamed() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, a, id) = hidden_by_picasa(&lib, &root);

        let b = root.join("x").join("b.jpg");
        fs::rename(&a, &b).unwrap();
        // The watcher's scan, of the one directory.
        let report = scan_sub(&lib, &watched, &root.join("x"), 2);

        assert_eq!((report.moved, report.rehidden), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().path, key(&b));
        assert!(is_hidden(&lib, id), "a rename un-hid the photo");
        // The new name's answer is on record, and the next scan agrees with it.
        assert_eq!(picasa_said(&lib, id), Some(false));
        assert_eq!(scan(&lib, &watched, 3).rehidden, 0);
        assert!(is_hidden(&lib, id));
    }

    /// The same for a photo moved out of the folder alone: it has left its INI behind, and
    /// the folder it is in now says nothing about it.
    #[test]
    fn a_photo_hidden_by_picasa_stays_hidden_when_it_is_moved_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, a, id) = hidden_by_picasa(&lib, &root);

        let new = root.join("y").join("a.jpg");
        fs::create_dir(root.join("y")).unwrap();
        fs::rename(&a, &new).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.rehidden), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().path, key(&new));
        assert!(is_hidden(&lib, id), "a move un-hid the photo");
        assert_eq!(scan(&lib, &watched, 3).rehidden, 0);
        assert!(is_hidden(&lib, id));
    }

    /// Forgetting the INI's answer has to stop at hidden photos. A photo the user un-hid in
    /// photon keeps the "yes" its INI still says (photon never writes `hidden=`), and that
    /// record is all that holds the unhide: forgotten, the renamed folder's INI was a first
    /// read of a `hidden=yes`, which is followed, and the photo was hidden again.
    #[test]
    fn a_photo_unhidden_in_photon_stays_visible_when_its_folder_is_renamed_with_its_ini() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, _, id) = hidden_by_picasa(&lib, &root);
        lib.set_hidden(&[id], false).unwrap();

        fs::rename(root.join("x"), root.join("z")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.rehidden), (2, 0));
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.path, key(&root.join("z").join("a.jpg")));
        assert!(!item.hidden, "a folder rename undid the user's unhide");
        assert_eq!(picasa_said(&lib, id), Some(true));
    }

    /// A photo Picasa hid, moved with its folder and its INI: the same line is read again
    /// under the new path, and nothing changes.
    #[test]
    fn a_photo_hidden_by_picasa_stays_hidden_when_its_folder_is_renamed_with_its_ini() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let (watched, _, id) = hidden_by_picasa(&lib, &root);

        fs::rename(root.join("x"), root.join("z")).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.rehidden), (2, 0));
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.path, key(&root.join("z").join("a.jpg")));
        assert!(item.hidden, "the photo came back visible");
        assert_eq!(picasa_said(&lib, id), Some(true));
    }

    /// A hidden folder hides what arrives in it, and the hidden pass must not take that back.
    /// The photo here is visible when it moves (un-hidden in photon, its INI still saying
    /// yes) and is hidden by the folder's flag in the move's own write; with the old "yes"
    /// kept, the folder's missing line read as Picasa un-hiding it, in a folder the user hid.
    #[test]
    fn a_photo_moved_into_a_hidden_folder_is_not_unhidden_by_the_ini_it_left() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "put-away/own.jpg", &jpeg_bytes(9, 6));
        let (watched, a, id) = hidden_by_picasa(&lib, &root);
        lib.set_hidden(&[id], false).unwrap();
        lib.set_folder_hidden(folder(&lib, "put-away").unwrap().id, true)
            .unwrap();

        let new = root.join("put-away").join("a.jpg");
        fs::rename(&a, &new).unwrap();
        let report = scan(&lib, &watched, 2);

        assert_eq!((report.moved, report.rehidden), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().path, key(&new));
        assert!(is_hidden(&lib, id), "visible in a folder the user hid");
    }

    /// What a scan told its sink about moved and indexed rows, in order.
    #[derive(Debug, PartialEq)]
    enum Reported {
        Moved(Vec<(u64, u64)>),
        Indexed(Vec<i64>),
    }

    #[derive(Default)]
    struct Reports(Vec<Reported>);

    impl ScanSink for Reports {
        fn progress(&mut self, _: &ScanProgress) {}
        fn moved(&mut self, keys: &[(u64, u64)]) {
            self.0.push(Reported::Moved(keys.to_vec()));
        }
        fn indexed(&mut self, ids: &[i64]) {
            self.0.push(Reported::Indexed(ids.to_vec()));
        }
    }

    /// The pair is what lets the engine carry the cached thumbnails over, so it has to be
    /// the keys the cache used and will use - the edited photo's, not the bare file's - and
    /// it has to arrive before `indexed` queues the row for a thumbnail it would otherwise
    /// render again.
    #[test]
    fn a_move_reports_the_old_and_new_thumbnail_keys() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = id_at(&lib, &watched, &a);
        decorate(&lib, id);
        let before = lib.item(id).unwrap().unwrap();
        assert_ne!(
            before.thumb_key(),
            fingerprint(&before.path, before.size, before.mtime_ms),
            "the photo is edited, so its key is not the file's"
        );

        fs::rename(&a, root.join("b.jpg")).unwrap();
        let mut sink = Reports::default();
        scan_watched(&lib, &watched, 2, &ScanOptions::default(), &mut sink).unwrap();

        let after = lib.item(id).unwrap().unwrap();
        assert_ne!(before.thumb_key(), after.thumb_key());
        assert_eq!(
            sink.0,
            [
                Reported::Moved(vec![(before.thumb_key(), after.thumb_key())]),
                Reported::Indexed(vec![id]),
            ]
        );
    }

    /// `item`'s file at `path`, as `describe` hands a new file over.
    fn described_at(item: &Item, path: &Path) -> NewItem {
        NewItem {
            folder_id: item.folder_id,
            path: key(path),
            file_name: path.file_name().unwrap().to_str().unwrap().to_string(),
            kind: item.kind,
            size: item.size,
            mtime_ms: item.mtime_ms,
            width: item.width,
            height: item.height,
            orientation: item.orientation,
            taken_at: item.taken_at,
            rating: None,
            camera: item.camera.clone(),
            tags: Vec::new(),
            caption: None,
            duration_ms: item.duration_ms,
        }
    }

    fn candidate_of(lib: &Library, item: &Item) -> MoveCandidate {
        lib.move_candidates(item.size, item.mtime_ms, item.kind)
            .unwrap()
            .into_iter()
            .find(|c| c.id == item.id)
            .unwrap()
    }

    /// Between the lookup and the write, a scan of another watched folder can purge the row
    /// or claim it for a file of its own. The file then has no row to take and must get one:
    /// dropped, it stays out of the library until some later scan. A race has no seam in a
    /// test, so this is pinned one level down, on the function that applies decided moves.
    #[test]
    fn a_file_whose_row_went_away_is_inserted() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let c = write_file(&root, "c.jpg", &jpeg_bytes(8, 7));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let lost = lib.item(id_at(&lib, &watched, &a)).unwrap().unwrap();
        let kept = lib.item(id_at(&lib, &watched, &c)).unwrap().unwrap();
        let (b, d) = (root.join("b.jpg"), root.join("d.jpg"));
        fs::rename(&a, &b).unwrap();
        fs::rename(&c, &d).unwrap();
        let moves = vec![
            (candidate_of(&lib, &lost), described_at(&lost, &b)),
            (candidate_of(&lib, &kept), described_at(&kept, &d)),
        ];
        // The lookup is done; now the other scan purges the first row.
        lib.purge_items(&[lost.id]).unwrap();

        let mut report = ScanReport::default();
        let mut sink = Reports::default();
        let unmoved = apply_moves(&lib, moves, &HashSet::new(), &mut report, &mut sink).unwrap();

        assert_eq!(unmoved, [described_at(&lost, &b)]);
        assert_eq!(report.moved, 1);
        let now = lib.item(kept.id).unwrap().unwrap();
        assert_eq!(now.path, key(&d));
        assert_eq!(
            sink.0,
            [
                Reported::Moved(vec![(kept.thumb_key(), now.thumb_key())]),
                Reported::Indexed(vec![kept.id]),
            ],
            "only the row that moved is reported"
        );

        // `flush_new` inserts what is handed back; here the next scan stands in for it.
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.added, report.unchanged), (1, 1));
        assert!(lib.known_items(watched.id).unwrap().contains_key(&key(&b)));
    }

    /// Like `restarred`, `moved` can be a report's only non-zero field, and a scan that only
    /// followed a rename must still rebuild the grid: it shows the old path's folder.
    #[test]
    fn a_scan_that_only_followed_a_move_touches_rows() {
        let report = ScanReport {
            moved: 1,
            ..ScanReport::default()
        };
        assert!(report.touched_rows());
        assert!(!ScanReport::default().touched_rows());
    }

    /// `move_items` moves the thumbnail collector's epoch, a moved row's old key being
    /// garbage. A batch that followed nothing must not reach it, or every import of new
    /// photos makes due the walk of the whole cache the epoch exists to avoid.
    #[test]
    fn an_import_that_follows_no_move_orphans_no_thumbnail() {
        const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 6));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let epoch = lib.thumb_gc_due(1_000, WEEK).unwrap().unwrap();
        lib.thumb_gc_done(epoch, 1_000).unwrap();

        assert_eq!(scan(&lib, &watched, 1).added, 1);
        assert_eq!(lib.thumb_gc_due(1_000, WEEK).unwrap(), None);

        fs::rename(&a, root.join("b.jpg")).unwrap();
        assert_eq!(scan(&lib, &watched, 2).moved, 1);
        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some());
    }
}
