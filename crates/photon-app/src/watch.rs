//! Turns filesystem events into subtree scans.
//!
//! The watcher itself and the mapping policy live in photon-core; this owns the lifecycle:
//! which roots are watched, what happens when the OS won't watch them, and how events
//! become scans without ever running two scans of one folder at once.

use crate::engine::Engine;
use parking_lot::Mutex;
use photon_core::watcher::{WatchedRoot, Watcher, merge_pending, plan_scans};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const DEBOUNCE: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_secs(2);
const OFFLINE_POLL: Duration = Duration::from_secs(30);
const DEGRADED_RESCAN: Duration = Duration::from_secs(300);

/// How often the event thread and the ticker thread check `stopping`, so `stop` never
/// blocks for longer than this.
const POLL: Duration = Duration::from_millis(200);

pub struct WatcherService {
    engine: Arc<Engine>,
    /// At most one queued follow-up per watched folder, merged with `merge_pending` as new
    /// requests arrive so it always covers every directory queued for that folder.
    ///
    /// Shared (not a bare `Mutex`) so the event and ticker threads can reach it without
    /// borrowing `self`, which a `'static` thread can't do.
    pending: Arc<Mutex<HashMap<i64, PathBuf>>>,
    /// Roots the OS would not let us watch; rescanned periodically instead, and dropped
    /// once re-registering their watch succeeds.
    ///
    /// Nothing outside this service reads it yet: it will surface through
    /// `FolderStatus { degraded }` once that lands (a later task), so for now it's kept
    /// only so the ticker thread's clone stays backed by the same list this service holds.
    #[allow(dead_code)]
    degraded: Arc<Mutex<Vec<i64>>>,
    /// The OS watcher, shared with the ticker thread so it can retry registering a
    /// degraded root, or restart the whole subsystem if it failed to start at all. `None`
    /// when there's currently no live OS watching (either `Watcher::start` failed and
    /// hasn't yet been retried successfully, or `stop` has dropped it).
    watcher: Arc<Mutex<Option<Watcher>>>,
    stopping: Arc<AtomicBool>,
    threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl WatcherService {
    /// Registers every online watched root with the OS watcher (recording any that fail as
    /// degraded), then spawns the event thread (if the watcher started at all) and the
    /// ticker thread.
    pub fn start(engine: &Arc<Engine>) -> Self {
        let engine = engine.clone();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(Vec::new()));
        let watcher_slot: Arc<Mutex<Option<Watcher>>> = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(false));
        let threads: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

        if let Some((watcher, rx)) = try_start_watcher(&engine, &degraded) {
            *watcher_slot.lock() = Some(watcher);
            threads.lock().push(spawn_event_thread(
                engine.clone(),
                pending.clone(),
                stopping.clone(),
                rx,
            ));
        }

        let tick_handle = {
            let tick_engine = engine.clone();
            let tick_pending = pending.clone();
            let tick_degraded = degraded.clone();
            let tick_watcher = watcher_slot.clone();
            let tick_threads = threads.clone();
            let tick_stopping = stopping.clone();
            std::thread::Builder::new()
                .name("photon-watch-ticker".into())
                .spawn(move || {
                    ticker_loop(
                        tick_engine,
                        tick_pending,
                        tick_degraded,
                        tick_watcher,
                        tick_threads,
                        tick_stopping,
                    );
                })
                .expect("failed to spawn watch ticker thread")
        };
        threads.lock().push(tick_handle);

        Self {
            engine,
            pending,
            degraded,
            watcher: watcher_slot,
            stopping,
            threads,
        }
    }

    /// Maps `dirs` to watched folders and starts a subtree scan for each, queueing a
    /// follow-up for any folder whose scan slot is already occupied.
    pub fn handle_batch(&self, dirs: Vec<PathBuf>) {
        plan_and_apply(&self.engine, &self.pending, dirs);
    }

    /// Retries every pending follow-up, dropping the ones that start.
    pub fn drain_pending(&self) {
        try_drain(&self.engine, &self.pending);
    }

    pub fn pending_len(&self) -> usize {
        self.pending.lock().len()
    }

    /// The directory currently queued as `id`'s follow-up, if any. Test-only: production
    /// code only needs `pending_len`, but the tests want to assert exactly what a merge
    /// produced rather than infer it indirectly from a scan's side effects.
    #[cfg(test)]
    fn pending_dir(&self, id: i64) -> Option<PathBuf> {
        self.pending.lock().get(&id).cloned()
    }

    /// Stops the event and ticker threads and blocks until both have actually finished,
    /// then drops the OS watcher (unregistering every root).
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let handles: Vec<JoinHandle<()>> = self.threads.lock().drain(..).collect();
        for handle in handles {
            let _ = handle.join();
        }
        // Safe only now that both threads have stopped: the ticker thread reads and writes
        // this same slot to retry a degraded root's registration, or to restart the whole
        // watcher subsystem.
        self.watcher.lock().take();
    }
}

