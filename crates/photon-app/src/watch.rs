//! Turns filesystem events into subtree scans.
//!
//! The watcher itself and the mapping policy live in photon-core; this owns the lifecycle:
//! which roots are watched, what happens when the OS won't watch them, and how events
//! become scans without ever running two scans of one folder at once.

use crate::engine::Engine;
use parking_lot::Mutex;
use photon_core::watcher::{
    WatchError, WatchedRoot, Watcher, insert_pending, plan_scans, roots_affected_by,
};
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

/// How long `stop` waits for those threads before leaving them to finish on their own.
///
/// They check `stopping` between operations, but either can be *inside* a filesystem call
/// when it is set: `Path::is_dir` on a root that lives on a dead network mount, or
/// `watch_root` on one. A hard mount does not fail fast, so joining unconditionally put
/// that stall on the quit path — `RunEvent::Exit` -> `Engine::shutdown` -> `stop_watcher`
/// -> here — with the window already gone and photon apparently hung. Generous enough that
/// a healthy machine under load always joins cleanly, short enough that a quit never looks
/// like a crash.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// How often `join_within` looks at the handles it is waiting on. `JoinHandle` has no timed
/// join, so the wait is a poll of `is_finished`.
const JOIN_POLL: Duration = Duration::from_millis(50);

pub struct WatcherService {
    engine: Arc<Engine>,
    /// The directories still waiting for a subtree scan, per watched folder: a bounded set
    /// maintained by `insert_pending`, so a folder whose scan slot is busy keeps covering
    /// every queued change without collapsing unrelated branches into a full rescan.
    ///
    /// Shared (not a bare `Mutex`) so the event and ticker threads can reach it without
    /// borrowing `self`, which a `'static` thread can't do.
    pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>>,
    /// Roots the OS would not let us watch; rescanned periodically instead, and dropped
    /// once re-registering their watch succeeds. Read by `is_degraded`, which
    /// `Engine::run_scan` consults so the `folder-status` event tells the UI when live
    /// updates for a folder are limited.
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

        if let Some((watcher, rx, errors)) = try_start_watcher(&engine, &degraded) {
            *watcher_slot.lock() = Some(watcher);
            push_thread(
                &threads,
                spawn_event_thread(
                    engine.clone(),
                    pending.clone(),
                    degraded.clone(),
                    watcher_slot.clone(),
                    stopping.clone(),
                    rx,
                    errors,
                ),
            );
        }

        // Tell the UI about every root that just failed to register, now rather than
        // whenever some later scan happens to finish. `folder-status` is otherwise only
        // emitted at the tail of a scan, and this runs *after* every start-up scan has
        // already reported `degraded: false` — so a launch-time failure (an exhausted
        // inotify limit, say) would stay invisible until the first five-minute tick
        // produced a scan of its own, with the status bar claiming live updates are fine
        // in the meantime.
        let just_degraded: Vec<i64> = degraded.lock().clone();
        for id in just_degraded {
            engine.emit_folder_status(id, true);
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
        push_thread(&threads, tick_handle);

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

    /// Retries one pending follow-up per folder, dropping the ones that start.
    pub fn drain_pending(&self) {
        try_drain(&self.engine, &self.pending);
    }

    /// How many directories are queued as follow-ups, across every watched folder.
    pub fn pending_len(&self) -> usize {
        self.pending.lock().values().map(Vec::len).sum()
    }

    /// True while `id`'s watch couldn't be registered with the OS, so it's relying on
    /// periodic rescans instead of live filesystem events.
    pub fn is_degraded(&self, id: i64) -> bool {
        self.degraded.lock().contains(&id)
    }

    /// Registers a watch for a folder added after the service started (`Engine::add_folder`
    /// calls this), so it gets live updates immediately instead of waiting for a restart.
    /// Falls back to `degraded` exactly like start-time registration when it fails, or when
    /// there's currently no live OS watcher to register with at all.
    pub fn watch_added(&self, id: i64, path: &Path) {
        let outcome = self
            .watcher
            .lock()
            .as_mut()
            .map(|watcher| watcher.watch_root(path));
        let mut degraded = self.degraded.lock();
        match outcome {
            Some(Ok(())) => {
                degraded.retain(|x| *x != id);
            }
            Some(Err(err)) => {
                tracing::warn!(
                    watched_id = id,
                    path = %path.display(),
                    error = %err.message,
                    "could not watch new folder; falling back to periodic rescans"
                );
                mark_degraded(&mut degraded, id);
            }
            None => mark_degraded(&mut degraded, id),
        }
    }

    /// Unregisters a folder's watch when it's removed (`Engine::remove_folder` calls this),
    /// so it stops delivering events for a folder photon no longer tracks, and drops any
    /// stale `degraded`/pending-follow-up entry for it.
    pub fn watch_removed(&self, id: i64, path: &Path) {
        if let Some(watcher) = self.watcher.lock().as_mut() {
            watcher.unwatch_root(path);
        }
        self.degraded.lock().retain(|x| *x != id);
        self.pending.lock().remove(&id);
    }

    /// The directories currently queued as `id`'s follow-ups, if any. Test-only: production
    /// code only needs `pending_len`, but the tests want to assert exactly what the pending
    /// set holds rather than infer it indirectly from a scan's side effects.
    #[cfg(test)]
    fn pending_dirs(&self, id: i64) -> Option<Vec<PathBuf>> {
        self.pending.lock().get(&id).cloned()
    }

    /// Stops the event and ticker threads, waiting up to `STOP_TIMEOUT` for them to finish,
    /// then drops the OS watcher (unregistering every root).
    ///
    /// The wait is bounded because a thread can be inside a filesystem call that a dead
    /// network mount will not return from promptly, and this runs on the quit path. A
    /// thread still running at the deadline is left to exit on its own: `stopping` is
    /// already set, and every loop rechecks it as soon as its current call returns.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let handles: Vec<JoinHandle<()>> = self.threads.lock().drain(..).collect();
        let still_running = join_within(handles, STOP_TIMEOUT);
        if still_running > 0 {
            tracing::warn!(
                threads = still_running,
                "watcher threads did not stop within {STOP_TIMEOUT:?}; leaving them to finish \
                 on their own (a watched folder on an unresponsive mount can hold a \
                 filesystem call open past this point)"
            );
        }
        // Not "safe because both threads have stopped" — after a timeout one of them may
        // still be running. It is safe because of what such a thread can do next, which is
        // nothing that matters: the slot has its own lock, so taking it cannot race;
        // `rescan_offline_roots` finding `None` does nothing; `retry_watcher_startup`
        // returns early once `stopping` is set, so it can neither install a fresh watcher
        // here nor spawn an event thread this `stop` would never see; and
        // `Engine::start_scan_inner` refuses to start anything while the engine is shutting
        // down.
        self.watcher.lock().take();
    }
}

