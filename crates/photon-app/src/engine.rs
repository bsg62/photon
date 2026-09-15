//! The running library: photon-core services plus the current grid snapshot and the
//! background scans. Plain Rust, so it can be tested without a webview.

use crate::events::{Events, FolderStatus, LibraryChanged, ScanProgressEvent};
use crate::watch::WatcherService;
use parking_lot::{Mutex, RwLock};
use photon_core::{
    Result,
    grid::{GridIndex, GridView},
    library::{Library, WatchedFolder},
    now_ms,
    scanner::{ScanOptions, ScanProgress, scan_subtree, scan_watched},
    thumbs::{ThumbCache, ThumbService},
};
use std::{
    collections::{HashMap, HashSet},
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

pub struct Engine {
    pub lib: Arc<Library>,
    pub thumbs: ThumbService,
    excluded: Vec<PathBuf>,
    grid: RwLock<(u64, Arc<GridIndex>)>,
    /// Which set of photos the grid currently shows.
    view: RwLock<GridView>,
    /// The active search query. Beside the view rather than inside it: `GridView` is `Copy`
    /// and is mirrored in TypeScript as plain strings (spec §4).
    search_query: RwLock<String>,
    /// Serialises `refresh_grid` end to end (read, publish, emit), so two concurrent
    /// refreshes can't publish a stale snapshot under a newer version or emit events out
    /// of order.
    refresh: Mutex<()>,
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
}

impl Engine {
    pub fn open(config: EngineConfig, events: Arc<dyn Events>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&config.cache_dir)?;
        let lib = Arc::new(Library::open(&config.db_path)?);
        let cache = Arc::new(ThumbCache::new(config.cache_dir.clone()));
        let thumbs = ThumbService::start(lib.clone(), cache, config.workers);
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
            .map(|p| dunce::canonicalize(&p).unwrap_or(p))
            .collect();

        let grid = Arc::new(GridIndex::build(lib.grid_entries()?));
        Ok(Arc::new(Self {
            lib,
            thumbs,
            excluded,
            grid: RwLock::new((0, grid)),
            view: RwLock::new(GridView::All),
            search_query: RwLock::new(String::new()),
            refresh: Mutex::new(()),
            events,
            scans: Mutex::new(HashMap::new()),
            removing: Mutex::new(HashSet::new()),
            next_token: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            startup: Mutex::new(None),
            watcher: Mutex::new(None),
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

    /// Rebuilds the grid from the database and tells the UI.
    ///
    /// Holds `refresh` across the read, the publish and the event so concurrent refreshes
    /// (e.g. two scans, or a scan racing `remove_folder`) can't publish a stale snapshot
    /// under a newer version number or emit `library_changed` out of order.
    pub fn refresh_grid(&self) -> Result<()> {
        let _serialize = self.refresh.lock();
        let index = Arc::new(GridIndex::build(
            self.lib
                .entries_for(*self.view.read(), &self.search_query.read())?,
        ));
        let (version, len) = {
            let mut grid = self.grid.write();
            grid.0 += 1;
            grid.1 = index;
            (grid.0, grid.1.len())
        };
        self.events.library_changed(LibraryChanged { version, len });
        Ok(())
    }

    pub fn view(&self) -> GridView {
        *self.view.read()
    }

    /// The active search query, or the empty string when no search is active.
    pub fn search_query(&self) -> String {
        self.search_query.read().clone()
    }

    /// Switches which photos the grid shows and rebuilds the index. Rebuilding is the same
    /// work startup already does; a second index kept in sync would be a large new surface
    /// for staleness bugs to speed up something already fast and rarely done.
    ///
    /// Rolls `view`/`search_query` back to their previous values if the rebuild fails, so a
    /// failed refresh can never leave `GridInfo` (the UI's one source of truth, spec §5)
    /// reporting a view/query the grid was never actually rebuilt for. Without the
    /// rollback the bad state is sticky: every later `refresh_grid` — including the scan
    /// and watcher paths — re-reads the same failing query/view and fails again, and the
    /// empty state can't rescue it either, since `len` still reflects the old, unrelated
    /// result set.
    pub fn set_view(&self, view: GridView) -> Result<()> {
        let previous = (*self.view.read(), self.search_query.read().clone());
        // A query left behind would reappear the next time Search is entered.
        if view != GridView::Search {
            self.search_query.write().clear();
        }
        *self.view.write() = view;
        if let Err(err) = self.refresh_grid() {
            *self.view.write() = previous.0;
            *self.search_query.write() = previous.1;
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
        let previous = (*self.view.read(), self.search_query.read().clone());
        *self.search_query.write() = query.to_string();
        *self.view.write() = GridView::Search;
        if let Err(err) = self.refresh_grid() {
            *self.view.write() = previous.0;
            *self.search_query.write() = previous.1;
            return Err(err);
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
    pub fn remove_folder(&self, watched_id: i64) -> Result<()> {
        // Set before the scan is cancelled, so nothing can start one in the window between
        // that and the deletion; cleared however this ends, so a failed removal does not
        // leave the folder unable to scan for the rest of the session.
        self.removing.lock().insert(watched_id);
        let result = self.remove_folder_inner(watched_id);
        self.removing.lock().remove(&watched_id);
        result
    }

    fn remove_folder_inner(&self, watched_id: i64) -> Result<()> {
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
        self.refresh_grid()
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
                match engine.thumbs.collect_garbage() {
                    Ok(removed) => tracing::info!(removed, "thumbnail garbage collected"),
                    Err(err) => tracing::warn!(%err, "thumbnail garbage collection failed"),
                }
            })
            .expect("failed to spawn startup thread");
        *self.startup.lock() = Some(handle);
    }

    /// Blocks until the thread spawned by `startup` has finished, if it hasn't already.
    /// Safe to call more than once (and safe to call when `startup` was never called).
    pub fn wait_for_startup(&self) {
        let handle = self.startup.lock().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// Stops new scans from starting, cancels every running scan (looping until none are
    /// left, since a scan or `startup` racing this can still insert one after the first
    /// pass), closes the thumbnail queue so its workers finish their current job and stop,
    /// then waits for the startup thread to finish (it checks `shutting_down` at each of
    /// its own checkpoints, so this doesn't wait for it to run to completion).
    pub fn shutdown(&self) {
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
    }

    fn run_scan(&self, watched: &WatchedFolder, subtree: Option<PathBuf>, cancel: Arc<AtomicBool>) {
        let options = ScanOptions {
            excluded: self.excluded.clone(),
            cancel,
        };
        let mut last = ScanProgress::default();
        let mut last_refresh = Instant::now();
        let mut refreshed_total = 0;
        let mut last_progress: Option<Instant> = None;
        let mut on_progress = |p: &ScanProgress| {
            last = *p;
            let total = p.added + p.changed;
            if total != refreshed_total && last_refresh.elapsed() >= THROTTLE {
                if let Err(err) = self.refresh_grid() {
                    tracing::warn!(%err, "grid refresh failed");
                }
                if let Err(err) = self.thumbs.enqueue_pending() {
                    tracing::warn!(%err, "could not queue pending thumbnails");
                }
                last_refresh = Instant::now();
                refreshed_total = total;
            }
            if last_progress.is_none_or(|t| t.elapsed() >= THROTTLE) {
                self.events
                    .scan_progress(ScanProgressEvent::new(watched.id, p, false, false));
                last_progress = Some(Instant::now());
            }
        };
        let result = match &subtree {
            Some(dir) => scan_subtree(
                &self.lib,
                watched,
                dir,
                now_ms(),
                &options,
                &mut on_progress,
            ),
            None => scan_watched(&self.lib, watched, now_ms(), &options, &mut on_progress),
        };
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
            // `restarred` counts alongside the others: starring a photo in Picasa never
            // changes the photo itself, so on a rescan every photo takes the `unchanged`
            // branch and this is often the only non-zero field in the report. Without it
            // here, a star-only scan would compute `false`, skip the refresh, and leave the
            // Starred view and its sidebar count stale until something else changed a row.
            Ok(report) => {
                report.added
                    + report.changed
                    + report.marked_missing
                    + report.purged
                    + report.restarred
                    > 0
            }
            // A scan that failed partway may still have committed earlier batches.
            Err(_) => true,
        };
        let online_changed = folder.as_ref().is_some_and(|f| f.online != watched.online);
        if (touched_rows || online_changed)
            && let Err(err) = self.refresh_grid()
        {
            tracing::warn!(%err, "grid refresh failed");
        }
        // Outside the guard: `refresh_grid` is the expensive half (it reads every grid row
        // and makes the UI refetch), but re-queueing pending thumbnails is cheap and is the
        // only thing that retries an item whose render failed transiently. Leaving it inside
        // meant such an item waited for an unrelated change, or a restart.
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
    use crate::testutil::{fixture, jpeg};
    use photon_core::media::ThumbState;

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

        let cache = dunce::canonicalize(real.join("cache").join("thumbs")).unwrap();
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

        // Checking `queued() > 0` right after `wait_for_scans` is racy in practice: the
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
