//! The running library: photon-core services plus the current grid snapshot and the
//! background scans. Plain Rust, so it can be tested without a webview.

use crate::events::{
    Events, ExportProgress, FacePhase, FaceProgress, FolderStatus, LibraryChanged,
    ScanProgressEvent,
};
use crate::watch::WatcherService;
use parking_lot::{Mutex, MutexGuard, RwLock};
use photon_core::{
    Error, Result,
    edit::Edit,
    export::{self, Options, Plan, Source},
    grid::{GridIndex, GridView},
    library::{Library, WatchedFolder},
    media::MediaKind,
    now_ms, paths, picasa,
    scanner::{ScanOptions, ScanProgress, ScanSink, scan_subtree, scan_watched},
    sort::Sort,
    thumbs::{Priority, ThumbCache, ThumbService},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Minimum time between progress events during one scan, and the floor under the time
/// between the grid rebuilds scans make as they go (see `RebuildPacer`).
const THROTTLE: Duration = Duration::from_millis(250);

/// How often a running face pass rebuilds the grid, and only while the view is a search
/// that reads faces. A pass over a large library runs for hours; rebuilding every few
/// seconds for all of it would be its largest cost, for nothing anyone is looking at.
const FACE_REBUILD_EVERY: Duration = Duration::from_secs(30);

/// How often a running face pass reports its progress. Each report counts the library.
const FACE_PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// What takes the face pass's progress line down once detection is off: nothing checked,
/// nothing to check, not running.
const FACE_PROGRESS_CLEARED: FaceProgress = FaceProgress {
    phase: FacePhase::Detecting,
    checked: 0,
    total: 0,
    running: false,
};

/// What `face_work` found first: a pass starts at that step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaceWork {
    None,
    Detect,
    Embed,
    Group,
}

/// What a running face pass owes when it ends: the rebuild for what it has written since
/// its last, and the phase its last event reports, which is the last step it ran.
struct FacePass {
    phase: FacePhase,
    /// Detections written since the last rebuild: a derived rebuild.
    unshown: bool,
    /// Faces grouped since the last rebuild: a data rebuild.
    regrouped: bool,
    last_rebuild: Instant,
}

impl FacePass {
    fn new() -> Self {
        Self {
            phase: FacePhase::Detecting,
            unshown: false,
            regrouped: false,
            last_rebuild: Instant::now(),
        }
    }
}

/// How many times its own duration the embedding step waits, after a grouping run ends,
/// before the next. A run reads every grouped face's vector, so run after every batch the
/// reads of a first recognition grow with the square of the library: about 40 GB at 100k
/// photos and 360 GB at 300k, minutes to tens of minutes, the same order as the embedding
/// itself. Waiting `K` times the run keeps grouping to at most `1 / (K + 1)` of the step's
/// time: 9 is 10%. A small library, whose run takes milliseconds, is still grouped after
/// nearly every batch.
const GROUP_COST_FACTOR: u32 = 9;

/// Paces the grouping runs of the embedding step, `RebuildPacer`'s way, with no floor:
/// the first batch that writes is grouped at once, each later one once the last run's
/// duration times `GROUP_COST_FACTOR` has passed since it ended. Faces a skipped run
/// would have placed wait for the next, or for the grouping that ends the pass. Only the
/// timing is kept between runs, never the groups: an edit or a rewritten file deletes
/// detections without `people_write`, so groups held over could name deleted faces.
#[derive(Debug, Default)]
struct GroupPacer {
    last_end: Option<Instant>,
    last_cost: Duration,
    /// Faces written since the last run, which a batch paced out leaves.
    pending: bool,
}

impl GroupPacer {
    /// Notes a batch that wrote `written` faces, and answers whether to group now.
    fn take(&mut self, written: usize, now: Instant) -> bool {
        self.pending |= written > 0;
        if self.pending && self.due(now) {
            self.pending = false;
            return true;
        }
        false
    }

    fn due(&self, now: Instant) -> bool {
        let Some(last_end) = self.last_end else {
            return true;
        };
        now.saturating_duration_since(last_end) >= self.last_cost * GROUP_COST_FACTOR
    }

    fn record(&mut self, start: Instant, end: Instant) {
        self.last_end = Some(end);
        self.last_cost = end.saturating_duration_since(start);
    }
}

/// How many times its own cost a scan's intermediate rebuild waits, after it ends, before
/// the next may start. A rebuild is a whole-library query and index build - 55 ms at 100k
/// photos, about 200 ms at 300k, more in Search - and it runs on the scan thread, so on a
/// large import a fixed 250 ms throttle spent close to half the scan rebuilding. Waiting
/// `K` times the cost keeps rebuilds to at most `1 / (K + 1)` of the time: 4 is 20%, while
/// a library small enough to rebuild in under 62 ms still refreshes every `THROTTLE`.
const REBUILD_COST_FACTOR: u32 = 4;

/// Minimum time between the grid rebuilds poster frames ask for. Longer than a scan's
/// `THROTTLE`: a frame changes no row the grid lays out, only brings a tile that gave up
/// back to asking (`Tile.svelte`'s `pageTick` effect), so a second's delay costs nothing
/// visible, while a rebuild per frame is a whole-library query - 55-70 ms at 100k items,
/// plus the UI's `gridInfo` and viewer re-reads - for every video in a folder of them.
const FRAME_REFRESH_EVERY: Duration = Duration::from_secs(1);

/// How long the thumbnail queue must stay quiet, after making new thumbnails ready, before
/// the look-alike pass is asked to hash them (`start_thumb_hashing`). The pass reads every
/// perceptual hash in the library even when it has two to add, so it is paced by the
/// bursts the queue works in - a scroll's visible tiles, an import - not by the thumbnail.
const THUMB_HASH_SETTLE: Duration = Duration::from_secs(5);

/// How long `shutdown` waits for a look-alike pass in progress to actually stop before
/// giving up and leaving it to finish on its own. Bounded for the same reason
/// `watch::STOP_TIMEOUT` is: the pass's `hash_candidates` reads the original photo files,
/// so a thread stuck on a dead network mount would otherwise mean the app never quits.
const SIMILAR_PASS_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// How old the last thumbnail collection may be before startup runs one regardless of
/// whether anything has orphaned a thumbnail since. See `Library::thumb_gc_due`.
const THUMB_GC_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The grid version of the empty index `open` starts with, before `startup` has built the
/// first real one. Every publish adds one to the version, so no built grid is ever at it,
/// and the UI can tell "not read yet" from "nothing to show" (`ui/src/lib/grid-state.ts`
/// holds the same number).
pub const NOT_BUILT: u64 = 0;

/// The waits before each retry of the first grid build: three attempts in all, over about
/// a second and a half. Enough to ride out a database that is briefly busy or unreadable
/// at launch; short enough that a lasting fault reaches the window before it looks hung.
const FIRST_GRID_BACKOFF: &[Duration] = &[Duration::from_millis(250), Duration::from_millis(1000)];

/// Runs `attempt`, and again after each wait in `backoff` while it keeps failing, unless
/// `stop` says to give up. Returns the last attempt's answer.
fn retry_after(
    backoff: &[Duration],
    stop: impl Fn() -> bool,
    mut attempt: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut result = attempt();
    for wait in backoff {
        if result.is_ok() || stop() {
            break;
        }
        std::thread::sleep(*wait);
        result = attempt();
    }
    result
}

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
    /// The user's sort, which every view is shown in. Part of the state a rebuild snapshots
    /// rather than read at query time, so a rebuild started before a sort change is
    /// discarded by the epoch like one started before a view switch.
    sort: Sort,
    epoch: u64,
}

/// What one rebuild was started against: the view state, and a sequence number that
/// orders it among all rebuilds. Both are read together, before the query begins.
#[derive(Clone, Debug)]
struct Rebuild {
    state: ViewState,
    seq: u64,
}

/// What `publish_if_current` did with a rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Publish {
    /// Published as this version.
    Published(u64),
    /// Dropped because a rebuild stamped later, for the same state, had already published.
    /// The grid on show - at this version - is for the state this rebuild was built for,
    /// and read the database later than it did.
    Overtaken(u64),
    /// Dropped because the view state has moved since this rebuild snapshotted it. Nothing
    /// published so far need show that state: the index for the new one is still coming.
    Superseded,
}

impl Publish {
    /// The grid version that shows this rebuild's view state, when one has been published.
    /// What a view setter hands the UI: any `GridInfo` at that version or later was read
    /// after the grid for the new state was in place, so the UI can skip a fetch it has
    /// already made. `Superseded` has no such version - the one on show may still be the
    /// old view's - so the UI must fetch unconditionally.
    fn shown_at(self) -> Option<u64> {
        match self {
            Publish::Published(version) | Publish::Overtaken(version) => Some(version),
            Publish::Superseded => None,
        }
    }
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

/// The sidebar's library-wide counts, as `grid_info` reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub starred: usize,
    pub duplicate: usize,
    pub hidden: usize,
    pub video: usize,
}

pub struct Engine {
    pub lib: Arc<Library>,
    pub thumbs: ThumbService,
    /// The same cache the thumbnail service writes into. Held here because the look-alike
    /// pass hashes the cached grid thumbnails rather than the photos.
    cache: Arc<ThumbCache>,
    excluded: Vec<PathBuf>,
    /// The published grid: its version, its index, and - only when the first build failed
    /// and this empty index stands in for it - why (`build_first_grid`). Held together so
    /// `grid_info` reads the reason with the version it belongs to. The fourth value is the
    /// layout generation: it moves only when a publish changes the sections or the folders,
    /// which is what lets `grid_info` leave them out (`commands::grid_info`).
    grid: RwLock<(u64, Arc<GridIndex>, Option<String>, u64)>,
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
    /// Set by every rebuild that follows a change to the data (`data_snapshot`), before it
    /// snapshots; taken back to false by whichever rebuild publishes next (a failed first
    /// build's empty stand-in says `true` and leaves it; see `publish`), which tells the
    /// UI so in `LibraryChanged::data_changed`. The UI refetches its sidebar collections -
    /// albums, people, tags, the slowest of them hundreds of milliseconds at 300k photos -
    /// only then, and not for the rebuilds of a view switch, a sort or a search keystroke,
    /// nor for `refresh_grid_derived`'s.
    ///
    /// Engine-wide rather than carried by each `Rebuild`, because a rebuild can be
    /// discarded: a scan's rebuild overtaken by a view switch never publishes, and a flag it
    /// carried would be lost with it while the switch's own publish said nothing had
    /// changed. Left here, the flag rides on the next publish instead. What makes that
    /// enough: the flag is set after the commit, and it is cleared only by a publish, which
    /// then emits it - so for every commit some event saying `data_changed` is emitted after
    /// it, and the UI's collections are read from the database, not from the index, so any
    /// fetch that event triggers sees the commit.
    data_dirty: AtomicBool,
    /// Moves whenever something a count reads may have changed: every data rebuild
    /// (`data_snapshot`) and both hashing passes (`hash_after_scan`: content hashes and
    /// look-alike groups, which the Duplicates count reads). The counts are
    /// library-wide, so a view or sort switch leaves it, and so does a poster frame.
    counts_epoch: AtomicU64,
    /// The four counts `grid_info` reports, and the epoch they were read at.
    counts: Mutex<Option<(u64, Counts)>>,
    #[cfg(test)]
    counts_computed: AtomicUsize,
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
    /// How many threads `spawn_pass` has let through that have not yet returned, which is
    /// every background pass photon runs: the look-alike pass's, a scan's included, and
    /// the face pass's. A count rather than a join handle: two requests close together are
    /// two threads, and a handle kept for the latest names the one that found its pass's
    /// lock (`hashing`) held and did nothing while the other does the work. Raised before
    /// the thread exists, so it also covers one spawned but not yet at its `try_lock`,
    /// which holds nothing a wait on that lock could see.
    background_passes: AtomicUsize,
    /// The thread `start_thumb_hashing` spawned, if any. `shutdown` joins it after closing
    /// the thumbnail queue, which is what ends its wait.
    thumb_hashing: Mutex<Option<JoinHandle<()>>>,
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
    /// Serialises `set_star` and `set_stars`. Tauri runs async commands concurrently on a
    /// worker pool, and two stars into one folder are two read-modify-writes of the same
    /// INI: unserialised, the second read would miss the first write and the rename would
    /// drop it. Held across the database write too, so the rows land in the order the file
    /// did - and no further. The grid rebuild runs after it is released: rebuilds order
    /// themselves (`publish_if_current`), and held across one, the next star waited out the
    /// whole of the previous star's rebuild before it could even write its file.
    ini_write: Mutex<()>,
    /// Serialises every write of a photo's edit; see `rotate_item`. Held from the read of
    /// the edit to its write and released before the rebuild, as `ini_write` is. Taken
    /// before the library's locks and never while holding any other.
    edit_write: Mutex<()>,
    /// Serialises `set_face_detection`: the move of the `face_enabled` mirror, the write of
    /// the stored setting, and the mirror's restore when that write fails. The commands
    /// run concurrently, and an on and an off from a quick double toggle each moved the
    /// mirror and then queued for the library's writer, which need not serve them in the
    /// order they swapped: the mirror ended on one answer and the database on the other.
    /// Mirror off and stored on, photon says it is off and detects again at the next
    /// launch; mirror on and stored off, every scan and drain runs the detector over the
    /// whole library while `write_face_batch` refuses each batch, marking nothing, for
    /// ever. Released before the pass request, the rebuild and the event, as `ini_write`
    /// is. Taken before the library's locks and never while holding any other; the face
    /// pass never takes it.
    face_write: Mutex<()>,
    /// Serialises the corrections of the People data (`write_people`) with the face pass's
    /// grouping step, which takes it for each of its runs: a pass cannot place a face in a
    /// group the user is merging or deleting, or into one a rejection has just ruled out.
    /// Taken before the library's locks and never while holding `face_write`; nothing takes
    /// `face_write` while holding this. Released before the rebuild and the pass request.
    people_write: Mutex<()>,
    /// Held by the one thread running the post-scan hashing passes; see `hash_after_scan`.
    /// It holds what the look-alike pass keeps between passes - its thumbnail reductions,
    /// and what its last regroup was asked, which is how a pass with nothing new skips the
    /// regroup - and lives here rather than in the pass because the pass runs again after
    /// every scan; see `photon_core::similar::Reductions`.
    hashing: Mutex<photon_core::similar::Reductions>,
    /// Set by every scan that ends, cleared by the pass as it starts a round. A scan that
    /// finds the pass already running leaves this behind instead of starting a second one.
    hash_requested: AtomicBool,
    /// The `face_detection` setting, mirrored so a running pass can be cancelled without a
    /// database read per photo. `set_face_detection` writes both; the stored value is the
    /// authority (`write_face_batch` reads it in its own transaction).
    face_enabled: AtomicBool,
    /// Held by the one running face pass, as `hashing` is by the look-alike pass.
    face_pass: Mutex<()>,
    /// A request that found the pass running: the runner goes round again.
    face_requested: AtomicBool,
    /// Coalesces the rebuilds poster frames ask for; see `frame_stored`.
    frame_refresh: Mutex<FrameRefresh>,
    /// Paces the rebuilds scans make as they go. One for the engine, not one per scan:
    /// several roots scanning at once are one library to rebuild, and a pacer each would
    /// rebuild it once per root per window.
    scan_rebuilds: Mutex<RebuildPacer>,
    /// How long each watched root's last full scan took, and when it ended; see
    /// `FullScan`. In memory only: the startup scan of every root times it again each
    /// session, and until one has, the watcher treats the root as untimed.
    full_scans: Mutex<HashMap<i64, FullScan>>,
}

/// A watched root's last full scan that ran to the end: how long the walk took, and when it
/// finished. The watcher paces its periodic rescans of a degraded root by it
/// (`watch::degraded_rescan_due`), so a root that takes minutes to walk is not kept busy
/// being walked.
///
/// Only a scan that stands for what the next one will cost is recorded: a whole root, not a
/// subtree (a watcher event's scan of one directory says nothing about the root), and not
/// one that was cancelled (its time is however far it got) or found its root offline (it
/// read nothing, so it takes no time at all - recorded, it would shrink the interval back
/// to the floor).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FullScan {
    pub took: Duration,
    pub finished: Instant,
}

/// When a scan's next intermediate grid rebuild is due: no sooner than `THROTTLE`, or
/// `REBUILD_COST_FACTOR` times the last one's measured cost, after the last one ended.
///
/// Those rebuilds are best-effort - the end-of-scan `refresh_grid` in `run_scan` is the one
/// every commit relies on - so a tick skipped here only shows new photos a little later.
#[derive(Debug, Default)]
struct RebuildPacer {
    /// When the last rebuild ended, or, while one runs, when it was claimed.
    last_end: Option<Instant>,
    last_cost: Duration,
}

impl RebuildPacer {
    fn due(&self, now: Instant) -> bool {
        let Some(last_end) = self.last_end else {
            return true;
        };
        let gap = THROTTLE.max(self.last_cost * REBUILD_COST_FACTOR);
        now.saturating_duration_since(last_end) >= gap
    }

    /// Takes the next rebuild if it is due. Claiming moves `last_end` to `now`, so a
    /// second scan asking while this rebuild runs is told to wait the same gap from its
    /// start. That is an estimate rather than a "running" flag on purpose: there is no flag
    /// for a rebuild that never records to leave set, and all overrunning it costs - a
    /// rebuild four times slower than the last - is one redundant rebuild, which
    /// `publish_if_current` orders like any other.
    fn claim(&mut self, now: Instant) -> bool {
        if !self.due(now) {
            return false;
        }
        self.last_end = Some(now);
        true
    }

    fn record(&mut self, start: Instant, end: Instant) {
        self.last_end = Some(end);
        self.last_cost = end.saturating_duration_since(start);
    }
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

