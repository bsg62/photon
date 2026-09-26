//! The running library: photon-core services plus the current grid snapshot and the
//! background scans. Plain Rust, so it can be tested without a webview.

use crate::events::{Events, ExportProgress, FolderStatus, LibraryChanged, ScanProgressEvent};
use crate::watch::{WatcherService, join_within};
use parking_lot::{Mutex, RwLock};
use photon_core::{
    Error, Result,
    edit::Edit,
    export::{self, Source},
    grid::{GridIndex, GridView},
    library::{Library, WatchedFolder},
    media::MediaKind,
    now_ms, paths, picasa,
    scanner::{ScanOptions, ScanProgress, ScanSink, scan_subtree, scan_watched},
    thumbs::{Priority, ThumbCache, ThumbService},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Minimum time between grid rebuilds and between progress events during one scan.
const THROTTLE: Duration = Duration::from_millis(250);

/// Minimum time between the grid rebuilds poster frames ask for. Longer than a scan's
/// `THROTTLE`: a frame changes no row the grid lays out, only brings a tile that gave up
/// back to asking (`Tile.svelte`'s `pageTick` effect), so a second's delay costs nothing
/// visible, while a rebuild per frame is a whole-library query - 55-70 ms at 100k items,
/// plus the UI's `gridInfo` and viewer re-reads - for every video in a folder of them.
const FRAME_REFRESH_EVERY: Duration = Duration::from_secs(1);

/// How long `shutdown` waits for a look-alike pass in progress to actually stop before
/// giving up and leaving it to finish on its own. Bounded for the same reason
/// `watch::STOP_TIMEOUT` is: the pass's `hash_candidates` reads the original photo files,
/// so a thread stuck on a dead network mount would otherwise mean the app never quits.
const SIMILAR_PASS_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// How old the last thumbnail collection may be before startup runs one regardless of
/// whether anything has orphaned a thumbnail since. See `Library::thumb_gc_due`.
const THUMB_GC_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

pub struct EngineConfig {
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
    pub workers: usize,
}

struct RunningScan {
    token: u64,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// Which photos the grid shows. The view's argument - the search query, a contact hash,
/// an album id, a keyword - lives beside the view rather than inside it: `GridView` is
/// `Copy` and is mirrored in TypeScript as plain strings (spec §4). Each parameterised
/// view reads `arg` its own way (`Library::entries_for`); the others ignore it.
///
/// `epoch` goes up on every change to the pair. A rebuild reads it with the state it is
/// about to query for and hands it back at publish time; a mismatch means a setter has
/// moved on and that setter's own rebuild is the one that gets published.
#[derive(Clone, Debug)]
struct ViewState {
    view: GridView,
    arg: String,
    epoch: u64,
}

/// What one rebuild was started against: the view state, and a sequence number that
/// orders it among all rebuilds. Both are read together, before the query begins.
#[derive(Clone, Debug)]
struct Rebuild {
    state: ViewState,
    seq: u64,
}

/// What one export came to: how many copies were written, how many photos could not be,
/// and the first reason why not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Export {
    pub written: usize,
    pub failed: usize,
    pub reason: Option<String>,
}

/// How often an export tells the UI where it has got to. Short enough to look live, long
/// enough that a fast export of small files is not mostly event traffic.
const EXPORT_PROGRESS_EVERY: Duration = Duration::from_millis(100);

pub struct Engine {
    pub lib: Arc<Library>,
    pub thumbs: ThumbService,
    /// The same cache the thumbnail service writes into. Held here because the look-alike
    /// pass hashes the cached grid thumbnails rather than the photos.
    cache: Arc<ThumbCache>,
    excluded: Vec<PathBuf>,
    grid: RwLock<(u64, Arc<GridIndex>)>,
    /// Which photos the grid shows (and the view's argument), and a counter that changes
    /// with it. Held only for the moment of reading or writing it - never across a query or
    /// an index build - so a view switch on the UI thread is never made to wait for a
    /// rebuild a scan is running.
    state: Mutex<ViewState>,
    /// Serialises the publish-and-emit step of a rebuild and holds the `seq` of the last
    /// rebuild published, so two rebuilds finishing together can't publish under
    /// out-of-order version numbers, emit `library_changed` out of order, or land an
    /// older read over a newer one. Not the query and build themselves: those run
    /// unserialised, and the two checks in `publish_if_current` are what keep a slow, stale
    /// one from overwriting a newer view (`epoch`) or newer rows (`seq`).
    refresh: Mutex<u64>,
    /// Source of `Rebuild::seq`. Taken at snapshot time, before the query begins.
    next_rebuild: AtomicU64,
    events: Arc<dyn Events>,
    scans: Mutex<HashMap<i64, RunningScan>>,
    /// Watched ids whose removal is under way. Per folder what `shutting_down` is for the
    /// whole engine: `remove_folder` cancels the running scan and only then deletes the
    /// folder, and the watcher is live throughout that window - an event arriving in it
    /// would start a *new* scan, which then writes the folder's rows back after the
    /// deletion has committed. `cancel_scan` closes that for the scan it cancelled; this
    /// closes it for the one that has not started yet.
    removing: Mutex<HashSet<i64>>,
    next_token: AtomicU64,
    /// Set once by `shutdown`. Once true, no new scan starts and the startup thread stops
    /// at its next checkpoint.
    shutting_down: AtomicBool,
    /// The handle of the thread spawned by `startup`, if any is still outstanding.
    /// `wait_for_startup` takes it and joins it, and is safe to call more than once.
    startup: Mutex<Option<JoinHandle<()>>>,
    /// The handle of the thread spawned by `request_similar_pass`, if any is still
    /// outstanding. Mirrors `startup`: `wait_for_similar_pass` takes it and joins it, and
    /// is safe to call when none was ever spawned.
    similar_pass: Mutex<Option<JoinHandle<()>>>,
    /// The running watcher service, if one has been started. `start_watcher` and
    /// `stop_watcher` both take this lock for their whole check-then-act, so a `shutdown`
    /// racing `startup`'s call to `start_watcher` can never miss stopping a service that
    /// gets created just after it looked, nor leave one running past `shutdown`.
    ///
    /// Held as a strong `Arc`, even though the service itself holds an `Arc<Engine>` back:
    /// that cycle is deliberately broken by `stop_watcher`, which always `take`s the slot
    /// (dropping this `Arc`) before or as part of stopping it, so it never outlives an
    /// explicit stop.
    watcher: Mutex<Option<Arc<WatcherService>>>,
    /// Serialises `set_star`. Tauri runs async commands concurrently on a worker pool, and
    /// two stars into one folder are two read-modify-writes of the same INI: unserialised,
    /// the second read would miss the first write and the rename would drop it. Held across
    /// the database write too, so the rows land in the order the file did.
    ini_write: Mutex<()>,
    /// Serialises every write of a photo's edit; see `rotate_item`. A leaf lock: taken
    /// before the library and view locks and never while holding them.
    edit_write: Mutex<()>,
    /// Held by the one thread running the post-scan hashing passes; see `hash_after_scan`.
    /// It holds the look-alike pass's thumbnail reductions, which live here rather than in
    /// the pass because the pass runs again after every scan and confirms the same pairs
    /// again; see `photon_core::similar::Reductions`.
    hashing: Mutex<photon_core::similar::Reductions>,
    /// Set by every scan that ends, cleared by the pass as it starts a round. A scan that
    /// finds the pass already running leaves this behind instead of starting a second one.
    hash_requested: AtomicBool,
    /// Coalesces the rebuilds poster frames ask for; see `frame_stored`.
    frame_refresh: Mutex<FrameRefresh>,
}

/// When the last poster-frame rebuild ran, and whether a trailing one is already waiting.
#[derive(Default)]
struct FrameRefresh {
    last: Option<Instant>,
    scheduled: bool,
}

impl Engine {
    pub fn open(config: EngineConfig, events: Arc<dyn Events>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&config.cache_dir)?;
        let lib = Arc::new(Library::open(&config.db_path)?);
        let cache = Arc::new(ThumbCache::new(config.cache_dir.clone()));
        let thumbs = ThumbService::start(lib.clone(), cache.clone(), config.workers);
        let mut excluded = vec![config.cache_dir.clone()];
        // The cache root too, not just the thumbnail directory inside it.
        if let Some(cache_root) = config.cache_dir.parent() {
            excluded.push(cache_root.to_path_buf());
        }
        if let Some(data_dir) = config.db_path.parent() {
            excluded.push(data_dir.to_path_buf());
        }
        // Canonicalized once, here, because both things that compare against this list are
        // handed canonical paths: `add_watched_folder` canonicalizes the root it is checking,
        // and the watcher canonicalizes every event directory before `plan_scans` tests it.
        // Left raw, a symlink anywhere in the configured path makes the comparison match
        // nothing - and watching `$HOME` is allowed, so photon would then schedule a subtree
        // scan for its own cache every time it writes a thumbnail into it, whose scan writes
        // more thumbnails. A path that cannot be canonicalized keeps its raw form: it is no
        // worse than what it replaces.
        let excluded: Vec<PathBuf> = excluded
            .into_iter()
            .map(|p| photon_core::paths::canonicalize(&p).unwrap_or(p))
            .collect();

        let grid = Arc::new(GridIndex::build(
            lib.grid_entries()?,
            GridView::All.layout(),
        ));
        Ok(Arc::new(Self {
            lib,
            thumbs,
            cache,
            excluded,
            grid: RwLock::new((0, grid)),
            state: Mutex::new(ViewState {
                view: GridView::All,
                arg: String::new(),
                epoch: 0,
            }),
            refresh: Mutex::new(0),
            next_rebuild: AtomicU64::new(1),
            events,
            scans: Mutex::new(HashMap::new()),
            removing: Mutex::new(HashSet::new()),
            next_token: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            startup: Mutex::new(None),
            similar_pass: Mutex::new(None),
            watcher: Mutex::new(None),
            ini_write: Mutex::new(()),
            edit_write: Mutex::new(()),
            hashing: Mutex::new(Default::default()),
            hash_requested: AtomicBool::new(false),
            frame_refresh: Mutex::new(FrameRefresh::default()),
        }))
    }

    /// photon's own directories, never watched or scanned.
    pub fn excluded(&self) -> &[PathBuf] {
        &self.excluded
    }

    /// The current grid and its version. The version increases with every rebuild.
    pub fn grid(&self) -> (u64, Arc<GridIndex>) {
        let grid = self.grid.read();
        (grid.0, grid.1.clone())
    }

    /// Rebuilds the grid from the database for the current view and tells the UI.
    ///
    /// The query and the index build run with no engine lock held. This used to hold read
    /// guards on the view and the query for their whole duration, so a view switch on the
    /// UI thread - which needs the write side - stalled behind every scan-triggered rebuild:
    /// ~60ms per tick on a 100k library, for as long as the scan ran. What makes that safe
    /// is the `epoch` check at publish time; see `publish_if_current`.
    pub fn refresh_grid(&self) -> Result<()> {
        let rebuild = self.snapshot();
        let index = Arc::new(self.build_index(&rebuild.state)?);
        self.publish_if_current(index, &rebuild);
        Ok(())
    }

    /// A poster frame was stored: rebuild the grid, at most once per `FRAME_REFRESH_EVERY`.
    ///
    /// The scan's throttle is leading-edge only - it may skip the last tick because the end
    /// of the scan refreshes anyway. Frames have no end to lean on: the webview draws them
    /// one by one for as long as any are pending, and the last of a burst would be left
    /// unshown. So the first frame after a quiet second rebuilds at once, and any frame
    /// inside the second schedules one trailing rebuild at its end, which every later frame
    /// in the window rides on. That trailing rebuild clears `scheduled` *before* it
    /// snapshots, so a frame that found it scheduled was committed before the snapshot and
    /// is in it - the same "every commit is followed by a later-stamped rebuild" promise
    /// `publish_if_current` relies on.
    ///
    /// A failure is logged, not returned: the frame is already stored, and an error here
    /// would reach the page as a failed `put`.
    pub fn frame_stored(self: &Arc<Self>) {
        let wait = {
            let mut pending = self.frame_refresh.lock();
            if pending.scheduled {
                return;
            }
            let now = Instant::now();
            match pending.last {
                Some(last) if now < last + FRAME_REFRESH_EVERY => {
                    pending.scheduled = true;
                    last + FRAME_REFRESH_EVERY - now
                }
                _ => {
                    pending.last = Some(now);
                    Duration::ZERO
                }
            }
        };
        if wait.is_zero() {
            self.refresh_for_frames();
            return;
        }
        let engine = self.clone();
        let spawned = std::thread::Builder::new()
            .name("photon-frame-refresh".into())
            .spawn(move || {
                std::thread::sleep(wait);
                engine.trailing_frame_refresh();
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not schedule a grid refresh; refreshing now");
            self.trailing_frame_refresh();
        }
    }

    fn trailing_frame_refresh(&self) {
        {
            let mut pending = self.frame_refresh.lock();
            pending.scheduled = false;
            pending.last = Some(Instant::now());
        }
        self.refresh_for_frames();
    }

    fn refresh_for_frames(&self) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        if let Err(err) = self.refresh_grid() {
            tracing::warn!(%err, "grid refresh after a poster frame failed");
        }
    }

    /// The index for one view state, laid out the way that view is drawn.
    fn build_index(&self, state: &ViewState) -> Result<GridIndex> {
        Ok(GridIndex::build(
            self.lib.entries_for(state.view, &state.arg)?,
            state.view.layout(),
        ))
    }

    /// The state a rebuild is about to query for, stamped with its place in the sequence
    /// of rebuilds. The stamp is taken before the query so that "a later stamp" means "a
    /// query that began later", which under WAL means "reads at least as new a database".
    fn snapshot(&self) -> Rebuild {
        let state = self.state.lock().clone();
        let seq = self.next_rebuild.fetch_add(1, Ordering::SeqCst);
        Rebuild { state, seq }
    }

    /// Publishes `index`, built from `rebuild`'s snapshot, unless it has been overtaken.
    /// Returns whether it published.
    ///
    /// Two guards let rebuilds run unlocked, and both are needed. The `epoch` guard: a
    /// scan's rebuild that read the All view, then lost the race to a click on Starred,
    /// would otherwise publish its full index over the Starred one while `GridInfo` still
    /// said Starred. The `seq` guard: two rebuilds for the *same* view - two startup scans'
    /// final refreshes, or a scan tick racing `remove_folder` - can finish in the opposite
    /// order from their reads, and the earlier read landing last would drop rows already
    /// committed and shown, with nothing left running to put them back. Once this shipped
    /// with only the epoch guard and did exactly that.
    ///
    /// Discarding is safe for committed rows in both cases. Every commit is followed, on
    /// the thread that made it, by a rebuild snapshotted after it; whichever rebuild
    /// carries the highest `seq` therefore began its query after that commit and includes
    /// it, and nothing can be stamped later to block it. A setter's rebuild likewise reads
    /// after its epoch bump.
    fn publish_if_current(&self, index: Arc<GridIndex>, rebuild: &Rebuild) -> bool {
        let mut last_published = self.refresh.lock();
        if self.state.lock().epoch != rebuild.state.epoch {
            tracing::debug!("discarding a grid rebuilt for a view that has since changed");
            return false;
        }
        if rebuild.seq < *last_published {
            tracing::debug!("discarding a grid rebuilt from an older read than the one published");
            return false;
        }
        *last_published = rebuild.seq;
        let (version, len) = {
            let mut grid = self.grid.write();
            grid.0 += 1;
            grid.1 = index;
            (grid.0, grid.1.len())
        };
        self.events.library_changed(LibraryChanged { version, len });
        true
    }

    pub fn view(&self) -> GridView {
        self.state.lock().view
    }

    /// The active search query, or the empty string when no search is active.
    pub fn search_query(&self) -> String {
        let state = self.state.lock();
        if state.view == GridView::Search {
            state.arg.clone()
        } else {
            String::new()
        }
    }

    /// The current view and its argument, together, as one read of the state: read apart
    /// they could straddle a view switch and pair a view with another view's argument.
    pub fn view_and_arg(&self) -> (GridView, String) {
        let state = self.state.lock();
        (state.view, state.arg.clone())
    }

    /// Switches which photos the grid shows and rebuilds the index. Rebuilding is the same
    /// work startup already does; a second index kept in sync would be a large new surface
    /// for staleness bugs to speed up something already fast and rarely done.
    ///
    /// Rolls the view and argument back to their previous values if the rebuild fails, so
    /// a failed refresh can never leave `GridInfo` (the UI's one source of truth, spec §5)
    /// reporting a view/argument the grid was never actually rebuilt for. Without the
    /// rollback the bad state is sticky: every later `refresh_grid` — including the scan
    /// and watcher paths — re-reads the same failing query/view and fails again, and the
    /// empty state can't rescue it either, since `len` still reflects the old, unrelated
    /// result set.
    pub fn set_view(&self, view: GridView) -> Result<()> {
        self.rebuild_or_restore(|state| {
            // An argument left behind would reappear the next time its view is entered -
            // or, worse, be read by a different view: a search query as a contact hash.
            // Only re-entering the same parameterised view keeps it.
            if !(view.takes_argument() && view == state.view) {
                state.arg.clear();
            }
            state.view = view;
        })
    }

    /// Applies `mutate` to the view/query pair, rebuilds the grid, and puts both back if
    /// that fails. One place rather than one per setter: the rollback is what keeps the
    /// invariant above true, and a third setter (a sort order, a date filter) copying it a
    /// third time is how one of the copies ends up missing a field.
    ///
    /// Both the change and the rollback bump the epoch, so a rebuild in flight for either
    /// superseded state is discarded rather than published.
    fn rebuild_or_restore(&self, mutate: impl FnOnce(&mut ViewState)) -> Result<()> {
        let previous = {
            let mut state = self.state.lock();
            let previous = state.clone();
            mutate(&mut state);
            state.epoch += 1;
            previous
        };
        if let Err(err) = self.refresh_grid() {
            {
                let mut state = self.state.lock();
                state.view = previous.view;
                state.arg = previous.arg;
                state.epoch += 1;
            }
            // The bump above discards every rebuild in flight for the state just restored,
            // so rows a scan committed meanwhile would otherwise wait for its next tick. A
            // best-effort rebuild for the restored state closes that; it is the query that
            // was working a moment ago, and if it fails too there is nothing better to do
            // than log it.
            if let Err(err) = self.refresh_grid() {
                tracing::warn!(%err, "grid refresh for the restored view failed");
            }
            return Err(err);
        }
        Ok(())
    }

    /// Searches for `query`, or returns to the full library when it is blank.
    ///
    /// An empty query is not a search: matching nothing would show an empty grid, and
    /// matching everything would be the All view under a different name (spec §4).
    ///
    /// Rolls back on a failed refresh; see `set_view`'s doc comment for why.
    pub fn set_search_query(&self, query: &str) -> Result<()> {
        if query.trim().is_empty() {
            return self.set_view(GridView::All);
        }
        self.rebuild_or_restore(|state| {
            state.arg = query.to_string();
            state.view = GridView::Search;
        })
    }

    /// Shows the photos with a face of one Picasa contact. Rolls back on a failed refresh.
    pub fn set_person_view(&self, contact: &str) -> Result<()> {
        self.rebuild_or_restore(|state| {
            state.arg = contact.to_string();
            state.view = GridView::Person;
        })
    }

    /// Shows one album. Rolls back on a failed refresh.
    pub fn set_album_view(&self, album_id: i64) -> Result<()> {
        self.rebuild_or_restore(|state| {
            state.arg = album_id.to_string();
            state.view = GridView::Album;
        })
    }

    /// Shows the photos carrying one keyword. Rolls back on a failed refresh.
    pub fn set_tag_view(&self, tag: &str) -> Result<()> {
        self.rebuild_or_restore(|state| {
            state.arg = tag.to_string();
            state.view = GridView::Tag;
        })
    }

    /// Shows one photo and its copies. Rolls back on a failed refresh.
    ///
    /// The photo's hash is read now, into the argument, so its twins survive the photo
    /// itself being deleted and purged while the view is open (`CopiesArg`).
    pub fn set_copies_view(&self, item_id: i64) -> Result<()> {
        let arg = self.lib.copies_view_arg(item_id)?;
        self.rebuild_or_restore(|state| {
            state.arg = arg;
            state.view = GridView::Copies;
        })
    }

    /// After an album mutation: rebuilds the grid if an album is what it is showing. Any
    /// other view is unaffected by album membership, and the UI refetches the album list
    /// itself after the call that got here.
    pub fn albums_changed(&self) -> Result<()> {
        if self.state.lock().view == GridView::Album {
            self.refresh_grid()?;
        }
        Ok(())
    }

    /// Renames a tag and carries an open Tag view from the old name to the new one.
    ///
    /// The view moves inside the rename's transaction, and the view lock is held until
    /// the commit has finished, so a rebuild sees the old name with the old rules or the
    /// new name with the new ones. Apart, a scan's rebuild landing between them read the
    /// old name under the new rules and published an empty grid. A rebuild that
    /// snapshotted the old name but queried after the commit publishes only after the lock
    /// is released, and the epoch bump discards it.
    ///
    /// The order is the library's write lock, then the view lock. Taking the view lock
    /// first would stall every view switch, `grid_info` and rebuild behind whatever write
    /// is running - seconds, for the purge of a large folder. Nothing takes the two the
    /// other way round: the write lock is private to `Library`, which never calls back
    /// into the engine except through this guard.
    ///
    /// Returns the stored name. The rebuild follows as in `tags_changed`.
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<String> {
        let moved = std::cell::Cell::new(false);
        let renamed = self.lib.rename_tag_with(from, to, |to| {
            let mut state = self.state.lock();
            if state.view == GridView::Tag && state.arg == from {
                state.arg = to.to_string();
                state.epoch += 1;
                moved.set(true);
            }
            state
        });
        match renamed {
            Ok(to) => {
                self.tags_changed();
                Ok(to)
            }
            Err(err) => {
                // The commit failed after the view moved: put it back on the name the
                // rules still answer to.
                if moved.get() {
                    {
                        let mut state = self.state.lock();
                        if state.view == GridView::Tag {
                            state.arg = from.to_string();
                            state.epoch += 1;
                        }
                    }
                    self.tags_changed();
                }
                Err(err)
            }
        }
    }

    /// After a tag rule change the caller has committed: rebuilds the grid, since the Tag
    /// and Search views read the rules, and the rebuild's `library_changed` is what makes
    /// the sidebar refetch its tag list.
    ///
    /// Not `rebuild_or_restore`, and no error: the rule is already saved, so a failed
    /// rebuild must not put the view back on a name that no longer answers, nor report the
    /// saved change as failed. The next rebuild, from any source, shows it.
    pub fn tags_changed(&self) {
        if let Err(err) = self.refresh_grid() {
            tracing::warn!(%err, "grid refresh after a tag rule change failed");
        }
    }

    /// Sets or clears a photo's star: into the folder's Picasa INI first, then into the
    /// database, then the grid.
    ///
    /// The INI is the authority and the database its mirror (`scanner::apply_picasa_stars`
    /// sets every row from the file on every scan), which fixes the order. A database write
    /// that landed without the file would be undone by the next scan and the star would
    /// simply vanish; a file write that landed without the database is put right by the
    /// scan our own write triggers. That scan is the file watcher reacting to photon's
    /// write like any other, and it is wanted: it re-reads what we wrote, finds the rows
    /// already agree, and rebuilds nothing. Do not add a suppression for it.
    ///
    /// A photo the scanner has marked missing is refused: its folder may be an unmounted
    /// drive, and the INI photon would create there would be the only thing on it.
    pub fn set_star(&self, id: i64, starred: bool) -> Result<()> {
        let _serialised = self.ini_write.lock();
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        let path = Path::new(&item.path);
        let (Some(dir), Some(file_name)) = (path.parent(), path.file_name()) else {
            return Err(Error::NonUtf8Path(path.to_path_buf()));
        };
        let file_name = file_name.to_string_lossy();
        picasa::set_star(dir, &file_name, starred).map_err(|source| Error::IniWrite {
            path: dir.join(picasa::ini_name(dir)),
            source,
        })?;
        self.lib.set_ratings(&[(id, u8::from(starred))])?;
        self.refresh_grid()
    }

    /// Stars or unstars several photos, returning how many landed.
    ///
    /// Grouped by directory so each folder's INI is rewritten once (`picasa::set_stars`),
    /// with one rating write and one `refresh_grid` for the whole batch: the per-photo
    /// route would rewrite the same file once per photo and rebuild the grid as many times.
    ///
    /// A folder whose INI cannot be written is **skipped**, not fatal - one read-only
    /// folder in a selection must not cost the user every other photo in it. The count tells
    /// the caller how many landed so it can say so. When no folder at all could be written
    /// the first error is returned instead, so the user sees the reason rather than a zero.
    ///
    /// Unknown and missing ids are skipped for the same reason: a selection can outlive the
    /// photos in it (a scan purges one between the right-click and the click), and that is
    /// not a failure of the other eleven. `set_star`, which acts on the photo the user is
    /// looking at, still refuses them - see `live_item` for why the two differ.
    pub fn set_stars(&self, ids: &[i64], starred: bool) -> Result<usize> {
        let _serialised = self.ini_write.lock();
        let mut by_dir: BTreeMap<PathBuf, Vec<(i64, String)>> = BTreeMap::new();
        for &id in ids {
            let Some(item) = self.lib.item(id)? else {
                continue;
            };
            if item.missing_since.is_some() {
                continue;
            }
            let path = PathBuf::from(&item.path);
            let (Some(dir), Some(file_name)) = (path.parent(), path.file_name()) else {
                continue;
            };
            by_dir
                .entry(dir.to_path_buf())
                .or_default()
                .push((id, file_name.to_string_lossy().into_owned()));
        }

        let mut ratings: Vec<(i64, u8)> = Vec::new();
        let mut first_error: Option<Error> = None;
        for (dir, files) in &by_dir {
            let changes: Vec<(&str, bool)> = files
                .iter()
                .map(|(_, name)| (name.as_str(), starred))
                .collect();
            match picasa::set_stars(dir, &changes) {
                Ok(_) => ratings.extend(files.iter().map(|&(id, _)| (id, u8::from(starred)))),
                Err(source) => {
                    // The file first, then the database, per folder: a rating written for a
                    // folder whose INI never took it is shown to the user and then silently
                    // cleared by the next scan.
                    first_error.get_or_insert(Error::IniWrite {
                        path: dir.join(picasa::ini_name(dir)),
                        source,
                    });
                }
            }
        }

        if ratings.is_empty() {
            return match first_error {
                Some(err) => Err(err),
                None => Ok(0),
            };
        }
        self.lib.set_ratings(&ratings)?;
        self.refresh_grid()?;
        Ok(ratings.len())
    }

    /// Adds `tag` to one photo, returning the name stored — a rename rule can make that
    /// differ from what was typed, and the caller shows the stored name.
    ///
    /// The refresh is not optional: a tag change moves Tag-view membership and the text
    /// search matches, so a change that skipped the refresh chain would update the database
    /// while the grid showed stale rows.
    pub fn add_item_tag(&self, id: i64, tag: &str) -> Result<String> {
        self.live_item(id)?;
        let name = self.lib.add_item_tag(id, tag)?;
        self.refresh_grid()?;
        Ok(name)
    }

    /// Adds `tag` to several photos at once, returning the name stored and how many photos
    /// took it.
    ///
    /// One write and one `refresh_grid` for the whole batch: the per-photo route rebuilds
    /// the index once per photo, and the index is what the Tag view and the sidebar's counts
    /// are read from - on a large library that is the difference between a click and a
    /// stall. Ids that are no longer live photos are skipped by the library rather than
    /// refused here, for the reason `set_stars` gives: a selection can outlive its photos.
    ///
    /// A write that changed nothing does not rebuild, the way an identical edit does not:
    /// every rebuild bumps the grid version, and a version bump is what makes the viewer
    /// re-read its photo and the UI re-render.
    pub fn add_items_tag(&self, ids: &[i64], tag: &str) -> Result<(String, usize)> {
        let (name, count) = self.lib.add_items_tag(ids, tag)?;
        if count > 0 {
            self.refresh_grid()?;
        }
        Ok((name, count))
    }

    /// Hides or unhides several photos, returning how many changed. One write and one
    /// refresh, as `add_items_tag`, and none when nothing changed.
    ///
    /// Always a whole `refresh_grid`, never a narrower update: a hidden photo leaves every
    /// view and every count the sidebar shows, and all of them are read on the rebuild.
    pub fn set_items_hidden(&self, ids: &[i64], hidden: bool) -> Result<usize> {
        let count = self.lib.set_hidden(ids, hidden)?;
        if count > 0 {
            self.refresh_grid()?;
        }
        Ok(count)
    }

    /// Hides or unhides a folder and every photo in it (`Library::set_folder_hidden`),
    /// returning how many photos changed. The grid rebuilds only when some did: hiding an
    /// empty folder changes what later arrivals get, not what any view shows now.
    pub fn set_folder_hidden(&self, folder_id: i64, hidden: bool) -> Result<usize> {
        let count = self.lib.set_folder_hidden(folder_id, hidden)?;
        if count > 0 {
            self.refresh_grid()?;
        }
        Ok(count)
    }

    /// Removes the displayed name `tag` from several photos, returning how many changed.
    /// One write and one refresh, as `add_items_tag`.
    pub fn remove_items_tag(&self, ids: &[i64], tag: &str) -> Result<usize> {
        let count = self.lib.remove_items_tag(ids, tag)?;
        if count > 0 {
            self.refresh_grid()?;
        }
        Ok(count)
    }

    /// Removes the displayed name `tag` from one photo. Refreshes for the same reason.
    pub fn remove_item_tag(&self, id: i64, tag: &str) -> Result<()> {
        self.live_item(id)?;
        self.lib.remove_item_tag(id, tag)?;
        self.refresh_grid()
    }

    /// Copies photos out of the library into `dest`, returning what landed.
    ///
    /// **The only place photon writes a photo file**, and it writes only new files, at a
    /// destination the user picked in the system's own folder picker. The watched photos
    /// themselves are never opened for writing.
    ///
    /// A destination inside a watched folder is refused before anything is written. The
    /// scanner would index the copies as new photos - one click would double the library and
    /// fill the duplicate finder with pairs the user did not make.
    ///
    /// An edited photo is decoded at full size to be exported as it is shown, under
    /// `protocol::RENDERING` - the lock that bounds how many full-size decodes exist at once
    /// across the whole app. It is held across the render and *not* across the write: by
    /// then the picture has been dropped, and a write to a slow stick or a share would
    /// otherwise stall the viewer for no memory benefit.
    ///
    /// A photo that cannot be read, or has gone from the library since the grid was built,
    /// is counted and skipped rather than ending the export: one unreadable file must not
    /// cost the user the other hundred and nineteen. The first reason is reported so the
    /// message can say what went wrong rather than only that something did.
    pub fn export_items(&self, ids: &[i64], dest: &Path, apply_edits: bool) -> Result<Export> {
        let dest = self.check_export_dest(dest)?;
        let total = ids.len();
        let mut report = Export {
            written: 0,
            failed: 0,
            reason: None,
        };
        // Progress is throttled the way the scanner's is: a per-file event for a 5,000-photo
        // export is 5,000 round trips into the webview, all to move one bar.
        let mut last = Instant::now();
        for (n, &id) in ids.iter().enumerate() {
            let outcome = self.export_one(id, &dest, apply_edits);
            match outcome {
                Ok(()) => report.written += 1,
                Err(err) => {
                    report.failed += 1;
                    if report.reason.is_none() {
                        report.reason = Some(err.to_string());
                    }
                    tracing::warn!(%err, id, "could not export a photo");
                }
            }
            let done = n + 1;
            if done == total || last.elapsed() >= EXPORT_PROGRESS_EVERY {
                last = Instant::now();
                self.events.export_progress(ExportProgress {
                    done,
                    total,
                    failed: report.failed,
                });
            }
        }
        Ok(report)
    }

    /// Whether copies may be written into `dest`, and its canonical form if so.
    ///
    /// Its own method because the dialog asks *when the folder is picked*, while it is still
    /// open and the answer can be shown against the field: this is the only refusal the
    /// feature expects to produce routinely, and `export_items` closes the dialog before it
    /// starts. `export_items` checks again anyway - a folder can be watched in between, and
    /// the check is a directory walk of nothing.
    ///
    /// Inside a watched root, not merely overlapping one: a folder that *contains* a watched
    /// folder (exporting to the home folder while `~/Pictures` is watched) is a perfectly
    /// good destination, because nothing scans it.
    pub fn check_export_dest(&self, dest: &Path) -> Result<PathBuf> {
        let dest = paths::canonicalize(dest)?;
        for watched in self.lib.watched_folders()? {
            if paths::is_within(&dest, Path::new(&watched.path)) {
                return Err(Error::ExportIntoLibrary {
                    existing: watched.path,
                });
            }
        }
        Ok(dest)
    }

    fn export_one(&self, id: i64, dest: &Path, apply_edits: bool) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        let source = Source {
            path: PathBuf::from(&item.path),
            orientation: item.orientation,
            edit: item.edit,
        };
        if source.needs_render(apply_edits) {
            let (bytes, mime) = {
                let _one_at_a_time = crate::protocol::RENDERING.lock();
                export::render_for_export(&source)?
            };
            export::write_rendered(&source, dest, mime, &bytes)?;
        } else {
            export::copy_original(&source, dest)?;
        }
        Ok(())
    }

    /// Records the user's edit of one photo (`photon_core::edit`). Nothing is written to the
    /// photo: the edit lives in the library and is applied wherever the photo is drawn.
    ///
    /// The row goes back to `Pending` under a new thumbnail key, so it is queued at the
    /// front - the user is looking at it - and the grid is rebuilt, because every tile and
    /// the viewer name a thumbnail by that key. An edit identical to the one in place does
    /// neither.
    pub fn set_item_edit(self: &Arc<Self>, id: i64, edit: Edit) -> Result<()> {
        let _serialised = self.edit_write.lock();
        self.write_edit(id, edit)
    }

    /// `set_item_edit` for a caller already holding `edit_write`.
    ///
    /// The pass request is not about the edited row's own hash - that is cleared by the
    /// write and picked up whenever a pass next runs. It is about the row's former
    /// *partners*: `set_item_edit` clears one member out of a group without rewriting the
    /// rest, so a pair becomes a survivor with a stale `similar_group`. `DUPLICATE_FILTER`
    /// keeps the view honest meanwhile; this is what makes the grouping right again, and
    /// soon, because an edit is not a file change and so no scan follows it to run a pass
    /// of its own.
    fn write_edit(self: &Arc<Self>, id: i64, edit: Edit) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        // photon never decodes a video, so it cannot render one turned or cropped.
        if item.kind != MediaKind::Image {
            return Err(Error::NotAPhoto(id));
        }
        if self.lib.set_item_edit(id, edit)? {
            self.thumbs.prioritize(&[id], Priority::Visible);
            self.refresh_grid()?;
            self.request_similar_pass();
        }
        Ok(())
    }

    /// Turns one photo a quarter, on top of whatever edit it has; the crop goes round with
    /// the picture (`Edit::turned`). Serialised, because it reads the edit it builds on:
    /// commands run on a thread pool, and two quick presses of `R` that both read the same
    /// starting edit would come out as one turn. `set_item_edit` takes the same lock, or an
    /// "Original" landing between this read and this write would be turned back on.
    pub fn rotate_item(self: &Arc<Self>, id: i64, clockwise: bool) -> Result<()> {
        let _serialised = self.edit_write.lock();
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        self.write_edit(id, item.edit.turned(clockwise))
    }

    /// `NotFound` for an id that has been purged or marked missing, so a stale viewer gets
    /// the same answer here as it does from `set_star` — not because a tag edit shares
    /// `set_star`'s reason (writing a `.picasa.ini` that must not land on an unmounted
    /// drive; a tag edit is database-only), but because the two should behave alike from
    /// the caller's side regardless.
    fn live_item(&self, id: i64) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Validates and watches `path`, registers it with the running watcher service (if
    /// any), and starts its first scan.
    ///
    /// Without this, a folder added after the service starts would never install an OS
    /// watch: it would sit silently unwatched until the next full app restart.
    pub fn add_folder(self: &Arc<Self>, path: &Path) -> Result<WatchedFolder> {
        let watched = self.lib.add_watched_folder(path, &self.excluded)?;
        if let Some(service) = self.watcher_service() {
            service.watch_added(watched.id, Path::new(&watched.path));
        }
        self.start_scan(watched.clone());
        Ok(watched)
    }

    /// Stops any scan of the folder, forgets it and everything under it, unregisters its
    /// watch with the running watcher service (if any), and refreshes the grid.
    pub fn remove_folder(self: &Arc<Self>, watched_id: i64) -> Result<()> {
        // Set before the scan is cancelled, so nothing can start one in the window between
        // that and the deletion; cleared however this ends, so a failed removal does not
        // leave the folder unable to scan for the rest of the session.
        self.removing.lock().insert(watched_id);
        let result = self.remove_folder_inner(watched_id);
        self.removing.lock().remove(&watched_id);
        result
    }

    /// Ends with a look-alike pass for the same reason `write_edit` does: the cascade
    /// deletes every item under the root, which can leave a photo in *another* root as the
    /// last member of a group, and removing a folder starts no scan that would run one.
    fn remove_folder_inner(self: &Arc<Self>, watched_id: i64) -> Result<()> {
        self.cancel_scan(watched_id);
        let path = self
            .lib
            .watched_folders()?
            .into_iter()
            .find(|w| w.id == watched_id)
            .map(|w| w.path);
        self.lib.remove_watched_folder(watched_id)?;
        if let (Some(service), Some(path)) = (self.watcher_service(), path) {
            service.watch_removed(watched_id, Path::new(&path));
        }
        self.refresh_grid()?;
        self.request_similar_pass();
        Ok(())
    }

    /// A clone of the running watcher service's handle, if one is currently running.
    /// `pub(crate)` (rather than private) so tests in `watch.rs` can reach the exact
    /// service instance `add_folder`/`remove_folder`/`run_scan` use.
    pub(crate) fn watcher_service(&self) -> Option<Arc<WatcherService>> {
        self.watcher.lock().clone()
    }

    /// Starts the watcher service unless one is already running or the engine is shutting
    /// down. Held under `watcher`'s lock for the whole check-then-create so a `shutdown`
    /// racing this can never miss the service this call is about to store.
    pub fn start_watcher(self: &Arc<Self>) {
        let mut slot = self.watcher.lock();
        if slot.is_none() && !self.shutting_down.load(Ordering::SeqCst) {
            *slot = Some(Arc::new(WatcherService::start(self)));
        }
    }

    /// Stops the watcher service if one is running. Idempotent, and safe to call even if
    /// `start_watcher` never ran.
    ///
    /// `take`s the slot (and so releases `watcher`'s lock) before calling `stop`, which
    /// blocks joining the service's threads: holding the lock across that would block
    /// `add_folder`/`remove_folder`/`run_scan` for as long as `stop` takes.
    pub fn stop_watcher(&self) {
        let service = self.watcher.lock().take();
        if let Some(service) = service {
            service.stop();
        }
    }

    /// Emits the current `folder-status` for one folder without scanning it, so a state
    /// change no scan will report still reaches the UI promptly.
    ///
    /// `degraded` is passed in rather than read back from the running watcher service: the
    /// caller is usually that service, still being constructed inside `start_watcher` (which
    /// holds `watcher`'s lock), so asking the engine for it would deadlock.
    pub(crate) fn emit_folder_status(&self, watched_id: i64, degraded: bool) {
        let folder = self
            .lib
            .watched_folders()
            .ok()
            .and_then(|all| all.into_iter().find(|w| w.id == watched_id));
        if let Some(folder) = folder {
            self.emit_status(&folder, degraded);
        }
    }

    fn emit_status(&self, folder: &WatchedFolder, degraded: bool) {
        self.events.folder_status(FolderStatus {
            watched_id: folder.id,
            online: folder.online,
            degraded,
        });
    }

    /// Starts a full background scan unless one is already running for this folder.
    pub fn start_scan(self: &Arc<Self>, watched: WatchedFolder) -> bool {
        self.start_scan_inner(watched, None)
    }

    /// Starts a scan of one directory beneath `watched`. It takes the same per-folder slot
    /// as a full scan, so a folder never has two scans running, and the watcher's work is
    /// cancelled by `remove_folder` and `shutdown` exactly like a manual rescan.
    pub fn start_subtree_scan(self: &Arc<Self>, watched: WatchedFolder, dir: PathBuf) -> bool {
        self.start_scan_inner(watched, Some(dir))
    }

    /// Starts a background scan unless one is already running for this folder, or the
    /// engine is shutting down.
    fn start_scan_inner(
        self: &Arc<Self>,
        watched: WatchedFolder,
        subtree: Option<PathBuf>,
    ) -> bool {
        let mut scans = self.scans.lock();
        if self.shutting_down.load(Ordering::SeqCst)
            || self.removing.lock().contains(&watched.id)
            || scans.contains_key(&watched.id)
        {
            return false;
        }
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let engine = Arc::clone(self);
        let thread_cancel = cancel.clone();
        let id = watched.id;
        // Removes this scan's entry from `scans`, but only if it's still the same scan
        // (matched by `token`), and only once the thread is actually finished: this runs
        // on normal return *and* on panic, so a panicking scan can never strand its entry
        // and leave `is_scanning`/`wait_for_scans` stuck forever.
        struct RemoveOnDrop {
            engine: Arc<Engine>,
            id: i64,
            token: u64,
        }
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let mut scans = self.engine.scans.lock();
                if scans.get(&self.id).is_some_and(|r| r.token == self.token) {
                    scans.remove(&self.id);
                }
            }
        }
        let handle = std::thread::Builder::new()
            .name(format!("photon-scan-{id}"))
            .spawn(move || {
                let _remove_on_drop = RemoveOnDrop {
                    engine: engine.clone(),
                    id,
                    token,
                };
                engine.run_scan(&watched, subtree, thread_cancel);
            })
            .expect("failed to spawn scan thread");
        scans.insert(
            id,
            RunningScan {
                token,
                cancel,
                handle: Some(handle),
            },
        );
        true
    }

    /// Cancels the folder's scan, if any, and blocks until it has actually stopped.
    ///
    /// The entry stays in `scans` (so `start_scan`, `is_scanning` and `wait_for_scans` all
    /// keep seeing it as running) until the scan thread itself removes it, right after
    /// `run_scan` returns or panics.
    ///
    /// `cancel_scan` always means "cancelled and stopped" to every caller, however many
    /// call it concurrently for the same id: whichever call gets the join handle first
    /// joins it directly, and every other concurrent call instead polls (never holding
    /// `scans` while it sleeps) until the entry is gone. Without this, a second caller
    /// (e.g. `remove_folder` racing `shutdown`, or two `remove_folder` calls) would return
    /// while the scan is still writing, letting its next `refresh_grid` put back rows from
    /// a folder that has since been deleted.
    pub fn cancel_scan(&self, watched_id: i64) {
        let handle = {
            let mut scans = self.scans.lock();
            match scans.get_mut(&watched_id) {
                Some(running) => {
                    running.cancel.store(true, Ordering::Relaxed);
                    running.handle.take()
                }
                None => return,
            }
        };
        match handle {
            Some(handle) => {
                let _ = handle.join();
            }
            None => {
                while self.scans.lock().contains_key(&watched_id) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }

    pub fn is_scanning(&self, watched_id: i64) -> bool {
        self.scans.lock().contains_key(&watched_id)
    }

    /// Blocks until no scan is running.
    pub fn wait_for_scans(&self) {
        while !self.scans.lock().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Test-only: occupies a folder's scan slot without actually running a scan, so a test
    /// that needs "this folder has a scan in progress" gets that deterministically instead
    /// of racing a real one to completion. The fixtures used in these tests are one or two
    /// tiny JPEGs, so a real scan started via `start_scan` can finish before the next line
    /// of the test runs - especially on a fast CI runner - at which point the slot is
    /// already free and the behaviour under test never happens. Since this occupies the
    /// slot without spawning anything, there is nothing that can finish early.
    ///
    /// Mirrors `start_scan_inner`'s bookkeeping - same `scans` map, a freshly issued token -
    /// but spawns no thread, so the entry's `handle` is `None`. `wait_for_scans` only polls
    /// the map for emptiness and never joins a handle, so that's safe by itself; what isn't
    /// safe is holding the returned guard across a call to `wait_for_scans`, since nothing
    /// will ever remove the entry and the wait would spin forever. Callers must drop the
    /// guard first.
    #[cfg(test)]
    pub(crate) fn occupy_scan_slot_for_test(self: &Arc<Self>, watched_id: i64) -> TestScanSlot {
        let mut scans = self.scans.lock();
        assert!(
            !scans.contains_key(&watched_id),
            "occupy_scan_slot_for_test: a scan is already running for {watched_id}"
        );
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        scans.insert(
            watched_id,
            RunningScan {
                token,
                cancel: Arc::new(AtomicBool::new(false)),
                handle: None,
            },
        );
        TestScanSlot {
            engine: Arc::clone(self),
            id: watched_id,
            token,
        }
    }

    /// Background start-up work: watch `pictures` if the library is empty, queue pending
    /// thumbnails, rescan every folder, then collect thumbnail garbage.
    ///
    /// Checks `shutting_down` before each step, and before garbage collection, so a
    /// `shutdown` racing start-up stops it promptly instead of letting it run to
    /// completion.
    pub fn startup(self: &Arc<Self>, pictures: Option<PathBuf>) {
        let engine = Arc::clone(self);
        let handle = std::thread::Builder::new()
            .name("photon-startup".into())
            .spawn(move || {
                let shutting_down = || engine.shutting_down.load(Ordering::SeqCst);
                if shutting_down() {
                    return;
                }
                match engine.lib.watched_folders() {
                    Ok(watched) if watched.is_empty() => {
                        if let Some(pictures) = pictures.filter(|p| p.is_dir())
                            && let Err(err) =
                                engine.lib.add_watched_folder(&pictures, &engine.excluded)
                        {
                            tracing::warn!(%err, "could not watch the Pictures folder");
                        }
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "could not list watched folders"),
                }
                if shutting_down() {
                    return;
                }
                if let Err(err) = engine.thumbs.enqueue_pending() {
                    tracing::warn!(%err, "could not queue pending thumbnails");
                }
                for watched in engine.lib.watched_folders().unwrap_or_default() {
                    if shutting_down() {
                        break;
                    }
                    engine.start_scan(watched);
                }
                engine.wait_for_scans();
                if shutting_down() {
                    return;
                }
                // Every folder's first scan has now run, so live watching won't race a
                // startup scan for the same directory.
                engine.start_watcher();
                engine.collect_thumb_garbage_if_due();
            })
            .expect("failed to spawn startup thread");
        *self.startup.lock() = Some(handle);
    }

    /// Walks the thumbnail cache for files with no item, but only when a write since the
    /// last walk could have produced one, or the last walk is older than
    /// [`THUMB_GC_MAX_AGE`]. The walk visits two files per photo and used to run on every
    /// launch; on the usual launch, where nothing was purged or replaced, it found nothing.
    ///
    /// After the startup scans, deliberately: a scan that purged something bumps the epoch
    /// first, so its garbage is collected on this launch rather than the next.
    fn collect_thumb_garbage_if_due(&self) {
        let epoch = match self.lib.thumb_gc_due(now_ms(), THUMB_GC_MAX_AGE) {
            Ok(Some(epoch)) => epoch,
            Ok(None) => {
                tracing::debug!("thumbnail cache is clean; skipping the walk");
                return;
            }
            Err(err) => {
                tracing::warn!(%err, "could not tell whether thumbnail garbage collection is due");
                return;
            }
        };
        match self.thumbs.collect_garbage() {
            Ok(removed) => {
                tracing::info!(removed, "thumbnail garbage collected");
                if let Err(err) = self.lib.thumb_gc_done(epoch, now_ms()) {
                    tracing::warn!(%err, "could not record the thumbnail garbage collection");
                }
            }
            Err(err) => tracing::warn!(%err, "thumbnail garbage collection failed"),
        }
    }

    /// Blocks until the thread spawned by `startup` has finished, if it hasn't already.
    /// Safe to call more than once (and safe to call when `startup` was never called).
    pub fn wait_for_startup(&self) {
        let handle = self.startup.lock().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// Requests a look-alike regroup at whatever distance is stored right now, on its own
    /// thread. `set_similar_distance` calls this after writing the setting: the
    /// `groups_changed` signal `hash_after_scan` already reports gets a regroup to the
    /// grid, but nothing runs a pass on its own between scans, so without this a distance
    /// change would sit unseen until some unrelated scan happened to hash something.
    ///
    /// Spawned rather than run inline because the caller is the IPC dispatcher, which must
    /// return immediately - a whole-library regroup blocking it would stall every other
    /// command. The spawned call goes through `hash_after_scan`'s own
    /// `hash_requested`/`hashing` machinery, so a request arriving while a pass is already
    /// running coalesces with it instead of doubling the work.
    ///
    /// Cancelled by `shutting_down` rather than a token of its own: a distance change has
    /// no scan to inherit a cancel from, and tying it to shutdown stops the walk on quit
    /// instead of grinding through a change nobody is left to see.
    ///
    /// Checked and refused here, not left to the cancel flag alone: `shutdown`'s bounded
    /// wait on `hashing` (below) runs once, and an IPC call landing just after it - already
    /// shutting down, but not yet exited - would otherwise spawn a fresh writer that wait
    /// never accounted for.
    pub fn request_similar_pass(self: &Arc<Self>) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let engine = Arc::clone(self);
        let handle = std::thread::Builder::new()
            .name("photon-similar-pass".into())
            .spawn(move || engine.hash_after_scan(&engine.shutting_down))
            .expect("failed to spawn similar-pass thread");
        *self.similar_pass.lock() = Some(handle);
    }

    /// Joins the thread spawned by the most recent `request_similar_pass` call, if one is
    /// outstanding. Safe to call when none was ever spawned.
    ///
    /// This does **not** by itself prove no look-alike pass is still running.
    /// `hash_after_scan` returns at once when `hashing` is already held elsewhere, so the
    /// handle stored here can be a thread that did nothing while a different, unrecorded
    /// thread does the actual work: two requests close together are exactly that case - the
    /// first thread is still inside `similar::update` when the second overwrites
    /// `similar_pass` with its own thread, which finds `hashing` held and returns at once.
    /// Joining *that* handle finishes instantly and proves nothing about the first.
    /// `shutdown` therefore does not rely on this call for correctness; it waits on
    /// `hashing` itself afterwards, which identifies whichever thread is actually running
    /// regardless of which request (or scan) started it. This call exists for the case
    /// that does discriminate on it: a single request with nothing racing it, where the
    /// spawned thread is necessarily the one that does the work.
    pub fn wait_for_similar_pass(&self) {
        let handle = self.similar_pass.lock().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// Disarms the thumbnail crash-loop guard first, then stops new scans from starting,
    /// cancels every running scan (looping until none are left, since a scan or `startup`
    /// racing this can still insert one after the first pass), stops the watcher, closes the
    /// thumbnail queue so its workers finish their current job and stop, waits for the
    /// startup thread to finish (it checks `shutting_down` at each of its own checkpoints, so
    /// this doesn't wait for it to run to completion), then waits for any look-alike pass.
    pub fn shutdown(&self) {
        // Disarmed first, before anything below that can itself stall - `stop_watcher` has.
        // A kill during that stretch is a deliberate quit already under way, not a crash;
        // waiting until `thumbs.close()` (which disarms again - harmless, see its own doc)
        // would leave every photo still in flight to be misread as having taken photon down
        // with it. This still has to run *before* `stop_watcher`, not instead of the later
        // `close()`: the queue itself must not stop taking jobs until after `stop_watcher`,
        // so a watcher-driven scan racing this can't enqueue into a pool already gone.
        self.thumbs.disarm();
        self.shutting_down.store(true, Ordering::SeqCst);
        loop {
            let ids: Vec<i64> = self.scans.lock().keys().copied().collect();
            if ids.is_empty() {
                break;
            }
            for id in ids {
                self.cancel_scan(id);
            }
        }
        // Before the thumbnail queue closes, so no watcher-driven scan can start (and try
        // to enqueue thumbnails) as the workers go away.
        self.stop_watcher();
        self.thumbs.close();
        self.wait_for_startup();
        self.stop_similar_pass(SIMILAR_PASS_STOP_TIMEOUT);
    }

    /// Waits, within `budget` in total, for every look-alike pass to have stopped.
    ///
    /// Two waits, one deadline between them, because either alone is incomplete.
    ///
    /// Joining the recorded handle closes a sliver nothing else does: a thread already
    /// spawned but not yet at its own `try_lock` holds nothing, so the wait on `hashing`
    /// below would sail past it and it would go on to regroup after `shutdown` returned.
    /// But the handle need not be the thread doing the work - per `wait_for_similar_pass`'s
    /// doc, a scan's inline `hash_after_scan` never records one, and a request that lost
    /// the race for `hashing` records a thread that did nothing - so taking and dropping
    /// `hashing` itself is what establishes "no pass is running", whichever thread got
    /// there.
    ///
    /// **Both are bounded, against one deadline.** An unbounded join here would defeat the
    /// bound below in the single-request case, which is the common one: there the recorded
    /// handle *is* the thread inside `similar::update`, so joining it waits for exactly the
    /// thread the timeout exists to give up on. `hash_candidates` reads the original photo
    /// files, so a root on a dead network mount would hang the quit forever - the scenario
    /// `SIMILAR_PASS_STOP_TIMEOUT` was introduced for - and the bound would have been real
    /// only in the interleaving where the recorded handle is a no-op thread. Sharing one
    /// deadline keeps the total within the stated bound rather than twice it.
    ///
    /// `budget` is a parameter rather than the constant read directly so a test can drive
    /// this with a short one; `shutdown` is its only caller.
    fn stop_similar_pass(&self, budget: Duration) {
        let deadline = Instant::now() + budget;
        let handle = self.similar_pass.lock().take();
        if let Some(handle) = handle
            && join_within(
                vec![handle],
                deadline.saturating_duration_since(Instant::now()),
            ) > 0
        {
            tracing::warn!("a requested look-alike pass did not stop in time; detaching it");
        }
        match self.hashing.try_lock_until(deadline) {
            Some(guard) => drop(guard),
            None => tracing::warn!(
                "a look-alike pass did not stop within {budget:?}; \
                 leaving it to finish on its own"
            ),
        }
    }

    fn run_scan(&self, watched: &WatchedFolder, subtree: Option<PathBuf>, cancel: Arc<AtomicBool>) {
        let options = ScanOptions {
            excluded: self.excluded.clone(),
            cancel: cancel.clone(),
        };
        let mut sink = ScanReporter {
            engine: self,
            watched_id: watched.id,
            last: ScanProgress::default(),
            last_refresh: Instant::now(),
            refreshed_total: 0,
            last_progress: None,
        };
        let result = match &subtree {
            Some(dir) => scan_subtree(&self.lib, watched, dir, now_ms(), &options, &mut sink),
            None => scan_watched(&self.lib, watched, now_ms(), &options, &mut sink),
        };
        let last = sink.last;
        let cancelled = match &result {
            Ok(report) => report.cancelled,
            Err(err) => {
                tracing::warn!(watched_id = watched.id, %err, "scan failed");
                false
            }
        };
        let folder = self
            .lib
            .watched_folders()
            .ok()
            .and_then(|all| all.into_iter().find(|w| w.id == watched.id));
        // A scan that changed nothing must not rebuild the grid: `refresh_grid` reads every
        // grid row, rebuilds the whole index and makes the UI refetch. An offline root is
        // rescanned every 30 seconds for as long as its drive stays unplugged, and each of
        // those scans finds nothing — doing the full rebuild anyway would burn a table scan
        // and a UI refresh twice a minute, indefinitely, on a library of any size.
        //
        // The online flag flipping counts as a change even when no row was touched: it
        // decides which folders the thumbnail queue will work on, so the queue has to be
        // re-primed when a drive comes back.
        let touched_rows = match &result {
            // Which counters mean "rows moved" is `ScanReport::touched_rows`' business, not
            // this crate's: spelled out here, a counter added to the report and forgotten
            // here compiled fine and silently stopped the grid rebuilding, which is what
            // `restarred` did once.
            Ok(report) => report.touched_rows(),
            // A scan that failed partway may still have committed earlier batches.
            Err(_) => true,
        };
        let online_changed = folder.as_ref().is_some_and(|f| f.online != watched.online);
        if (touched_rows || online_changed)
            && let Err(err) = self.refresh_grid()
        {
            tracing::warn!(%err, "grid refresh failed");
        }
        // Outside the guard, and the one full sweep a scan makes. New and replaced items
        // were queued as they were indexed (`ScanReporter::indexed`); this catches what
        // that cannot: an item whose render failed transiently and sits `Pending` with
        // nothing else to retry it, and a drive that came back online, whose items the
        // sweep skipped while it was away. Leaving it inside the guard meant such an item
        // waited for an unrelated change, or a restart.
        if let Err(err) = self.thumbs.enqueue_pending() {
            tracing::warn!(%err, "could not queue pending thumbnails");
        }
        if let Some(folder) = folder {
            let degraded = self
                .watcher_service()
                .is_some_and(|service| service.is_degraded(folder.id));
            self.emit_status(&folder, degraded);
        }
        self.events
            .scan_progress(ScanProgressEvent::new(watched.id, &last, true, cancelled));
        // After the scan has reported done, not before: the pass reads files, and on a
        // library with many duplicates on a slow drive that is minutes during which the
        // status bar should not claim the folder is still being scanned.
        if !cancelled {
            self.hash_after_scan(&cancel);
        }
    }

    /// Runs the two passes the Duplicates view is built from - the byte-identical one
    /// (`photon_core::duplicates`, which hashes files) and the look-alike one
    /// (`photon_core::similar`, which hashes cached thumbnails and regroups) - and rebuilds
    /// the grid if either changed what the view shows.
    ///
    /// The two passes are gated differently, and that asymmetry is load-bearing, not an
    /// oversight to tidy up. `hash_candidates` (the byte-identical half) hashes rows whose
    /// `content_hash` is read straight into `DUPLICATE_FILTER` at query time, so hashing a
    /// row can by itself change who has a twin - `Ok(_)` there refreshes unconditionally.
    /// `photon_core::similar::update`'s hash half writes `percep_hash`, which appears in no
    /// view filter and no `GridInfo` field; membership in Duplicates comes only from the
    /// *materialised* `similar_group` the regroup half writes. So `pass.hashed > 0` can
    /// never itself change what any grid shows, and gating on it only rebuilt the grid for
    /// no reason - which is what a rescan that hashes a newly-thumbnailed, unstarred photo
    /// did, moving the version and failing
    /// `a_rescan_after_set_star_agrees_with_what_photon_wrote`. `pass.groups_changed` is
    /// the one signal that means the view moved: a regroup that hashes nothing still moves
    /// photos into and out of the view (changing the distance setting is exactly that), so
    /// the refresh is gated on `groups_changed` alone.
    ///
    /// Here rather than in the scanner because a duplicate is a fact about the whole
    /// library, not about the root or subtree one scan walked - and because `walk_tree` has
    /// two callers, which a pass wired into the scanner has to remember and this does not.
    /// It runs after *every* scan, changed rows or not: the first scan after the upgrade
    /// that added the column touches nothing and still has the whole library to hash. With
    /// nothing to do it is one indexed query.
    ///
    /// One guard covers both passes, in order: a photo is a look-alike candidate only once
    /// its thumbnail exists, and nothing in the duplicate pass changes that, so the order is
    /// only about keeping the cheap whole-library regroup last.
    ///
    /// One pass at a time. Several roots finish their startup scans close together, and two
    /// passes would read the same files twice. A scan that finds the pass running sets
    /// `hash_requested` and leaves; the runner goes round again while the flag is set, so
    /// files indexed after its candidate list was read are not left for the next launch.
    /// The re-check after the guard is dropped closes the window where the flag is set
    /// after the runner's last look and before it lets go.
    ///
    /// `cancel` is the calling scan's. Cancelling it (its folder is being removed, or photon
    /// is shutting down) stops the pass even though the files may belong to other roots;
    /// the next scan of anything picks the work up.
    fn hash_after_scan(&self, cancel: &AtomicBool) {
        self.hash_requested.store(true, Ordering::Release);
        loop {
            let Some(mut guard) = self.hashing.try_lock() else {
                return;
            };
            while self.hash_requested.swap(false, Ordering::AcqRel) {
                match photon_core::duplicates::hash_candidates(&self.lib, cancel) {
                    Ok(0) => {}
                    Ok(_) => {
                        if let Err(err) = self.refresh_grid() {
                            tracing::warn!(%err, "grid refresh failed");
                        }
                    }
                    Err(err) => tracing::warn!(%err, "duplicate hashing failed"),
                }
                let distance = match self.lib.similar_distance() {
                    Ok(distance) => distance as u32,
                    Err(err) => {
                        tracing::warn!(%err, "could not read the look-alike distance setting");
                        photon_core::similar::EXACT_RECALL_DISTANCE
                    }
                };
                match photon_core::similar::update(
                    &self.lib,
                    &self.cache,
                    distance,
                    cancel,
                    &mut guard,
                ) {
                    Ok(pass) if pass.groups_changed => {
                        if let Err(err) = self.refresh_grid() {
                            tracing::warn!(%err, "grid refresh failed");
                        }
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "look-alike hashing failed"),
                }
            }
            drop(guard);
            if !self.hash_requested.load(Ordering::Acquire) {
                return;
            }
        }
    }
}

/// What one running scan reports back into the engine: the grid rebuilds and progress
/// events, throttled to [`THROTTLE`], and the ids of freshly indexed items for the
/// thumbnail queue.
struct ScanReporter<'a> {
    engine: &'a Engine,
    watched_id: i64,
    last: ScanProgress,
    last_refresh: Instant,
    refreshed_total: u64,
    last_progress: Option<Instant>,
}

impl ScanSink for ScanReporter<'_> {
    fn progress(&mut self, p: &ScanProgress) {
        self.last = *p;
        let total = p.added + p.changed;
        if total != self.refreshed_total && self.last_refresh.elapsed() >= THROTTLE {
            if let Err(err) = self.engine.refresh_grid() {
                tracing::warn!(%err, "grid refresh failed");
            }
            self.last_refresh = Instant::now();
            self.refreshed_total = total;
        }
        if self.last_progress.is_none_or(|t| t.elapsed() >= THROTTLE) {
            self.engine.events.scan_progress(ScanProgressEvent::new(
                self.watched_id,
                p,
                false,
                false,
            ));
            self.last_progress = Some(Instant::now());
        }
    }

    /// Straight onto the queue, in the order the scanner found them. This used to be a
    /// full `enqueue_pending` on every throttled tick above: a grid-order sort of every
    /// pending row, every 250ms, for the whole of an import - and, with the grid rebuild
    /// beside it, most of each tick spent inside the database. The end-of-scan sweep in
    /// `run_scan` still runs once, for the rows a push cannot know about.
    fn indexed(&mut self, ids: &[i64]) {
        self.engine.thumbs.prioritize(ids, Priority::Background);
    }
}

/// RAII guard returned by `occupy_scan_slot_for_test`. Releases the slot on drop, mirroring
/// `start_scan_inner`'s own `RemoveOnDrop`: it removes the entry only if the token still
/// matches, so a guard dropped late (e.g. after a test panics and unwinds past it) can never
/// evict a real, unrelated scan that later claimed the same folder id.
#[cfg(test)]
pub(crate) struct TestScanSlot {
    engine: Arc<Engine>,
    id: i64,
    token: u64,
}

#[cfg(test)]
impl Drop for TestScanSlot {
    fn drop(&mut self) {
        let mut scans = self.engine.scans.lock();
        if scans.get(&self.id).is_some_and(|r| r.token == self.token) {
            scans.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Recorded;
    use crate::testutil::{fixture, jpeg, jpeg_pattern};
    use photon_core::media::ThumbState;

    /// An edit travels the whole refresh chain: the row, a new grid version, and a tile
    /// that names a different thumbnail. Two turns are two turns - `rotate_item` reads the
    /// edit it builds on, so it has to be serialised against itself.
    #[test]
    fn rotating_a_photo_rebuilds_the_grid_under_a_new_thumbnail_key() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let (version, grid) = f.engine.grid();
        let before = grid.rows(0, 1)[0];

        f.engine.rotate_item(id, true).unwrap();
        f.engine.rotate_item(id, true).unwrap();

        assert_eq!(f.engine.lib.item(id).unwrap().unwrap().edit.turns, 2);
        let (after_version, grid) = f.engine.grid();
        assert!(after_version > version);
        assert_ne!(grid.rows(0, 1)[0].thumb_key, before.thumb_key);

        // The same edit again is not a change: no rebuild, no UI refetch.
        let edit = f.engine.lib.item(id).unwrap().unwrap().edit;
        f.engine.set_item_edit(id, edit).unwrap();
        assert_eq!(f.engine.grid().0, after_version);

        assert!(matches!(
            f.engine.rotate_item(9_999, true),
            Err(Error::NotFound(9_999))
        ));
    }

    /// The hashing pass is wired into the end of a scan, and its result reaches everything
    /// built on it: the count in `grid_info`, the Duplicates view, and a photo's copies.
    /// `wait_for_scans` covers the pass, since it runs on the scan's thread. Without the
    /// call in `run_scan` nothing is ever hashed and every assertion below reads zero.
    #[test]
    fn a_scan_finds_the_duplicates_it_indexed() {
        let same = jpeg(4, 2);
        let mut padded = same.clone();
        padded.extend_from_slice(b"a size of its own");
        let f = fixture(&[
            ("a/one.jpg", &same),
            ("b/two.jpg", &same),
            ("b/three.jpg", &padded),
        ]);
        f.add_photos();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.duplicate_count, 2);
        assert_eq!(info.len, 3, "the All view is untouched");

        f.engine.set_view(GridView::Duplicates).unwrap();
        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        let item = crate::commands::viewer_item(&f.engine, ids[0]).unwrap();
        assert_eq!(item.copies.len(), 1);
        assert_eq!(item.copies[0].id, ids[1]);
    }

    /// Videos are in content hashing (spec, *Duplicates*): a byte-identical copy of a video
    /// is a real duplicate, found by the same end-of-scan pass as a photo's, even though the
    /// look-alike pass leaves videos out. The third file shares the pair's size but not its
    /// bytes, so the pass has to hash rather than trust the size.
    #[test]
    fn a_scan_finds_a_byte_identical_pair_of_videos() {
        let f = fixture(&[
            ("a/clip.mp4", b"the same video bytes"),
            ("b/clip copy.mp4", b"the same video bytes"),
            ("b/other.mp4", b"other video bytes!!!"),
        ]);
        f.add_photos();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.duplicate_count, 2);
        f.engine.set_view(GridView::Duplicates).unwrap();
        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        for &id in &ids {
            assert_eq!(
                f.engine.lib.item(id).unwrap().unwrap().kind,
                MediaKind::Video
            );
        }
        let item = crate::commands::viewer_item(&f.engine, ids[0]).unwrap();
        assert_eq!(item.copies.len(), 1);
        assert_eq!(item.copies[0].id, ids[1]);
    }

    /// The look-alike pass is wired into the same end-of-scan guard, and reaches the same
    /// places: two files that are one picture at two sizes share no byte and no size, so
    /// only the look-alike pass can put them in the Duplicates view.
    ///
    /// The two flat photos are the case `pairs_with_anything` exists for: they are distance
    /// 0 from each other and from every other blank frame, and must be in no group at all.
    /// They are different sizes, so nothing but the look-alike pass could pair them.
    ///
    /// The second scan is what makes this deterministic rather than a race: the pass hashes
    /// cached thumbnails, and the first scan's pass runs while the thumbnail workers are
    /// still going. By the time `wait_idle` returns every thumbnail is on disk, and the
    /// scan after it has all four to hash. **Any future test that asserts on
    /// `duplicate_count` with structured fixtures needs the same `wait_idle` and second
    /// scan**: the race is not fixed, only invisible to fixtures whose flat colours are
    /// never grouped whenever the pass happens to run.
    #[test]
    fn a_scan_finds_the_look_alikes_it_indexed() {
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
            ("a/blank.jpg", &jpeg(60, 40)),
            ("a/blanker.jpg", &jpeg(30, 20)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(
            info.duplicate_count, 2,
            "the resized copy is not a look-alike, or the blank frames were grouped"
        );

        f.engine.set_view(GridView::Duplicates).unwrap();
        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        let paths: Vec<String> = ids
            .iter()
            .map(|id| f.engine.lib.item(*id).unwrap().unwrap().path)
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("big.jpg")));
        assert!(paths.iter().any(|p| p.ends_with("small.jpg")));
    }

    /// A regroup that hashes nothing still moves photos into and out of the Duplicates
    /// view, so it has to rebuild the grid too. Gated on rows hashed, as it was first
    /// written, the view stays as it was until some unrelated scan happens to hash a row -
    /// and changing the distance setting (Task 6) is precisely a regroup that hashes
    /// nothing, so the user would change it and see the view not move.
    ///
    /// Staged rather than scanned, because the hashing is what has to be seen *not* to
    /// happen: the groups are cleared behind the engine and the grid refreshed to match, so
    /// the view genuinely holds nothing before the pass restores it.
    #[test]
    fn a_regroup_that_hashes_nothing_still_rebuilds_the_grid() {
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        f.engine.set_view(GridView::Duplicates).unwrap();
        assert_eq!(f.ids().len(), 2, "the pair was not grouped to begin with");

        f.engine.lib.set_similar_groups(&[]).unwrap();
        f.engine.refresh_grid().unwrap();
        assert_eq!(f.ids().len(), 0);

        f.engine.hash_after_scan(&AtomicBool::new(false));

        assert_eq!(
            f.ids().len(),
            2,
            "the regroup restored the groups but left the grid showing the old ones"
        );
    }

    /// Nothing but `request_similar_pass` runs a pass between scans, so a distance change
    /// on its own would sit unseen until an unrelated scan happened by. `set_similar_distance`
    /// (`commands.rs`) calls it after writing the setting; this drives that same path (not
    /// `hash_after_scan` directly) and waits for the pass with `wait_for_similar_pass`,
    /// which is safe here because nothing else is requesting a pass concurrently - see that
    /// method's own doc for the case where it would not be.
    ///
    /// Distance 0 is "off": a resized copy is never pixel-identical to its original, so at
    /// distance 0 the pair that groups at the default distance 3 must not.
    #[test]
    fn changing_the_similar_distance_requests_its_own_regroup() {
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        f.engine.set_view(GridView::Duplicates).unwrap();
        assert_eq!(
            f.ids().len(),
            2,
            "grouped at the default (conservative) distance"
        );

        crate::commands::set_similar_distance(&f.engine, 0).unwrap();
        f.engine.wait_for_similar_pass();

        assert_eq!(
            f.ids().len(),
            0,
            "the setting change alone should have taken the pair out of the view"
        );
    }

    /// A photo's stored `similar_group`, or `None`. The group column is what a pass leaves
    /// behind, so it is what a test about *requesting* a pass has to read: the Duplicates
    /// view itself is kept honest by `DUPLICATE_FILTER` whether or not a pass ever runs.
    fn stored_group(f: &crate::testutil::Fixture, id: i64) -> Option<i64> {
        f.engine
            .lib
            .similar_groups()
            .unwrap()
            .into_iter()
            .find(|(item, _)| *item == id)
            .map(|(_, group)| group)
    }

    /// An edit takes one photo out of its group, and nothing else. No file changed, so no
    /// scan follows and no pass would otherwise run: the partner keeps a `similar_group`
    /// naming a group it is now alone in, all session. `write_edit` requests a pass for
    /// that reason, not for the edited row's own hash.
    ///
    /// Rotating is what makes the assertion stable: `dhash` is deliberately not
    /// rotation-invariant, so even once the turned photo's thumbnail is re-rendered and
    /// hashed, the two do not group again.
    #[test]
    fn an_edit_requests_a_pass_so_the_partner_loses_its_stale_group() {
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        assert!(
            stored_group(&f, ids[0]).is_some(),
            "not grouped to begin with"
        );

        f.engine.rotate_item(ids[0], true).unwrap();
        f.engine.wait_for_similar_pass();

        assert_eq!(
            stored_group(&f, ids[1]),
            None,
            "the unedited partner kept a group it is the only member of"
        );
    }

    /// The same hole by the other route, and the one that survives a restart least
    /// gracefully: removing a root cascade-deletes its items, which can leave a photo in
    /// *another* root as its group's last member. Two roots are the point - removing the
    /// only root leaves no row to be wrong about.
    #[test]
    fn removing_a_folder_requests_a_pass_for_the_photos_left_in_other_roots() {
        let f = fixture(&[("a/big.jpg", &jpeg_pattern(180, 120))]);
        let other = f.dir.path().join("more-photos");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("small.jpg"), jpeg_pattern(72, 48)).unwrap();
        let first = f.add_photos();
        let second = f.engine.add_folder(&other).unwrap();
        f.engine.wait_for_scans();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(first);
        f.engine.wait_for_scans();

        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        let survivors: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| {
                !f.engine
                    .lib
                    .item(*id)
                    .unwrap()
                    .unwrap()
                    .path
                    .ends_with("small.jpg")
            })
            .collect();
        assert_eq!(survivors.len(), 1);
        assert!(
            stored_group(&f, survivors[0]).is_some(),
            "not grouped to begin with"
        );

        f.engine.remove_folder(second.id).unwrap();
        f.engine.wait_for_similar_pass();

        assert_eq!(
            stored_group(&f, survivors[0]),
            None,
            "the photo in the surviving root kept a group whose other member is gone"
        );
    }

    /// `set_similar_distance` must return without waiting for the regroup: it runs on the
    /// IPC dispatcher, and a whole-library pass blocking it would stall every other
    /// command. Holding `hashing` here from the test thread makes that provable rather than
    /// timed: the spawned pass thread can never acquire it and so can never finish, so if
    /// the command waited for the pass inline it would deadlock and this test would hang
    /// until the harness times it out, instead of returning.
    #[test]
    fn set_similar_distance_returns_without_waiting_for_the_pass() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();

        let held = f.engine.hashing.lock();
        let clamped = crate::commands::set_similar_distance(&f.engine, 10).unwrap();
        assert_eq!(clamped, 10, "the write itself still happens");
        drop(held);

        f.engine.wait_for_similar_pass();
    }

    /// The interleaving `wait_for_similar_pass` alone cannot cover: a thread (standing in
    /// for one already inside `similar::update`) holds `hashing`, then a second
    /// `request_similar_pass` call spawns a thread whose own `try_lock` fails at once and
    /// which is therefore the handle `similar_pass` stores and `wait_for_similar_pass`
    /// joins - instantly, having done nothing. If `shutdown` relied on that join alone it
    /// would return while the first thread is still "running" (here, still holding the
    /// lock); it must instead still be waiting on `hashing` itself.
    #[test]
    fn shutdown_waits_for_the_pass_actually_holding_hashing_not_just_the_latest_requested_thread() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();

        let held = f.engine.hashing.lock();
        f.engine.request_similar_pass();
        f.engine.wait_for_similar_pass(); // joins the no-op thread; proves nothing by itself

        let engine = Arc::clone(&f.engine);
        let shutdown = std::thread::spawn(move || engine.shutdown());
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !shutdown.is_finished(),
            "shutdown returned before the pass actually holding `hashing` had stopped"
        );

        drop(held);
        shutdown.join().unwrap();
    }

    /// The bound `shutdown` promises is over *both* its waits, and the join is the one that
    /// could quietly remove it. In the single-request case (a distance change, then a
    /// quit) the recorded handle is the thread running the pass, so an unbounded join would
    /// wait for exactly the thread `SIMILAR_PASS_STOP_TIMEOUT` exists to give up on, and an
    /// app whose photos are on a dead mount would never quit.
    ///
    /// Both halves are made to time out here: a recorded thread that will not finish until
    /// the test releases it, and `hashing` held by the test thread. With both bounded
    /// against one deadline the call returns after the budget; with the join unbounded it
    /// never returns at all, and with two separate budgets it would take twice as long.
    ///
    /// The margin between "once" and "twice" has to survive a loaded CI runner: at a 200ms
    /// budget with a 400ms bar, measured from outside the thread (its spawn and the 10ms
    /// polling below included), macOS runners crossed the bar with the code correct. The
    /// call is timed inside its own thread now, and the budget is a second, so the bar sits
    /// half a second clear of either answer.
    #[test]
    fn stopping_a_pass_is_bounded_across_both_of_its_waits() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();

        let release = Arc::new(AtomicBool::new(false));
        let stuck = Arc::clone(&release);
        *f.engine.similar_pass.lock() = Some(std::thread::spawn(move || {
            while !stuck.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }));
        let held = f.engine.hashing.lock();

        let engine = Arc::clone(&f.engine);
        let budget = Duration::from_secs(1);
        let started = Instant::now();
        let stopping = std::thread::spawn(move || {
            let call = Instant::now();
            engine.stop_similar_pass(budget);
            call.elapsed()
        });
        while !stopping.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "stop_similar_pass outran its budget of {budget:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let took = stopping.join().unwrap();
        assert!(
            took < budget * 3 / 2,
            "the two waits were budgeted separately, not against one deadline: {took:?}"
        );

        drop(held);
        release.store(true, Ordering::SeqCst);
    }

    /// An IPC call can land after `shutdown` has already set `shutting_down` and run its
    /// bounded wait on `hashing`, but before the process actually exits. Without this check
    /// that call would spawn a fresh writer `shutdown` never accounted for.
    #[test]
    fn request_similar_pass_is_a_no_op_once_shutting_down() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();
        f.engine.shutdown();

        f.engine.request_similar_pass();

        assert!(
            f.engine.similar_pass.lock().is_none(),
            "a request arriving after shutdown must not spawn a thread"
        );
    }

    /// Both readers of `excluded` compare it against a canonical path - `add_watched_folder`
    /// canonicalizes the root it checks, and the watcher canonicalizes every event directory
    /// before `plan_scans` tests it - so a raw config path with a symlink in it excludes
    /// nothing. Watching `$HOME` is allowed, so photon would then schedule a subtree scan of
    /// its own cache for every thumbnail it writes there.
    #[test]
    #[cfg(unix)]
    fn excluded_paths_are_canonical_so_they_match_what_the_watcher_reports() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("data")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let engine = Engine::open(
            EngineConfig {
                db_path: link.join("data").join("library.db"),
                cache_dir: link.join("cache").join("thumbs"),
                workers: 1,
            },
            Arc::new(crate::events::Recorder::default()),
        )
        .unwrap();

        let cache = photon_core::paths::canonicalize(real.join("cache").join("thumbs")).unwrap();
        assert!(
            engine.excluded().contains(&cache),
            "the cache the watcher will report events for is {cache:?}, but excluded holds \
             {:?}",
            engine.excluded()
        );
    }

    #[test]
    fn a_scan_that_changed_nothing_still_re_primes_pending_thumbnails() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.wait_for_scans();

        // Simulate a render that failed transiently: the item is pending again, but a
        // rescan finds nothing changed on disk.
        let id = f.ids()[0];
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Pending, None)
            .unwrap();

        assert!(f.engine.start_scan(watched.clone()));
        f.engine.wait_for_scans();

        // Checking the queue length right after `wait_for_scans` is racy in practice: the
        // cache for this item is already complete from the first scan, so re-processing it
        // is a single fast DB update that the lone worker thread routinely finishes before
        // this assertion runs (the same worker sits parked in a blocking pop, and
        // `wait_for_scans`'s own polling loop hands it ample time). `wait_idle` makes the
        // check deterministic: it blocks until the worker has drained the queue, so by the
        // time we look, retrying has either happened (state back to `Ready`) or the item was
        // never re-queued at all (state stuck at `Pending`).
        f.engine.thumbs.wait_idle();
        let item = f.engine.lib.item(id).unwrap().unwrap();
        assert_eq!(
            item.thumb_state,
            ThumbState::Ready,
            "a no-change scan must still queue pending thumbnails; otherwise a transient \
             render failure is never retried without restarting photon"
        );
    }

    #[test]
    fn add_folder_scans_and_publishes_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        let watched = f.add_photos();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 2);
        assert!(version >= 1);
        let events = f.events.all();
        assert!(events.contains(&Recorded::Library(LibraryChanged { version, len: 2 })));
        assert!(events.iter().any(|e| matches!(e,
            Recorded::Scan(s) if s.watched_id == watched.id && s.done && !s.cancelled && s.added == 2)));
        assert!(events.contains(&Recorded::Folder(FolderStatus {
            watched_id: watched.id,
            online: true,
            degraded: false,
        })));
        assert!(!f.engine.is_scanning(watched.id));
    }

    /// An offline root is rescanned every 30 seconds for as long as its drive stays
    /// unplugged, and each of those scans finds nothing. Refreshing anyway would rebuild the
    /// whole grid index from a full `grid_entries()` query and make the UI refetch, twice a
    /// minute, indefinitely.
    #[test]
    fn a_scan_that_changes_nothing_does_not_rebuild_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();

        // The first poll after the drive goes away flips the folder offline, which is a
        // real change and legitimately refreshes.
        std::fs::remove_dir_all(&f.photos).unwrap();
        f.engine.start_scan(watched.clone());
        f.engine.wait_for_scans();
        let version = f.engine.grid().0;

        // The second finds exactly what the first did: nothing added, changed, marked or
        // purged, and the folder already offline.
        let offline = f
            .engine
            .lib
            .watched_folders()
            .unwrap()
            .into_iter()
            .find(|w| w.id == watched.id)
            .unwrap();
        assert!(!offline.online);
        f.engine.start_scan(offline);
        f.engine.wait_for_scans();

        assert_eq!(
            f.engine.grid().0,
            version,
            "a scan that changed nothing must not rebuild the grid"
        );
    }

    /// THE regression test for the star-only-scan bug: starring a photo in Picasa never
    /// changes the photo itself, so the scan's `added`/`changed`/`marked_missing`/`purged`
    /// counters are all zero and only `restarred` moves. Before `ScanReport::restarred` was
    /// folded into `touched_rows`, a scan like this computed `touched_rows == false`, skipped
    /// `refresh_grid`, and left the grid (and so the Starred view and its sidebar count)
    /// stale until something unrelated changed a row or the app restarted.
    #[test]
    fn a_star_added_via_the_ini_after_the_first_scan_rebuilds_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        let version = f.engine.grid().0;

        std::fs::write(f.photos.join(".picasa.ini"), b"[a.jpg]\nstar=yes\n").unwrap();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();

        assert!(
            f.engine.grid().0 > version,
            "a star landing via the Picasa INI, with no photo file changing, must still \
             rebuild the grid"
        );
    }

    #[test]
    fn viewer_item_reports_whether_the_photo_is_starred() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        std::fs::write(f.photos.join(".picasa.ini"), b"[a.jpg]\nstar=yes\n").unwrap();
        f.add_photos();
        let ids = f.ids();
        assert!(
            crate::commands::viewer_item(&f.engine, ids[0])
                .unwrap()
                .starred
        );
        assert!(
            !crate::commands::viewer_item(&f.engine, ids[1])
                .unwrap()
                .starred
        );
    }

    #[test]
    fn viewer_item_refuses_a_missing_photo() {
        // The viewer probes this after the photo has left the grid, to tell "left the
        // current view" from "gone". A soft-deleted row answering `Ok` would keep a
        // vanished photo on screen with no message.
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        f.engine.lib.mark_missing(&[id], 1).unwrap();
        let err = crate::commands::viewer_item(&f.engine, id).unwrap_err();
        assert_eq!(err.kind, "notFound");
    }

    /// The Hidden view's viewer needs a hidden photo to answer, and so does the orphan
    /// check when the photo on screen is hidden: refusing it like a missing photo would say
    /// "no longer available" about a photo that is one click away.
    #[test]
    fn viewer_item_answers_for_a_hidden_photo() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        assert!(!crate::commands::viewer_item(&f.engine, id).unwrap().hidden);
        f.engine.set_items_hidden(&[id], true).unwrap();
        assert!(crate::commands::viewer_item(&f.engine, id).unwrap().hidden);
    }

    #[test]
    fn hiding_a_folder_rebuilds_the_grid_and_reports_its_flag() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let folder = f.engine.lib.folders().unwrap()[0].id;
        let before = crate::commands::grid_info(&f.engine);

        assert_eq!(f.engine.set_folder_hidden(folder, true).unwrap(), 2);
        let after = crate::commands::grid_info(&f.engine);
        assert_eq!((after.len, after.hidden_count), (0, 2));
        assert!(after.version > before.version);
        let listed = crate::commands::list_folders(&f.engine).unwrap();
        assert!(listed.folders.iter().any(|x| x.id == folder && x.hidden));

        assert_eq!(f.engine.set_folder_hidden(folder, true).unwrap(), 0);
        assert_eq!(
            crate::commands::grid_info(&f.engine).version,
            after.version,
            "hiding a hidden folder rebuilt the grid"
        );
    }

    #[test]
    fn hiding_rebuilds_the_grid_and_a_no_op_does_not() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let before = crate::commands::grid_info(&f.engine);
        assert_eq!((before.len, before.hidden_count), (2, 0));

        assert_eq!(f.engine.set_items_hidden(&[id], true).unwrap(), 1);
        let after = crate::commands::grid_info(&f.engine);
        assert_eq!((after.len, after.hidden_count), (1, 1));
        assert!(after.version > before.version);

        assert_eq!(f.engine.set_items_hidden(&[id], true).unwrap(), 0);
        assert_eq!(
            crate::commands::grid_info(&f.engine).version,
            after.version,
            "hiding a hidden photo rebuilt the grid"
        );

        f.engine.set_view(GridView::Hidden).unwrap();
        let hidden = crate::commands::grid_info(&f.engine);
        assert_eq!((hidden.view, hidden.len), (GridView::Hidden, 1));
    }

    #[test]
    fn set_star_writes_the_ini_and_the_grid_follows() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;

        f.engine.set_star(ids[0], true).unwrap();

        assert_eq!(
            std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
            b"[a.jpg]\r\nstar=yes\r\n"
        );
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.starred_count, 1);
        assert!(f.engine.grid().1.rows(0, 2)[0].starred);
        assert!(
            f.engine.grid().0 > version,
            "the grid is rebuilt so the tile badge and the count follow"
        );
        assert!(
            crate::commands::viewer_item(&f.engine, ids[0])
                .unwrap()
                .starred
        );
    }

    #[test]
    fn set_stars_writes_one_ini_per_folder_and_refreshes_once() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img), ("sub/c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;

        assert_eq!(f.engine.set_stars(&ids, true).unwrap(), 3);

        assert_eq!(
            std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
            b"[a.jpg]\r\nstar=yes\r\n[b.jpg]\r\nstar=yes\r\n",
            "both of this folder's photos in one file"
        );
        assert_eq!(
            std::fs::read(f.photos.join("sub").join(".picasa.ini")).unwrap(),
            b"[c.jpg]\r\nstar=yes\r\n"
        );
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.starred_count, 3);
        assert_eq!(
            f.engine.grid().0,
            version + 1,
            "one refresh for the whole batch, not one per photo"
        );
    }

    /// The promise the whole feature has to keep: the photos it copies are not touched.
    #[test]
    fn exporting_copies_photos_out_and_leaves_the_originals_alone() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let out = f.dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let before: Vec<_> = ["a.jpg", "b.jpg"]
            .iter()
            .map(|n| {
                let p = f.photos.join(n);
                (
                    std::fs::read(&p).unwrap(),
                    std::fs::metadata(&p).unwrap().modified().unwrap(),
                )
            })
            .collect();

        let report = f.engine.export_items(&ids, &out, true).unwrap();

        assert_eq!((report.written, report.failed), (2, 0));
        assert_eq!(std::fs::read(out.join("a.jpg")).unwrap(), img);
        assert_eq!(std::fs::read(out.join("b.jpg")).unwrap(), img);
        for (n, (bytes, modified)) in ["a.jpg", "b.jpg"].iter().zip(before) {
            let p = f.photos.join(n);
            assert_eq!(std::fs::read(&p).unwrap(), bytes, "{n} was rewritten");
            assert_eq!(
                std::fs::metadata(&p).unwrap().modified().unwrap(),
                modified,
                "{n} was touched"
            );
        }
    }

    /// Copies written inside a watched folder are scanned back in as new photos: one click
    /// would double the library. Refused before anything is written, not after.
    #[test]
    fn exporting_into_a_watched_folder_is_refused_and_writes_nothing() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();

        for dest in [f.photos.clone(), f.photos.join("sub")] {
            let err = f.engine.export_items(&ids, &dest, false).unwrap_err();
            assert!(
                matches!(err, Error::ExportIntoLibrary { .. }),
                "{dest:?}: {err}"
            );
        }

        // The folder *holding* the watched one is not the library: copies there are never
        // scanned, and refusing it would cost the user their home folder as a destination.
        let above = f.photos.parent().unwrap().join("beside");
        std::fs::create_dir_all(&above).unwrap();
        assert_eq!(
            f.engine.export_items(&ids, &above, false).unwrap().written,
            2
        );
        let parent = f.photos.parent().unwrap().to_path_buf();
        assert!(
            f.engine.export_items(&ids[..1], &parent, false).is_ok(),
            "a folder that contains a watched one is a usable destination"
        );

        // Nothing landed: the watched folder still holds exactly what it did.
        let mut names: Vec<_> = std::fs::read_dir(&f.photos)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.jpg", "sub"]);
    }

    /// A selection outlives its photos, and one purged id must not end the export.
    #[test]
    fn an_export_reports_what_it_could_not_write_and_keeps_going() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let out = f.dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let report = f
            .engine
            .export_items(&[ids[0], 9_999, ids[1]], &out, false)
            .unwrap();

        assert_eq!((report.written, report.failed), (2, 1));
        assert!(report.reason.is_some(), "the first reason is reported");
    }

    /// The progress a user watches: it ends at the total whatever happened on the way, so a
    /// failure cannot leave the bar short of the end for ever.
    #[test]
    fn an_export_reports_progress_that_ends_at_the_total() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = vec![f.ids()[0], 9_999, f.ids()[1]];
        let out = f.dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        f.engine.export_items(&ids, &out, false).unwrap();

        let last = f
            .events
            .all()
            .into_iter()
            .filter_map(|e| match e {
                Recorded::Export(p) => Some(p),
                _ => None,
            })
            .next_back()
            .expect("an export reports progress");
        assert_eq!(
            last,
            ExportProgress {
                done: 3,
                total: 3,
                failed: 1
            }
        );
    }

    #[test]
    fn a_keyword_written_to_a_selection_refreshes_the_grid_once() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img), ("sub/c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;

        let (name, count) = f.engine.add_items_tag(&ids, "beach").unwrap();
        assert_eq!((name.as_str(), count), ("beach", 3));
        assert_eq!(
            f.engine.grid().0,
            version + 1,
            "one refresh for the whole batch, not one per photo"
        );
        for id in &ids {
            assert_eq!(f.engine.lib.item_tags(*id).unwrap(), ["beach"]);
        }

        let version = f.engine.grid().0;
        assert_eq!(f.engine.remove_items_tag(&ids, "beach").unwrap(), 3);
        assert_eq!(f.engine.grid().0, version + 1);
        assert!(f.engine.lib.item_tags(ids[0]).unwrap().is_empty());

        // A write that changed nothing must not bump the version: the bump is what makes
        // every listener re-read, and the viewer re-read its photo.
        let version = f.engine.grid().0;
        assert_eq!(f.engine.remove_items_tag(&ids, "beach").unwrap(), 0);
        assert_eq!(f.engine.add_items_tag(&[9_999], "sun").unwrap().1, 0);
        assert_eq!(f.engine.grid().0, version);
    }

    #[test]
    fn set_stars_skips_a_folder_it_cannot_write_and_reports_the_count() {
        // An oversized INI is unreadable (MAX_INI), which is how the single-photo test
        // arranges a failing write. The other folder must still be starred: a read-only
        // folder in a selection cannot cost the user every other photo in it.
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        std::fs::write(
            f.photos.join(".picasa.ini"),
            vec![b' '; photon_core::picasa::MAX_INI as usize + 1],
        )
        .unwrap();

        assert_eq!(f.engine.set_stars(&ids, true).unwrap(), 1);

        assert_eq!(
            std::fs::read(f.photos.join("sub").join(".picasa.ini")).unwrap(),
            b"[c.jpg]\r\nstar=yes\r\n"
        );
        let starred: Vec<i64> = f
            .engine
            .grid()
            .1
            .rows(0, 2)
            .iter()
            .filter(|e| e.starred)
            .map(|e| e.id)
            .collect();
        assert_eq!(starred.len(), 1, "only the folder that could be written");
        assert_ne!(
            f.engine.lib.item(ids[0]).unwrap().unwrap().rating,
            Some(1),
            "a folder whose INI write failed gets no rating either"
        );
    }

    #[test]
    fn set_stars_fails_when_no_folder_could_be_written() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;
        std::fs::write(
            f.photos.join(".picasa.ini"),
            vec![b' '; photon_core::picasa::MAX_INI as usize + 1],
        )
        .unwrap();

        let err: crate::error::AppError = f.engine.set_stars(&ids, true).unwrap_err().into();

        assert_eq!(err.kind, "iniWrite");
        assert_eq!(
            f.engine.grid().0,
            version,
            "nothing landed, nothing to refresh"
        );
    }

    #[test]
    fn set_stars_ignores_unknown_and_missing_photos() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        f.engine.lib.mark_missing(&[ids[0]], 1).unwrap();

        assert_eq!(
            f.engine
                .set_stars(&[ids[0], ids[1], ids[1] + 1000], true)
                .unwrap(),
            1,
            "the missing one and the unknown one are skipped, not fatal"
        );
        assert_eq!(
            std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
            b"[b.jpg]\r\nstar=yes\r\n"
        );
    }

    /// A tag change moves Tag-view membership and what search matches, so it has to travel
    /// the refresh chain. Without it the database moves while the grid shows stale rows.
    #[test]
    fn setting_a_tag_refreshes_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;

        assert_eq!(f.engine.add_item_tag(ids[0], "sunset").unwrap(), "sunset");
        assert!(
            f.engine.grid().0 > version,
            "the grid version must move with the tag"
        );

        f.engine.set_tag_view("sunset").unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);

        f.engine.remove_item_tag(ids[0], "sunset").unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
    }

    #[test]
    fn a_rescan_after_set_star_agrees_with_what_photon_wrote() {
        // The INI is the authority: a star that lived only in the database would be undone
        // by the next scan's Picasa pass, which sets every row from the file. A DB-only
        // implementation fails here twice over - the scan clears the star and, having
        // changed a row, bumps the version.
        //
        // `wait_idle` before the rescan is what makes this deterministic rather than a race
        // against the thumbnail workers: it lets the rescan's look-alike pass find the
        // photo's thumbnail already cached and hash it, which is exactly the case PR #70
        // regressed on macOS CI - hashing a row through a gate reading `pass.hashed > 0`
        // rebuilt the grid for nothing, since `percep_hash` decides no view. Without this
        // call the pass sees no thumbnail yet on most runs and the bug is invisible here.
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        let id = f.ids()[0];
        f.engine.set_star(id, true).unwrap();
        f.engine.thumbs.wait_idle();
        let version = f.engine.grid().0;

        f.engine.start_scan(watched);
        f.engine.wait_for_scans();

        assert_eq!(
            f.engine.grid().0,
            version,
            "nothing moved, so nothing was rebuilt"
        );
        assert_eq!(f.engine.lib.item(id).unwrap().unwrap().rating, Some(1));
    }

    #[test]
    fn unstarring_removes_only_that_photo_s_star() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        std::fs::write(
            f.photos.join(".picasa.ini"),
            b"[a.jpg]\r\nstar=yes\r\nbackuphash=1\r\n[b.jpg]\r\nstar=yes\r\n",
        )
        .unwrap();
        f.add_photos();
        let ids = f.ids();
        f.engine.set_view(GridView::Starred).unwrap();
        assert_eq!(f.engine.grid().1.len(), 2);

        f.engine.set_star(ids[0], false).unwrap();

        assert_eq!(
            std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
            b"[a.jpg]\r\nbackuphash=1\r\n[b.jpg]\r\nstar=yes\r\n"
        );
        assert_eq!(
            f.engine.grid().1.len(),
            1,
            "the Starred view drops the photo at once"
        );
        assert_eq!(f.engine.grid().1.rows(0, 1)[0].id, ids[1]);
    }

    #[test]
    fn set_star_refuses_a_missing_or_unknown_photo_and_writes_nothing() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        f.engine.lib.mark_missing(&[id], 1).unwrap();
        assert!(matches!(
            f.engine.set_star(id, true),
            Err(photon_core::Error::NotFound(_))
        ));
        assert!(matches!(
            f.engine.set_star(id + 1000, true),
            Err(photon_core::Error::NotFound(_))
        ));
        assert!(!f.photos.join(".picasa.ini").exists());
    }

    #[test]
    fn a_failed_ini_write_leaves_the_database_untouched() {
        // The file first, then the database. The other order would leave a star in the
        // database that no INI confirms - shown in the UI, then silently cleared by the next
        // scan - which is the exact staleness the Picasa pass exists to prevent.
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let version = f.engine.grid().0;
        std::fs::write(
            f.photos.join(".picasa.ini"),
            vec![b' '; photon_core::picasa::MAX_INI as usize + 1],
        )
        .unwrap();

        let err: crate::error::AppError = f.engine.set_star(id, true).unwrap_err().into();

        assert_eq!(err.kind, "iniWrite");
        assert!(err.message.contains(".picasa.ini"), "{}", err.message);
        assert_ne!(f.engine.lib.item(id).unwrap().unwrap().rating, Some(1));
        assert_eq!(f.engine.grid().0, version);
    }

    #[test]
    fn open_loads_the_existing_grid_without_scanning() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let reopened =
            Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        assert_eq!(reopened.grid().1.len(), 1);
    }

    #[test]
    fn add_folder_refuses_photons_own_directories() {
        let f = fixture(&[]);
        let cache = f.dir.path().join("cache").join("thumbs");
        let cache_root = f.dir.path().join("cache");
        let data = f.dir.path().join("data");
        assert!(matches!(
            f.engine.add_folder(&cache),
            Err(photon_core::Error::FolderExcluded { .. })
        ));
        assert!(matches!(
            f.engine.add_folder(&cache_root),
            Err(photon_core::Error::FolderExcluded { .. })
        ));
        assert!(matches!(
            f.engine.add_folder(&data),
            Err(photon_core::Error::FolderExcluded { .. })
        ));
    }

    #[test]
    fn remove_folder_clears_its_items_from_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.remove_folder(watched.id).unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
        assert!(f.engine.lib.watched_folders().unwrap().is_empty());
        assert!(matches!(
            f.events.all().last(),
            Some(Recorded::Library(LibraryChanged { len: 0, .. }))
        ));
    }

    /// The startup collection used to walk the whole cache, two files per photo, on every
    /// launch. It now runs only when a write since the last collection could have orphaned a
    /// thumbnail. A planted orphan is the probe: it survives a launch where nothing changed
    /// and goes on the launch after a purge.
    #[test]
    fn startup_walks_the_thumbnail_cache_only_when_something_could_have_orphaned_one() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let orphan = f
            .config()
            .cache_dir
            .join("grid")
            .join("de")
            .join("deadbeefdeadbeef.webp");
        std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
        std::fs::write(&orphan, b"stale").unwrap();
        // The previous launch collected after that scan.
        let epoch = f
            .engine
            .lib
            .thumb_gc_due(now_ms(), Duration::from_secs(1))
            .unwrap()
            .unwrap();
        f.engine.lib.thumb_gc_done(epoch, now_ms()).unwrap();

        f.engine.startup(None);
        f.engine.wait_for_startup();
        assert!(
            orphan.exists(),
            "nothing could have orphaned a thumbnail, so the cache was not walked"
        );

        f.engine.lib.purge_items(&f.ids()).unwrap();
        f.engine.startup(None);
        f.engine.wait_for_startup();
        assert!(!orphan.exists(), "a purge makes the next launch collect");
    }

    #[test]
    fn startup_adds_pictures_only_to_an_empty_library() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.engine.startup(Some(f.photos.clone()));
        f.engine.wait_for_startup();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
        assert_eq!(f.engine.grid().1.len(), 1);

        let other = f.dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        f.engine.startup(Some(other));
        f.engine.wait_for_startup();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
    }

    #[test]
    fn cancelling_an_idle_folder_is_a_no_op() {
        let f = fixture(&[]);
        f.engine.cancel_scan(42);
        f.engine.shutdown();
        assert!(!f.engine.is_scanning(42));
    }

    #[test]
    fn wait_for_startup_without_startup_is_a_no_op() {
        let f = fixture(&[]);
        f.engine.wait_for_startup();
        f.engine.wait_for_startup();
    }

    #[test]
    fn shutdown_joins_the_startup_thread_and_is_idempotent() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.engine.startup(Some(f.photos.clone()));
        f.engine.shutdown();
        assert!(f.engine.startup.lock().is_none());
        // Calling shutdown (and thus wait_for_startup) again must not hang or panic.
        f.engine.shutdown();
    }

    /// Many tiny files so the scan (and thus `remove_folder`'s cancellation) has real work
    /// to do; `remove_folder` calls `cancel_scan`, which blocks until the scan thread has
    /// actually finished, so the assertions below hold regardless of how far the scan got.
    fn many_jpegs(n: usize) -> Vec<(String, Vec<u8>)> {
        let img = jpeg(4, 4);
        (0..n).map(|i| (format!("{i}.jpg"), img.clone())).collect()
    }

    #[test]
    fn remove_folder_while_scanning_leaves_no_stale_state() {
        let files = many_jpegs(300);
        let named: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let f = fixture(&named);
        let watched = f.engine.add_folder(&f.photos).unwrap();

        f.engine.remove_folder(watched.id).unwrap();

        assert_eq!(f.engine.grid().1.len(), 0);
        assert!(f.engine.lib.watched_folders().unwrap().is_empty());
        assert!(!f.engine.is_scanning(watched.id));
    }

    #[test]
    fn start_scan_after_shutdown_returns_false() {
        let f = fixture(&[]);
        let watched = f
            .engine
            .lib
            .add_watched_folder(&f.photos, f.engine.excluded())
            .unwrap();
        f.engine.shutdown();
        assert!(!f.engine.start_scan(watched));
    }

    /// `start_scan` inserts the running-scan entry before it returns, under the same lock
    /// as the "already running" check, so the second call below is guaranteed to see it -
    /// no dependency on scan timing.
    #[test]
    fn start_scan_twice_while_running_returns_false_the_second_time() {
        let files = many_jpegs(300);
        let named: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let f = fixture(&named);
        let watched = f
            .engine
            .lib
            .add_watched_folder(&f.photos, f.engine.excluded())
            .unwrap();

        assert!(f.engine.start_scan(watched.clone()));
        assert!(!f.engine.start_scan(watched));

        f.engine.wait_for_scans();
    }

    /// `remove_folder` cancels the running scan before deleting the folder, but the watcher
    /// is live in that whole window and maps events to roots by path. A subtree scan that
    /// starts in it writes the folder's rows back *after* the deletion commits - exactly
    /// what `cancel_scan`'s contract exists to prevent, which it closes for the scan it
    /// cancelled and not for a new one.
    #[test]
    fn no_scan_starts_for_a_folder_being_removed() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();

        f.engine.removing.lock().insert(watched.id);

        assert!(
            !f.engine.start_scan(watched.clone()),
            "a full rescan is refused for a folder being removed"
        );
        assert!(
            !f.engine
                .start_subtree_scan(watched.clone(), f.photos.join("a")),
            "and so is the watcher's subtree scan, which is the one that actually races"
        );

        f.engine.removing.lock().remove(&watched.id);
        f.engine.remove_folder(watched.id).unwrap();
        assert!(
            f.engine.removing.lock().is_empty(),
            "the gate lifts once the removal is done, however it ended"
        );
    }

    /// Deterministic regardless of interleaving: `cancel_scan`'s contract is "cancelled
    /// and stopped" for every caller, so once both threads' calls have returned, the scan
    /// is guaranteed gone, whichever of the two actually joined the scan thread.
    #[test]
    fn cancel_scan_from_two_threads_at_once_both_see_it_stopped() {
        let files = many_jpegs(300);
        let named: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let f = fixture(&named);
        let watched = f.engine.add_folder(&f.photos).unwrap();
        let id = watched.id;

        let e1 = f.engine.clone();
        let e2 = f.engine.clone();
        let t1 = std::thread::spawn(move || e1.cancel_scan(id));
        let t2 = std::thread::spawn(move || e2.cancel_scan(id));
        t1.join().unwrap();
        t2.join().unwrap();

        assert!(!f.engine.is_scanning(id));
    }

    /// Rebuilds run with no engine lock held, so one can still be building for the old
    /// view when a setter has already moved on and published its own. That stale index
    /// must be dropped, not published: it would put the full library on screen while
    /// `GridInfo` said Starred.
    ///
    /// Here the setter has *published*, so the `seq` stamp is what discards the stale index
    /// and this test passes with the epoch check removed. The epoch's own window - the state
    /// moved, its rebuild not yet landed - is the test below it.
    #[test]
    fn a_rebuild_started_before_a_view_change_is_not_published_over_it() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();
        f.engine.lib.set_ratings(&[(f.ids()[0], 2)]).unwrap();

        // A scan's rebuild reads the state and starts querying for All...
        let stale = f.engine.snapshot();
        let stale_index = Arc::new(f.engine.build_index(&stale.state).unwrap());
        assert_eq!(stale_index.len(), 2);
        // ...and while it does, the user clicks Starred, whose rebuild lands first.
        f.engine.set_view(GridView::Starred).unwrap();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 1);

        assert!(
            !f.engine.publish_if_current(stale_index, &stale),
            "an index built for a superseded view is discarded"
        );
        let (after, grid) = f.engine.grid();
        assert_eq!((after, grid.len()), (version, 1));
    }

    /// The combination left untested when search shipped, and recorded as the one most
    /// likely to regress if the rebuild path were ever refactored - which it since was, into
    /// the unlocked `snapshot`/`publish_if_current` pair. A scan finishing while a search is
    /// active must rebuild *the search*, not quietly restore the whole library underneath a
    /// query the UI still shows.
    #[test]
    fn a_scan_finishing_during_a_search_rebuilds_the_search() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.wait_for_scans();
        f.engine.set_search_query("beach").unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);
        let version = f.engine.grid().0;

        // A photo arrives that the query matches, and a scan picks it up.
        std::fs::write(f.photos.join("a").join("beach hut.jpg"), &img).unwrap();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(
            (info.view, info.search_query.as_str()),
            (GridView::Search, "beach"),
            "the scan must not move the view or lose the query"
        );
        assert_eq!(info.len, 2, "the new match is in the rebuilt search");
        assert!(f.engine.grid().0 > version, "and the UI was told");
        // The sidebar reads the same index, so its sections describe the filtered set.
        assert_eq!(info.sections.len(), 1);
    }

    /// The window the epoch guard exists for, and the one the scenario tests miss.
    ///
    /// `rebuild_or_restore` moves the view (or the query) and bumps the epoch *first*, and
    /// only then runs its own query - so there is a stretch where the state has changed and
    /// nothing has published yet. A rebuild stamped before that change is not stale by `seq`
    /// during it: its stamp is still the highest one published. Only the epoch says it was
    /// built for a set of photos the UI is no longer reporting.
    ///
    /// The whole app suite passed with the epoch check removed before this test existed,
    /// which is what `publish_if_current`'s "both are needed" was resting on.
    ///
    /// The `arg` half matters as much as the view: two searches are two different result
    /// sets under one `GridView::Search`, and an index built for the first would land while
    /// `GridInfo` reported the second's query.
    #[test]
    fn a_rebuild_is_refused_once_the_state_has_moved_even_before_the_new_one_publishes() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();
        f.engine.lib.set_ratings(&[(f.ids()[0], 2)]).unwrap();

        for moved in ["view", "query"] {
            f.engine.set_view(GridView::All).unwrap();
            let stale = f.engine.snapshot();
            let stale_index = Arc::new(f.engine.build_index(&stale.state).unwrap());
            assert_eq!(stale_index.len(), 2, "{moved}: built for the whole library");

            // The setter has moved the state; its own rebuild is still querying.
            {
                let mut state = f.engine.state.lock();
                match moved {
                    "view" => state.view = GridView::Starred,
                    _ => {
                        state.view = GridView::Search;
                        state.arg = "beach".to_string();
                    }
                }
                state.epoch += 1;
            }
            let (version, grid) = f.engine.grid();
            let len = grid.len();

            assert!(
                !f.engine.publish_if_current(stale_index, &stale),
                "{moved}: an index built for state that has moved is discarded"
            );
            let (after, grid) = f.engine.grid();
            assert_eq!(
                (after, grid.len()),
                (version, len),
                "{moved}: nothing was published and nothing was told to re-read"
            );
        }
    }

    /// The epoch only says which *view* an index was built for. Two rebuilds for the same
    /// view run their queries unserialised, and the one that read the database earlier can
    /// finish later: two startup scans, say, where the slower one's final rebuild lands
    /// last without the rows the other had already committed - and with both scans done,
    /// nothing rebuilds again. The sequence stamp taken at snapshot time is what orders
    /// them: an index stamped earlier than one already published is dropped.
    #[test]
    fn a_rebuild_snapshotted_earlier_is_not_published_over_a_later_one_for_the_same_view() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();

        // A slow rebuild reads both rows...
        let early = f.engine.snapshot();
        let early_index = Arc::new(f.engine.build_index(&early.state).unwrap());
        assert_eq!(early_index.len(), 2);
        // ...then a purge commits and its own rebuild, stamped later, publishes first.
        f.engine.lib.purge_items(&[f.ids()[0]]).unwrap();
        f.engine.refresh_grid().unwrap();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 1);

        assert!(
            !f.engine.publish_if_current(early_index, &early),
            "an index that read the database before an already-published one is dropped"
        );
        let (after, grid) = f.engine.grid();
        assert_eq!((after, grid.len()), (version, 1));
    }

    #[test]
    fn switching_to_the_starred_view_rebuilds_the_grid_with_only_starred_photos() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();
        let ids = f.ids();
        // Star one of them the way the Picasa pass would, then rebuild the index for the
        // new view. `rating` is `set_ratings`' column now, not `update_items`'.
        f.engine.lib.set_ratings(&[(ids[0], 2)]).unwrap();

        f.engine.set_view(GridView::Starred).unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);
        assert_eq!(f.engine.view(), GridView::Starred);

        f.engine.set_view(GridView::All).unwrap();
        assert_eq!(
            f.engine.grid().1.len(),
            2,
            "switching back restores the full set"
        );
    }

    #[test]
    fn setting_a_search_query_switches_to_the_search_view_and_filters_the_grid() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();

        f.engine.set_search_query("beach").unwrap();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.view, GridView::Search);
        assert_eq!(info.search_query, "beach");
        assert_eq!(info.len, 1);
    }

    #[test]
    fn an_empty_search_query_returns_to_the_all_view() {
        // Clearing the box must restore the library, not leave a "search" view that is
        // indistinguishable from All but labelled differently (spec §4).
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();

        f.engine.set_search_query("beach").unwrap();
        f.engine.set_search_query("   ").unwrap();

        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.view, GridView::All);
        assert_eq!(info.search_query, "");
        assert_eq!(info.len, 2, "the whole library is back");
    }

    #[test]
    fn the_album_person_and_tag_views_take_their_argument_from_the_setter() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let album = f.engine.lib.create_album("Trip", 1).unwrap();
        f.engine.lib.add_to_album(album.id, &ids[..1], 1).unwrap();

        f.engine.set_album_view(album.id).unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(
            (info.view, info.album, info.len),
            (GridView::Album, Some(album.id), 1)
        );
        assert_eq!(
            info.search_query, "",
            "the argument is an album id, not a query"
        );

        // A membership change while the album is on screen reaches the grid at once.
        f.engine.lib.add_to_album(album.id, &ids[1..], 2).unwrap();
        f.engine.albums_changed().unwrap();
        assert_eq!(f.engine.grid().1.len(), 2);

        f.engine.set_tag_view("beach").unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(
            (info.view, info.tag.as_deref(), info.album),
            (GridView::Tag, Some("beach"), None)
        );

        f.engine.set_person_view("abc").unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(
            (info.view, info.person.as_deref()),
            (GridView::Person, Some("abc"))
        );

        // Leaving for a plain view drops the argument, so it cannot be read by the next
        // parameterised view as its own.
        f.engine.set_view(GridView::All).unwrap();
        let (view, arg) = f.engine.view_and_arg();
        assert_eq!((view, arg.as_str()), (GridView::All, ""));
    }

    /// Same fixture as `commands.rs`'s `the_copy_count_is_the_info_panels_list_counted_once`,
    /// and the same reason for the second scan: three files sharing one image (as the
    /// original brief's manual `set_similar_groups` would) are indistinguishable byte
    /// copies of each other, which does not exercise the setter against a real Copies view.
    #[test]
    fn the_copies_view_takes_its_photo_from_the_setter_and_forgets_it_on_leaving() {
        use photon_core::grid::GridView;
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
            ("a/unrelated.jpg", &jpeg(64, 64)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let id_of = |name: &str| {
            f.ids()
                .into_iter()
                .find(|&id| path_of(id).ends_with(name))
                .unwrap()
        };
        let orig = id_of("orig.jpg");
        let name = |id: i64| {
            std::path::Path::new(&path_of(id))
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };

        f.engine.set_copies_view(orig).unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!((info.view, info.len), (GridView::Copies, 3));
        let copies_of = info.copies_of.expect("reported while the view is open");
        assert_eq!((copies_of.id, copies_of.file_name), (orig, name(orig)));
        assert!(!copies_of.gone, "the anchor photo is still in the library");
        assert_eq!(
            info.album, None,
            "the argument is a photo id, not an album id"
        );

        f.engine.set_view(GridView::All).unwrap();
        assert!(crate::commands::grid_info(&f.engine).copies_of.is_none());

        // A different parameterised view whose argument happens to parse as an id must not
        // be read as a Copies argument either - `view == Copies` is the guard, not "the
        // argument parses". `orig` itself is a valid id, so this is not a vacuous check.
        f.engine.set_search_query(&orig.to_string()).unwrap();
        assert!(crate::commands::grid_info(&f.engine).copies_of.is_none());

        // Re-entering without the setter must not bring the old photo back.
        f.engine.set_view(GridView::Copies).unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
    }

    /// `takes_argument` for Copies is what keeps the argument alive across a re-entry of the
    /// same view without going through `set_copies_view` again - the grid's own "reopen the
    /// same photo's tile menu" path does exactly this via `set_view`, not the setter. Probe:
    /// removing `| Self::Copies` from `GridView::takes_argument` (grid.rs) makes this fail,
    /// because `set_view` would then clear the argument even when re-entering the same view.
    #[test]
    fn the_copies_view_keeps_its_argument_when_re_entered() {
        use photon_core::grid::GridView;
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
            ("a/unrelated.jpg", &jpeg(64, 64)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();

        f.engine.set_copies_view(orig).unwrap();
        assert_eq!(f.engine.grid().1.len(), 3);

        f.engine.set_view(GridView::Copies).unwrap();
        assert_eq!(
            f.engine.grid().1.len(),
            3,
            "re-entering the same parameterised view keeps its argument"
        );
    }

    /// F1: once the anchor photo itself leaves the library, the filter (which keys off the
    /// anchor's own row) matches nothing, so the grid empties even though the other copies
    /// are still live. `gone` is what lets the UI tell that apart from "no copies any more" -
    /// it must not silently read `false` once the anchor's row is purged. Probe: replacing
    /// the `gone` computation in `commands::grid_info` with a bare `false` makes this fail.
    /// Hiding the anchor of an open Copies view takes it out of the view and leaves its
    /// copies, and `hidden` is what lets the UI say so rather than nothing.
    #[test]
    fn the_copies_view_reports_a_hidden_anchor_and_keeps_its_copies() {
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();
        f.engine.set_copies_view(orig).unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!(info.len, 2);
        assert!(!info.copies_of.unwrap().hidden);

        f.engine.set_items_hidden(&[orig], true).unwrap();
        let info = crate::commands::grid_info(&f.engine);
        let copies = info.copies_of.unwrap();
        assert!(copies.hidden, "the hidden anchor was not reported");
        assert!(!copies.gone, "a hidden anchor is not gone");
        assert_eq!(
            info.len, 1,
            "the copy left with the anchor, or the anchor stayed"
        );
        let only = f.engine.grid().1.rows(0, 1)[0].id;
        assert!(path_of(only).ends_with("identical.jpg"));
    }

    #[test]
    fn the_copies_view_reports_the_anchor_as_gone_once_its_row_is_purged() {
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
            ("a/unrelated.jpg", &jpeg(64, 64)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();

        f.engine.set_copies_view(orig).unwrap();
        assert!(
            !crate::commands::grid_info(&f.engine)
                .copies_of
                .unwrap()
                .gone,
            "the anchor is still live"
        );

        f.engine.lib.purge_items(&[orig]).unwrap();
        f.engine.refresh_grid().unwrap();

        // The twin stays through the hash frozen into the argument; the look-alike cannot be
        // frozen (see `CopiesArg`) and drops out.
        let info = crate::commands::grid_info(&f.engine);
        let path_at = |offset: usize| path_of(f.engine.grid().1.rows(offset, 1)[0].id);
        assert_eq!(info.len, 1);
        assert!(path_at(0).ends_with("identical.jpg"));
        let copies_of = info.copies_of.expect("the argument is still held");
        assert_eq!(
            copies_of.id, orig,
            "the id is read back out of the longer argument"
        );
        assert!(copies_of.gone, "the anchor's row is gone");
    }

    #[test]
    fn switching_to_another_view_clears_the_search_query() {
        // Otherwise a stale query rides along and reappears the next time Search is entered.
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();

        f.engine.set_search_query("beach").unwrap();
        f.engine.set_view(GridView::Starred).unwrap();

        assert_eq!(crate::commands::grid_info(&f.engine).search_query, "");
    }
}