impl Drop for WatcherService {
    /// `stop` is idempotent, so it's safe to call unconditionally here: without this, a
    /// `WatcherService` dropped without an explicit `stop()` would leak its event and
    /// ticker threads (each holding an `Arc<Engine>`), with the ticker still starting scans
    /// in the background indefinitely.
    fn drop(&mut self) {
        self.stop();
    }
}

/// The event thread's body: reads batches from `rx` and turns them into scans, checking
/// `stopping` between each `POLL`-long wait so it can't outlive `stop`.
fn spawn_event_thread(
    engine: Arc<Engine>,
    pending: Arc<Mutex<HashMap<i64, PathBuf>>>,
    stopping: Arc<AtomicBool>,
    rx: Receiver<Vec<PathBuf>>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("photon-watch-events".into())
        .spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match rx.recv_timeout(POLL) {
                    Ok(dirs) => plan_and_apply(&engine, &pending, dirs),
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("failed to spawn watch event thread")
}

/// Attempts to start the OS watcher and register every online watched root with it.
///
/// Used both by `start` and by the ticker's retry when the watcher subsystem is down
/// entirely, so both paths mark/clear `degraded` the same way: a root that registers
/// successfully is cleared (it may have been marked degraded by an earlier attempt), one
/// that fails is (re-)marked, logged once per attempt.
///
/// Returns `None` (every online root marked degraded) if `Watcher::start` itself fails.
fn try_start_watcher(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
) -> Option<(Watcher, Receiver<Vec<PathBuf>>)> {
    let watched = engine.lib.watched_folders().unwrap_or_default();
    match Watcher::start(DEBOUNCE) {
        Ok((mut watcher, rx)) => {
            let mut recovered = Vec::new();
            let mut failed = Vec::new();
            for w in watched.iter().filter(|w| w.online) {
                match watcher.watch_root(Path::new(&w.path)) {
                    Ok(()) => recovered.push(w.id),
                    Err(err) => {
                        tracing::warn!(
                            watched_id = w.id,
                            path = %w.path,
                            error = %err.message,
                            "could not watch folder; falling back to periodic rescans"
                        );
                        failed.push(w.id);
                    }
                }
            }
            let mut degraded = degraded.lock();
            degraded.retain(|id| !recovered.contains(id));
            for id in failed {
                mark_degraded(&mut degraded, id);
            }
            drop(degraded);
            Some((watcher, rx))
        }
        Err(err) => {
            tracing::warn!(
                %err,
                "could not start the filesystem watcher; watched folders will rely on periodic rescans"
            );
            let mut degraded = degraded.lock();
            for w in watched.iter().filter(|w| w.online) {
                mark_degraded(&mut degraded, w.id);
            }
            None
        }
    }
}

/// Adds `id` to `degraded` if it isn't there already.
fn mark_degraded(degraded: &mut Vec<i64>, id: i64) {
    if !degraded.contains(&id) {
        degraded.push(id);
    }
}

/// The ticker thread's body: every `TICK`, retry pending follow-ups; every `OFFLINE_POLL`,
/// rescan offline roots (installing a watch for any that have come back); every
/// `DEGRADED_RESCAN`, restart the watcher subsystem if it's down entirely, retry
/// registering each degraded root's watch, and full-rescan it. The last is kept on the slow
/// tick, not `OFFLINE_POLL`, so a permanently unwatchable root doesn't retry in a tight
/// loop.
fn ticker_loop(
    engine: Arc<Engine>,
    pending: Arc<Mutex<HashMap<i64, PathBuf>>>,
    degraded: Arc<Mutex<Vec<i64>>>,
    watcher: Arc<Mutex<Option<Watcher>>>,
    threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    stopping: Arc<AtomicBool>,
) {
    let mut last_tick = Instant::now();
    let mut last_offline_poll = Instant::now();
    let mut last_degraded_rescan = Instant::now();
    while !stopping.load(Ordering::SeqCst) {
        std::thread::sleep(POLL);
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        let now = Instant::now();
        if now.duration_since(last_tick) >= TICK {
            try_drain(&engine, &pending);
            last_tick = now;
        }
        if now.duration_since(last_offline_poll) >= OFFLINE_POLL {
            rescan_offline_roots(&engine, &degraded, &watcher);
            last_offline_poll = now;
        }
        if now.duration_since(last_degraded_rescan) >= DEGRADED_RESCAN {
            retry_watcher_startup(&engine, &pending, &degraded, &watcher, &stopping, &threads);
            rescan_degraded_roots(&engine, &degraded, &watcher);
            last_degraded_rescan = now;
        }
    }
}

/// Turns a batch of changed directories into subtree scans, queueing a follow-up for any
/// folder whose scan slot is already occupied.
fn plan_and_apply(
    engine: &Arc<Engine>,
    pending: &Mutex<HashMap<i64, PathBuf>>,
    dirs: Vec<PathBuf>,
) {
    let watched = engine.lib.watched_folders().unwrap_or_default();
    let roots: Vec<WatchedRoot> = watched
        .iter()
        .map(|w| WatchedRoot {
            watched_id: w.id,
            path: PathBuf::from(&w.path),
        })
        .collect();
    let requests = plan_scans(&dirs, &roots, engine.excluded());
    for (id, dir) in requests {
        let Some(folder) = watched.iter().find(|w| w.id == id) else {
            continue;
        };
        if !engine.start_subtree_scan(folder.clone(), dir.clone()) {
            queue_pending(pending, id, dir, Path::new(&folder.path));
        }
    }
}

/// Records `dir` as (part of) the follow-up for `id`. If a follow-up is already queued for
/// this folder, merges the two with `merge_pending` (rather than dropping either) so the
/// eventual rescan covers both.
fn queue_pending(pending: &Mutex<HashMap<i64, PathBuf>>, id: i64, dir: PathBuf, root: &Path) {
    let mut pending = pending.lock();
    match pending.get(&id) {
        Some(existing) => {
            let merged = merge_pending(existing, &dir, root);
            pending.insert(id, merged);
        }
        None => {
            pending.insert(id, dir);
        }
    }
}

/// Retries every pending follow-up, removing the ones that start (or whose folder is no
/// longer watched, in which case there's nothing left to scan).
fn try_drain(engine: &Arc<Engine>, pending: &Mutex<HashMap<i64, PathBuf>>) {
    let candidates: Vec<(i64, PathBuf)> = pending
        .lock()
        .iter()
        .map(|(id, dir)| (*id, dir.clone()))
        .collect();
    if candidates.is_empty() {
        return;
    }
    let watched = engine.lib.watched_folders().unwrap_or_default();
    for (id, dir) in candidates {
        let resolved = match watched.iter().find(|w| w.id == id) {
            Some(folder) => engine.start_subtree_scan(folder.clone(), dir),
            None => true,
        };
        if resolved {
            pending.lock().remove(&id);
        }
    }
}

/// For each root currently marked offline: if its directory has reappeared, installs a
/// watch for it before rescanning (falling back to `degraded` if that fails, so the
/// existing retry path takes over) — otherwise it would stay on this far more expensive
/// periodic rescan forever even after a watch could have been installed once and then kept
/// it live.
fn rescan_offline_roots(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
    watcher: &Mutex<Option<Watcher>>,
) {
    let watched = engine.lib.watched_folders().unwrap_or_default();
    let mut newly_degraded = Vec::new();
    for w in watched.into_iter().filter(|w| !w.online) {
        if Path::new(&w.path).is_dir() {
            // Locked only for this one call, not across the loop: `watch_root` can block
            // (e.g. a dead network mount), and this thread also drains pending follow-ups.
            let outcome = watcher
                .lock()
                .as_mut()
                .map(|watcher| watcher.watch_root(Path::new(&w.path)));
            if let Some(Err(err)) = outcome {
                tracing::warn!(
                    watched_id = w.id,
                    path = %w.path,
                    error = %err.message,
                    "root is back but could not be watched; falling back to periodic rescans"
                );
                newly_degraded.push(w.id);
            }
        }
        engine.start_scan(w);
    }
    if !newly_degraded.is_empty() {
        let mut degraded = degraded.lock();
        for id in newly_degraded {
            mark_degraded(&mut degraded, id);
        }
    }
}

/// If the OS watcher isn't running at all (a previous `Watcher::start` failed, degrading
/// every online root), retries starting it. On success, installs it and spawns a fresh
/// event thread so live updates resume for whichever roots register successfully.
fn retry_watcher_startup(
    engine: &Arc<Engine>,
    pending: &Arc<Mutex<HashMap<i64, PathBuf>>>,
    degraded: &Mutex<Vec<i64>>,
    watcher: &Mutex<Option<Watcher>>,
    stopping: &Arc<AtomicBool>,
    threads: &Mutex<Vec<JoinHandle<()>>>,
) {
    if watcher.lock().is_some() {
        return;
    }
    let Some((new_watcher, rx)) = try_start_watcher(engine, degraded) else {
        return;
    };
    *watcher.lock() = Some(new_watcher);
    let handle = spawn_event_thread(engine.clone(), pending.clone(), stopping.clone(), rx);
    threads.lock().push(handle);
}

/// For each degraded root: retries registering its OS watch (dropping it from `degraded` on
/// success, so it goes back to live updates) and always full-rescans it, since a watch that
/// only just started can't have seen whatever changed while it was unregistered. A root
/// whose registration still fails stays in `degraded` for the next tick.
fn rescan_degraded_roots(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
    watcher: &Mutex<Option<Watcher>>,
) {
    let ids: Vec<i64> = degraded.lock().clone();
    if ids.is_empty() {
        return;
    }
    let watched = engine.lib.watched_folders().unwrap_or_default();
    let mut no_longer_watched = Vec::new();
    let mut recovered = Vec::new();
    for id in ids {
        let Some(folder) = watched.iter().find(|w| w.id == id) else {
            no_longer_watched.push(id);
            continue;
        };
        // Locked only for this one call, not across the loop or across `start_scan`: see
        // the same note in `rescan_offline_roots`.
        let outcome = watcher
            .lock()
            .as_mut()
            .map(|watcher| watcher.watch_root(Path::new(&folder.path)));
        match outcome {
            Some(Ok(())) => recovered.push(id),
            Some(Err(err)) => tracing::debug!(
                watched_id = folder.id,
                path = %folder.path,
                error = %err.message,
                "watch registration still failing; will retry on the next tick"
            ),
            None => {}
        }
        engine.start_scan(folder.clone());
    }
    if !no_longer_watched.is_empty() || !recovered.is_empty() {
        let mut degraded = degraded.lock();
        degraded.retain(|id| !no_longer_watched.contains(id) && !recovered.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};

    #[test]
    fn an_event_in_a_watched_folder_scans_that_subtree() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        std::fs::write(f.photos.join("a").join("two.jpg"), &img).unwrap();
        service.handle_batch(vec![f.photos.join("a")]);
        f.engine.wait_for_scans();

        assert_eq!(f.engine.grid().1.len(), 2);
        assert_eq!(watched.id, f.engine.lib.watched_folders().unwrap()[0].id);
        service.stop();
    }

    #[test]
    fn events_inside_photons_own_directories_are_ignored() {
        let f = fixture(&[]);
        f.add_photos();
        let service = WatcherService::start(&f.engine);
        let before = f.engine.grid().0;

        service.handle_batch(vec![f.engine.excluded()[0].clone()]);
        f.engine.wait_for_scans();

        assert_eq!(f.engine.grid().0, before, "no scan, so no new grid version");
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn an_event_during_a_running_scan_becomes_a_pending_follow_up() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        // Occupy the folder's scan slot, then deliver an event for it.
        let blocker = f.engine.clone();
        assert!(blocker.start_scan(watched.clone()));
        service.handle_batch(vec![f.photos.join("a")]);
        assert_eq!(service.pending_len(), 1);

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn pending_follow_ups_for_sibling_directories_are_merged_not_dropped() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        // Occupy the folder's scan slot, then deliver events for two sibling directories.
        // Neither is an ancestor of the other, so a naive "keep one, drop the other" policy
        // would silently lose whichever branch's changes aren't queued.
        let blocker = f.engine.clone();
        assert!(blocker.start_scan(watched.clone()));
        service.handle_batch(vec![f.photos.join("a")]);
        service.handle_batch(vec![f.photos.join("b")]);

        // Assert the merge directly rather than inferring it from a scan's side effects:
        // that would race the real ticker and the OS watcher's own events.
        assert_eq!(
            service.pending_dir(watched.id),
            Some(PathBuf::from(&watched.path)),
            "two sibling requests merge into their common ancestor: the watched root itself"
        );

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn a_degraded_root_is_reregistered_and_rescanned() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();

        // Simulate a root that failed to register at `start` time: it's in `degraded`, but
        // its watch was never actually installed on the `Watcher` below.
        let degraded = Mutex::new(vec![watched.id]);
        let (watcher, _rx) = Watcher::start(DEBOUNCE).unwrap();
        let watcher_slot = Mutex::new(Some(watcher));

        rescan_degraded_roots(&f.engine, &degraded, &watcher_slot);

        assert!(
            degraded.lock().is_empty(),
            "registration succeeds this time (the directory exists), so the root drops out \
             of `degraded` and returns to live updates"
        );
        f.engine.wait_for_scans();
    }

    #[test]
    fn a_still_failing_degraded_root_stays_degraded_but_is_still_rescanned() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();

        // Remove the directory so `watch_root` keeps failing, while the watched-folder row
        // (and its path) is still there for `start_scan` to find and rescan.
        std::fs::remove_dir_all(&f.photos).unwrap();

        let degraded = Mutex::new(vec![watched.id]);
        let (watcher, _rx) = Watcher::start(DEBOUNCE).unwrap();
        let watcher_slot = Mutex::new(Some(watcher));

        rescan_degraded_roots(&f.engine, &degraded, &watcher_slot);

        assert_eq!(
            degraded.lock().clone(),
            vec![watched.id],
            "registration keeps failing (the directory is gone), so the root stays degraded \
             for the next tick"
        );
        f.engine.wait_for_scans();
        assert!(
            !f.engine.lib.watched_folders().unwrap()[0].online,
            "start_scan ran regardless of the failed registration, and found the folder gone"
        );
    }

    #[test]
    fn an_offline_root_that_is_back_is_watched_again_and_rescanned() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();

        // Simulate a root recorded as offline (e.g. after a dead network mount) whose
        // directory has actually reappeared, before anything has rescanned it yet.
        f.engine.lib.set_watched_online(watched.id, false).unwrap();

        let degraded = Mutex::new(Vec::new());
        let (watcher, _rx) = Watcher::start(DEBOUNCE).unwrap();
        let watcher_slot = Mutex::new(Some(watcher));

        rescan_offline_roots(&f.engine, &degraded, &watcher_slot);

        assert!(
            degraded.lock().is_empty(),
            "the directory exists, so registering the watch for it succeeds"
        );
        f.engine.wait_for_scans();
        assert!(
            f.engine.lib.watched_folders().unwrap()[0].online,
            "the rescan finds the directory and brings it back online"
        );
    }

    #[test]
    fn a_still_offline_root_is_not_watched_but_is_still_polled() {
        let f = fixture(&[]);
        let watched = f
            .engine
            .lib
            .add_watched_folder(&f.photos, f.engine.excluded())
            .unwrap();
        std::fs::remove_dir_all(&f.photos).unwrap();
        f.engine.lib.set_watched_online(watched.id, false).unwrap();

        let degraded = Mutex::new(Vec::new());
        let (watcher, _rx) = Watcher::start(DEBOUNCE).unwrap();
        let watcher_slot = Mutex::new(Some(watcher));

        rescan_offline_roots(&f.engine, &degraded, &watcher_slot);

        assert!(
            degraded.lock().is_empty(),
            "still gone, so no watch is even attempted for it"
        );
        f.engine.wait_for_scans();
        assert!(!f.engine.lib.watched_folders().unwrap()[0].online);
    }

    #[test]
    fn a_fully_down_watcher_is_restarted_and_resumes_live_updates() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();

        // Simulate `Watcher::start` itself having failed at `start` time: no watcher in the
        // slot at all, and the online root marked degraded as a result.
        let pending: Arc<Mutex<HashMap<i64, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Mutex::new(vec![watched.id]);
        let watcher_slot: Mutex<Option<Watcher>> = Mutex::new(None);
        let stopping = Arc::new(AtomicBool::new(false));
        let threads: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

        retry_watcher_startup(
            &f.engine,
            &pending,
            &degraded,
            &watcher_slot,
            &stopping,
            &threads,
        );

        assert!(
            watcher_slot.lock().is_some(),
            "the watcher subsystem restarts successfully"
        );
        assert!(
            degraded.lock().is_empty(),
            "the root's registration succeeds on retry, so it's no longer degraded"
        );
        assert_eq!(
            threads.lock().len(),
            1,
            "a fresh event thread is spawned once the watcher comes back"
        );

        // Clean up the thread this test spawned directly (not through a `WatcherService`).
        stopping.store(true, Ordering::SeqCst);
        for handle in threads.lock().drain(..) {
            let _ = handle.join();
        }
    }

    #[test]
    fn an_unknown_directory_is_dropped() {
        let f = fixture(&[]);
        f.add_photos();
        let service = WatcherService::start(&f.engine);
        service.handle_batch(vec![f.dir.path().join("elsewhere")]);
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }
}