        let sort = lib.grid_sort()?;
        let face_enabled = lib.face_detection()?;
        // The first grid is not built here. `open` runs inside Tauri's `setup`, on the main
        // thread, before the webview can load: a full read of the library and its index -
        // 200ms warm at 300k photos, far longer on a cold disk or a network share - held
        // the window blank for all of it. `startup` builds it instead, as an ordinary
        // `refresh_grid`, and until then the engine holds an empty index at version 0,
        // which no publish produces (the first is 1): the UI reads version 0 as "not built
        // yet" rather than "no photos" (`ui/src/lib/grid-state.ts`).
        //
        // Building it here also refused a library the grid query cannot run against, with
        // the error dialog. Preparing the query keeps that for every schema fault, at a
        // cost that does not grow with the library.
        lib.check_grid_query()?;
        Ok(Arc::new(Self {
            lib,
            thumbs,
            cache,
            excluded,
            grid: RwLock::new((
                NOT_BUILT,
                Arc::new(GridIndex::build(Vec::new(), sort.layout(GridView::All))),
                None,
                0,
            )),
            state: Mutex::new(ViewState {
                view: GridView::All,
                arg: String::new(),
                sort,
                epoch: 0,
            }),
            refresh: Mutex::new(0),
            next_rebuild: AtomicU64::new(1),
            data_dirty: AtomicBool::new(false),
            counts_epoch: AtomicU64::new(0),
            counts: Mutex::new(None),
            #[cfg(test)]
            counts_computed: AtomicUsize::new(0),
            events,
            scans: Mutex::new(HashMap::new()),
            removing: Mutex::new(HashSet::new()),
            next_token: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            startup: Mutex::new(None),
            background_passes: AtomicUsize::new(0),
            thumb_hashing: Mutex::new(None),
            watcher: Mutex::new(None),
            ini_write: Mutex::new(()),
            edit_write: Mutex::new(()),
            face_write: Mutex::new(()),
            people_write: Mutex::new(()),
            hashing: Mutex::new(Default::default()),
            hash_requested: AtomicBool::new(false),
            face_enabled: AtomicBool::new(face_enabled),
            face_pass: Mutex::new(()),
            face_requested: AtomicBool::new(false),
            frame_refresh: Mutex::new(FrameRefresh::default()),
            scan_rebuilds: Mutex::new(RebuildPacer::default()),
            full_scans: Mutex::new(HashMap::new()),
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

    /// The published grid in one read: its version, the index, why it is empty when only the
    /// first build failed (`None` for every grid actually built), and its layout generation.
    /// One read so the layout `grid_info` sends or leaves out is the version's own.
    pub fn published(&self) -> (u64, Arc<GridIndex>, Option<String>, u64) {
        let grid = self.grid.read();
        (grid.0, grid.1.clone(), grid.2.clone(), grid.3)
    }

    /// The four counts, from the cache when nothing they read has moved since it was filled.
    ///
    /// The epoch is read before the queries and the result stored under it: a write that
    /// lands while they run bumps past it, so the next read queries again rather than
    /// keeping an answer from before the write. A query that fails reads as 0 - the
    /// sidebar then hides that row - and the answer is not kept, so the next read tries
    /// again.
    pub fn counts(&self) -> Counts {
        let epoch = self.counts_epoch.load(Ordering::SeqCst);
        if let Some((at, counts)) = *self.counts.lock()
            && at == epoch
        {
            return counts;
        }
        #[cfg(test)]
        self.counts_computed.fetch_add(1, Ordering::SeqCst);
        let mut failed = false;
        let mut read = |what: &str, count: Result<usize>| {
            count.unwrap_or_else(|err| {
                tracing::warn!(%err, "{what} count query failed");
                failed = true;
                0
            })
        };
        let counts = Counts {
            starred: read("starred", self.lib.starred_count()),
            duplicate: read("duplicate", self.lib.duplicate_count()),
            hidden: read("hidden", self.lib.hidden_count()),
            video: read("video", self.lib.video_count()),
        };
        if !failed {
            *self.counts.lock() = Some((epoch, counts));
        }
        counts
    }

    /// Rebuilds the grid from the database for the current view and tells the UI.
    ///
    /// The query and the index build run with no engine lock held. This used to hold read
    /// guards on the view and the query for their whole duration, so a view switch on the
    /// UI thread - which needs the write side - stalled behind every scan-triggered rebuild:
    /// ~60ms per tick on a 100k library, for as long as the scan ran. What makes that safe
    /// is the `epoch` check at publish time; see `publish_if_current`.
    ///
    /// For callers that changed the data, which is every caller but the view setters: the
    /// event this publishes tells the UI to refetch the collections as well as the grid.
    pub fn refresh_grid(&self) -> Result<()> {
        self.rebuild(self.data_snapshot()).map(|_| ())
    }

    /// `refresh_grid`, after a write that has already committed: a failed rebuild is logged,
    /// not returned. `write` names the write in the log.
    ///
    /// Returned, it reached the UI as the write having failed, and the UI answers a failed
    /// write by undoing its side: the viewer's star and keyword chips flip back, an album
    /// checkbox unticks, a hide keeps its selection for a retry, the folder list and the
    /// album list are not refetched. Every one of those contradicts a database that took
    /// the write - and a retry has nothing left to do. The write stands, and the next
    /// rebuild that publishes, from any source, shows it: `data_snapshot` marked
    /// `data_dirty` before this build, a failure leaves it set, and so that publish also
    /// sends the UI to refetch the collections this write moved.
    ///
    /// Not for a view setter, whose rebuild *is* the change: that is `rebuild_or_restore`,
    /// which rolls back instead.
    fn refresh_after_write(&self, write: &'static str) {
        if let Err(err) = self.refresh_grid() {
            tracing::warn!(%err, write, "the grid could not be rebuilt after a committed write");
        }
    }

    /// `refresh_grid`, for a write that moved nothing the sidebar's collections or
    /// Settings read: a poster frame (thumbnail state) and the hashing passes
    /// (`content_hash`, `similar_group`). Those change which photos the grid draws, and how -
    /// the Duplicates view and its count, which travel in the grid and `GridInfo` the UI
    /// re-reads on every version - but no album, person, tag, tag rule or folder count, so
    /// its publish does not send the UI to refetch them. A frame is stored up to once a
    /// second while a page extracts them, and each one refetched the tag counts alone for
    /// a quarter of a second at 300k photos.
    ///
    /// The "every commit is followed by a later-stamped rebuild" promise is unchanged: this
    /// is an ordinary rebuild, stamped after the write it follows. It only leaves
    /// `data_dirty` as it found it, so a data rebuild it overtakes still has its flag
    /// carried - by this publish, if it is the next.
    fn refresh_grid_derived(&self) -> Result<()> {
        self.rebuild(self.snapshot()).map(|_| ())
    }

    /// Builds the index for `rebuild`'s snapshot and publishes it unless overtaken.
    fn rebuild(&self, rebuild: Rebuild) -> Result<Publish> {
        let index = Arc::new(self.build_index(&rebuild.state)?);
        Ok(self.publish_if_current(index, &rebuild))
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
        // Derived: a frame is a thumbnail, which no collection reads.
        if let Err(err) = self.refresh_grid_derived() {
            tracing::warn!(%err, "grid refresh after a poster frame failed");
        }
    }

    /// The index for one view state, laid out the way that view is drawn.
    fn build_index(&self, state: &ViewState) -> Result<GridIndex> {
        Ok(GridIndex::build(
            self.lib
                .sorted_entries(state.view, &state.arg, state.sort)?,
            state.sort.layout(state.view),
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

    /// `snapshot`, for a rebuild that follows a change to the data: marks `data_dirty`
    /// first. Set here, not when this rebuild publishes, so that a rebuild discarded on
    /// the way still leaves it for the publish that overtook it; see `data_dirty`. Moves
    /// `counts_epoch` at the same point for the same reason: the write this rebuild follows
    /// has committed, so a count read after this has seen it.
    fn data_snapshot(&self) -> Rebuild {
        self.data_dirty.store(true, Ordering::SeqCst);
        self.counts_epoch.fetch_add(1, Ordering::SeqCst);
        self.snapshot()
    }

    /// Publishes `index`, built from `rebuild`'s snapshot, unless it has been overtaken.
    /// Returns what it did; see `Publish`.
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
    ///
    /// A rebuild the `seq` guard drops is `Overtaken`, not `Superseded`: the epoch check
    /// has just passed, so the state is still the one it was built for, and the rebuild
    /// that published with the higher stamp snapshotted after this one did - so after the
    /// state reached this epoch - and passed the same check when it published. Epochs only
    /// rise, so it was built for this epoch too: the grid on show is this rebuild's state,
    /// read later.
    fn publish_if_current(&self, index: Arc<GridIndex>, rebuild: &Rebuild) -> Publish {
        self.publish(index, rebuild, None)
    }

    /// `publish_if_current`, carrying `failure` with the grid: the reason `build_first_grid`
    /// is publishing an empty stand-in rather than a grid it built. A failure never replaces
    /// a built grid: anything published before it - a view switch's rebuild, a scan's -
    /// read the library successfully, and an empty grid saying it could not would be false.
    /// That refusal comes before the epoch and `seq` checks, so the `Overtaken` it answers
    /// names only the version on show, which need not be for this rebuild's state; its one
    /// caller ignores the answer.
    ///
    /// Nor does a failure take the `seq` stamp: it read no rows, so it cannot overtake a
    /// rebuild that did. `build_first_grid` stamps the stand-in after its retries, and a
    /// view switch made during them is stamped earlier; had the stand-in raised
    /// `last_published`, the switch's successful read would land as overtaken and the grid
    /// would go on saying the library could not be read over one that just was.
    fn publish(
        &self,
        index: Arc<GridIndex>,
        rebuild: &Rebuild,
        failure: Option<String>,
    ) -> Publish {
        let mut last_published = self.refresh.lock();
        if failure.is_some() {
            let version = self.grid.read().0;
            if version != NOT_BUILT {
                tracing::debug!("not replacing a built grid with a failed first build");
                return Publish::Overtaken(version);
            }
        }
        if self.state.lock().epoch != rebuild.state.epoch {
            tracing::debug!("discarding a grid rebuilt for a view that has since changed");
            return Publish::Superseded;
        }
        if rebuild.seq < *last_published {
            tracing::debug!("discarding a grid rebuilt from an older read than the one published");
            return Publish::Overtaken(self.grid.read().0);
        }
        let failed = failure.is_some();
        if !failed {
            *last_published = rebuild.seq;
        }
        let (version, len) = {
            let mut grid = self.grid.write();
            // What the UI draws the grid and the sidebar from. A star, a keyword, an edit, a
            // poster frame or a hashing pass leaves both as they were, and then `grid_info`
            // need not send them again; compared here, once per publish, rather than on every
            // `grid_info`.
            if grid.1.sections() != index.sections() || grid.1.folders() != index.folders() {
                grid.3 += 1;
            }
            grid.0 += 1;
            grid.1 = index;
            grid.2 = failure;
            (grid.0, grid.1.len())
        };
        // Taken only by a rebuild that publishes, and under the `refresh` lock, so the
        // value and the version it is sent with are one publish's. A rebuild discarded
        // above leaves it for the next one. A failure says `true` without taking it: the
        // UI has no collections yet, but the refetch it asks for reads the database the
        // build could not, so the flag stays for the first publish that did read it - a
        // view switch's, which marks nothing itself, would otherwise say nothing changed.
        let data_changed = failed || self.data_dirty.swap(false, Ordering::SeqCst);
        self.events.library_changed(LibraryChanged {
            version,
            len,
            data_changed,
        });
        Publish::Published(version)
    }

    pub fn view(&self) -> GridView {
        self.state.lock().view
    }

    /// What the grid is sorted by.
    pub fn sort(&self) -> Sort {
        self.state.lock().sort
    }

    /// Sorts every view by `sort`, rebuilds the grid, and remembers the choice for the next
    /// launch. Stored only once the rebuild has succeeded: a sort that could not be shown
    /// is rolled back, and remembering it would bring the failure back at startup.
    pub fn set_sort(&self, sort: Sort) -> Result<Option<u64>> {
        let shown = self.rebuild_or_restore(|state| state.sort = sort)?;
        self.lib.set_grid_sort(sort)?;
        Ok(shown)
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
    pub fn set_view(&self, view: GridView) -> Result<Option<u64>> {
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

    /// Applies `mutate` to the view state, rebuilds the grid, and puts the state back if
    /// that fails. One place rather than one per setter: the rollback is what keeps the
    /// invariant above true, and a setter copying it is how one of the copies ends up
    /// missing a field - the rollback here once restored the view and query by name, and
    /// the sort, added later, would have stayed at the value that failed.
    ///
    /// Both the change and the rollback bump the epoch, so a rebuild in flight for either
    /// superseded state is discarded rather than published.
    ///
    /// Returns the grid version that shows the new state (`Publish::shown_at`), which the
    /// setter's command hands the UI so it can skip a refetch the `library_changed` of that
    /// publish has already made; `None` when the state moved again before this rebuild could
    /// land, and the UI must then fetch whatever is there. A rollback returns the error, never
    /// a version: a same-version `GridInfo` can report the restored view, so no version would
    /// tell the UI anything.
    fn rebuild_or_restore(&self, mutate: impl FnOnce(&mut ViewState)) -> Result<Option<u64>> {
        let previous = {
            let mut state = self.state.lock();
            let previous = state.clone();
            mutate(&mut state);
            state.epoch += 1;
            previous
        };
        // `snapshot`, not `data_snapshot`: moving the view changes no data, so its publish
        // does not ask the UI to refetch the collections - unless a data rebuild it
        // overtook left `data_dirty` set, which it then carries.
        let published = match self.rebuild(self.snapshot()) {
            Ok(published) => published,
            Err(err) => return Err(self.restore(previous, err)),
        };
        Ok(published.shown_at())
    }

    /// `rebuild_or_restore`'s rollback: puts `previous` back and returns `err`.
    fn restore(&self, previous: ViewState, err: Error) -> Error {
        {
            // The whole state rather than field by field, so a field added to it cannot
            // be left out of the rollback. Only the epoch moves on.
            let mut state = self.state.lock();
            *state = ViewState {
                epoch: state.epoch + 1,
                ..previous
            };
        }
        // The bump above discards every rebuild in flight for the state just restored,
        // so rows a scan committed meanwhile would otherwise wait for its next tick. A
        // best-effort rebuild for the restored state closes that; it is the query that
        // was working a moment ago, and if it fails too there is nothing better to do
        // than log it. A view rebuild too: a scan's commit marked the data itself.
        if let Err(err) = self.rebuild(self.snapshot()) {
            tracing::warn!(%err, "grid refresh for the restored view failed");
        }
        err
    }

    /// Searches for `query`, or returns to the full library when it is blank.
    ///
    /// An empty query is not a search: matching nothing would show an empty grid, and
    /// matching everything would be the All view under a different name (spec §4).
    ///
    /// Rolls back on a failed refresh; see `set_view`'s doc comment for why.
    pub fn set_search_query(&self, query: &str) -> Result<Option<u64>> {
        if query.trim().is_empty() {
            return self.set_view(GridView::All);
        }
        self.rebuild_or_restore(|state| {
            state.arg = query.to_string();
            state.view = GridView::Search;
        })
    }

    /// Shows the photos of one person, by `Person::key`. Rolls back on a failed refresh.
    pub fn set_person_view(&self, person: &str) -> Result<Option<u64>> {
        self.rebuild_or_restore(|state| {
            state.arg = person.to_string();
            state.view = GridView::Person;
        })
    }

    /// Shows one album. Rolls back on a failed refresh.
    pub fn set_album_view(&self, album_id: i64) -> Result<Option<u64>> {
        self.rebuild_or_restore(|state| {
            state.arg = album_id.to_string();
            state.view = GridView::Album;
        })
    }

    /// Shows the photos carrying one keyword. Rolls back on a failed refresh.
    pub fn set_tag_view(&self, tag: &str) -> Result<Option<u64>> {
        self.rebuild_or_restore(|state| {
            state.arg = tag.to_string();
            state.view = GridView::Tag;
        })
    }

    /// Shows one photo and its copies. Rolls back on a failed refresh.
    ///
    /// The photo's hash is read now, into the argument, so its twins survive the photo
    /// itself being deleted and purged while the view is open (`CopiesArg`).
    pub fn set_copies_view(&self, item_id: i64) -> Result<Option<u64>> {
        let arg = self.lib.copies_view_arg(item_id)?;
        self.rebuild_or_restore(|state| {
            state.arg = arg;
            state.view = GridView::Copies;
        })
    }

    /// After an album mutation: rebuilds the grid if an album is what it is showing. Any
    /// other view is unaffected by album membership, and the UI refetches the album list
    /// itself after the call that got here.
    pub fn albums_changed(&self) {
        if self.state.lock().view == GridView::Album {
            self.refresh_after_write("an album change");
        }
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
        self.refresh_after_write("a tag rule change");
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
    /// Holds the INI-write lock, so a test can park a star on it the way a slow share does.
    #[cfg(test)]
    pub(crate) fn hold_ini_write(&self) -> MutexGuard<'_, ()> {
        self.ini_write.lock()
    }

    pub fn set_star(&self, id: i64, starred: bool) -> Result<()> {
        let serialised = self.ini_write.lock();
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
        // The file and then its row are written, which is all the lock orders. The rebuild
        // snapshots after this commit, on this thread, so it shows the star with no lock
        // held; see `ini_write`.
        drop(serialised);
        self.refresh_after_write("a star");
        Ok(())
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
        let serialised = self.ini_write.lock();
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
        // Every file, then every row: the lock's work is done, as in `set_star`.
        drop(serialised);
        self.refresh_after_write("stars");
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
        self.refresh_after_write("a keyword");
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
            self.refresh_after_write("a keyword");
        }
        Ok((name, count))
    }

    /// Hides or unhides several photos, returning how many changed. One write and one
    /// refresh, as `add_items_tag`, and none when nothing changed.
    ///
    /// Always a whole `refresh_grid`, never a narrower update: a hidden photo leaves every
    /// view and every count the sidebar shows, and all of them are read on the rebuild.
    ///
    /// A rebuild that fails is logged, not returned: the write has committed, and the next
    /// rebuild that succeeds shows it. Returned, it reached the UI as a hide that failed,
    /// which keeps the photos selected for a retry that has nothing left to do.
    pub fn set_items_hidden(&self, ids: &[i64], hidden: bool) -> Result<usize> {
        let count = self.lib.set_hidden(ids, hidden)?;
        if count > 0 {
            self.refresh_after_write("a hide");
        }
        Ok(count)
    }

    /// Hides or unhides a folder and every photo in it (`Library::set_folder_hidden`),
    /// returning how many photos changed. The grid rebuilds only when some did: hiding an
    /// empty folder changes what later arrivals get, not what any view shows now.
    pub fn set_folder_hidden(&self, folder_id: i64, hidden: bool) -> Result<usize> {
        let count = self.lib.set_folder_hidden(folder_id, hidden)?;
        if count > 0 {
            self.refresh_after_write("a folder hide");
        }
        Ok(count)
    }

    /// Names a folder in photon (`Library::set_folder_alias`), reporting whether the name
    /// changed. The grid rebuilds when it did: search reads the alias as a haystack, so an
    /// open Search view has to follow, and no item row moved to make the refresh chain run
    /// on its own.
    pub fn set_folder_alias(&self, folder_id: i64, alias: Option<&str>) -> Result<bool> {
        let changed = self.lib.set_folder_alias(folder_id, alias)?;
        if changed {
            self.refresh_after_write("a folder name");
        }
        Ok(changed)
    }

    /// Removes the displayed name `tag` from several photos, returning how many changed.
    /// One write and one refresh, as `add_items_tag`.
    pub fn remove_items_tag(&self, ids: &[i64], tag: &str) -> Result<usize> {
        let count = self.lib.remove_items_tag(ids, tag)?;
        if count > 0 {
            self.refresh_after_write("a keyword removal");
        }
        Ok(count)
    }

    /// Removes the displayed name `tag` from one photo. Refreshes for the same reason.
    pub fn remove_item_tag(&self, id: i64, tag: &str) -> Result<()> {
        self.live_item(id)?;
        self.lib.remove_item_tag(id, tag)?;
        self.refresh_after_write("a keyword removal");
        Ok(())
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
    ///
    /// `options.max_edge` scales down the photos that exceed it, which re-encodes them like
    /// an edit does; everything within it, every video and every GIF is still a byte copy
    /// (`Source::plan`).
    pub fn export_items(&self, ids: &[i64], dest: &Path, options: Options) -> Result<Export> {
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
            let outcome = self.export_one(id, &dest, options);
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

    fn export_one(&self, id: i64, dest: &Path, options: Options) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        let source = Source {
            path: PathBuf::from(&item.path),
            orientation: item.orientation,
            edit: item.edit,
            width: item.width,
            height: item.height,
            kind: item.kind,
        };
        match source.plan(options) {
            Plan::Render { edit, max_edge } => {
                let (bytes, mime) = {
                    let _one_at_a_time = crate::protocol::RENDERING.lock();
                    export::render_for_export(&source, edit, max_edge)?
                };
                export::write_rendered(&source, dest, mime, &bytes)?;
            }
            Plan::Copy => {
                export::copy_original(&source, dest)?;
            }
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
        self.write_edit(self.edit_write.lock(), id, edit)
    }

    /// `set_item_edit` for a caller holding `edit_write`, who hands the guard over so the
    /// lock ends at the commit rather than when the caller returns.
    ///
    /// The pass request is not about the edited row's own hash - that is cleared by the
    /// write and picked up whenever a pass next runs. It is about the row's former
    /// *partners*: `set_item_edit` clears one member out of a group without rewriting the
    /// rest, so a pair becomes a survivor with a stale `similar_group`. `DUPLICATE_FILTER`
    /// keeps the view honest meanwhile; this is what makes the grouping right again, and
    /// soon, because an edit is not a file change and so no scan follows it to run a pass
    /// of its own.
    fn write_edit(
        self: &Arc<Self>,
        serialised: MutexGuard<'_, ()>,
        id: i64,
        edit: Edit,
    ) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        // photon never decodes a video, so it cannot render one turned or cropped.
        if item.kind != MediaKind::Image {
            return Err(Error::NotAPhoto(id));
        }
        let changed = self.lib.set_item_edit(id, edit)?;
        // Committed: the next turn reads this edit to build on, which is all the lock
        // serialises. Nothing below reads the edit to write one, and the rebuild snapshots
        // after the commit, on this thread, so it shows the edit with no lock held. Held
        // across it, a second press of R waited out this one's whole rebuild.
        drop(serialised);
        if changed {
            self.thumbs.prioritize(&[id], Priority::Visible);
            self.refresh_after_write("an edit");
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
        let serialised = self.edit_write.lock();
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        self.write_edit(serialised, id, item.edit.turned(clockwise))
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
        // SQLite may hand a later folder this id again; it must not inherit this one's pace.
        self.full_scans.lock().remove(&watched_id);
        if let (Some(service), Some(path)) = (self.watcher_service(), path) {
            service.watch_removed(watched_id, Path::new(&path));
        }
        self.refresh_after_write("a folder removal");
        self.request_similar_pass();
        Ok(())
    }

    /// The root's last full scan that ran to the end (`FullScan`), if one has this session.
    pub(crate) fn last_full_scan(&self, watched_id: i64) -> Option<FullScan> {
        self.full_scans.lock().get(&watched_id).copied()
    }

    /// Records a root's full scan. Its own function so the watcher's tests can give a root a
    /// scan time no fixture takes to walk.
    pub(crate) fn record_full_scan(&self, watched_id: i64, scan: FullScan) {
        self.full_scans.lock().insert(watched_id, scan);
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
        self.start_scan_inner(watched, ScanKind::Full)
    }

    /// Starts a scan of one directory beneath `watched`. It takes the same per-folder slot
    /// as a full scan, so a folder never has two scans running, and the watcher's work is
    /// cancelled by `remove_folder` and `shutdown` exactly like a manual rescan.
    pub fn start_subtree_scan(self: &Arc<Self>, watched: WatchedFolder, dir: PathBuf) -> bool {
        self.start_scan_inner(watched, ScanKind::Subtree(dir))
    }

    /// Rereads the Picasa INI of each of `dirs` beneath `watched` without walking them: for
    /// folders whose only change is their INI. Under the same per-folder slot as a scan, so
    /// it never interleaves with a scan's own read of the same INIs - where the older read
    /// could land last.
    pub fn start_ini_pass(self: &Arc<Self>, watched: WatchedFolder, dirs: Vec<PathBuf>) -> bool {
        self.start_scan_inner(watched, ScanKind::Ini(dirs))
    }

    /// Starts a background scan unless one is already running for this folder, or the
    /// engine is shutting down.
    fn start_scan_inner(self: &Arc<Self>, watched: WatchedFolder, kind: ScanKind) -> bool {
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
                match kind {
                    ScanKind::Full => engine.run_scan(&watched, None, thread_cancel),
                    ScanKind::Subtree(dir) => engine.run_scan(&watched, Some(dir), thread_cancel),
                    ScanKind::Ini(dirs) => engine.run_ini_pass(&watched, dirs, thread_cancel),
                }
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

    /// Builds the grid `open` left unbuilt, retrying after each wait in `backoff`, and
    /// publishes an empty one if every attempt fails.
    ///
    /// A failure left alone kept the grid at `NOT_BUILT`, which the UI draws as nothing at
    /// all - no photos, no empty notice, no count - and an unchanged library then rebuilds
    /// only on a view switch, since a scan that moves no rows rebuilds nothing. Before
    /// `open` stopped building the grid, the same failure was the error dialog at launch.
    /// `open` has shown the query compiles, so what is left is mostly transient - a busy or
    /// briefly unreadable database - and a few retries answer that.
    ///
    /// When they do not, an empty index is published so the window leaves the not-built
    /// state, and the error travels with it (`GridInfo::build_error`), so the grid says
    /// photon could not read the library rather than "No photos yet" - which, said of a
    /// library full of photos, sent the user off to add a folder they already have. The
    /// next rebuild that succeeds (a view switch, a scan that moves rows) puts the photos
    /// back and clears it. In `GridInfo` rather than a toast: the UI pulls it with the grid,
    /// so it cannot be lost to a webview whose listener is not up yet, and it stays for as
    /// long as it is true rather than for as long as a toast does.
    ///
    /// Not published over a grid something else has built meanwhile (see `publish`).
    fn build_first_grid(&self, backoff: &[Duration]) {
        let stopping = || self.shutting_down.load(Ordering::SeqCst);
        if let Err(err) = retry_after(backoff, stopping, || self.refresh_grid()) {
            tracing::error!(%err, "could not build the grid at startup; showing it empty");
            let rebuild = self.data_snapshot();
            let empty = GridIndex::build(Vec::new(), rebuild.state.sort.layout(rebuild.state.view));
            self.publish(Arc::new(empty), &rebuild, Some(err.to_string()));
        }
    }

    /// Background start-up work: build the first grid, take up an unfinished face pass,
    /// watch `pictures` if the library is empty, queue pending thumbnails, rescan every
    /// folder, then collect thumbnail garbage.
    ///
    /// Checks `shutting_down` before each step, and before garbage collection, so a
    /// `shutdown` racing start-up stops it promptly instead of letting it run to
    /// completion.
    pub fn startup(self: &Arc<Self>, pictures: Option<PathBuf>) {
        self.start_thumb_hashing(THUMB_HASH_SETTLE);
        let engine = Arc::clone(self);
        let handle = std::thread::Builder::new()
            .name("photon-startup".into())
            .spawn(move || {
                let shutting_down = || engine.shutting_down.load(Ordering::SeqCst);
                if shutting_down() {
                    return;
                }
                // The grid `open` left unbuilt, first and before any scan: it is what the
                // window is waiting on, and a scan's first rebuild would come a throttle
                // later at best. An ordinary rebuild, so a view switch the UI makes
                // meanwhile is ordered against it by `publish_if_current` like any other:
                // the switch bumps the epoch, and whichever of the two snapshotted the
                // switched-to view with the higher stamp publishes.
                engine.build_first_grid(FIRST_GRID_BACKOFF);
                if shutting_down() {
                    return;
                }
                // A face pass left unfinished by the last session, taken up without
                // waiting for a scan to ask: a scan that finds its root still offline
                // asks for nothing (`run_scan`), so with every drive unplugged no scan
                // would, and the pass reads only photon's own cache, which is here
                // whatever is plugged in. Nothing with the switch off, and one query on a
                // library already detected.
                engine.request_face_pass();
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
                // startup scan for the same directory. Not before them: the watcher registers
                // only roots marked online, and it is these scans that settle the flag - a
                // drive plugged in since the last session is offline until its scan, and
                // the offline poll that would otherwise register it stops looking once the
                // scan has marked it online. Not after the passes those scans request,
                // either, which run beyond the scan slot for that reason (`run_scan`).
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

    /// Requests a duplicate and look-alike pass (`hash_after_scan`), at whatever distance is
    /// stored right now, on its own thread. Every scan that read its root ends with one
    /// (`run_scan` says why not inline). `set_similar_distance` calls this after writing
    /// the setting: the
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
    /// no scan to inherit a cancel from, and a scan's pass outlives the scan whose token
    /// `cancel_scan` would set. Tying it to shutdown stops the walk on quit instead of
    /// grinding through a change nobody is left to see. A folder's removal no longer stops
    /// a pass under way, and need not: the pass only updates rows, so a deleted row takes
    /// no write, and `remove_folder` requests a pass of its own, which the running one
    /// picks up as another round.
    ///
    /// Refused once shutting down, and counted for `shutdown` to wait on: `spawn_pass`.
    pub fn request_similar_pass(self: &Arc<Self>) {
        self.spawn_pass("photon-similar-pass", |engine| {
            engine.hash_after_scan(&engine.shutting_down)
        });
    }

    /// Whether photon looks for faces itself.
    pub fn face_detection(&self) -> bool {
        self.face_enabled.load(Ordering::SeqCst)
    }

    /// Switches face detection. On requests a pass; off cancels the running one and
    /// deletes what was found.
    ///
    /// The mirror moves first, so a pass in flight stops at its next photo rather than
    /// detecting through the delete. A batch it had already detected is refused by
    /// `write_face_batch`, which reads the stored setting inside its own transaction.
    ///
    /// The mirror and the stored setting move together under `face_write`, so two calls
    /// cannot leave them disagreeing, and a failed write restores the mirror it moved
    /// rather than one a later call set.
    pub fn set_face_detection(self: &Arc<Self>, enabled: bool) -> Result<()> {
        {
            let _serialised = self.face_write.lock();
            let was = self.face_enabled.swap(enabled, Ordering::SeqCst);
            if let Err(err) = self.lib.set_face_detection(enabled) {
                self.face_enabled.store(was, Ordering::SeqCst);
                return Err(err);
            }
        }
        // Outside the lock: none of these writes the setting, and each reads what is in
        // force when it runs. `request_face_pass` and the pass read the mirror, so an on
        // overtaken by an off requests nothing or stops at once; the rebuild orders
        // itself (`publish_if_current`); and a cleared line sent after a later on is
        // followed by that on's pass reporting for itself. Held across the rebuild, a
        // second toggle would wait out a whole-library query to flip a flag.
        if enabled {
            self.request_face_pass();
        } else {
            self.refresh_after_write("switching face detection off");
            self.events.face_progress(FACE_PROGRESS_CLEARED);
        }
        Ok(())
    }

    /// Runs a correction of the People data under `people_write`, which the grouping step
    /// also takes, so a pass cannot place a face in a group the user is merging or
    /// deleting. Then the rebuild every data write ends with, and a request for a pass:
    /// faces an operation leaves ungrouped (rejected, no longer ignored) are placed by its
    /// grouping step, and nothing else would run one until the next scan.
    pub fn write_people<T>(
        self: &Arc<Self>,
        what: &'static str,
        op: impl FnOnce(&Library) -> Result<T>,
    ) -> Result<T> {
        let result = {
            let _serialised = self.people_write.lock();
            op(&self.lib)?
        };
        self.refresh_after_write(what);
        self.request_face_pass();
        Ok(result)
    }

    /// Requests a face pass, on its own thread: a no-op with the switch off. Coalesces
    /// with one already running, like `request_similar_pass`.
    pub fn request_face_pass(self: &Arc<Self>) {
        if !self.face_detection() {
            return;
        }
        self.spawn_pass("photon-face-pass", |engine| engine.detect_faces());
    }

    /// One pass at a time, `hash_after_scan`'s way: a request that finds the pass running
    /// sets the flag and leaves, and the runner goes round while it is set, so photos
    /// whose thumbnails became ready after its last batch are not left for the next scan.
    fn detect_faces(&self) {
        self.face_requested.store(true, Ordering::Release);
        loop {
            let Some(guard) = self.face_pass.try_lock() else {
                return;
            };
            while self.face_requested.swap(false, Ordering::AcqRel) {
                self.run_face_pass();
            }
            drop(guard);
            if !self.face_requested.load(Ordering::Acquire) {
                return;
            }
        }
    }

    fn face_cancelled(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst) || !self.face_enabled.load(Ordering::SeqCst)
    }

    /// Whether the grid on screen is a search whose query reads faces: the one view a
    /// detection changes while the pass is still running.
    fn view_reads_faces(&self) -> bool {
        let state = self.state.lock();
        state.view == GridView::Search
            && photon_core::search::Query::parse(&state.arg).needs().faces
    }

    /// Counts the library in `phase`'s units - photos looked at while detecting, faces
    /// while recognising - and reports it.
    fn send_face_progress(&self, phase: FacePhase, running: bool) {
        let counted = match phase {
            FacePhase::Detecting => self
                .lib
                .face_progress(photon_core::face_detect::DETECTOR_VERSION),
            FacePhase::Recognising => self
                .lib
                .embed_progress(photon_core::face_embed::EMBEDDER_VERSION),
        };
        match counted {
            Ok((checked, total)) => self.events.face_progress(FaceProgress {
                phase,
                checked,
                total,
                running,
            }),
            Err(err) => tracing::warn!(%err, "could not count the face pass's progress"),
        }
    }

    /// The first step with work, asked in the pass's order and no further. Each is one
    /// row asked for, never a count: this runs after every scan and every thumbnail drain.
    fn face_work(&self) -> Result<FaceWork> {
        use photon_core::{face_detect::DETECTOR_VERSION, face_embed::EMBEDDER_VERSION};
        Ok(
            if !self.lib.face_candidates(0, 1, DETECTOR_VERSION)?.is_empty() {
                FaceWork::Detect
            } else if !self
                .lib
                .embed_candidates(0, 1, EMBEDDER_VERSION)?
                .is_empty()
            {
                FaceWork::Embed
            } else if self.lib.has_ungrouped_faces()? {
                FaceWork::Group
            } else {
                FaceWork::None
            },
        )
    }

    /// One pass: the photos nobody has looked for faces in, then the faces nobody has
    /// recognised, each batch of those grouped as it lands, then any face an operation
    /// left ungrouped. It reports as it goes.
    ///
    /// It asks whether any step has work before anything else, for two reasons. A pass is
    /// requested at the end of every scan and every time the thumbnail queue goes quiet,
    /// and on a library already done each of those would otherwise load a model (about
    /// 50 ms for the detector) to find it has nothing to do. And a step that has work says
    /// so at once: the next report comes with its first batch written, which is eight
    /// seconds away on four workers and a minute on a small machine, long enough for
    /// someone who has just ticked the box to conclude nothing happened and tick it again.
    fn run_face_pass(&self) {
        if self.face_cancelled() {
            return;
        }
        let mut pass = FacePass::new();
        let work = match self.face_work() {
            Ok(FaceWork::None) => {
                self.end_face_pass(&pass);
                return;
            }
            Ok(work) => work,
            Err(err) => {
                tracing::warn!(%err, "could not list the photos and faces the face pass would do");
                self.end_face_pass(&pass);
                return;
            }
        };
        if work == FaceWork::Detect && !self.detect_step(&mut pass) {
            self.end_face_pass(&pass);
            return;
        }
        // After detection, asked again: it has just made faces. Otherwise `face_work`'s
        // answer stands - it asked this very question a moment ago, and the walk over every
        // face is not worth making twice.
        let embed = match work {
            FaceWork::Detect => self.has_faces_to_embed(),
            work => work == FaceWork::Embed,
        };
        if !self.face_cancelled() && embed && !self.embed_step(&mut pass) {
            self.end_face_pass(&pass);
            return;
        }
        // What the operations leave (`write_people`): a rejected face, or one no longer
        // ignored, has a vector and no group, and the embedding step above runs only when
        // something needs a vector. Asked first, unless `face_work` has just answered it:
        // the step takes the library's writer before it looks, and a pass that groups
        // reports the recognising phase last.
        let group = work == FaceWork::Group || self.has_ungrouped_faces();
        if !self.face_cancelled() && group {
            pass.phase = FacePhase::Recognising;
            self.group_faces(&mut pass);
        }
        self.end_face_pass(&pass);
    }

    fn has_faces_to_embed(&self) -> bool {
        self.lib
            .embed_candidates(0, 1, photon_core::face_embed::EMBEDDER_VERSION)
            .map(|candidates| !candidates.is_empty())
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "could not list the faces to recognise");
                false
            })
    }

    fn has_ungrouped_faces(&self) -> bool {
        self.lib.has_ungrouped_faces().unwrap_or_else(|err| {
            tracing::warn!(%err, "could not ask whether any face is ungrouped");
            false
        })
    }

    /// Detection over the photos whose preview is ready and that the current detector has
    /// not looked at. False when the detector could not be loaded, which ends the pass.
    fn detect_step(&self, pass: &mut FacePass) -> bool {
        use photon_core::face_detect::{DETECTOR_VERSION, Detector, pass as detection};
        pass.phase = FacePhase::Detecting;
        self.send_face_progress(FacePhase::Detecting, true);
        let detector = match Detector::new() {
            Ok(detector) => detector,
            Err(err) => {
                tracing::warn!(%err, "the face detector could not be loaded");
                return false;
            }
        };
        let mut last_progress: Option<Instant> = None;
        let result = detection::run(
            &self.lib,
            &self.cache,
            DETECTOR_VERSION,
            detection::workers(),
            &|preview| detector.detect(preview),
            &|| self.face_cancelled(),
            &mut |written| {
                if last_progress.is_none_or(|t| t.elapsed() >= FACE_PROGRESS_EVERY) {
                    self.send_face_progress(FacePhase::Detecting, true);
                    last_progress = Some(Instant::now());
                }
                if written == 0 {
                    return;
                }
                pass.unshown = true;
                if pass.last_rebuild.elapsed() >= FACE_REBUILD_EVERY && self.view_reads_faces() {
                    // Derived: a detection is read by a face search's grid and by the
                    // viewer, and by no album, person, tag, tag rule or folder count.
                    if let Err(err) = self.refresh_grid_derived() {
                        tracing::warn!(%err, "grid refresh failed");
                    }
                    pass.last_rebuild = Instant::now();
                    pass.unshown = false;
                }
            },
        );
        // The faces it did write are still recognised: the step after reads the library,
        // not this result.
        if let Err(err) = result {
            tracing::warn!(%err, "the face pass failed");
        }
        true
    }

    /// The vectors of the faces the current embedder has not looked at, each batch grouped
    /// as it lands, so the People page fills in during a pass that runs for hours. False
    /// when the embedder could not be loaded, which ends the pass.
    fn embed_step(&self, pass: &mut FacePass) -> bool {
        use photon_core::face_embed::{EMBEDDER_VERSION, Embedder, pass as embedding};
        pass.phase = FacePhase::Recognising;
        self.send_face_progress(FacePhase::Recognising, true);
        let embedder = match Embedder::new() {
            Ok(embedder) => embedder,
            Err(err) => {
                tracing::warn!(%err, "the face recogniser could not be loaded");
                return false;
            }
        };
        let mut last_progress: Option<Instant> = None;
        let mut pacer = GroupPacer::default();
        let result = embedding::run(
            &self.lib,
            &self.cache,
            EMBEDDER_VERSION,
            photon_core::face_detect::pass::workers(),
            &|image, face| embedder.embed(image, face),
            &|| self.face_cancelled(),
            &mut |written| {
                // Before the report, so a face the report counts after a run is in its group.
                let now = Instant::now();
                if pacer.take(written, now) {
                    self.group_faces(pass);
                    pacer.record(now, Instant::now());
                }
                if last_progress.is_none_or(|t| t.elapsed() >= FACE_PROGRESS_EVERY) {
                    self.send_face_progress(FacePhase::Recognising, true);
                    last_progress = Some(Instant::now());
                }
                // Whatever the view, unlike detection's: what grouping writes is read by
                // the People page, refetched on a data change (`end_face_pass` has why).
                // A data rebuild, which publishes any detections too.
                if pass.regrouped && pass.last_rebuild.elapsed() >= FACE_REBUILD_EVERY {
                    if let Err(err) = self.refresh_grid() {
                        tracing::warn!(%err, "grid refresh failed");
                    }
                    pass.last_rebuild = Instant::now();
                    pass.regrouped = false;
                    pass.unshown = false;
                }
            },
        );
        if let Err(err) = result {
            tracing::warn!(%err, "the face pass could not recognise faces");
        }
        true
    }

    /// The grouping step: places every face that has a vector, no group and is not
    /// ignored. Under `people_write`, so no correction lands in the middle of it.
    fn group_faces(&self, pass: &mut FacePass) {
        let _serialised = self.people_write.lock();
        match self.lib.group_ungrouped_faces(&|| self.face_cancelled()) {
            Ok(0) => {}
            Ok(_) => pass.regrouped = true,
            Err(err) => tracing::warn!(%err, "the faces could not be grouped"),
        }
    }

    /// The end of a pass: the rebuild it owes the grid for what it wrote since its last,
    /// and its last word.
    fn end_face_pass(&self, pass: &FacePass) {
        // A pass ended by the quit owes nobody either: the rebuild and the count are each
        // a query over the whole library, run inside the time `shutdown` waits for this
        // thread, for a window that is closing. What it wrote is shown at the next launch.
        let quitting = self.shutting_down.load(Ordering::SeqCst);
        if !quitting {
            // Grouping is rebuilt as a data change. Not for the sidebar's People list, the
            // Person view, `person:` or the viewer: they read confirmed faces only, and
            // grouping writes none - only suggestions, new unnamed groups and the removal
            // of emptied ones. Those are read by `people_page`, which the People page and
            // its sidebar count of groups to name refetch on `data_changed` (the UI for
            // both is the next plan's); a derived rebuild would leave them stale. A data
            // rebuild publishes the detections with it.
            let rebuilt = if pass.regrouped {
                self.refresh_grid()
            } else if pass.unshown {
                self.refresh_grid_derived()
            } else {
                Ok(())
            };
            if let Err(err) = rebuilt {
                tracing::warn!(%err, "grid refresh failed");
            }
        }
        // The pass's last word says it is not running, whatever it did. After a
        // switch-off that is the cleared line again, although the command has sent one:
        // the batch the switch interrupted still reports, as running, and a report the
        // command's line overtook would otherwise be left standing.
        if !self.face_enabled.load(Ordering::SeqCst) {
            self.events.face_progress(FACE_PROGRESS_CLEARED);
        } else if !quitting {
            self.send_face_progress(pass.phase, false);
        }
    }

    /// Runs `pass` on a thread of its own, counted in `background_passes` from before the
    /// thread exists until it returns, however it ends.
    ///
    /// Shutting down is checked and refused here, not left to the pass's cancel flag alone:
    /// `shutdown`'s bounded wait (below) runs once, and an IPC call landing just after it -
    /// already shutting down, but not yet exited - would otherwise spawn a fresh writer
    /// that wait never accounted for. The count is raised *before* the flag is read, both
    /// `SeqCst`, and `shutdown` sets the flag before it reads the count: so either
    /// `shutdown` sees this request in the count and waits for it, or this request sees
    /// the flag and refuses. Checked the other way round, a request could read the flag
    /// clear, then `shutdown` set it and find the count at zero, and only then would the
    /// thread be counted.
    fn spawn_pass(self: &Arc<Self>, name: &str, pass: fn(&Arc<Engine>)) {
        self.background_passes.fetch_add(1, Ordering::SeqCst);
        if self.shutting_down.load(Ordering::SeqCst) {
            self.background_passes.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        // Lowers the count however the pass ends, a panic included, and also when the
        // thread never starts: made out here and moved in, it is dropped with the closure
        // if the spawn fails. Left raised, the count holds every later `shutdown` to its
        // full timeout.
        struct Done(Arc<Engine>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.background_passes.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let done = Done(Arc::clone(self));
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || pass(&done.0))
            .expect("failed to spawn a pass thread");
    }

    /// Requests a look-alike pass whenever the thumbnail workers go quiet after making new
    /// thumbnails ready, `settle` after the last job finished (`DrainSignal::wait`).
    ///
    /// The pass hashes a photo from its cached grid thumbnail, so a photo is a candidate only
    /// once that exists - and after a scan most do not yet: the scan's own pass runs as it
    /// ends, while the queue it fed is still rendering. Those photos waited for the next scan
    /// of anything, which on a quiet library is the next launch; an edit's re-render did
    /// the same, since `write_edit`'s request runs before the new picture is drawn.
    ///
    /// `request_similar_pass` coalesces with a pass already running, and a pass with
    /// nothing new skips its regroup, so a drain that readied only photos a scan's pass
    /// already hashed costs the pass's reads and nothing more.
    ///
    /// The face pass rides the same signal, for the same reason: a photo is a candidate
    /// only once its preview exists. With the switch off its request is a no-op.
    ///
    /// The thread holds the engine weakly and the queue through its own handle, so it keeps
    /// neither alive: `shutdown` closes the queue and joins it, and an engine dropped
    /// without one drops the service, which closes the queue too. Once only; a second call
    /// is a no-op, and so is one after `shutdown` has begun.
    pub fn start_thumb_hashing(self: &Arc<Self>, settle: Duration) {
        let mut slot = self.thumb_hashing.lock();
        if slot.is_some() || self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let drained = self.thumbs.drain_signal();
        let engine = Arc::downgrade(self);
        let handle = std::thread::Builder::new()
            .name("photon-thumb-hashing".into())
            .spawn(move || {
                while drained.wait(settle) {
                    let Some(engine) = engine.upgrade() else {
                        return;
                    };
                    engine.request_similar_pass();
                    engine.request_face_pass();
                }
            })
            .expect("failed to spawn thumbnail-hashing thread");
        *slot = Some(handle);
    }

    /// Blocks until every thread `spawn_pass` has spawned - a scan's pass among them - has
    /// returned (`background_passes`). Safe to call when none was ever spawned.
    /// Unbounded, so for tests and not for the quit path, which is `stop_passes`.
    ///
    /// What this does not see is `hashing` held by something other than a requested pass,
    /// which in the tests is the test thread standing in for one.
    pub fn wait_for_passes(&self) {
        while self.background_passes.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Disarms the thumbnail crash-loop guard first, then stops new scans from starting,
    /// cancels every running scan (looping until none are left, since a scan or `startup`
    /// racing this can still insert one after the first pass), stops the watcher, closes the
    /// thumbnail queue so its workers finish their current job and stop, waits for the
    /// startup thread to finish (it checks `shutting_down` at each of its own checkpoints, so
    /// this doesn't wait for it to run to completion), then waits for any look-alike pass and
    /// any face pass.
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
        // Before the look-alike pass is waited for: the closed queue has ended its wait, and
        // joined here it cannot request a pass after `stop_passes` has looked. It
        // returns at once - `request_similar_pass` only spawns, and refuses while shutting
        // down.
        let thumb_hashing = self.thumb_hashing.lock().take();
        if let Some(handle) = thumb_hashing {
            let _ = handle.join();
        }
        self.wait_for_startup();
        self.stop_passes(SIMILAR_PASS_STOP_TIMEOUT);
    }

    /// Waits, within `budget` in total, for every look-alike pass and every face pass to
    /// have stopped. The face pass is waited for the look-alike pass's way, by the shared
    /// count and then by its own lock (`face_pass`), against the same deadline.
    ///
    /// Two waits, one deadline between them.
    ///
    /// The count of requested passes (`background_passes`) closes a sliver the wait on
    /// `hashing` cannot: a thread already spawned but not yet at its own `try_lock` holds
    /// nothing, so that wait would sail past it and it would go on to regroup after
    /// `shutdown` returned. Every scan's pass is such a thread, so several roots finishing
    /// together leave several: the count is raised before each spawn and lowered as each
    /// returns. Taking and dropping `hashing`
    /// afterwards still establishes "no pass is running" for whatever holds it, requested
    /// or not.
    ///
    /// **Both are bounded, against one deadline.** An unbounded wait on the count would
    /// defeat the bound below in the common case, where the thread counted *is* the one
    /// inside `similar::update`: it would wait for exactly the thread the timeout exists to
    /// give up on. `hash_candidates` reads the original photo files, so a root on a dead
    /// network mount would hang the quit forever - the scenario `SIMILAR_PASS_STOP_TIMEOUT`
    /// was introduced for. Sharing one deadline keeps the total within the stated bound
    /// rather than twice it.
    ///
    /// `budget` is a parameter rather than the constant read directly so a test can drive
    /// this with a short one; `shutdown` is its only caller.
    fn stop_passes(&self, budget: Duration) {
        let deadline = Instant::now() + budget;
        while self.background_passes.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                tracing::warn!(
                    "a requested look-alike or face pass did not stop in time; detaching it"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        match self.hashing.try_lock_until(deadline) {
            Some(guard) => drop(guard),
            None => tracing::warn!(
                "a look-alike pass did not stop within {budget:?}; \
                 leaving it to finish on its own"
            ),
        }
        match self.face_pass.try_lock_until(deadline) {
            Some(guard) => drop(guard),
            None => tracing::warn!(
                "a face pass did not stop within {budget:?}; leaving it to finish on its own"
            ),
        }
    }

    /// An INI pass. It emits no scan progress - nothing is being scanned from the user's point
    /// of view - requests no hashing pass (no file was read), and records no full-scan time.
    /// Folders photon has never scanned are walked afterwards, in this same slot.
    fn run_ini_pass(
        self: &Arc<Self>,
        watched: &WatchedFolder,
        dirs: Vec<PathBuf>,
        cancel: Arc<AtomicBool>,
    ) {
        let pass = match photon_core::scanner::refresh_picasa(&self.lib, watched, &dirs, &cancel) {
            Ok(pass) => pass,
            Err(err) => {
                tracing::warn!(watched_id = watched.id, %err, "INI pass failed");
                return;
            }
        };
        let went_offline = pass.report.offline && watched.online;
        if (pass.report.touched_rows() || went_offline)
            && let Err(err) = self.refresh_grid()
        {
            tracing::warn!(%err, "grid refresh after an INI pass failed");
        }
        if went_offline
            && let Some(folder) = self
                .lib
                .watched_folders()
                .ok()
                .and_then(|all| all.into_iter().find(|w| w.id == watched.id))
        {
            let degraded = self
                .watcher_service()
                .is_some_and(|service| service.is_degraded(folder.id));
            self.emit_status(&folder, degraded);
        }
        for dir in pass.needs_walk {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            self.run_scan(watched, Some(dir), cancel.clone());
        }
    }

    fn run_scan(
        self: &Arc<Self>,
        watched: &WatchedFolder,
        subtree: Option<PathBuf>,
        cancel: Arc<AtomicBool>,
    ) {
        let options = ScanOptions {
            excluded: self.excluded.clone(),
            cancel: cancel.clone(),
        };
        let mut sink = ScanReporter {
            engine: self,
            watched_id: watched.id,
            last: ScanProgress::default(),
            started: Instant::now(),
            refreshed_total: 0,
            last_progress: None,
        };
        let started = Instant::now();
        let result = match &subtree {
            Some(dir) => scan_subtree(&self.lib, watched, dir, now_ms(), &options, &mut sink),
            None => scan_watched(&self.lib, watched, now_ms(), &options, &mut sink),
        };
        if subtree.is_none()
            && let Ok(report) = &result
            && !report.cancelled
            && !report.offline
        {
            let finished = Instant::now();
            self.record_full_scan(
                watched.id,
                FullScan {
                    took: finished.saturating_duration_since(started),
                    finished,
                },
            );
        }
        let last = sink.last;
        let cancelled = match &result {
            Ok(report) => {
                // One line for a scan that changed rows, with what it changed. `moved` is
                // why it is here: a photo followed to the wrong file shows as nothing but a
                // photo carrying another's albums and names, and this count - with the row
                // by row lines `apply_moves` writes at debug level - is what a user's log
                // has to say about it. Not for a scan that touched nothing: an unplugged
                // drive is polled every 30 seconds, and the watcher rescans directories
                // nothing changed in.
                //
                // Every counter `touched_rows` adds up, since any one of them alone makes
                // the line: with only some carried, a scan that did nothing but follow a
                // star logged "scan finished" over a row of zeros.
                if report.touched_rows() {
                    tracing::info!(
                        watched_id = watched.id,
                        subtree = subtree.is_some(),
                        added = report.added,
                        changed = report.changed,
                        moved = report.moved,
                        marked_missing = report.marked_missing,
                        purged = report.purged,
                        restarred = report.restarred,
                        refaced = report.refaced,
                        rehidden = report.rehidden,
                        realbumed = report.realbumed,
                        enriched = report.enriched,
                        "scan finished"
                    );
                }
                report.cancelled
            }
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
        // A scan that found its root's drive still away: above all the 30-second poll of an
        // unplugged drive, but also the startup scan of one, or the watcher's scan of a root
        // already marked gone. It read no file and wrote no row, and a root going offline
        // gives neither sweep below work - both look only at online folders - so they would
        // find exactly what the last real scan left them, and pay for it twice a minute for
        // as long as the drive stays unplugged, the look-alike regroup reading every hash in
        // the library each time. What they would catch waits for the next real scan
        // instead, which is what a library with no offline root does anyway. Only while it
        // *stays* offline: the scan that flips the flag either way changes which folders
        // both sweeps work on, so it runs them.
        let still_offline = !online_changed && result.as_ref().is_ok_and(|r| r.offline);
        // Outside the guard, and the one full sweep a scan makes. New and replaced items
        // were queued as they were indexed (`ScanReporter::indexed`); this catches what
        // that cannot: an item whose render failed transiently and sits `Pending` with
        // nothing else to retry it, and a drive that came back online, whose items the
        // sweep skipped while it was away. Leaving it inside the guard meant such an item
        // waited for an unrelated change, or a restart. It runs after every scan but one
        // that found its root still offline (`still_offline`), which can have caused
        // neither.
        if !still_offline && let Err(err) = self.thumbs.enqueue_pending() {
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
        //
        // On a thread of its own rather than on this one, so the scan slot is released
        // when the scan is done and not when the pass is. `startup` starts the watcher once
        // the slots are empty, and run here the pass held it back for the session's first
        // whole-library regroup, while files changing in that window went unseen until
        // something rescanned their directory; a watcher event for this folder waited out
        // the pass as a queued follow-up just the same. The request coalesces with a pass
        // already running (`hash_after_scan`), and `shutdown` waits for it by count
        // (`stop_passes`), so leaving the slot loses neither.
        if !cancelled && !still_offline {
            self.request_similar_pass();
            self.request_face_pass();
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
    /// the refresh is gated on `groups_changed` alone. Both rebuild with
    /// `refresh_grid_derived`: a hash or a group is read by the Duplicates view and its
    /// count in `GridInfo`, never by an album, person, tag or folder count, so neither
    /// sends the UI to refetch the collections.
    ///
    /// Here rather than in the scanner because a duplicate is a fact about the whole
    /// library, not about the root or subtree one scan walked - and because `walk_tree` has
    /// two callers, which a pass wired into the scanner has to remember and this does not.
    /// It runs after every scan, changed rows or not: the first scan after the upgrade that
    /// added the column touches nothing and still has the whole library to hash, and a
    /// thumbnail that became ready since the last pass is hashed only when some later pass
    /// runs. The one exception is a scan that finds its root still offline (`run_scan`'s
    /// `still_offline`), above all the poll of an unplugged drive: it read no file and wrote
    /// no row, so it leaves the pass nothing the last real scan did not, and the poll
    /// recurs every 30 seconds for as long as the drive is away. With nothing to do the pass is its two candidate queries, one
    /// read of every perceptual hash and one of the stored groups: the regroup itself is
    /// skipped when neither has moved since the last one (`photon_core::similar::update`).
    ///
    /// One guard covers both passes, in order: a photo is a look-alike candidate only once
    /// its thumbnail exists, and nothing in the duplicate pass changes that, so the order is
    /// only about keeping the cheap whole-library regroup last.
    ///
    /// One pass at a time. Several roots finish their startup scans close together, and two
    /// passes would read the same files twice. A request that finds the pass running sets
    /// `hash_requested` and leaves; the runner goes round again while the flag is set, so
    /// files indexed after its candidate list was read are not left for the next launch.
    /// The re-check after the guard is dropped closes the window where the flag is set
    /// after the runner's last look and before it lets go.
    ///
    /// `cancel` is `shutting_down` for every pass photon runs (see `request_similar_pass`);
    /// it stops the pass on quit, and the next launch's scans pick the work up.
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
                        // `content_hash` moved, which the Duplicates count reads, and this
                        // rebuild is a derived one that does not mark the data.
                        self.counts_epoch.fetch_add(1, Ordering::SeqCst);
                        if let Err(err) = self.refresh_grid_derived() {
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
                        // `similar_group` moved, which the Duplicates count reads too
                        // (`duplicate_ids!`), and this rebuild is a derived one.
                        self.counts_epoch.fetch_add(1, Ordering::SeqCst);
                        if let Err(err) = self.refresh_grid_derived() {
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

/// What one running scan reports back into the engine: grid rebuilds, paced by the
/// engine's shared [`RebuildPacer`], progress events, throttled to [`THROTTLE`], and the
/// ids of freshly indexed items for the thumbnail queue.
struct ScanReporter<'a> {
    engine: &'a Engine,
    watched_id: i64,
    last: ScanProgress,
    /// No intermediate rebuild in a scan's first `THROTTLE`: one shorter than that - the
    /// watcher's scan of one new file - is refreshed by its end-of-scan rebuild alone.
    started: Instant,
    refreshed_total: u64,
    last_progress: Option<Instant>,
}

impl ScanSink for ScanReporter<'_> {
    fn progress(&mut self, p: &ScanProgress) {
        self.last = *p;
        let total = p.added + p.changed;
        // Inline, on the scan thread, rather than on a thread of its own. The pacer already
        // holds rebuilds to a fifth of the scan's time; a background one would need an
        // `Arc<Engine>` this borrow does not have, and threads `shutdown` would have to find
        // and wait out. `publish_if_current` would keep it correct - it is the bookkeeping
        // that is not worth the remaining fifth.
        if total != self.refreshed_total
            && self.started.elapsed() >= THROTTLE
            && self.engine.scan_rebuilds.lock().claim(Instant::now())
        {
            let start = Instant::now();
            if let Err(err) = self.engine.refresh_grid() {
                tracing::warn!(%err, "grid refresh failed");
            }
            self.engine
                .scan_rebuilds
                .lock()
                .record(start, Instant::now());
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

    /// Before `indexed` queues the rows, so the worker finds the files under the new key and
    /// marks the row Ready without decoding. The thumbnails are of the same picture; only
    /// the path half of their key changed.
    fn moved(&mut self, keys: &[(u64, u64)]) {
        for &(old, new) in keys {
            self.engine.cache.rename(old, new);
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

/// What a scan thread does with its slot.
enum ScanKind {
    Full,
    Subtree(PathBuf),
    /// Reread these folders' INIs (`photon_core::scanner::refresh_picasa`).
    Ini(Vec<PathBuf>),
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
    use crate::testutil::{Fixture, fixture, jpeg, jpeg_pattern, portrait_jpeg, portrait_jpeg_at};
    use photon_core::media::ThumbState;
    use photon_core::thumbs::ThumbSize;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_first_grouping_run_is_due_at_once() {
        assert!(GroupPacer::default().due(Instant::now()));
    }

    #[test]
    fn a_grouping_run_holds_off_the_next_by_its_duration_times_the_factor() {
        let t0 = Instant::now();
        let mut pacer = GroupPacer::default();
        pacer.record(t0, t0 + 100 * MS);
        let end = t0 + 100 * MS;
        assert!(!pacer.due(end), "a run straight after the last");
        assert!(!pacer.due(end + 899 * MS));
        assert!(pacer.due(end + 900 * MS));
    }

    /// A batch inside the wait is not grouped, and its faces are not forgotten: the first
    /// batch after the wait groups them, though it wrote nothing itself. With nothing
    /// written since, nothing is.
    #[test]
    fn a_batch_inside_the_wait_is_grouped_by_the_first_after_it() {
        let t0 = Instant::now();
        let mut pacer = GroupPacer::default();
        assert!(pacer.take(64, t0), "the first batch is grouped at once");
        pacer.record(t0, t0 + 100 * MS);
        assert!(!pacer.take(64, t0 + 150 * MS), "grouped inside the wait");
        assert!(!pacer.take(0, t0 + 999 * MS));
        assert!(
            pacer.take(0, t0 + 1000 * MS),
            "the paced-out batch was forgotten"
        );
        pacer.record(t0 + 1000 * MS, t0 + 1000 * MS);
        assert!(
            !pacer.take(0, t0 + 2000 * MS),
            "grouped with nothing written"
        );
    }

    /// No floor: a library small enough to group in no time is grouped after every batch.
    #[test]
    fn a_grouping_run_that_took_no_time_holds_nothing_off() {
        let t0 = Instant::now();
        let mut pacer = GroupPacer::default();
        pacer.record(t0, t0);
        assert!(pacer.due(t0));
    }

    #[test]
    fn a_pacer_that_never_rebuilt_is_due_at_once() {
        assert!(RebuildPacer::default().due(Instant::now()));
    }

    #[test]
    fn a_costly_rebuild_pushes_the_next_one_back_by_its_cost() {
        let t0 = Instant::now();
        let mut pacer = RebuildPacer::default();
        pacer.record(t0, t0 + 100 * MS);
        let end = t0 + 100 * MS;
        assert!(
            !pacer.due(end + 300 * MS),
            "a 100 ms rebuild waits 400 ms, not THROTTLE"
        );
        assert!(!pacer.due(end + 399 * MS));
        assert!(pacer.due(end + 400 * MS));
        assert!(pacer.due(end + 1000 * MS));
    }

    #[test]
    fn a_cheap_rebuild_waits_for_the_throttle() {
        let t0 = Instant::now();
        let mut pacer = RebuildPacer::default();
        pacer.record(t0, t0 + 5 * MS);
        let end = t0 + 5 * MS;
        assert!(!pacer.due(end + THROTTLE - MS));
        assert!(pacer.due(end + THROTTLE));
    }

    #[test]
    fn a_claimed_rebuild_holds_off_the_next_claim_until_it_is_recorded() {
        let t0 = Instant::now();
        let mut pacer = RebuildPacer::default();
        pacer.record(t0, t0 + 100 * MS);
        let start = t0 + 600 * MS;
        assert!(pacer.claim(start));
        assert!(!pacer.claim(start + 10 * MS), "one rebuild at a time");
        pacer.record(start, start + 100 * MS);
        assert!(!pacer.claim(start + 400 * MS));
        assert!(pacer.claim(start + 500 * MS));
    }

    /// Two roots scanning at once share one rebuild budget: the second reporter's tick,
    /// straight after the first's rebuild, is not another rebuild. Not timing-sensitive in
    /// practice: the second tick comes microseconds after the first rebuild ends, against a
    /// `THROTTLE` of 250 ms.
    #[test]
    fn concurrent_scans_share_one_rebuild_budget() {
        let f = fixture(&[]);
        let started = Instant::now()
            .checked_sub(THROTTLE * 4)
            .expect("the clock is past the throttle");
        let reporter = |watched_id| ScanReporter {
            engine: &f.engine,
            watched_id,
            last: ScanProgress::default(),
            started,
            refreshed_total: 0,
            last_progress: None,
        };
        let (mut a, mut b) = (reporter(1), reporter(2));
        let progress = ScanProgress {
            files_seen: 1,
            added: 1,
            changed: 0,
        };
        let before = f.engine.grid().0;
        a.progress(&progress);
        assert_eq!(f.engine.grid().0, before + 1, "the first tick rebuilds");
        b.progress(&progress);
        assert_eq!(
            f.engine.grid().0,
            before + 1,
            "the second root's tick rides on the first's rebuild"
        );
    }

    /// A scan younger than `THROTTLE` leaves its rows to the end-of-scan rebuild, even with
    /// the shared pacer due: the watcher's scan of one new file would otherwise rebuild the
    /// whole library twice.
    #[test]
    fn a_scan_younger_than_the_throttle_makes_no_intermediate_rebuild() {
        let f = fixture(&[]);
        let mut reporter = ScanReporter {
            engine: &f.engine,
            watched_id: 1,
            last: ScanProgress::default(),
            started: Instant::now(),
            refreshed_total: 0,
            last_progress: None,
        };
        let before = f.engine.grid().0;
        reporter.progress(&ScanProgress {
            files_seen: 1,
            added: 1,
            changed: 0,
        });
        assert_eq!(f.engine.grid().0, before);
    }

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

    /// A sink that parks the first rebuild to publish after `arm` inside its
    /// `library_changed`, until the test lets it go. That is the last step of
    /// `refresh_grid`, run on the thread that committed the change, and parked there the
    /// thread holds the engine's `refresh` lock and whatever its caller still holds: a
    /// second rebuild gets as far as its own publish, and a second write needing no lock the
    /// first still holds finishes outright.
    #[derive(Default)]
    struct ParkedPublish {
        armed: AtomicBool,
        channels: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    }

    /// The test's ends of an armed `ParkedPublish`. Dropping it lets the rebuild go, so an
    /// assertion that fails while one is parked does not leave its thread parked for good.
    struct Parked {
        parked: std::sync::mpsc::Receiver<()>,
        _release: std::sync::mpsc::Sender<()>,
    }

    impl ParkedPublish {
        fn arm(&self) -> Parked {
            let (parked_tx, parked) = std::sync::mpsc::channel();
            let (release, release_rx) = std::sync::mpsc::channel();
            *self.channels.lock() = Some((parked_tx, release_rx));
            self.armed.store(true, Ordering::SeqCst);
            Parked {
                parked,
                _release: release,
            }
        }
    }

    impl Events for ParkedPublish {
        fn library_changed(&self, _: LibraryChanged) {
            if self.armed.swap(false, Ordering::SeqCst) {
                let (parked, release) = self.channels.lock().take().unwrap();
                let _ = parked.send(());
                // Nothing is ever sent: this returns when the test drops its `Parked`.
                let _ = release.recv();
            }
        }
        fn scan_progress(&self, _: ScanProgressEvent) {}
        fn folder_status(&self, _: FolderStatus) {}
        fn export_progress(&self, _: ExportProgress) {}
        fn face_progress(&self, _: FaceProgress) {}
    }

    /// An engine over `f`'s photos, scanned, whose rebuilds `sink` can park. Nothing else
    /// rebuilds once the scan is waited for - no watcher runs in a test - so the first
    /// rebuild after `arm` is the one the test starts.
    fn parkable_engine(f: &Fixture, sink: &Arc<ParkedPublish>) -> Arc<Engine> {
        let engine = Engine::open(f.config(), sink.clone()).unwrap();
        engine.add_folder(&f.photos).unwrap();
        engine.wait_for_scans();
        engine.wait_for_passes();
        engine
    }

    /// Runs `first` on a thread and parks its rebuild, runs `second` on another, and
    /// reports whether `landed` came true while `first` was still parked; then lets the
    /// rebuild go and requires both calls to have succeeded.
    ///
    /// `landed` is polled rather than `second` joined: the parked rebuild holds `refresh`,
    /// so the second call's own rebuild cannot publish and the call cannot return - only
    /// its write can land. Ten seconds is generous on purpose: when the code is right the
    /// answer arrives in milliseconds, and only a failing run waits the whole time.
    fn lands_while_parked<A: Send + 'static, B: Send + 'static>(
        sink: &ParkedPublish,
        first: impl FnOnce() -> Result<A> + Send + 'static,
        second: impl FnOnce() -> Result<B> + Send + 'static,
        landed: impl Fn() -> bool,
    ) -> bool {
        let parked = sink.arm();
        let first = std::thread::spawn(first);
        parked
            .parked
            .recv_timeout(Duration::from_secs(10))
            .expect("the first call's rebuild reached its publish");
        let second = std::thread::spawn(second);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut in_time = landed();
        while !in_time && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            in_time = landed();
        }
        drop(parked);
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        in_time
    }

    /// `ini_write` orders the INI and the rating, not the rebuild after them: the next star
    /// must not wait out the last one's rebuild, which is ~200ms at 300k photos. Each round
    /// parks one star in its publish and requires the next to write its file and its row
    /// meanwhile - once with `set_star` parked and once with `set_stars`, since each
    /// releases the lock itself. Let go, the rebuilds still show every star.
    #[test]
    fn a_star_does_not_wait_for_the_previous_stars_rebuild() {
        let img = jpeg(16, 16);
        let f = fixture(&[
            ("a.jpg", &img),
            ("b.jpg", &img),
            ("c.jpg", &img),
            ("d.jpg", &img),
        ]);
        let sink = Arc::new(ParkedPublish::default());
        let engine = parkable_engine(&f, &sink);
        let ids: Vec<i64> = engine.grid().1.rows(0, 4).iter().map(|e| e.id).collect();
        let star = |id: i64| {
            let engine = engine.clone();
            move || engine.set_star(id, true)
        };
        let star_all = |id: i64| {
            let engine = engine.clone();
            move || engine.set_stars(&[id], true)
        };
        let starred = |id| engine.lib.item(id).unwrap().unwrap().rating == Some(1);

        assert!(
            lands_while_parked(&sink, star(ids[0]), star_all(ids[1]), || starred(ids[1])),
            "a star waited for the rebuild of the `set_star` before it"
        );
        assert!(
            lands_while_parked(&sink, star_all(ids[2]), star(ids[3]), || starred(ids[3])),
            "a star waited for the rebuild of the `set_stars` before it"
        );

        assert_eq!(
            std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
            b"[a.jpg]\r\nstar=yes\r\n[b.jpg]\r\nstar=yes\r\n\
              [c.jpg]\r\nstar=yes\r\n[d.jpg]\r\nstar=yes\r\n"
        );
        assert!(engine.grid().1.rows(0, 4).iter().all(|e| e.starred));
    }

    /// The same for an edit: two presses of R on one photo, as in the viewer. The second
    /// turn must land while the first's rebuild is parked.
    #[test]
    fn a_turn_does_not_wait_for_the_previous_turns_rebuild() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        let sink = Arc::new(ParkedPublish::default());
        let engine = parkable_engine(&f, &sink);
        let id = engine.grid().1.rows(0, 1)[0].id;
        let turn = || {
            let engine = engine.clone();
            move || engine.rotate_item(id, true)
        };
        let turns = || engine.lib.item(id).unwrap().unwrap().edit.turns;

        assert!(
            lands_while_parked(&sink, turn(), turn(), || turns() == 2),
            "the second turn waited for the first turn's rebuild"
        );
        // Each turn asked for a look-alike pass; stop it before the fixture's directory goes.
        engine.shutdown();
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

        let info = crate::commands::grid_info(&f.engine, None);
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

        let info = crate::commands::grid_info(&f.engine, None);
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
        f.settle();

        let info = crate::commands::grid_info(&f.engine, None);
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
        f.settle();
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
    /// `hash_after_scan` directly) and waits for the pass with `wait_for_passes`.
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
        f.settle();
        f.engine.set_view(GridView::Duplicates).unwrap();
        assert_eq!(
            f.ids().len(),
            2,
            "grouped at the default (conservative) distance"
        );

        crate::commands::set_similar_distance(&f.engine, 0).unwrap();
        f.engine.wait_for_passes();

        assert_eq!(
            f.ids().len(),
            0,
            "the setting change alone should have taken the pair out of the view"
        );
    }

    /// The Duplicates count reads look-alike groups too (`duplicate_ids!`), and a regroup
    /// rebuilds through `refresh_grid_derived`, which does not move the counts epoch: the
    /// regroup moves it itself. Read once at the old distance so the cache is warm.
    #[test]
    fn a_regroup_changes_the_duplicate_count_on_the_next_read() {
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.settle();
        assert_eq!(
            f.engine.counts().duplicate,
            2,
            "grouped at the default distance"
        );

        crate::commands::set_similar_distance(&f.engine, 0).unwrap();
        f.engine.wait_for_passes();

        assert_eq!(f.engine.counts().duplicate, 0);
    }

    /// A thumbnail rendered after the scan's own pass has run - here, the re-render of an
    /// edit - is hashed without a further scan: the workers going quiet requests a pass.
    ///
    /// Staged through the library rather than `rotate_item`, which requests a pass of its
    /// own that could race the render and hash the new picture by luck: here nothing but
    /// the drain signal runs one.
    #[test]
    fn a_thumbnail_rendered_after_the_scan_is_hashed_without_another_scan() {
        let f = fixture(&[("a/one.jpg", &jpeg_pattern(180, 120))]);
        f.add_photos();
        f.settle();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        let hashed = |f: &crate::testutil::Fixture| {
            f.engine
                .lib
                .percep_hashes()
                .unwrap()
                .iter()
                .any(|photo| photo.id == id)
        };
        f.engine.start_thumb_hashing(Duration::from_millis(50));

        // A turn: a new picture, so a render, and the hash cleared with the thumbnail.
        let turned = f.engine.lib.item(id).unwrap().unwrap().edit.turned(true);
        assert!(f.engine.lib.set_item_edit(id, turned).unwrap());
        assert!(!hashed(&f));
        f.engine.thumbs.prioritize(&[id], Priority::Visible);

        let wait_hashed = |why: &str| {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !hashed(&f) {
                assert!(Instant::now() < deadline, "{why}: never hashed");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait_hashed("the turned photo's new thumbnail");

        // Turned back: the original picture is still cached, so the worker renders nothing
        // and only moves the row to `Ready` - a picture new to the pass all the same.
        f.engine.wait_for_passes();
        let original = f.engine.lib.item(id).unwrap().unwrap().edit.turned(false);
        assert!(f.engine.lib.set_item_edit(id, original).unwrap());
        assert!(!hashed(&f));
        f.engine.thumbs.prioritize(&[id], Priority::Visible);
        wait_hashed("the photo turned back to a cached picture");
        f.engine.shutdown();
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
        f.settle();
        let ids = f.ids();
        assert_eq!(ids.len(), 2);
        assert!(
            stored_group(&f, ids[0]).is_some(),
            "not grouped to begin with"
        );

        f.engine.rotate_item(ids[0], true).unwrap();
        f.engine.wait_for_passes();

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
        f.settle();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(first);
        f.settle();

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
        f.engine.wait_for_passes();

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

        f.engine.wait_for_passes();
    }

    /// What the count of requested passes cannot see: `hashing` held by something that is
    /// not one (here the test thread, standing in for a pass already inside
    /// `similar::update`). A request made meanwhile finds it held and returns at once, so
    /// the count is back at zero and `wait_for_passes` returns. If `shutdown` relied
    /// on the count alone it would return while the lock is still held; it must instead
    /// still be waiting on `hashing` itself.
    #[test]
    fn shutdown_waits_for_the_pass_actually_holding_hashing_not_just_the_latest_requested_thread() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();

        let held = f.engine.hashing.lock();
        f.engine.request_similar_pass();
        f.engine.wait_for_passes(); // the request found `hashing` held and returned

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

    /// A pass requested but not yet at its `try_lock` on `hashing` holds nothing, so the
    /// wait on `hashing` alone sails past it, and it goes on to hash and regroup after
    /// `shutdown` has returned. Every scan now ends by requesting its pass on a thread of its
    /// own, and two roots finishing together leave two such threads with only the later one
    /// recorded, so the count is what `shutdown` waits on. Raised by hand here, standing in
    /// for that thread, since nothing can hold a real one short of its `try_lock`.
    #[test]
    fn shutdown_waits_for_a_requested_pass_that_has_not_reached_hashing_yet() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();

        f.engine.background_passes.fetch_add(1, Ordering::SeqCst);
        let engine = Arc::clone(&f.engine);
        let shutdown = std::thread::spawn(move || engine.shutdown());
        std::thread::sleep(Duration::from_millis(150));
        let returned_early = shutdown.is_finished();
        f.engine.background_passes.fetch_sub(1, Ordering::SeqCst);
        shutdown.join().unwrap();
        assert!(
            !returned_early,
            "shutdown returned while a requested pass had yet to take `hashing`"
        );
    }

    /// The bound `shutdown` promises is over *both* its waits, and the wait on the count is
    /// the one that could quietly remove it. In the common case (a scan's pass, or a
    /// distance change, then a quit) the thread counted is the one running the pass, so an
    /// unbounded wait would wait for exactly the thread `SIMILAR_PASS_STOP_TIMEOUT` exists
    /// to give up on, and an app whose photos are on a dead mount would never quit.
    ///
    /// Both halves are made to time out here: a counted pass that never returns (the count
    /// raised by hand, lowered only once the test is done), and `hashing` held by the test
    /// thread. With both bounded against one deadline the call returns after the budget;
    /// with the count's wait unbounded it never returns at all, and with two separate
    /// budgets it would take twice as long.
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

        f.engine.background_passes.fetch_add(1, Ordering::SeqCst);
        let held = f.engine.hashing.lock();

        let engine = Arc::clone(&f.engine);
        let budget = Duration::from_secs(1);
        let started = Instant::now();
        let stopping = std::thread::spawn(move || {
            let call = Instant::now();
            engine.stop_passes(budget);
            call.elapsed()
        });
        while !stopping.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "stop_passes outran its budget of {budget:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let took = stopping.join().unwrap();
        assert!(
            took < budget * 3 / 2,
            "the two waits were budgeted separately, not against one deadline: {took:?}"
        );

        drop(held);
        f.engine.background_passes.fetch_sub(1, Ordering::SeqCst);
    }

    /// An IPC call can land after `shutdown` has already set `shutting_down` and run its
    /// bounded wait, but before the process actually exits. Without this check that call
    /// would spawn a fresh writer `shutdown` never accounted for.
    ///
    /// Two traces of a thread, since either alone can miss one: the count, raised for as
    /// long as the thread runs, and `hash_requested`, which a pass sets first thing and,
    /// with `hashing` held here, cannot clear.
    #[test]
    fn request_similar_pass_is_a_no_op_once_shutting_down() {
        let f = fixture(&[("a.jpg", &jpeg(4, 2))]);
        f.add_photos();
        f.engine.shutdown();

        let held = f.engine.hashing.lock();
        f.engine.hash_requested.store(false, Ordering::SeqCst);
        f.engine.request_similar_pass();

        assert_eq!(
            f.engine.background_passes.load(Ordering::SeqCst),
            0,
            "a request after shutdown was let through, or left itself counted - which would \
             hold any later wait to its timeout"
        );
        f.engine.wait_for_passes();
        assert!(
            !f.engine.hash_requested.load(Ordering::SeqCst),
            "a request arriving after shutdown must not spawn a thread"
        );
        drop(held);
    }

    /// How many faces photon has detected on the photos in the grid.
    fn detected(f: &Fixture) -> usize {
        f.ids()
            .into_iter()
            .map(|id| f.engine.lib.item_detected_faces(id).unwrap().len())
            .sum()
    }

    /// Scans, waits for the thumbnails, and lets every pass that follows finish.
    fn scanned(f: &Fixture) {
        f.add_photos();
        f.engine.thumbs.wait_idle();
        f.settle();
    }

    fn face_events(f: &Fixture) -> Vec<FaceProgress> {
        f.events
            .all()
            .into_iter()
            .filter_map(|e| match e {
                Recorded::Face(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    /// Off by default: a scan, its thumbnails and every pass after them leave no face data.
    #[test]
    fn nothing_is_detected_until_the_switch_is_on() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        assert!(!f.engine.face_detection());
        assert_eq!(detected(&f), 0);
    }

    /// Switching on is itself a request: the photos already there are detected without a
    /// scan happening by.
    #[test]
    fn switching_on_detects_the_photos_already_there() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/plain.jpg", &jpeg(200, 150)),
        ]);
        scanned(&f);

        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();

        assert_eq!(detected(&f), 1);
        let (checked, total) = f
            .engine
            .lib
            .face_progress(photon_core::face_detect::DETECTOR_VERSION)
            .unwrap();
        assert_eq!((checked, total), (2, 2));
    }

    /// With the switch on, a photo that arrives later is detected by the pass its own
    /// scan and thumbnail bring, with nothing else asking.
    #[test]
    fn a_photo_scanned_with_the_switch_on_is_detected() {
        let f = fixture(&[("a/plain.jpg", &jpeg(200, 150))]);
        f.engine.set_face_detection(true).unwrap();
        f.engine.start_thumb_hashing(Duration::from_millis(20));
        let watched = f.add_photos();
        std::fs::write(f.photos.join("a").join("face.jpg"), portrait_jpeg()).unwrap();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        f.engine.thumbs.wait_idle();
        // The drain's settle, then the pass it requests: neither can be waited on before
        // it exists, so this looks again until the face is there or the time is up.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            f.settle();
            if detected(&f) == 1 || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(detected(&f), 1);
    }

    #[test]
    fn switching_off_leaves_no_detections() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(detected(&f), 1);

        f.engine.set_face_detection(false).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(detected(&f), 0);
        assert!(!f.engine.face_detection());
    }

    /// The mirror and the stored setting move as one step. A second switch arriving while
    /// one is between the two (here: `face_write` held by the test, standing in for it)
    /// must wait, not move the mirror and then queue for the writer in whatever order
    /// the writer serves: that left the mirror on one answer and the database on the other.
    ///
    /// This pins that the switch waits for the lock before it touches either. That the
    /// lock is held across *both* steps has no test: the window is between the swap and
    /// the library's writer, and nothing outside photon-core can park a call there.
    #[test]
    fn a_face_switch_waits_for_the_one_before_it() {
        let f = fixture(&[]);

        let held = f.engine.face_write.lock();
        let engine = Arc::clone(&f.engine);
        let switching = std::thread::spawn(move || engine.set_face_detection(true).unwrap());
        std::thread::sleep(Duration::from_millis(150));
        let mirror = f.engine.face_detection();
        let stored = f.engine.lib.face_detection().unwrap();
        drop(held);
        switching.join().unwrap();
        f.engine.wait_for_passes();

        assert!(
            !mirror && !stored,
            "a switch went ahead beside another: mirror {mirror}, stored {stored}"
        );
        assert!(f.engine.face_detection());
        assert!(f.engine.lib.face_detection().unwrap());
    }

    /// Detections are read by the grid of a face search and by the viewer, never by an
    /// album, person, tag or folder count: the rebuild of a pass that only detected must
    /// not send the UI to refetch the sidebar. A photo with no face, so nothing is grouped:
    /// grouping is announced as a data change (`grouping_announces_a_data_change`).
    #[test]
    fn a_face_pass_does_not_announce_a_data_change() {
        let f = fixture(&[("a/plain.jpg", &jpeg(200, 150))]);
        scanned(&f);
        // Anything pending from the scan is announced by this.
        f.engine.refresh_grid().unwrap();
        let before = f.events.all().len();

        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();

        let after: Vec<_> = f.events.all().split_off(before);
        let libraries: Vec<_> = after
            .iter()
            .filter_map(|e| match e {
                Recorded::Library(l) => Some(*l),
                _ => None,
            })
            .collect();
        assert!(!libraries.is_empty(), "the pass rebuilt nothing: {after:?}");
        assert!(libraries.iter().all(|l| !l.data_changed), "{libraries:?}");
    }

    /// With nothing to detect the pass still ends with an event that says it is not
    /// running, so a progress line cannot be left standing - and says nothing else: a
    /// pass that claimed to be running first would flash the line on every scan of a
    /// library already detected.
    #[test]
    fn a_pass_with_nothing_to_do_says_only_that_it_ended() {
        let f = fixture(&[]);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(
            face_events(&f),
            [FaceProgress {
                phase: FacePhase::Detecting,
                checked: 0,
                total: 0,
                running: false
            }],
            "no face progress was sent, or one says the pass is running"
        );
    }

    /// A pass with work to do says so before it has done any: its first event is running
    /// with nothing checked yet, not the first batch's report, which on a real library is
    /// a batch of 64 detections away. It ends on the step after detection, recognising.
    #[test]
    fn a_pass_with_work_says_it_is_running_before_its_first_batch() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        let events = face_events(&f);
        assert_eq!(
            events.first(),
            Some(&FaceProgress {
                phase: FacePhase::Detecting,
                checked: 0,
                total: 1,
                running: true
            }),
            "{events:?}"
        );
        assert_eq!(
            events.last(),
            Some(&FaceProgress {
                phase: FacePhase::Recognising,
                checked: 1,
                total: 1,
                running: false
            }),
            "{events:?}"
        );
    }

    /// Records like `Recorder`, and quits the engine it is given at the second face event
    /// that says a pass is running: the report after the first batch, the first being the
    /// one a pass with work opens with. That is the one place a test can stand between a
    /// batch's write and the end of its pass.
    #[derive(Default)]
    struct QuitAfterABatch {
        engine: std::sync::OnceLock<std::sync::Weak<Engine>>,
        running: AtomicUsize,
        recorded: Mutex<Vec<Recorded>>,
    }

    impl Events for QuitAfterABatch {
        fn library_changed(&self, e: LibraryChanged) {
            self.recorded.lock().push(Recorded::Library(e));
        }
        fn scan_progress(&self, _: ScanProgressEvent) {}
        fn folder_status(&self, _: FolderStatus) {}
        fn export_progress(&self, _: ExportProgress) {}
        fn face_progress(&self, e: FaceProgress) {
            self.recorded.lock().push(Recorded::Face(e));
            if e.running
                && self.running.fetch_add(1, Ordering::SeqCst) == 1
                && let Some(engine) = self.engine.get().and_then(|e| e.upgrade())
            {
                engine.shutting_down.store(true, Ordering::SeqCst);
            }
        }
    }

    /// A pass the quit ends leaves without its rebuild and without its last count: each
    /// reads the whole library, inside the time `shutdown` waits for the pass, and nobody
    /// is left to see either. The batch it wrote stays written.
    #[test]
    fn a_pass_ended_by_the_quit_neither_rebuilds_nor_counts() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        let ids = f.ids();
        f.engine.shutdown();

        let sink = Arc::new(QuitAfterABatch::default());
        let engine = Engine::open(f.config(), sink.clone()).unwrap();
        sink.engine.set(Arc::downgrade(&engine)).ok().unwrap();
        engine.set_face_detection(true).unwrap();
        engine.wait_for_passes();

        let written: usize = ids
            .iter()
            .map(|id| engine.lib.item_detected_faces(*id).unwrap().len())
            .sum();
        assert_eq!(written, 1, "the pass did not get as far as its batch");
        let recorded = sink.recorded.lock().clone();
        let running = FaceProgress {
            phase: FacePhase::Detecting,
            checked: 0,
            total: 1,
            running: true,
        };
        let after_the_batch = FaceProgress {
            checked: 1,
            ..running
        };
        assert_eq!(
            recorded,
            [Recorded::Face(running), Recorded::Face(after_the_batch)],
            "the pass rebuilt the grid or counted the library on its way out"
        );
        engine.shutdown();
    }

    /// A launch with every drive unplugged still takes up the pass the last session left:
    /// the scan of a root that is still offline asks for no pass, and the previews are in
    /// photon's own cache. Here the photo was scanned and thumbnailed with the switch off,
    /// its folder then went away, and the switch is on in the database alone, as it is at
    /// the next launch.
    #[test]
    fn startup_takes_up_the_face_pass_with_every_root_offline() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        let ids = f.ids();
        let watched = f.engine.lib.watched_folders().unwrap().remove(0);
        std::fs::rename(&f.photos, f.dir.path().join("unplugged")).unwrap();
        f.engine.start_scan(watched);
        f.settle();
        let watched = f.engine.lib.watched_folders().unwrap().remove(0);
        assert!(!watched.online, "the scan did not find the root gone");
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine.shutdown();

        let reopened =
            Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        // A settle nothing reaches: the thumbnail queue going quiet must not be what asks.
        reopened.start_thumb_hashing(Duration::from_secs(3600));
        reopened.startup(None);
        reopened.wait_for_startup();
        reopened.wait_for_passes();

        let detected: usize = ids
            .iter()
            .map(|id| reopened.lib.item_detected_faces(*id).unwrap().len())
            .sum();
        assert_eq!(detected, 1);
        reopened.shutdown();
    }

    /// The pass ends with what it reached, running false. A photo with no face, so the
    /// pass has nothing to recognise and detecting is the last step it runs.
    #[test]
    fn progress_ends_at_the_count_checked() {
        let f = fixture(&[("a/plain.jpg", &jpeg(200, 150))]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(
            face_events(&f).last(),
            Some(&FaceProgress {
                phase: FacePhase::Detecting,
                checked: 1,
                total: 1,
                running: false
            })
        );
    }

    /// A request landing after `shutdown` has run its bounded wait must not spawn a
    /// thread that wait never accounted for.
    ///
    /// That nothing is detected does not show it: a pass that did start finds
    /// `shutting_down` set and leaves before its first photo. The thread's own traces do,
    /// the two `request_similar_pass_is_a_no_op_once_shutting_down` reads: the count, and
    /// `face_requested`, which a pass sets first thing and, with `face_pass` held here,
    /// cannot clear.
    #[test]
    fn a_face_pass_is_not_requested_once_shutting_down() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        f.engine.lib.set_face_detection(false).unwrap();
        f.engine.lib.set_face_detection(true).unwrap(); // every photo a candidate again
        f.engine.shutdown();

        let held = f.engine.face_pass.lock();
        f.engine.face_requested.store(false, Ordering::SeqCst);
        f.engine.request_face_pass();

        assert_eq!(
            f.engine.background_passes.load(Ordering::SeqCst),
            0,
            "a request after shutdown was let through, or left itself counted"
        );
        f.engine.wait_for_passes();
        assert!(
            !f.engine.face_requested.load(Ordering::SeqCst),
            "a request arriving after shutdown must not spawn a thread"
        );
        drop(held);
        assert_eq!(detected(&f), 0);
    }

    /// The scan's own request, which the drain cannot stand in for: a scan that finds
    /// every thumbnail already cached readies nothing, so the queue never drains and the
    /// drain never asks. Here the library was scanned and thumbnailed with the switch off,
    /// and the switch is then on in the database alone, as it is at the next launch: the
    /// one scan of the unchanged folder is all that can bring the pass.
    #[test]
    fn a_scan_with_every_thumbnail_cached_still_requests_a_face_pass() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        let ids = f.ids();
        f.engine.lib.set_face_detection(true).unwrap();

        let reopened =
            Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        assert!(reopened.face_detection(), "open did not read the setting");
        reopened.start_thumb_hashing(Duration::from_millis(20));
        let watched = reopened.lib.watched_folders().unwrap().remove(0);
        reopened.start_scan(watched);
        reopened.wait_for_scans();
        reopened.wait_for_passes();

        let detected: usize = ids
            .iter()
            .map(|id| reopened.lib.item_detected_faces(*id).unwrap().len())
            .sum();
        assert_eq!(detected, 1);
        reopened.shutdown();
    }

    /// The People page with up to a hundred faces of each group listed.
    fn people(f: &Fixture) -> photon_core::library::PeoplePage {
        f.engine.lib.people_page(100).unwrap()
    }

    /// The faces still to be embedded, by id: none once the pass has been.
    fn faces_to_embed(f: &Fixture) -> Vec<i64> {
        f.engine
            .lib
            .embed_candidates(0, 64, photon_core::face_embed::EMBEDDER_VERSION)
            .unwrap()
            .iter()
            .flat_map(|c| c.faces.iter().map(|(id, _)| *id))
            .collect()
    }

    /// The pass goes on from a face it found to the person it is: the face gets a vector,
    /// and the grouping step puts it in a group, of one, as the only face there is.
    #[test]
    fn a_detected_face_is_embedded_and_grouped() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.settle();

        assert_eq!(
            f.engine
                .lib
                .embed_progress(photon_core::face_embed::EMBEDDER_VERSION)
                .unwrap(),
            (1, 1),
            "the face was not embedded"
        );
        let page = people(&f);
        assert_eq!(page.single_count, 1, "{page:?}");
        assert_eq!(
            page.single_faces
                .iter()
                .map(|face| face.item_id)
                .collect::<Vec<_>>(),
            f.ids(),
            "{page:?}"
        );
        assert!(
            page.unnamed.is_empty() && page.people.is_empty(),
            "{page:?}"
        );
    }

    /// One face in two files, the second a smaller copy: the second is placed in the
    /// first's group, not a group of its own.
    #[test]
    fn two_photos_of_one_face_share_a_group() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/smaller.jpg", &portrait_jpeg_at(720)),
        ]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.settle();

        let page = people(&f);
        assert_eq!(page.unnamed.len(), 1, "{page:?}");
        assert_eq!(page.single_count, 0, "{page:?}");
        let group = &page.unnamed[0];
        assert_eq!(group.face_count, 2, "{page:?}");
        let mut items: Vec<i64> = group.faces.iter().map(|face| face.item_id).collect();
        items.sort_unstable();
        let mut ids = f.ids();
        ids.sort_unstable();
        assert_eq!(items, ids);
    }

    /// Grouping writes suggestions and unnamed groups, which `people_page` reads - not the
    /// People list or a person's view, which read confirmed faces only - and the People
    /// page refetches it on a data change: unlike detecting
    /// (`a_face_pass_does_not_announce_a_data_change`), the rebuild after it must send it
    /// to refetch.
    #[test]
    fn grouping_announces_a_data_change() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        // Anything pending from the scan is announced by this.
        f.engine.refresh_grid().unwrap();
        let before = f.events.all().len();

        f.engine.set_face_detection(true).unwrap();
        f.settle();

        assert_eq!(people(&f).single_count, 1, "nothing was grouped");
        let after: Vec<_> = f.events.all().split_off(before);
        let last = after.iter().rev().find_map(|e| match e {
            Recorded::Library(l) => Some(*l),
            _ => None,
        });
        assert!(
            last.is_some_and(|l| l.data_changed),
            "the pass's last rebuild did not announce a data change: {after:?}"
        );
    }

    /// A pass that embedded ends on that step: its last event counts faces recognised,
    /// running false. The step said it was running before that. A photo with no face
    /// beside the one with a face, so a count of photos (two) is not a count of faces (one).
    #[test]
    fn the_last_event_is_recognising_not_running() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/plain.jpg", &jpeg(200, 150)),
        ]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.settle();

        let events = face_events(&f);
        assert!(
            events
                .iter()
                .any(|e| e.phase == FacePhase::Recognising && e.running),
            "the step never said it was running: {events:?}"
        );
        assert_eq!(
            events.last(),
            Some(&FaceProgress {
                phase: FacePhase::Recognising,
                checked: 1,
                total: 1,
                running: false
            }),
            "{events:?}"
        );
    }

    /// "Not this person" takes a face out of its group, and nothing but a pass puts it
    /// anywhere else: the operation asks for one, whose grouping step places the face
    /// again, passing over the group it was taken from - here into a new group of its own.
    #[test]
    fn an_operation_requests_a_pass_that_regroups() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/smaller.jpg", &portrait_jpeg_at(720)),
        ]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.settle();
        let page = people(&f);
        assert_eq!(page.unnamed.len(), 1, "{page:?}");
        let group = page.unnamed[0].id;
        let (rejected, kept) = (page.unnamed[0].faces[0].id, page.unnamed[0].faces[1].id);

        f.engine
            .write_people("rejecting a face", |lib| lib.reject_faces(&[rejected]))
            .unwrap();
        f.settle();

        let page = people(&f);
        assert!(page.unnamed.is_empty(), "{page:?}");
        assert_eq!(
            page.single_count, 2,
            "the face was not placed again: {page:?}"
        );
        assert!(
            page.single_faces.iter().any(|face| face.id == rejected),
            "{page:?}"
        );
        let left: Vec<i64> = f
            .engine
            .lib
            .person_faces(group, photon_core::library::FaceFilter::All, 0, 10)
            .unwrap()
            .iter()
            .map(|face| face.id)
            .collect();
        assert_eq!(
            left,
            [kept],
            "the face went back into the group it was taken from"
        );
    }

    /// A correction is a data write like any other: the People list and a person's photos
    /// move with it, so it is announced as one at once, not left to a pass that may have
    /// nothing to do - naming a group leaves no face for the pass to place.
    #[test]
    fn a_correction_announces_a_data_change() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/smaller.jpg", &portrait_jpeg_at(720)),
        ]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.settle();
        let group = people(&f).unnamed[0].id;
        let before = f.events.all().len();

        let person = f
            .engine
            .write_people("naming a person", |lib| lib.name_group(group, "Ada"))
            .unwrap();

        let after: Vec<_> = f.events.all().split_off(before);
        assert!(
            after
                .iter()
                .any(|e| matches!(e, Recorded::Library(l) if l.data_changed)),
            "the correction was not announced: {after:?}"
        );
        f.settle();
        let page = people(&f);
        assert_eq!(page.people.len(), 1, "{page:?}");
        assert_eq!((page.people[0].id, page.people[0].face_count), (person, 2));
    }

    /// A library photon 0.47.0 detected has faces and no vectors. The pass takes those up
    /// without detecting again: the faces keep their ids (a detection rewrites a photo's
    /// rows, under new ids) and are embedded and grouped.
    #[test]
    fn a_library_detected_before_this_is_embedded() {
        use photon_core::face_detect::{DETECTOR_VERSION, Detector, pass};
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        // Detected as 0.47.0 did: the switch on in the database and the detection pass
        // alone, with the engine's mirror still off so no pass of its own runs.
        f.engine.lib.set_face_detection(true).unwrap();
        let detector = Detector::new().unwrap();
        pass::run(
            &f.engine.lib,
            &f.engine.cache,
            DETECTOR_VERSION,
            1,
            &|preview| detector.detect(preview),
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        let detected = faces_to_embed(&f);
        assert_eq!(detected.len(), 1, "the fixture's face was not detected");

        f.engine.set_face_detection(true).unwrap();
        f.settle();

        assert!(faces_to_embed(&f).is_empty(), "the face was not embedded");
        assert_eq!(
            f.engine.lib.face_progress(DETECTOR_VERSION).unwrap(),
            (1, 1)
        );
        let page = people(&f);
        assert_eq!(
            page.single_faces
                .iter()
                .map(|face| face.id)
                .collect::<Vec<_>>(),
            detected,
            "the face was detected again, or not grouped: {page:?}"
        );
    }

    /// Records, at the first report of the recognising step that counts a face done, how
    /// many groups of one the People page has then: what the grouping step had placed by
    /// the end of that batch.
    #[derive(Default)]
    struct PageAtFirstEmbedReport {
        engine: std::sync::OnceLock<std::sync::Weak<Engine>>,
        singles: Mutex<Option<i64>>,
    }

    impl Events for PageAtFirstEmbedReport {
        fn library_changed(&self, _: LibraryChanged) {}
        fn scan_progress(&self, _: ScanProgressEvent) {}
        fn folder_status(&self, _: FolderStatus) {}
        fn export_progress(&self, _: ExportProgress) {}
        fn face_progress(&self, e: FaceProgress) {
            if e.phase != FacePhase::Recognising || !e.running || e.checked == 0 {
                return;
            }
            let mut singles = self.singles.lock();
            if singles.is_none()
                && let Some(engine) = self.engine.get().and_then(|e| e.upgrade())
            {
                *singles = Some(engine.lib.people_page(1).unwrap().single_count);
            }
        }
    }

    /// Each batch embedded is grouped as it lands, not at the end of the pass: on a large
    /// library the embedding runs for hours, and the People page fills in as it goes.
    #[test]
    fn faces_are_grouped_as_each_batch_is_embedded() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.shutdown();

        let sink = Arc::new(PageAtFirstEmbedReport::default());
        let engine = Engine::open(f.config(), sink.clone()).unwrap();
        sink.engine.set(Arc::downgrade(&engine)).ok().unwrap();
        engine.set_face_detection(true).unwrap();
        engine.wait_for_passes();

        assert_eq!(
            *sink.singles.lock(),
            Some(1),
            "the batch's face was not grouped by the time its report went out"
        );
        engine.shutdown();
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
        f.settle();

        // Simulate a render that failed transiently: the item is pending again, but a
        // rescan finds nothing changed on disk.
        let id = f.ids()[0];
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Pending, None)
            .unwrap();

        assert!(f.engine.start_scan(watched.clone()));
        f.settle();

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
    fn a_moved_photos_thumbnails_are_carried_to_its_new_key() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        let before = f.engine.lib.item(id).unwrap().unwrap();
        assert_eq!(before.thumb_state, ThumbState::Ready);
        let key_before = before.thumb_key();
        assert!(f.engine.cache.is_complete(key_before));
        let bytes = std::fs::read(f.engine.cache.path_for(key_before, ThumbSize::Preview)).unwrap();

        let watched = f.engine.lib.watched_folders().unwrap().remove(0);
        std::fs::rename(f.photos.join("a/one.jpg"), f.photos.join("a/two.jpg")).unwrap();
        assert!(f.engine.start_scan(watched));
        f.engine.wait_for_scans();
        // The move left the row pending under its new key. The worker has to find the
        // carried files there and say so, or the photo stays a placeholder in the grid.
        f.engine.thumbs.wait_idle();

        // Nothing but the carry-over removes a file from the old key, whatever the worker
        // does with the row meanwhile; and a render would not leave the old key behind.
        let after = f.engine.lib.item(id).unwrap().unwrap();
        assert_eq!(after.thumb_state, ThumbState::Ready);
        let key_after = after.thumb_key();
        assert_ne!(key_after, key_before);
        assert!(f.engine.cache.is_complete(key_after));
        for size in ThumbSize::ALL {
            assert!(!f.engine.cache.path_for(key_before, size).exists());
        }
        // The very file, not a second render of the same picture.
        assert_eq!(
            std::fs::read(f.engine.cache.path_for(key_after, ThumbSize::Preview)).unwrap(),
            bytes
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
        // The current version need not be the scan's own rebuild: the two photos share a
        // byte size, so the duplicate pass after it hashes them and rebuilds again, as a
        // derived rebuild (`refresh_grid_derived`) that is no data change.
        assert!(events.iter().any(|e| matches!(e,
            Recorded::Library(LibraryChanged { version: v, len: 2, .. }) if *v == version)));
        assert!(events.iter().any(|e| matches!(
            e,
            Recorded::Library(LibraryChanged {
                len: 2,
                data_changed: true,
                ..
            })
        )));
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
        f.settle();
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
        f.settle();

        assert_eq!(
            f.engine.grid().0,
            version,
            "a scan that changed nothing must not rebuild the grid"
        );
    }

    #[test]
    fn removing_a_folder_forgets_its_scan_time() {
        let f = fixture(&[("a.jpg", &jpeg(16, 16))]);
        let watched = f.add_photos();
        assert!(f.engine.last_full_scan(watched.id).is_some());
        f.engine.remove_folder(watched.id).unwrap();
        assert!(f.engine.last_full_scan(watched.id).is_none());
    }

    /// Only a whole-root scan that ran to the end is timed for the watcher's pacing of
    /// degraded rescans: a subtree scan, a cancelled one and one that found the root offline
    /// all leave the last good time in place, since each is far quicker than walking the
    /// root and would shrink the wait back to the floor.
    #[test]
    fn only_a_complete_full_scan_of_an_online_root_is_timed() {
        let f = fixture(&[("a/one.jpg", &jpeg(16, 16))]);
        let watched = f.add_photos();
        let first = f
            .engine
            .last_full_scan(watched.id)
            .expect("the first full scan is timed")
            .finished;
        let finished = || f.engine.last_full_scan(watched.id).unwrap().finished;

        assert!(
            f.engine
                .start_subtree_scan(watched.clone(), f.photos.join("a"))
        );
        f.settle();
        assert_eq!(finished(), first, "a subtree scan is not timed");

        f.engine
            .run_scan(&watched, None, Arc::new(AtomicBool::new(true)));
        assert_eq!(finished(), first, "a cancelled scan is not timed");

        std::fs::remove_dir_all(&f.photos).unwrap();
        assert!(f.engine.start_scan(watched.clone()));
        f.settle();
        assert_eq!(
            finished(),
            first,
            "a scan that finds the root offline is not timed"
        );

        std::fs::create_dir_all(f.photos.join("a")).unwrap();
        std::fs::write(f.photos.join("a").join("one.jpg"), jpeg(16, 16)).unwrap();
        let back = f
            .engine
            .lib
            .watched_folders()
            .unwrap()
            .into_iter()
            .find(|w| w.id == watched.id)
            .unwrap();
        assert!(f.engine.start_scan(back));
        f.settle();
        assert!(finished() > first, "the next complete one is");
    }

    /// The same 30-second poll must not run the two library-wide sweeps either: a whole
    /// library's look-alike regroup, and a sort of every pending thumbnail, twice a minute
    /// for as long as a drive stays unplugged, having read nothing.
    ///
    /// Seen through work waiting in *another*, online root, which both sweeps would do: two
    /// files of one size and different bytes (candidates for the byte-identical hash), and
    /// thumbnails never queued. They are indexed by the scanner directly, not through the
    /// engine, so no pass and no queue has seen them before the poll runs. The last scan
    /// shows the work was there to be done, so "still undone" is the poll's doing.
    #[test]
    fn a_poll_of_a_root_still_offline_runs_no_post_scan_sweep() {
        let f = fixture(&[("a.jpg", &jpeg(16, 16))]);
        let away = f.add_photos();
        std::fs::remove_dir_all(&f.photos).unwrap();
        f.engine.start_scan(away.clone());
        f.settle();
        let away = f
            .engine
            .lib
            .watched_folders()
            .unwrap()
            .into_iter()
            .find(|w| w.id == away.id)
            .unwrap();
        assert!(!away.online);

        let other = f.dir.path().join("more-photos");
        std::fs::create_dir_all(&other).unwrap();
        // Different bytes after the end-of-image marker: one size, two contents, and both
        // still decode, so their thumbnails can be rendered.
        for (name, tail) in [("one.jpg", b"one"), ("two.jpg", b"two")] {
            let mut bytes = jpeg(16, 16);
            bytes.extend_from_slice(tail);
            std::fs::write(other.join(name), bytes).unwrap();
        }
        let online = f
            .engine
            .lib
            .add_watched_folder(&other, f.engine.excluded())
            .unwrap();
        photon_core::scanner::scan_watched(
            &f.engine.lib,
            &online,
            now_ms(),
            &ScanOptions::default(),
            &mut photon_core::scanner::progress_only(|_| {}),
        )
        .unwrap();
        let waiting: Vec<i64> = f
            .engine
            .lib
            .hash_candidates()
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(waiting.len(), 2, "the fixture left nothing to hash");

        f.engine.start_scan(away);
        f.settle();
        f.engine.thumbs.wait_idle();

        assert_eq!(
            f.engine.lib.hash_candidates().unwrap().len(),
            2,
            "a poll of a root still offline ran the duplicate pass"
        );
        for &id in &waiting {
            assert_eq!(
                f.engine.lib.item(id).unwrap().unwrap().thumb_state,
                ThumbState::Pending,
                "a poll of a root still offline swept the pending thumbnails"
            );
        }

        // Both are real work a real scan does: the fixture is not merely unhashable.
        f.engine.start_scan(online);
        f.settle();
        f.engine.thumbs.wait_idle();
        assert!(f.engine.lib.hash_candidates().unwrap().is_empty());
        for &id in &waiting {
            assert_eq!(
                f.engine.lib.item(id).unwrap().unwrap().thumb_state,
                ThumbState::Ready
            );
        }
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
        f.settle();

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
    fn aliasing_a_folder_rebuilds_the_grid_and_search_follows() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let folder = f.engine.lib.folders().unwrap()[0].id;
        f.engine.set_search_query("easter").unwrap();
        let before = crate::commands::grid_info(&f.engine, None);
        assert_eq!(before.len, 0);

        assert!(f.engine.set_folder_alias(folder, Some("Easter")).unwrap());
        let after = crate::commands::grid_info(&f.engine, None);
        assert_eq!(after.len, 2, "an open search did not follow the alias");
        assert!(after.version > before.version);
        let listed = crate::commands::list_folders(&f.engine).unwrap();
        assert!(
            listed
                .folders
                .iter()
                .any(|x| x.id == folder && x.alias.as_deref() == Some("Easter"))
        );

        assert!(!f.engine.set_folder_alias(folder, Some("Easter")).unwrap());
        assert_eq!(
            crate::commands::grid_info(&f.engine, None).version,
            after.version,
            "an unchanged alias rebuilt the grid"
        );
    }

    #[test]
    fn hiding_a_folder_rebuilds_the_grid_and_reports_its_flag() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let folder = f.engine.lib.folders().unwrap()[0].id;
        let before = crate::commands::grid_info(&f.engine, None);

        assert_eq!(f.engine.set_folder_hidden(folder, true).unwrap(), 2);
        let after = crate::commands::grid_info(&f.engine, None);
        assert_eq!((after.len, after.hidden_count), (0, 2));
        assert!(after.version > before.version);
        let listed = crate::commands::list_folders(&f.engine).unwrap();
        assert!(listed.folders.iter().any(|x| x.id == folder && x.hidden));

        assert_eq!(f.engine.set_folder_hidden(folder, true).unwrap(), 0);
        assert_eq!(
            crate::commands::grid_info(&f.engine, None).version,
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
        let before = crate::commands::grid_info(&f.engine, None);
        assert_eq!((before.len, before.hidden_count), (2, 0));

        assert_eq!(f.engine.set_items_hidden(&[id], true).unwrap(), 1);
        let after = crate::commands::grid_info(&f.engine, None);
        assert_eq!((after.len, after.hidden_count), (1, 1));
        assert!(after.version > before.version);

        assert_eq!(f.engine.set_items_hidden(&[id], true).unwrap(), 0);
        assert_eq!(
            crate::commands::grid_info(&f.engine, None).version,
            after.version,
            "hiding a hidden photo rebuilt the grid"
        );

        f.engine.set_view(GridView::Hidden).unwrap();
        let hidden = crate::commands::grid_info(&f.engine, None);
        assert_eq!((hidden.view, hidden.len), (GridView::Hidden, 1));
    }

    /// Makes every grid rebuild fail from here on while the writers below still work: every
    /// grid query names `file_name`, and none of those writers does. Returns a connection
    /// for reading back what a write committed.
    fn break_grid_rebuilds(f: &Fixture) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(&f.config().db_path).unwrap();
        db.execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();
        assert!(
            f.engine.refresh_grid().is_err(),
            "the grid still rebuilds, so nothing below would test a failed one"
        );
        db
    }

    fn item_column(db: &rusqlite::Connection, column: &str, id: i64) -> i64 {
        db.query_row(
            &format!("SELECT {column} FROM items WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    // Each writer below commits before it rebuilds, so a rebuild that fails has not undone
    // the write (`refresh_after_write`). Reported as an error, the UI undid its own side of
    // a write that stands: a star or a keyword flipped back, a selection kept for a retry
    // with nothing left to do, the folder or album list not refetched.

    #[test]
    fn a_hide_whose_rebuild_fails_still_reports_the_photos_it_hid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let db = break_grid_rebuilds(&f);

        assert_eq!(f.engine.set_items_hidden(&[id], true).unwrap(), 1);
        assert_eq!(item_column(&db, "hidden", id), 1);
    }

    #[test]
    fn a_star_whose_rebuild_fails_is_reported_as_the_star_it_was() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let db = break_grid_rebuilds(&f);

        f.engine.set_star(id, true).unwrap();
        assert_eq!(item_column(&db, "rating", id), 1);
    }

    #[test]
    fn stars_whose_rebuild_fails_still_report_how_many_landed() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let db = break_grid_rebuilds(&f);

        assert_eq!(f.engine.set_stars(&ids, true).unwrap(), 2);
        assert_eq!(item_column(&db, "rating", ids[1]), 1);
    }

    #[test]
    fn a_keyword_whose_rebuild_fails_still_reports_the_name_stored() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        break_grid_rebuilds(&f);

        assert_eq!(f.engine.add_item_tag(id, "beach").unwrap(), "beach");
        assert_eq!(f.engine.lib.item_tags(id).unwrap(), ["beach"]);
    }

    #[test]
    fn a_keyword_removal_whose_rebuild_fails_is_reported_as_done() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        f.engine.add_item_tag(id, "beach").unwrap();
        break_grid_rebuilds(&f);

        f.engine.remove_item_tag(id, "beach").unwrap();
        assert!(f.engine.lib.item_tags(id).unwrap().is_empty());
    }

    #[test]
    fn a_bulk_keyword_whose_rebuild_fails_still_reports_how_many_took_it() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        break_grid_rebuilds(&f);

        assert_eq!(
            f.engine.add_items_tag(&ids, "beach").unwrap(),
            ("beach".to_string(), 2)
        );
        assert_eq!(f.engine.lib.item_tags(ids[1]).unwrap(), ["beach"]);
    }