/// Joins `handles`, waiting at most `timeout`, and returns how many were still running when
/// it gave up. Those are dropped rather than waited on, which detaches them.
///
/// `JoinHandle` has no timed join, so this polls `is_finished` and joins whatever has
/// finished — those joins return immediately and still surface a panicking thread.
fn join_within(handles: Vec<JoinHandle<()>>, timeout: Duration) -> usize {
    let deadline = Instant::now() + timeout;
    let mut waiting = handles;
    loop {
        let (finished, running): (Vec<_>, Vec<_>) =
            waiting.into_iter().partition(JoinHandle::is_finished);
        for handle in finished {
            let _ = handle.join();
        }
        if running.is_empty() {
            return 0;
        }
        let now = Instant::now();
        if now >= deadline {
            return running.len();
        }
        waiting = running;
        std::thread::sleep(JOIN_POLL.min(deadline - now));
    }
}

/// Records `handle` as one of the threads `stop` waits for, dropping any handle whose
/// thread has already finished.
///
/// `retry_watcher_startup` spawns a replacement event thread every time the watcher
/// subsystem is restarted, so without the reaping this vec grows one dead handle per
/// restart and is only ever emptied by `stop`. Dropping a finished handle instead of
/// joining it loses nothing but the chance to observe a panic that has already happened.
fn push_thread(threads: &Mutex<Vec<JoinHandle<()>>>, handle: JoinHandle<()>) {
    let mut threads = threads.lock();
    threads.retain(|h| !h.is_finished());
    threads.push(handle);
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

/// The event thread's body: reads batches from `rx` and turns them into scans, draining any
/// watch errors reported alongside them, and checking `stopping` between each `POLL`-long
/// wait so it can't outlive `stop`.
#[allow(clippy::too_many_arguments)]
fn spawn_event_thread(
    engine: Arc<Engine>,
    pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>>,
    degraded: Arc<Mutex<Vec<i64>>>,
    watcher: Arc<Mutex<Option<Watcher>>>,
    stopping: Arc<AtomicBool>,
    rx: Receiver<Vec<PathBuf>>,
    errors: Receiver<Vec<WatchError>>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("photon-watch-events".into())
        .spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                for failures in errors.try_iter() {
                    degrade_failed_roots(&engine, &degraded, &failures);
                }
                match rx.recv_timeout(POLL) {
                    Ok(dirs) => plan_and_apply(&engine, &pending, dirs),
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        watcher_died(&engine, &degraded, &watcher);
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn watch event thread")
}

/// Handles the debouncer having stopped: its sender is gone, so this watcher will never
/// deliver another event and live updates are over for every root it carried.
///
/// Clearing the watcher slot is what makes recovery possible at all: `retry_watcher_startup`
/// returns immediately while a watcher — even a dead one — is still installed, so without
/// this the subsystem would stay down until photon was restarted, silently, with the status
/// bar still reporting healthy folders. Marking the online roots degraded both tells the UI
/// and puts them on the five-minute rescan until the restart succeeds.
fn watcher_died(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
    watcher: &Mutex<Option<Watcher>>,
) {
    tracing::warn!(
        "the filesystem watcher stopped delivering events; falling back to periodic rescans \
         until it can be restarted"
    );
    watcher.lock().take();
    let watched = engine.lib.watched_folders().unwrap_or_default();
    let mut degraded = degraded.lock();
    let newly: Vec<i64> = watched
        .iter()
        .filter(|w| w.online)
        .map(|w| {
            mark_degraded(&mut degraded, w.id);
            w.id
        })
        .collect();
    drop(degraded);
    // Tell the UI now. `folder-status` is otherwise only emitted at the tail of a scan, so
    // without this the status bar claims live updates are fine until the five-minute tick.
    for id in newly {
        engine.emit_folder_status(id, true);
    }
}

/// Marks the roots an OS-reported watch error touched as degraded. An error on an already
/// established watch means events may have been lost (an inotify queue overflow, say), which
/// nothing else would ever notice: the periodic rescan those roots fall back to is what
/// picks the missed changes up.
fn degrade_failed_roots(engine: &Arc<Engine>, degraded: &Mutex<Vec<i64>>, failures: &[WatchError]) {
    let roots: Vec<WatchedRoot> = engine
        .lib
        .watched_folders()
        .unwrap_or_default()
        .iter()
        .filter(|w| w.online)
        .map(|w| WatchedRoot {
            watched_id: w.id,
            path: PathBuf::from(&w.path),
        })
        .collect();
    let affected = roots_affected_by(failures, &roots);
    if affected.is_empty() {
        return;
    }
    let mut degraded = degraded.lock();
    for id in &affected {
        mark_degraded(&mut degraded, *id);
    }
    drop(degraded);
    for id in affected {
        engine.emit_folder_status(id, true);
    }
}

/// Attempts to start the OS watcher and register every online watched root with it.
///
/// Used both by `start` and by the ticker's retry when the watcher subsystem is down
/// entirely. A root that fails to register is (re-)marked `degraded`, logged once per
/// attempt; one that succeeds is deliberately *left* degraded for `rescan_degraded_roots`
/// to clear, because clearing it here would empty the list that function reads and it would
/// return without scanning. A watch that only just started cannot have seen whatever changed
/// while the subsystem was down, so dropping that rescan loses every addition and deletion
/// from the outage until the user next touches those directories.
///
/// Returns `None` (every online root marked degraded) if `Watcher::start` itself fails.
#[allow(clippy::type_complexity)]
fn try_start_watcher(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
) -> Option<(Watcher, Receiver<Vec<PathBuf>>, Receiver<Vec<WatchError>>)> {
    let watched = engine.lib.watched_folders().unwrap_or_default();
    match Watcher::start(DEBOUNCE) {
        Ok((mut watcher, rx, errors)) => {
            let mut failed = Vec::new();
            for w in watched.iter().filter(|w| w.online) {
                match watcher.watch_root(Path::new(&w.path)) {
                    Ok(()) => {}
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
            for id in failed {
                mark_degraded(&mut degraded, id);
            }
            drop(degraded);
            Some((watcher, rx, errors))
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
    pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>>,
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
    pending: &Mutex<HashMap<i64, Vec<PathBuf>>>,
    dirs: Vec<PathBuf>,
) {
    // Canonicalize before mapping directories to roots: `add_watched_folder` stores every
    // root canonicalized, while an event arrives with whatever path the OS reported. On
    // macOS a directory under `/var` (or any symlinked path) is reported there while the
    // stored root reads `/private/var`, and on Windows the short 8.3 and long forms differ;
    // comparing raw against canonical matches nothing, so every request for such a folder is
    // dropped and live updates simply never happen for it. A user watching a symlinked
    // folder hits exactly the same mismatch on Linux.
    //
    // Once per batch and before any lock is taken: this is a filesystem call, and it can
    // block on a dead network mount.
    //
    // A directory that has since been deleted cannot be canonicalized and keeps its raw
    // path; `scan_subtree` resolves that by walking up to its nearest living ancestor.
    let dirs: Vec<PathBuf> = dirs
        .into_iter()
        .map(|dir| dunce::canonicalize(&dir).unwrap_or(dir))
        .collect();
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

/// Adds `dir` to the set of follow-ups queued for `id`, via `insert_pending`, so nothing
/// queued is ever dropped: a directory an already-queued ancestor covers is folded into it,
/// unrelated branches are kept separately, and only an overflowing set collapses to `root`.
fn queue_pending(pending: &Mutex<HashMap<i64, Vec<PathBuf>>>, id: i64, dir: PathBuf, root: &Path) {
    let mut pending = pending.lock();
    insert_pending(pending.entry(id).or_default(), &dir, root);
}

/// Retries one pending follow-up per watched folder, removing the ones that start, and
/// forgetting every follow-up of a folder that is no longer watched (there's nothing left to
/// scan for it).
///
/// One per folder, not all of them: a folder can only have one scan running at a time, so
/// the rest would be refused anyway. They stay queued for the next tick, two seconds later.
fn try_drain(engine: &Arc<Engine>, pending: &Mutex<HashMap<i64, Vec<PathBuf>>>) {
    let candidates: Vec<(i64, PathBuf)> = pending
        .lock()
        .iter()
        .filter_map(|(id, dirs)| dirs.first().map(|dir| (*id, dir.clone())))
        .collect();
    if candidates.is_empty() {
        return;
    }
    let watched = engine.lib.watched_folders().unwrap_or_default();
    for (id, dir) in candidates {
        match watched.iter().find(|w| w.id == id) {
            Some(folder) => {
                if engine.start_subtree_scan(folder.clone(), dir.clone()) {
                    let mut pending = pending.lock();
                    if let Some(dirs) = pending.get_mut(&id) {
                        dirs.retain(|queued| queued != &dir);
                        if dirs.is_empty() {
                            pending.remove(&id);
                        }
                    }
                }
            }
            None => {
                pending.lock().remove(&id);
            }
        }
    }
}

/// For each root currently marked offline: if its directory has reappeared, installs a
/// watch for it before rescanning (falling back to `degraded` if that fails, so the
/// existing retry path takes over) — otherwise it would stay on this far more expensive
/// periodic rescan forever even after a watch could have been installed once and then kept
/// it live.
///
/// `degraded` is updated for this root *before* `start_scan` is called for it, not batched
/// until after the loop: `start_scan`'s eventual `FolderStatus` event reads `degraded` when
/// the scan finishes, so clearing it only afterwards would let that event still claim live
/// updates are limited for a root that has already recovered.
fn rescan_offline_roots(
    engine: &Arc<Engine>,
    degraded: &Mutex<Vec<i64>>,
    watcher: &Mutex<Option<Watcher>>,
) {
    let watched = engine.lib.watched_folders().unwrap_or_default();
    for w in watched.into_iter().filter(|w| !w.online) {
        if Path::new(&w.path).is_dir() {
            // Locked only for this one call, not across the loop: `watch_root` can block
            // (e.g. a dead network mount), and this thread also drains pending follow-ups.
            let outcome = watcher
                .lock()
                .as_mut()
                .map(|watcher| watcher.watch_root(Path::new(&w.path)));
            match outcome {
                Some(Ok(())) => {
                    degraded.lock().retain(|id| *id != w.id);
                }
                Some(Err(err)) => {
                    tracing::warn!(
                        watched_id = w.id,
                        path = %w.path,
                        error = %err.message,
                        "root is back but could not be watched; falling back to periodic rescans"
                    );
                    mark_degraded(&mut degraded.lock(), w.id);
                }
                None => {}
            }
        }
        engine.start_scan(w);
    }
}

/// If the OS watcher isn't running at all (a previous `Watcher::start` failed, degrading
/// every online root), retries starting it. On success, installs it and spawns a fresh
/// event thread so live updates resume for whichever roots register successfully.
fn retry_watcher_startup(
    engine: &Arc<Engine>,
    pending: &Arc<Mutex<HashMap<i64, Vec<PathBuf>>>>,
    degraded: &Arc<Mutex<Vec<i64>>>,
    watcher: &Arc<Mutex<Option<Watcher>>>,
    stopping: &Arc<AtomicBool>,
    threads: &Mutex<Vec<JoinHandle<()>>>,
) {
    if watcher.lock().is_some() {
        return;
    }
    let Some((new_watcher, rx, errors)) = try_start_watcher(engine, degraded) else {
        return;
    };
    // `stop` may have run while this was starting: it has already drained and joined the
    // thread list, so installing this watcher and spawning an event thread now would leave
    // `stop` returned with a live, unjoined thread behind it. Dropping the watcher here also
    // unregisters the roots it just registered.
    if stopping.load(Ordering::SeqCst) {
        drop(new_watcher);
        return;
    }
    *watcher.lock() = Some(new_watcher);
    let handle = spawn_event_thread(
        engine.clone(),
        pending.clone(),
        degraded.clone(),
        watcher.clone(),
        stopping.clone(),
        rx,
        errors,
    );
    push_thread(threads, handle);
}

/// For each degraded root: retries registering its OS watch (dropping it from `degraded` on
/// success, so it goes back to live updates) and always full-rescans it, since a watch that
/// only just started can't have seen whatever changed while it was unregistered. A root
/// whose registration still fails stays in `degraded` for the next tick.
///
/// `degraded` is cleared for a recovered root *before* `start_scan` is called for it, for
/// the same reason as in `rescan_offline_roots`: otherwise the scan's own `FolderStatus`
/// event could still report it degraded.
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
    for id in ids {
        let Some(folder) = watched.iter().find(|w| w.id == id) else {
            degraded.lock().retain(|x| *x != id);
            continue;
        };
        // Locked only for this one call, not across the loop or across `start_scan`: see
        // the same note in `rescan_offline_roots`.
        let outcome = watcher
            .lock()
            .as_mut()
            .map(|watcher| watcher.watch_root(Path::new(&folder.path)));
        match outcome {
            Some(Ok(())) => {
                degraded.lock().retain(|x| *x != id);
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Recorded;
    use crate::testutil::{fixture, jpeg};
    use photon_core::watcher::MAX_PENDING_DIRS;

    #[test]
    fn a_dead_watcher_tells_the_ui_its_folders_are_degraded() {
        let f = fixture(&[]);
        let watched = f.add_photos();
        let degraded = Mutex::new(Vec::new());
        let slot: Mutex<Option<Watcher>> = Mutex::new(None);

        watcher_died(&f.engine, &degraded, &slot);

        assert_eq!(
            degraded.lock().clone(),
            vec![watched.id],
            "the root is degraded"
        );
        let told = last_degraded(&f, watched.id);
        assert!(
            told,
            "a dead watcher must emit folder-status immediately; without it the status bar \
             claims live updates work until the next five-minute tick"
        );
    }

    /// True if the last `folder-status` recorded for `id` reported degraded.
    fn last_degraded(f: &crate::testutil::Fixture, id: i64) -> bool {
        f.events
            .all()
            .iter()
            .filter_map(|e| match e {
                Recorded::Folder(s) if s.watched_id == id => Some(s.degraded),
                _ => None,
            })
            .next_back()
            .unwrap_or(false)
    }

    #[test]
    fn join_within_returns_as_soon_as_the_threads_finish() {
        let handles = vec![
            std::thread::spawn(|| {}),
            std::thread::spawn(|| std::thread::sleep(Duration::from_millis(20))),
        ];
        let started = Instant::now();
        assert_eq!(join_within(handles, Duration::from_secs(5)), 0);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "waited {:?} for threads that finish in 20ms",
            started.elapsed()
        );
    }

    /// The whole point of the bound. A ticker stuck in a filesystem call on a dead network
    /// mount does not come back on request, and joining it unconditionally is what made
    /// quitting photon hang: `RunEvent::Exit` -> `Engine::shutdown` -> `stop`. The stuck
    /// thread here stands in for that syscall — it cannot be cancelled, only outlived.
    #[test]
    fn join_within_gives_up_on_a_thread_that_will_not_finish() {
        let stuck = std::thread::spawn(|| std::thread::sleep(Duration::from_secs(10)));
        let quick = std::thread::spawn(|| {});
        let started = Instant::now();

        assert_eq!(
            join_within(vec![stuck, quick], Duration::from_millis(150)),
            1
        );

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "gave up after {:?}, so it waited for the stuck thread rather than the timeout",
            started.elapsed()
        );
    }

    /// `retry_watcher_startup` spawns a replacement event thread on every watcher restart,
    /// and until this the vec kept every one of their handles — a slow leak on a machine
    /// whose watcher keeps dying, reaped only by `stop`.
    #[test]
    fn pushing_a_thread_reaps_the_handles_that_have_already_finished() {
        let threads: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
        push_thread(&threads, std::thread::spawn(|| {}));
        while !threads.lock()[0].is_finished() {
            std::thread::sleep(Duration::from_millis(5));
        }

        push_thread(&threads, std::thread::spawn(|| std::thread::sleep(POLL)));

        assert_eq!(
            threads.lock().len(),
            1,
            "the finished handle should have been dropped when the second was pushed"
        );
    }

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

        // Occupy the folder's scan slot, then deliver an event for it. A real scan (via
        // `start_scan`) would race this: the fixture is a single tiny JPEG, so it can finish
        // before `handle_batch` runs, freeing the slot before the event has a chance to find
        // it occupied. `occupy_scan_slot_for_test` makes the slot occupied a guarantee rather
        // than a hope.
        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        service.handle_batch(vec![f.photos.join("a")]);
        assert_eq!(service.pending_len(), 1);
        drop(slot);

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn pending_follow_ups_for_sibling_directories_are_kept_side_by_side() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        // Occupy the folder's scan slot, then deliver events for two sibling directories.
        // Neither is an ancestor of the other, so a naive "keep one, drop the other" policy
        // would silently lose whichever branch's changes aren't queued.
        //
        // `occupy_scan_slot_for_test` holds the slot deterministically instead of racing a
        // real scan of these tiny fixtures to completion (a real scan can finish, and free
        // the slot, before both `handle_batch` calls below run).
        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        service.handle_batch(vec![f.photos.join("a")]);
        service.handle_batch(vec![f.photos.join("b")]);

        // Assert the pending set directly rather than inferring it from a scan's side
        // effects: that would race the real ticker and the OS watcher's own events.
        let root = PathBuf::from(&watched.path);
        assert_eq!(
            service.pending_dirs(watched.id),
            Some(vec![root.join("a"), root.join("b")]),
            "both branches stay queued: collapsing siblings into their common ancestor would \
             be the watched root, whose subtree scan is a full rescan of the whole folder"
        );
        drop(slot);

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(
            service.pending_len(),
            1,
            "a folder runs one scan at a time, so the sibling waits for the next tick"
        );
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    /// The bound is what keeps the set from becoming an unbounded queue: past it, one full
    /// rescan is cheaper than tracking every branch, and still drops nothing.
    #[test]
    fn more_pending_directories_than_the_bound_collapse_to_one_full_rescan() {
        let img = jpeg(16, 16);
        let names: Vec<String> = (0..=MAX_PENDING_DIRS)
            .map(|i| format!("d{i}/one.jpg"))
            .collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &img[..])).collect();
        let f = fixture(&files);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        // Occupy the folder's scan slot deterministically: with this many tiny fixtures a
        // real scan could still finish, and free the slot, before the loop below delivers
        // every event.
        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        for i in 0..=MAX_PENDING_DIRS {
            service.handle_batch(vec![f.photos.join(format!("d{i}"))]);
        }

        assert_eq!(
            service.pending_dirs(watched.id),
            Some(vec![PathBuf::from(&watched.path)]),
            "one directory past the bound, the set becomes a single rescan of the root"
        );
        drop(slot);

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    /// The OS reports events with whatever path it was handed, while `add_watched_folder`
    /// stores every root canonicalized: on macOS a temp directory arrives as `/var/...` where
    /// the root reads `/private/var/...`, and on Windows the short 8.3 and long forms differ.
    /// Comparing raw against canonical matches nothing, so every request is dropped and live
    /// updates silently do nothing for that folder. A symlinked watched folder reproduces
    /// exactly that mismatch on Linux, where the two forms are otherwise identical — so this
    /// covers the same bug the macOS and Windows CI runners hit.
    #[test]
    #[cfg(unix)]
    fn an_event_reported_through_a_symlinked_path_still_maps_to_its_watched_root() {
        let img = jpeg(16, 16);
        let f = fixture(&[]);
        let real = f.dir.path().join("real");
        std::fs::create_dir_all(real.join("a")).unwrap();
        let link = f.dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let watched = f.engine.add_folder(&link).unwrap();
        f.engine.wait_for_scans();
        assert_eq!(
            Path::new(&watched.path),
            dunce::canonicalize(&real).unwrap(),
            "the watched root is stored canonicalized, not as the symlinked path given"
        );

        let service = WatcherService::start(&f.engine);
        std::fs::write(link.join("a").join("new.jpg"), &img).unwrap();
        service.handle_batch(vec![link.join("a")]);
        f.engine.wait_for_scans();

        assert_eq!(
            f.engine.grid().1.len(),
            1,
            "the event directory must be canonicalized before it is matched against roots"
        );
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
        let (watcher, _rx, _errors) = Watcher::start(DEBOUNCE).unwrap();
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
        let (watcher, _rx, _errors) = Watcher::start(DEBOUNCE).unwrap();
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
        let (watcher, _rx, _errors) = Watcher::start(DEBOUNCE).unwrap();
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
        let (watcher, _rx, _errors) = Watcher::start(DEBOUNCE).unwrap();
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
        let pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>> = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(vec![watched.id]));
        let watcher_slot: Arc<Mutex<Option<Watcher>>> = Arc::new(Mutex::new(None));
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
        assert_eq!(
            degraded.lock().clone(),
            vec![watched.id],
            "the retry does not clear the flag itself: `rescan_degraded_roots` reads that \
             list to decide what to rescan, and clears it as it goes"
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

    /// The whole point of degrading a root is that a scan makes up for the events nobody
    /// delivered. A restart that only re-registers the watch leaves everything added or
    /// deleted during the outage unindexed until the user happens to touch those directories
    /// again, because the freshly installed watch cannot have seen any of it.
    #[test]
    fn a_restarted_watcher_rescans_the_roots_it_recovers() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();

        // The outage: `Watcher::start` failed at `start` time, so no event was delivered for
        // this file and nothing but a rescan can find it.
        std::fs::write(f.photos.join("a").join("two.jpg"), &img).unwrap();
        let pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>> = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(vec![watched.id]));
        let watcher_slot: Arc<Mutex<Option<Watcher>>> = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(false));
        let threads: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

        // Exactly what the five-minute tick does, in its order.
        retry_watcher_startup(
            &f.engine,
            &pending,
            &degraded,
            &watcher_slot,
            &stopping,
            &threads,
        );
        rescan_degraded_roots(&f.engine, &degraded, &watcher_slot);
        f.engine.wait_for_scans();

        assert_eq!(
            f.ids().len(),
            2,
            "the recovered root is rescanned, so what changed during the outage is indexed"
        );
        assert!(
            degraded.lock().is_empty(),
            "and it is only cleared once that rescan has been started for it"
        );

        stopping.store(true, Ordering::SeqCst);
        for handle in threads.lock().drain(..) {
            let _ = handle.join();
        }
    }

    /// A watcher whose debouncer has stopped delivering events must not leave photon
    /// silently and permanently without live updates: the event thread clears the watcher
    /// slot (otherwise `retry_watcher_startup` returns immediately, seeing a watcher that
    /// happens to be dead) and degrades every online root, so the five-minute tick brings the
    /// subsystem back.
    #[test]
    fn a_dead_watcher_degrades_its_roots_and_is_restarted_by_the_tick() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();

        // A live watcher in the slot, but an event channel we own: dropping its sender is
        // exactly what the debouncer's thread going away looks like from here.
        let pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>> = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(Vec::new()));
        let (installed, _installed_rx, _installed_errors) = Watcher::start(DEBOUNCE).unwrap();
        let watcher_slot = Arc::new(Mutex::new(Some(installed)));
        let stopping = Arc::new(AtomicBool::new(false));
        let threads: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
        let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
        let (_error_tx, error_rx) = std::sync::mpsc::channel::<Vec<WatchError>>();
        let handle = spawn_event_thread(
            f.engine.clone(),
            pending.clone(),
            degraded.clone(),
            watcher_slot.clone(),
            stopping.clone(),
            rx,
            error_rx,
        );

        drop(tx);
        let _ = handle.join();

        assert!(
            watcher_slot.lock().is_none(),
            "a dead watcher must be cleared, or the restart below returns without doing \
             anything and live updates never come back"
        );
        assert_eq!(
            degraded.lock().clone(),
            vec![watched.id],
            "its roots fall back to periodic rescans, and the UI is told they are limited"
        );

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
            "the five-minute tick restarts the watcher subsystem"
        );
        assert_eq!(threads.lock().len(), 1, "with a fresh event thread");

        rescan_degraded_roots(&f.engine, &degraded, &watcher_slot);
        f.engine.wait_for_scans();
        assert!(
            degraded.lock().is_empty(),
            "and the root's watch registers again, so - once it has been rescanned for what \
             the dead watcher never reported - it is no longer degraded"
        );

        stopping.store(true, Ordering::SeqCst);
        for handle in threads.lock().drain(..) {
            let _ = handle.join();
        }
    }

    /// `stop` sets `stopping`, then joins the thread list and takes the watcher slot. A
    /// restart that got past `try_start_watcher` just before that must install nothing
    /// afterwards, or `stop` returns with a live, unjoined event thread behind it.
    #[test]
    fn a_restart_racing_stop_installs_no_watcher_and_spawns_no_thread() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();

        let pending: Arc<Mutex<HashMap<i64, Vec<PathBuf>>>> = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(Vec::new()));
        let watcher_slot: Arc<Mutex<Option<Watcher>>> = Arc::new(Mutex::new(None));
        let stopping = Arc::new(AtomicBool::new(true));
        let threads: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

        retry_watcher_startup(
            &f.engine,
            &pending,
            &degraded,
            &watcher_slot,
            &stopping,
            &threads,
        );

        assert!(watcher_slot.lock().is_none());
        assert!(
            threads.lock().is_empty(),
            "no event thread may be spawned once stop has joined the thread list"
        );
    }

    /// A root that fails to register when the service starts must reach the UI right away.
    /// `folder-status` is otherwise only emitted at the tail of a scan, and the service
    /// starts *after* every start-up scan has already reported `degraded: false` — so
    /// without this the status bar would keep claiming live updates are fine for up to five
    /// minutes.
    #[test]
    fn a_root_that_cannot_be_watched_at_start_is_reported_degraded_immediately() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();

        // The folder's row stays online while its directory is gone, so registering its
        // watch fails the way an exhausted inotify limit would.
        std::fs::remove_dir_all(&f.photos).unwrap();
        let before = f.events.all().len();

        let service = WatcherService::start(&f.engine);

        assert!(service.is_degraded(watched.id));
        let after = f.events.all()[before..].to_vec();
        assert!(
            after.iter().any(|e| matches!(
                e,
                crate::events::Recorded::Folder(crate::events::FolderStatus {
                    watched_id,
                    degraded: true,
                    ..
                }) if *watched_id == watched.id
            )),
            "the failed registration must be reported without waiting for a scan to finish"
        );
        service.stop();
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

    /// A folder added after the service has already started must get a watch registration
    /// *attempted* immediately (`Engine::add_folder` calls `WatcherService::watch_added`),
    /// not just on the next full restart.
    ///
    /// A happy-path assertion here (registration succeeds, so the folder isn't degraded)
    /// would hold whether or not `add_folder` ever calls `watch_added` at all, since a
    /// brand new id starts out not-degraded regardless: `degraded` only ever becomes true
    /// via an explicit failed registration. So instead this forces that registration to
    /// fail (by taking the OS watcher out of the service before adding the folder, so
    /// `watch_added` finds nothing to register with) and asserts the folder ends up
    /// degraded - something that can only happen if `add_folder` actually invoked
    /// `watch_added`.
    #[test]
    fn a_folder_added_after_start_has_its_watch_registration_attempted() {
        let f = fixture(&[]);
        f.engine.start_watcher();
        let service = f.engine.watcher_service().unwrap();

        // Simulate the OS watcher subsystem being down when the new folder is added.
        service.watcher.lock().take();

        let other = f.dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let watched = f.engine.add_folder(&other).unwrap();
        f.engine.wait_for_scans();

        assert!(
            service.is_degraded(watched.id),
            "watch_added must have been called and found no watcher to register with"
        );
        assert!(
            f.events.all().iter().any(|e| matches!(
                e,
                crate::events::Recorded::Folder(crate::events::FolderStatus {
                    watched_id,
                    degraded: true,
                    ..
                }) if *watched_id == watched.id
            )),
            "the finishing scan's status event must reflect the failed registration"
        );

        f.engine.stop_watcher();
    }

    /// Removing a folder must unregister its watch and forget any degraded/pending state
    /// for it, so a stale entry can't outlive the folder it described.
    #[test]
    fn removing_a_folder_forgets_its_degraded_and_pending_state() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.start_watcher();
        let service = f.engine.watcher_service().unwrap();

        mark_degraded(&mut service.degraded.lock(), watched.id);
        service
            .pending
            .lock()
            .insert(watched.id, vec![f.photos.clone()]);

        f.engine.remove_folder(watched.id).unwrap();

        assert!(!service.is_degraded(watched.id));
        assert_eq!(service.pending_len(), 0);
        f.engine.stop_watcher();
    }

    /// A root that recovers through the 30-second offline poll must not have its own
    /// recovery rescan still call it degraded: without clearing `degraded` before
    /// `start_scan` runs (rather than batching the clear until after the whole loop), the
    /// `folder-status` event that scan emits would still say live updates are limited, for
    /// up to five more minutes until the next `DEGRADED_RESCAN` tick.
    #[test]
    fn a_root_recovering_via_the_offline_poll_is_not_reported_degraded_by_its_own_rescan() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.start_watcher();
        let service = f.engine.watcher_service().unwrap();

        // Simulate a root that's both offline and was left marked degraded by an earlier
        // failed registration, whose directory has since reappeared.
        f.engine.lib.set_watched_online(watched.id, false).unwrap();
        mark_degraded(&mut service.degraded.lock(), watched.id);
        let before = f.events.all().len();

        rescan_offline_roots(&f.engine, &service.degraded, &service.watcher);
        f.engine.wait_for_scans();

        // Only the events from this recovery rescan onward: `add_photos` already recorded
        // an earlier (unrelated) `degraded: false` status, before `service.degraded` was
        // ever touched, which would make a plain `.any(...)` over every recorded event pass
        // regardless of whether this rescan's own status report is correct.
        let after = f.events.all()[before..].to_vec();
        assert!(
            after.iter().any(|e| matches!(
                e,
                crate::events::Recorded::Folder(crate::events::FolderStatus {
                    watched_id,
                    degraded: false,
                    ..
                }) if *watched_id == watched.id
            )),
            "the root's own recovery rescan must report it as no longer degraded"
        );

        f.engine.stop_watcher();
    }

    /// Real filesystem events are timing-dependent, so this is excluded from CI, matching
    /// the same convention as `photon_core::watcher::fs`'s ignored test.
    /// Run it locally with: cargo test -p photon-app -- --ignored copying_a_photo
    #[test]
    #[ignore]
    fn copying_a_photo_into_a_folder_added_after_start_appears_without_a_restart() {
        let f = fixture(&[]);
        f.engine.start_watcher();

        let other = f.dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        f.engine.add_folder(&other).unwrap();
        f.engine.wait_for_scans();

        std::fs::write(other.join("new.jpg"), jpeg(16, 16)).unwrap();

        let start = Instant::now();
        while f.engine.grid().1.is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }

        assert_eq!(f.engine.grid().1.len(), 1);
        f.engine.stop_watcher();
    }
}