    #[test]
    fn a_bulk_keyword_removal_whose_rebuild_fails_still_reports_how_many_lost_it() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        f.engine.add_items_tag(&ids, "beach").unwrap();
        break_grid_rebuilds(&f);

        assert_eq!(f.engine.remove_items_tag(&ids, "beach").unwrap(), 2);
        assert!(f.engine.lib.item_tags(ids[1]).unwrap().is_empty());
    }

    #[test]
    fn a_folder_hide_whose_rebuild_fails_still_reports_the_photos_it_hid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let folder = f.engine.lib.folders().unwrap()[0].id;
        let ids = f.ids();
        let db = break_grid_rebuilds(&f);

        assert_eq!(f.engine.set_folder_hidden(folder, true).unwrap(), 2);
        assert_eq!(item_column(&db, "hidden", ids[1]), 1);
    }

    #[test]
    fn a_folder_name_whose_rebuild_fails_is_reported_as_changed() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let folder = f.engine.lib.folders().unwrap()[0].id;
        let db = break_grid_rebuilds(&f);

        assert!(f.engine.set_folder_alias(folder, Some("Easter")).unwrap());
        let alias: String = db
            .query_row("SELECT alias FROM folders WHERE id = ?1", [folder], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(alias, "Easter");
    }

    /// Through the command, which is where `albums_changed` is called: an error there kept
    /// the UI from refetching the album list and put the info panel's checkbox back.
    #[test]
    fn an_album_change_whose_rebuild_fails_is_reported_as_done() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let album = f.engine.lib.create_album("Trip", 1).unwrap();
        // Only the album on screen is rebuilt for.
        f.engine.set_album_view(album.id).unwrap();
        let db = break_grid_rebuilds(&f);

        crate::commands::add_to_album(&f.engine, album.id, &ids).unwrap();
        let members: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM album_items WHERE album_id = ?1",
                [album.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(members, 2);
    }

    #[test]
    fn an_edit_whose_rebuild_fails_is_reported_as_the_edit_it_was() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let db = break_grid_rebuilds(&f);

        f.engine.rotate_item(id, true).unwrap();
        assert_eq!(item_column(&db, "edit_turns", id), 1);
    }

    #[test]
    fn a_folder_removal_whose_rebuild_fails_is_reported_as_done() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        break_grid_rebuilds(&f);

        f.engine.remove_folder(watched.id).unwrap();
        assert!(f.engine.lib.watched_folders().unwrap().is_empty());
    }

    /// Logging the failure is safe only because the write still reaches the UI: the failed
    /// rebuild marked `data_dirty` before it built, so the next rebuild that publishes -
    /// here a view switch, which marks nothing of its own - sends the UI to refetch the
    /// tag, album and folder counts the write moved.
    #[test]
    fn a_write_whose_rebuild_fails_is_announced_by_the_next_publish() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        f.settle();
        let id = f.ids()[0];
        f.engine.set_view(GridView::Starred).unwrap();
        assert!(!last_data_changed(&f), "a view switch");
        // Not `break_grid_rebuilds`: its own `refresh_grid` would mark the flag.
        let db = rusqlite::Connection::open(f.config().db_path).unwrap();
        db.execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();

        f.engine.add_item_tag(id, "beach").unwrap();
        db.execute_batch("ALTER TABLE items RENAME COLUMN renamed TO file_name")
            .unwrap();
        f.engine.set_view(GridView::All).unwrap();
        assert!(
            last_data_changed(&f),
            "the keyword's failed rebuild left no data change for the next publish"
        );
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
        let info = crate::commands::grid_info(&f.engine, None);
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
        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!(info.starred_count, 3);
        assert_eq!(
            f.engine.grid().0,
            version + 1,
            "one refresh for the whole batch, not one per photo"
        );
    }

    /// The promise the whole feature has to keep: the photos it copies are not touched.
    /// An export at full size: what every export was before a size could be asked for.
    fn full_size(apply_edits: bool) -> Options {
        Options {
            apply_edits,
            max_edge: None,
        }
    }

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

        let report = f.engine.export_items(&ids, &out, full_size(true)).unwrap();

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
    /// The size travels from the options to the copies, read from the library's own
    /// dimensions: the large photo is scaled, the small one keeps its bytes.
    #[test]
    fn an_export_with_a_size_scales_only_the_photos_over_it() {
        let (big, small) = (jpeg(64, 32), jpeg(16, 16));
        let f = fixture(&[("big.jpg", &big), ("small.jpg", &small)]);
        f.add_photos();
        let out = f.dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let options = Options {
            apply_edits: true,
            max_edge: Some(32),
        };
        let report = f.engine.export_items(&f.ids(), &out, options).unwrap();
        assert_eq!((report.written, report.failed), (2, 0));
        let scaled = image::open(out.join("big.jpg")).unwrap();
        assert_eq!((scaled.width(), scaled.height()), (32, 16));
        assert_eq!(std::fs::read(out.join("small.jpg")).unwrap(), small);
        assert_eq!(std::fs::read(f.photos.join("big.jpg")).unwrap(), big);
    }

    #[test]
    fn exporting_into_a_watched_folder_is_refused_and_writes_nothing() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();

        for dest in [f.photos.clone(), f.photos.join("sub")] {
            let err = f
                .engine
                .export_items(&ids, &dest, full_size(false))
                .unwrap_err();
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
            f.engine
                .export_items(&ids, &above, full_size(false))
                .unwrap()
                .written,
            2
        );
        let parent = f.photos.parent().unwrap().to_path_buf();
        assert!(
            f.engine
                .export_items(&ids[..1], &parent, full_size(false))
                .is_ok(),
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
            .export_items(&[ids[0], 9_999, ids[1]], &out, full_size(false))
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

        f.engine.export_items(&ids, &out, full_size(false)).unwrap();

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
        f.settle();

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

    /// A sort by anything but date is about the photos, not the folders: the grid is one
    /// flat run in the key's order, a folder jump lands on the folder's first photo in it,
    /// and the choice outlives the process.
    #[test]
    fn a_sort_by_name_lays_the_grid_out_flat_and_is_remembered() {
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[
            ("a/Cove.jpg", &img),
            ("a/apple.jpg", &img),
            ("b/beach.jpg", &img),
        ]);
        f.add_photos();
        let name_of = |id: i64| {
            let path = f.engine.lib.item(id).unwrap().unwrap().path;
            Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        let by_name = Sort {
            key: SortKey::Name,
            reverse: false,
        };

        f.engine.set_sort(by_name).unwrap();

        let names: Vec<String> = f.ids().into_iter().map(name_of).collect();
        assert_eq!(names, ["apple.jpg", "beach.jpg", "Cove.jpg"]);
        let (_, grid) = f.engine.grid();
        assert_eq!(grid.sections().len(), 1);
        assert_eq!(grid.sections()[0].folder_id, None);
        let b = grid.rows(1, 1)[0].folder_id;
        assert_eq!(grid.offset_of_folder(b), Some(1));
        assert_eq!(crate::commands::grid_info(&f.engine, None).sort, by_name);

        let reopened =
            Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        assert_eq!(reopened.sort(), by_name);
        // The first grid is `startup`'s to build, in the remembered sort.
        reopened.startup(None);
        reopened.wait_for_startup();
        assert_eq!(reopened.grid().1.sections()[0].folder_id, None);
        reopened.shutdown();
    }

    /// The rollback restores the whole view state, the sort included, and a sort that
    /// could not be shown is not stored for the next launch to trip over.
    #[test]
    fn a_sort_whose_rebuild_fails_is_rolled_back_and_not_remembered() {
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        // Every grid query names the column, so every rebuild from here on fails.
        rusqlite::Connection::open(&f.config().db_path)
            .unwrap()
            .execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();

        let by_size = Sort {
            key: SortKey::Size,
            reverse: true,
        };
        assert!(f.engine.set_sort(by_size).is_err());
        assert_eq!(f.engine.sort(), Sort::default());
        assert_eq!(f.engine.lib.grid_sort().unwrap(), Sort::default());
    }

    /// `open` runs on the main thread before the window can draw, so it leaves the first
    /// grid - a read of the whole library - to `startup`, and says so with the version.
    /// Startup's scan of the unchanged folder moves no rows and so rebuilds nothing: the
    /// grid that arrives is startup's own build.
    #[test]
    fn open_leaves_the_first_grid_to_startup() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events.clone()).unwrap();
        let (version, grid) = reopened.grid();
        assert_eq!(version, NOT_BUILT);
        assert_eq!(grid.len(), 0);

        reopened.startup(None);
        reopened.wait_for_startup();
        let (version, grid) = reopened.grid();
        assert!(version > NOT_BUILT);
        assert_eq!(grid.len(), 1);
        // Announced like any rebuild, and as a change to the data, so the UI fetches the
        // sidebar's collections with it.
        assert!(events.all().iter().any(|e| matches!(
            e,
            Recorded::Library(LibraryChanged { version: v, len: 1, data_changed: true })
                if *v == version
        )));
        reopened.shutdown();
    }

    /// Left at `NOT_BUILT`, the window drew nothing at all - no photos, no notice, no
    /// count - until a view switch.
    #[test]
    fn a_first_grid_that_cannot_be_built_is_published_empty_rather_than_left_unbuilt() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events.clone()).unwrap();
        // After `open`'s check, so the query compiles there and fails every time here.
        rusqlite::Connection::open(&f.config().db_path)
            .unwrap()
            .execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();

        reopened.build_first_grid(&[Duration::ZERO, Duration::ZERO]);

        let (version, grid) = reopened.grid();
        assert!(version > NOT_BUILT);
        assert_eq!(grid.len(), 0);
        assert!(events.all().iter().any(|e| matches!(
            e,
            Recorded::Library(LibraryChanged { version: v, len: 0, .. }) if *v == version
        )));
        reopened.shutdown();
    }

    /// The empty stand-in says why it is empty: shown as "No photos yet", a failed read of a
    /// library full of photos sent the user off to add a folder they already had. And only
    /// for as long as it is true - the next build that succeeds clears it.
    #[test]
    fn a_failed_first_build_reports_its_error_until_a_build_succeeds() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events).unwrap();
        let db = rusqlite::Connection::open(&f.config().db_path).unwrap();
        db.execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();

        reopened.build_first_grid(&[Duration::ZERO, Duration::ZERO]);

        let info = crate::commands::grid_info(&reopened, None);
        assert_eq!(info.len, 0);
        let error = info
            .build_error
            .expect("the failure is reported with the grid");
        assert!(
            error.contains("file_name"),
            "the database's own reason: {error}"
        );

        db.execute_batch("ALTER TABLE items RENAME COLUMN renamed TO file_name")
            .unwrap();
        reopened.refresh_grid().unwrap();
        let info = crate::commands::grid_info(&reopened, None);
        assert_eq!((info.len, info.build_error), (1, None));
        reopened.shutdown();
    }

    /// A grid built while the first build was still retrying - a view switch the UI made
    /// meanwhile - read the library successfully. The failure landing after it must not
    /// replace it with an empty grid claiming the library could not be read.
    #[test]
    fn a_failed_first_build_does_not_replace_a_grid_built_meanwhile() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events).unwrap();
        reopened.set_view(GridView::All).unwrap();
        let (version, _) = reopened.grid();
        rusqlite::Connection::open(&f.config().db_path)
            .unwrap()
            .execute_batch("ALTER TABLE items RENAME COLUMN file_name TO renamed")
            .unwrap();

        reopened.build_first_grid(&[Duration::ZERO, Duration::ZERO]);

        let info = crate::commands::grid_info(&reopened, None);
        assert_eq!(
            (info.version, info.len, info.build_error),
            (version, 1, None)
        );
        reopened.shutdown();
    }

    /// The other order. A view switch made while the first build was retrying snapshots
    /// before the retries give up, so the stand-in is stamped after it; if the stand-in
    /// publishes first and takes the stamp, the switch's rebuild - which read the library
    /// successfully - is dropped as overtaken, and "could not read the library" stays up
    /// over a library that can be read until something else rebuilds.
    #[test]
    fn a_failed_first_build_does_not_block_a_grid_read_before_it() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events).unwrap();

        // The switch's rebuild reads the library...
        let switch = reopened.snapshot();
        let switch_index = Arc::new(reopened.build_index(&switch.state).unwrap());
        // ...and the first build gives up, stamped later, and publishes first.
        let stand_in = reopened.data_snapshot();
        let empty = GridIndex::build(Vec::new(), stand_in.state.sort.layout(stand_in.state.view));
        reopened.publish(Arc::new(empty), &stand_in, Some("busy".into()));
        assert!(
            crate::commands::grid_info(&reopened, None)
                .build_error
                .is_some()
        );

        assert!(matches!(
            reopened.publish_if_current(switch_index, &switch),
            Publish::Published(_)
        ));
        let info = crate::commands::grid_info(&reopened, None);
        assert_eq!((info.len, info.build_error), (1, None));
        reopened.shutdown();
    }

    /// The stand-in says the data changed - the window has nothing yet, so the UI must
    /// fetch its collections - but it read nothing, and the collections it sends the UI to
    /// refetch sit in the same database the build could not read. It must leave the flag
    /// for the first publish that did read: a view switch's rebuild, which marks nothing
    /// itself, took it `false` and the sidebar stayed empty until a scan moved rows.
    #[test]
    fn a_failed_first_build_leaves_the_data_change_for_the_build_that_succeeds() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let events = Arc::new(crate::events::Recorder::default());
        let reopened = Engine::open(f.config(), events.clone()).unwrap();

        let stand_in = reopened.data_snapshot();
        let empty = GridIndex::build(Vec::new(), stand_in.state.sort.layout(stand_in.state.view));
        reopened.publish(Arc::new(empty), &stand_in, Some("busy".into()));
        // A view switch's rebuild, which does not mark the data dirty itself.
        let switch = reopened.snapshot();
        let switch_index = Arc::new(reopened.build_index(&switch.state).unwrap());
        assert!(matches!(
            reopened.publish_if_current(switch_index, &switch),
            Publish::Published(_)
        ));

        let flags: Vec<bool> = events
            .all()
            .iter()
            .filter_map(|e| match e {
                Recorded::Library(e) => Some(e.data_changed),
                _ => None,
            })
            .collect();
        assert_eq!(flags, [true, true]);
        reopened.shutdown();
    }

    /// Only the first attempts may fail: once one succeeds, nothing is tried again, and
    /// its answer is the one returned.
    #[test]
    fn retry_after_retries_a_failure_once_per_wait_then_gives_up() {
        let fail = || Err(Error::ThumbFailed("busy".into()));

        let mut calls = 0;
        let result = retry_after(
            &[Duration::ZERO; 3],
            || false,
            || {
                calls += 1;
                if calls < 3 { fail() } else { Ok(()) }
            },
        );
        assert!(result.is_ok());
        assert_eq!(calls, 3);

        let mut calls = 0;
        let result = retry_after(
            &[Duration::ZERO; 2],
            || false,
            || {
                calls += 1;
                fail()
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 3);

        // Shutting down: no second attempt.
        let mut calls = 0;
        let result = retry_after(
            &[Duration::ZERO; 2],
            || true,
            || {
                calls += 1;
                fail()
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    /// Building the first grid in `open` also refused a library the grid query could not
    /// run against, with the error dialog; leaving the build to `startup` must not lose that.
    #[test]
    fn open_refuses_a_library_the_grid_query_cannot_run_against() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        f.engine.shutdown();
        rusqlite::Connection::open(&f.config().db_path)
            .unwrap()
            .execute_batch("ALTER TABLE items RENAME COLUMN edit_crop TO renamed")
            .unwrap();
        assert!(Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).is_err());
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

    /// Parks the duplicate and look-alike pass that follows a scan, at its grid rebuild:
    /// armed by a scan reporting done, it holds the next `library_changed` sent from a scan
    /// or pass thread until the test lets it go. The thread names keep a thumbnail worker's
    /// rebuild from being the one parked in its place, which would let the pass run to the
    /// end and prove nothing.
    #[derive(Default)]
    struct ParkedPass {
        armed: AtomicBool,
        parked: Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl Events for ParkedPass {
        fn library_changed(&self, _: LibraryChanged) {
            let name = std::thread::current().name().unwrap_or_default().to_owned();
            let on_pass = name.starts_with("photon-scan-") || name == "photon-similar-pass";
            if on_pass && self.armed.swap(false, Ordering::SeqCst) {
                let release = self.release.lock().take().unwrap();
                let _ = self.parked.lock().take().unwrap().send(());
                // Nothing is ever sent: this returns when the test drops its sender.
                let _ = release.recv();
            }
        }
        fn scan_progress(&self, event: ScanProgressEvent) {
            if event.done {
                self.armed.store(true, Ordering::SeqCst);
            }
        }
        fn folder_status(&self, _: FolderStatus) {}
        fn export_progress(&self, _: ExportProgress) {}
        fn face_progress(&self, _: FaceProgress) {}
    }

    /// The watcher is started once the startup scans are done, and "done" is the scan slot
    /// being released. The slot used to be held through the duplicate and look-alike pass
    /// as well, so on a large library the watcher waited for the session's first
    /// whole-library pass, and a file changed in that window was not seen until something
    /// rescanned its directory. Two byte-identical photos give the pass a rebuild to park
    /// at; while it is parked, the watcher must already be running.
    #[test]
    fn startup_starts_the_watcher_without_waiting_for_the_pass_after_its_scans() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        let sink = Arc::new(ParkedPass::default());
        let (parked_tx, parked) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel::<()>();
        *sink.parked.lock() = Some(parked_tx);
        *sink.release.lock() = Some(release_rx);
        let engine = Engine::open(f.config(), sink.clone()).unwrap();
        engine
            .lib
            .add_watched_folder(&f.photos, engine.excluded())
            .unwrap();
        // Taken first, with a settle nothing reaches, so the only pass is the scan's own:
        // one requested by the thumbnail queue going quiet would take `hashing` from it,
        // and a scan finding `hashing` held returns at once, whatever the order.
        engine.start_thumb_hashing(Duration::from_secs(3600));

        engine.startup(None);
        parked
            .recv_timeout(Duration::from_secs(10))
            .expect("the pass after the startup scan never rebuilt the grid");

        let started = Instant::now();
        while engine.watcher_service().is_none() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let watching = engine.watcher_service().is_some();
        drop(release);
        engine.wait_for_startup();
        engine.shutdown();
        assert!(
            watching,
            "the watcher waited for the pass that follows the startup scan"
        );
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

        f.settle();
    }

    #[test]
    fn an_ini_pass_applies_the_ini_under_the_scan_slot() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let dir = PathBuf::from(&watched.path).join("a");
        photon_core::picasa::set_star(&dir, "one.jpg", true).unwrap();

        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        assert!(
            !f.engine.start_ini_pass(watched.clone(), vec![dir.clone()]),
            "the slot is taken"
        );
        drop(slot);
        assert!(f.engine.start_ini_pass(watched, vec![dir]));
        f.settle();

        assert_eq!(f.engine.counts().starred, 1);
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
        f.settle();
        f.engine.lib.set_ratings(&[(f.ids()[0], 2)]).unwrap();

        // A scan's rebuild reads the state and starts querying for All...
        let stale = f.engine.snapshot();
        let stale_index = Arc::new(f.engine.build_index(&stale.state).unwrap());
        assert_eq!(stale_index.len(), 2);
        // ...and while it does, the user clicks Starred, whose rebuild lands first.
        f.engine.set_view(GridView::Starred).unwrap();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 1);

        assert_eq!(
            f.engine.publish_if_current(stale_index, &stale),
            Publish::Superseded,
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
        f.settle();
        f.engine.set_search_query("beach").unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);
        let version = f.engine.grid().0;

        // A photo arrives that the query matches, and a scan picks it up.
        std::fs::write(f.photos.join("a").join("beach hut.jpg"), &img).unwrap();
        f.engine.start_scan(watched);
        f.settle();

        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!(
            (info.view, info.search_query.as_str()),
            (GridView::Search, "beach"),
            "the scan must not move the view or lose the query"
        );
        assert_eq!(info.len, 2, "the new match is in the rebuilt search");
        assert!(f.engine.grid().0 > version, "and the UI was told");
        // The sidebar reads the same index, so its sections describe the filtered set.
        assert_eq!(info.layout.as_ref().unwrap().sections.len(), 1);
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
        f.settle();
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

            assert_eq!(
                f.engine.publish_if_current(stale_index, &stale),
                Publish::Superseded,
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

    /// The `data_changed` of every `library_changed` so far, oldest first.
    fn data_changed_flags(f: &Fixture) -> Vec<bool> {
        f.events
            .all()
            .iter()
            .filter_map(|e| match e {
                Recorded::Library(e) => Some(e.data_changed),
                _ => None,
            })
            .collect()
    }

    /// The `data_changed` of the most recent `library_changed`.
    fn last_data_changed(f: &Fixture) -> bool {
        *data_changed_flags(f)
            .last()
            .expect("a library_changed was sent")
    }

    /// A view switch, a sort change and a search keystroke rebuild the grid and change no
    /// data, so their events must not send the UI to refetch the sidebar's collections:
    /// that refetch is the slowest thing a rebuild triggers (the tag list alone is
    /// ~220ms at 300k photos with three keywords each), and every keystroke of a search
    /// would pay for it.
    #[test]
    fn a_view_change_does_not_announce_a_data_change() {
        use photon_core::grid::GridView;
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.settle();
        // Not the last event: the two photos share a byte size, so the duplicate pass
        // after the scan hashes them and publishes a rebuild of its own, which is not a
        // data change to the collections.
        assert!(
            data_changed_flags(&f).contains(&true),
            "the scan's own rebuild changed data"
        );

        f.engine.set_view(GridView::Starred).unwrap();
        assert!(!last_data_changed(&f), "a view switch");
        f.engine.set_search_query("beach").unwrap();
        assert!(!last_data_changed(&f), "a search");
        f.engine
            .set_sort(Sort {
                key: SortKey::Name,
                reverse: true,
            })
            .unwrap();
        assert!(!last_data_changed(&f), "a sort");
    }

    /// The `version` of the most recent `library_changed`.
    fn last_announced(f: &Fixture) -> u64 {
        f.events
            .all()
            .iter()
            .rev()
            .find_map(|e| match e {
                Recorded::Library(e) => Some(e.version),
                _ => None,
            })
            .expect("a library_changed was sent")
    }

    /// Every view setter answers with the version its own rebuild published - the one its
    /// `library_changed` announced. The UI skips its refetch when it already holds that
    /// version, because the event's listener got there first; a version from before the
    /// publish would let it skip while still showing the previous view.
    #[test]
    fn a_view_setter_answers_with_the_version_it_published() {
        use photon_core::grid::GridView;
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/beach.jpg", &img), ("a/mountain.jpg", &img)]);
        f.add_photos();
        f.settle();
        let album = f.engine.lib.create_album("trip", 0).unwrap();
        let photo = f.ids()[0];

        type Setter<'a> = &'a dyn Fn() -> Result<Option<u64>>;
        let setters: [(&str, Setter); 7] = [
            ("view", &|| f.engine.set_view(GridView::Starred)),
            ("search", &|| f.engine.set_search_query("beach")),
            ("blank search", &|| f.engine.set_search_query("  ")),
            ("sort", &|| {
                f.engine.set_sort(Sort {
                    key: SortKey::Name,
                    reverse: true,
                })
            }),
            ("person", &|| f.engine.set_person_view("c:abc")),
            ("album", &|| f.engine.set_album_view(album.id)),
            ("tag", &|| f.engine.set_tag_view("holiday")),
        ];
        for (name, set) in setters {
            let before = f.engine.grid().0;
            let answered = set().unwrap();
            assert!(f.engine.grid().0 > before, "{name}: the setter published");
            assert_eq!(answered, Some(last_announced(&f)), "{name}");
            assert_eq!(answered, Some(f.engine.grid().0), "{name}");
        }
        let answered = f.engine.set_copies_view(photo).unwrap();
        assert_eq!(answered, Some(last_announced(&f)), "copies");
    }

    /// Only a rebuild whose state has moved on names no version: the grid on show may
    /// still be the old view's, and a UI told to wait for "this version or later" would
    /// accept it. Overtaken by a later read of the same state, the grid on show is the
    /// setter's own view, so its version is safe to hand over.
    #[test]
    fn only_a_superseded_rebuild_names_no_version_for_the_ui() {
        assert_eq!(Publish::Published(4).shown_at(), Some(4));
        assert_eq!(Publish::Overtaken(5).shown_at(), Some(5));
        assert_eq!(Publish::Superseded.shown_at(), None);
    }

    /// A star moves no photo between folders in the All view, so the sidebar's folders and
    /// the grid's sections are the ones the UI already holds: the generation stays, and
    /// `grid_info` can leave them out.
    #[test]
    fn a_star_leaves_the_layout_generation_alone() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (version, _, _, layout) = f.engine.published();
        assert_ne!(layout, 0, "the first built grid has a layout of its own");

        f.engine.set_star(f.ids()[0], true).unwrap();

        let (after, _, _, same) = f.engine.published();
        assert!(after > version, "the star rebuilt the grid");
        assert_eq!(same, layout);
    }

    #[test]
    fn a_hide_moves_the_layout_generation() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (_, _, _, layout) = f.engine.published();

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();

        assert_eq!(f.engine.published().3, layout + 1);
    }

    /// Under a flat sort the grid is one run, which a file growing leaves as it was; the
    /// sidebar's folder sizes move, and the sidebar orders by them under a size sort.
    #[test]
    fn a_change_to_the_folders_alone_moves_the_layout_generation() {
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        let watched = f.add_photos();
        f.engine
            .set_sort(Sort {
                key: SortKey::Size,
                reverse: false,
            })
            .unwrap();
        let (_, before, _, layout) = f.engine.published();

        // More bytes after the end-of-image marker: the same picture, a bigger file.
        let mut bigger = img.clone();
        bigger.resize(img.len() + 4096, 0);
        std::fs::write(f.photos.join("a").join("one.jpg"), &bigger).unwrap();
        f.engine.start_scan(watched);
        f.settle();

        let (_, after, _, moved) = f.engine.published();
        assert_eq!(
            after.sections(),
            before.sections(),
            "one flat run, as before"
        );
        assert_ne!(after.folders(), before.folders());
        assert_eq!(moved, layout + 1);
    }

    /// A sort by name lays the same photos out as one flat run: the sections change while
    /// the sidebar's folders do not.
    #[test]
    fn a_change_to_the_sections_alone_moves_the_layout_generation() {
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (_, before, _, layout) = f.engine.published();

        f.engine
            .set_sort(Sort {
                key: SortKey::Name,
                reverse: false,
            })
            .unwrap();

        let (_, after, _, moved) = f.engine.published();
        assert_ne!(after.sections(), before.sections());
        assert_eq!(
            after.folders(),
            before.folders(),
            "the same photos, the same folders"
        );
        assert_eq!(moved, layout + 1);
    }

    /// Review Focus 1: in Starred a star moves a photo into the view.
    #[test]
    fn starring_in_the_starred_view_moves_the_layout_generation() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        f.engine.set_view(GridView::Starred).unwrap();
        let (_, _, _, layout) = f.engine.published();

        f.engine.set_star(ids[0], true).unwrap();

        assert_eq!(f.engine.published().3, layout + 1);
    }

    #[test]
    fn a_star_changes_the_starred_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        assert_eq!(f.engine.counts().starred, 0);

        f.engine.set_star(f.ids()[0], true).unwrap();

        assert_eq!(f.engine.counts().starred, 1);
    }

    /// Review Focus 5: a hide is a data write like any other.
    #[test]
    fn a_hide_changes_the_hidden_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        assert_eq!(f.engine.counts().hidden, 0);

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();

        assert_eq!(f.engine.counts().hidden, 1);
    }

    /// The hashing pass writes `content_hash` without a data rebuild, so it bumps the epoch
    /// itself. The counts are read (and cached) while the pass is held off, then the pass
    /// runs.
    #[test]
    fn a_duplicate_pass_changes_the_duplicate_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        {
            // Held, every pass the scan requests finds `hashing` taken and leaves.
            let _held = f.engine.hashing.lock();
            f.engine.add_folder(&f.photos).unwrap();
            f.engine.wait_for_scans();
            f.engine.wait_for_passes();
            assert_eq!(f.engine.counts().duplicate, 0);
        }
        f.engine.request_similar_pass();
        f.engine.wait_for_passes();

        assert_eq!(f.engine.counts().duplicate, 2);
    }

    /// A frame moves a thumbnail, which no count reads; its rebuild - up to one a second -
    /// must not rerun the queries.
    #[test]
    fn a_poster_frame_rebuild_does_not_recount() {
        let f = fixture(&[("a/one.jpg", &jpeg(16, 16))]);
        f.add_photos();
        f.engine.counts();
        let computed = f.engine.counts_computed.load(Ordering::SeqCst);

        f.engine.frame_stored();
        f.engine.counts();

        assert_eq!(f.engine.counts_computed.load(Ordering::SeqCst), computed);
    }

    /// A poster frame moves a thumbnail's state and nothing any collection or Settings
    /// reads, so its rebuild - up to one a second while a page extracts frames - must not
    /// send the UI to refetch the albums, people and tags.
    #[test]
    fn a_poster_frame_rebuild_does_not_announce_a_data_change() {
        use photon_core::grid::GridView;
        let f = fixture(&[("a/one.jpg", &jpeg(16, 16))]);
        f.add_photos();
        f.settle();
        f.engine.set_view(GridView::All).unwrap();
        assert!(!last_data_changed(&f));
        let before = f.engine.grid().0;

        // The first frame after a quiet second rebuilds at once, on this thread.
        f.engine.frame_stored();

        assert!(f.engine.grid().0 > before, "the frame rebuilt the grid");
        assert!(!last_data_changed(&f));
    }

    /// The same for the duplicate pass: a content hash moves the Duplicates view and its
    /// count, which the UI re-reads with the grid on every version, but no album, person,
    /// tag or folder count.
    ///
    /// Two different pictures padded to one byte size: the pass hashes both (a shared size
    /// is what makes a candidate) and rebuilds, and being different pictures they form no
    /// look-alike group whose regroup would rebuild after it - so the duplicate pass's is
    /// the scan's last rebuild, whether or not their thumbnails were ready in time.
    #[test]
    fn a_duplicate_pass_rebuild_does_not_announce_a_data_change() {
        let pattern = jpeg_pattern(180, 120);
        let mut solid = jpeg(16, 16);
        assert!(solid.len() < pattern.len());
        // After the end-of-image marker, where no decoder reads.
        solid.resize(pattern.len(), 0);
        let f = fixture(&[("a/one.jpg", &solid), ("b/two.jpg", &pattern)]);
        f.add_photos();
        f.settle();
        assert_eq!(
            crate::commands::grid_info(&f.engine, None).duplicate_count,
            0
        );

        let flags = data_changed_flags(&f);
        assert!(flags.contains(&true), "the scan's own rebuild changed data");
        assert_eq!(
            flags.last(),
            Some(&false),
            "the duplicate pass's rebuild, the scan's last, announced a data change"
        );
    }

    /// And for the look-alike regroup, staged as in
    /// `a_regroup_that_hashes_nothing_still_rebuilds_the_grid`: the groups are cleared
    /// behind the engine, so the pass has a regroup to publish and nothing to hash.
    #[test]
    fn a_look_alike_regroup_does_not_announce_a_data_change() {
        use photon_core::grid::GridView;
        let f = fixture(&[
            ("a/big.jpg", &jpeg_pattern(180, 120)),
            ("a/small.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.settle();
        f.engine.lib.set_similar_groups(&[]).unwrap();
        f.engine.set_view(GridView::Duplicates).unwrap();
        assert_eq!(f.ids().len(), 0);
        assert!(!last_data_changed(&f));

        f.engine.hash_after_scan(&AtomicBool::new(false));

        assert_eq!(f.ids().len(), 2, "the regroup rebuilt the grid");
        assert!(!last_data_changed(&f));
    }

    /// A star moves the data - and `refresh_grid` stands for every writer like it.
    #[test]
    fn a_star_announces_a_data_change() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        f.settle();
        // The flag is known to be clear, and the last event said so.
        f.engine.set_view(GridView::All).unwrap();
        assert!(!last_data_changed(&f));

        f.engine.set_star(f.ids()[0], true).unwrap();
        assert!(last_data_changed(&f));
    }

    /// The race the engine-wide flag exists for. A scan commits and its rebuild snapshots,
    /// then the user switches view: the switch publishes first and the scan's index is
    /// discarded by the epoch, so it never sends an event of its own. Had the flag
    /// travelled with the scan's rebuild, the only event after the commit would be the
    /// switch's, saying nothing changed, and the sidebar would miss the commit until some
    /// unrelated change came along.
    #[test]
    fn a_data_rebuild_overtaken_by_a_view_switch_still_announces_its_change() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        f.settle();
        f.engine.set_view(GridView::All).unwrap();
        assert!(!last_data_changed(&f));

        // A commit, and the rebuild that follows it starts querying...
        f.engine.lib.set_ratings(&[(f.ids()[0], 2)]).unwrap();
        let stale = f.engine.data_snapshot();
        let stale_index = Arc::new(f.engine.build_index(&stale.state).unwrap());
        // ...when the view switch lands first.
        f.engine.set_view(GridView::Starred).unwrap();
        assert_eq!(
            f.engine.publish_if_current(stale_index, &stale),
            Publish::Superseded,
            "the scan's rebuild is discarded, so it sends nothing"
        );
        assert!(
            last_data_changed(&f),
            "the switch's publish carries the change the discarded rebuild could not"
        );

        // And it was carried once: the next switch is a view change again.
        f.engine.set_view(GridView::All).unwrap();
        assert!(!last_data_changed(&f));
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
        f.settle();

        // A slow rebuild reads both rows...
        let early = f.engine.snapshot();
        let early_index = Arc::new(f.engine.build_index(&early.state).unwrap());
        assert_eq!(early_index.len(), 2);
        // ...then a purge commits and its own rebuild, stamped later, publishes first.
        f.engine.lib.purge_items(&[f.ids()[0]]).unwrap();
        f.engine.refresh_grid().unwrap();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 1);

        assert_eq!(
            f.engine.publish_if_current(early_index, &early),
            Publish::Overtaken(version),
            "an index that read the database before an already-published one is dropped, \
             and the grid on show is named as the one for its state"
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
        f.settle();
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
        f.settle();

        f.engine.set_search_query("beach").unwrap();

        let info = crate::commands::grid_info(&f.engine, None);
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
        f.settle();

        f.engine.set_search_query("beach").unwrap();
        f.engine.set_search_query("   ").unwrap();

        let info = crate::commands::grid_info(&f.engine, None);
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
        let info = crate::commands::grid_info(&f.engine, None);
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
        f.engine.albums_changed();
        assert_eq!(f.engine.grid().1.len(), 2);

        f.engine.set_tag_view("beach").unwrap();
        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!(
            (info.view, info.tag.as_deref(), info.album),
            (GridView::Tag, Some("beach"), None)
        );

        f.engine.set_person_view("c:abc").unwrap();
        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!(
            (info.view, info.person.as_deref()),
            (GridView::Person, Some("c:abc"))
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
        f.settle();
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
        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!((info.view, info.len), (GridView::Copies, 3));
        let copies_of = info.copies_of.expect("reported while the view is open");
        assert_eq!((copies_of.id, copies_of.file_name), (orig, name(orig)));
        assert!(!copies_of.gone, "the anchor photo is still in the library");
        assert_eq!(
            info.album, None,
            "the argument is a photo id, not an album id"
        );

        f.engine.set_view(GridView::All).unwrap();
        assert!(
            crate::commands::grid_info(&f.engine, None)
                .copies_of
                .is_none()
        );

        // A different parameterised view whose argument happens to parse as an id must not
        // be read as a Copies argument either - `view == Copies` is the guard, not "the
        // argument parses". `orig` itself is a valid id, so this is not a vacuous check.
        f.engine.set_search_query(&orig.to_string()).unwrap();
        assert!(
            crate::commands::grid_info(&f.engine, None)
                .copies_of
                .is_none()
        );

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
        f.settle();
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
        f.settle();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();
        f.engine.set_copies_view(orig).unwrap();
        let info = crate::commands::grid_info(&f.engine, None);
        assert_eq!(info.len, 2);
        assert!(!info.copies_of.unwrap().hidden);

        f.engine.set_items_hidden(&[orig], true).unwrap();
        let info = crate::commands::grid_info(&f.engine, None);
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
        f.settle();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();

        f.engine.set_copies_view(orig).unwrap();
        assert!(
            !crate::commands::grid_info(&f.engine, None)
                .copies_of
                .unwrap()
                .gone,
            "the anchor is still live"
        );

        f.engine.lib.purge_items(&[orig]).unwrap();
        f.engine.refresh_grid().unwrap();

        // The twin stays through the hash frozen into the argument; the look-alike cannot be
        // frozen (see `CopiesArg`) and drops out.
        let info = crate::commands::grid_info(&f.engine, None);
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
        f.settle();

        f.engine.set_search_query("beach").unwrap();
        f.engine.set_view(GridView::Starred).unwrap();

        assert_eq!(crate::commands::grid_info(&f.engine, None).search_query, "");
    }
}
