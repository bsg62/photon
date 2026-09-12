//! Turns filesystem events into subtree scans.
//!
//! The watcher itself and the mapping policy live in photon-core; this owns the lifecycle:
//! which roots are watched, what happens when the OS won't watch them, and how events
//! become scans without ever running two scans of one folder at once.

use crate::engine::Engine;
use parking_lot::Mutex;
use photon_core::watcher::{WatchedRoot, Watcher, plan_scans};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
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
    /// At most one queued follow-up per watched folder: the shallowest directory wins,
    /// because scanning it covers the others.
    ///
    /// Shared (not a bare `Mutex`) so the event and ticker threads can reach it without
    /// borrowing `self`, which a `'static` thread can't do.
    pending: Arc<Mutex<HashMap<i64, PathBuf>>>,
    /// Roots the OS would not let us watch; rescanned periodically instead.
    ///
    /// Nothing outside this service reads it yet: it will surface through
    /// `FolderStatus { degraded }` once that lands (a later task), so for now it's kept
    /// only so the ticker thread's clone stays backed by the same list this service holds.
    #[allow(dead_code)]
    degraded: Arc<Mutex<Vec<i64>>>,
    stopping: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl WatcherService {
    /// Registers every online watched root with the OS watcher (recording any that fail as
    /// degraded), then spawns the event thread and the ticker thread.
    pub fn start(engine: &Arc<Engine>) -> Self {
        let engine = engine.clone();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let degraded = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        let watched = engine.lib.watched_folders().unwrap_or_default();
        match Watcher::start(DEBOUNCE) {
            Ok((mut watcher, rx)) => {
                for w in watched.iter().filter(|w| w.online) {
                    if let Err(err) = watcher.watch_root(Path::new(&w.path)) {
                        tracing::warn!(
                            watched_id = w.id,
                            path = %w.path,
                            error = %err.message,
                            "could not watch folder; falling back to periodic rescans"
                        );
                        degraded.lock().push(w.id);
                    }
                }
                let ev_engine = engine.clone();
                let ev_pending = pending.clone();
                let ev_stopping = stopping.clone();
                threads.push(
                    std::thread::Builder::new()
                        .name("photon-watch-events".into())
                        .spawn(move || {
                            // Kept alive for the thread's lifetime: dropping it (when the
                            // thread ends, on `stop`) unregisters every watched root.
                            let _watcher = watcher;
                            while !ev_stopping.load(Ordering::SeqCst) {
                                match rx.recv_timeout(POLL) {
                                    Ok(dirs) => plan_and_apply(&ev_engine, &ev_pending, dirs),
                                    Err(RecvTimeoutError::Timeout) => continue,
                                    Err(RecvTimeoutError::Disconnected) => break,
                                }
                            }
                        })
                        .expect("failed to spawn watch event thread"),
                );
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    "could not start the filesystem watcher; watched folders will rely on periodic rescans"
                );
                for w in watched.iter().filter(|w| w.online) {
                    degraded.lock().push(w.id);
                }
            }
        }

        let tick_engine = engine.clone();
        let tick_pending = pending.clone();
        let tick_degraded = degraded.clone();
        let tick_stopping = stopping.clone();
        threads.push(
            std::thread::Builder::new()
                .name("photon-watch-ticker".into())
                .spawn(move || {
                    ticker_loop(tick_engine, tick_pending, tick_degraded, tick_stopping);
                })
                .expect("failed to spawn watch ticker thread"),
        );

        Self {
            engine,
            pending,
            degraded,
            stopping,
            threads: Mutex::new(threads),
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

    /// Stops the event and ticker threads (dropping the OS watcher along the way) and
    /// blocks until both have actually finished.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let handles: Vec<JoinHandle<()>> = self.threads.lock().drain(..).collect();
        for handle in handles {
            let _ = handle.join();
        }
    }
}

/// The ticker thread's body: every `TICK`, retry pending follow-ups; every `OFFLINE_POLL`,
/// rescan offline roots in case they're back; every `DEGRADED_RESCAN`, full-rescan roots
/// the OS watcher couldn't register.
fn ticker_loop(
    engine: Arc<Engine>,
    pending: Arc<Mutex<HashMap<i64, PathBuf>>>,
    degraded: Arc<Mutex<Vec<i64>>>,
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
            rescan_offline_roots(&engine);
            last_offline_poll = now;
        }
        if now.duration_since(last_degraded_rescan) >= DEGRADED_RESCAN {
            rescan_degraded_roots(&engine, &degraded);
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
            queue_pending(pending, id, dir);
        }
    }
}

/// Records `dir` as the follow-up for `id`, keeping whichever of the old and new directory
/// is shallower (scanning it covers the other).
fn queue_pending(pending: &Mutex<HashMap<i64, PathBuf>>, id: i64, dir: PathBuf) {
    pending
        .lock()
        .entry(id)
        .and_modify(|existing| {
            if dir.components().count() < existing.components().count() {
                *existing = dir.clone();
            }
        })
        .or_insert(dir);
}

/// Retries every pending follow-up, removing the ones that start (or whose folder is no
/// longer watched).
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
        let started = match watched.iter().find(|w| w.id == id) {
            Some(folder) => engine.start_subtree_scan(folder.clone(), dir),
            None => true,
        };
        if started {
            pending.lock().remove(&id);
        }
    }
}

fn rescan_offline_roots(engine: &Arc<Engine>) {
    for w in engine
        .lib
        .watched_folders()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !w.online)
    {
        engine.start_scan(w);
    }
}

fn rescan_degraded_roots(engine: &Arc<Engine>, degraded: &Mutex<Vec<i64>>) {
    let ids: Vec<i64> = degraded.lock().clone();
    if ids.is_empty() {
        return;
    }
    let watched = engine.lib.watched_folders().unwrap_or_default();
    for id in ids {
        match watched.iter().find(|w| w.id == id) {
            Some(folder) => {
                engine.start_scan(folder.clone());
            }
            None => degraded.lock().retain(|&x| x != id),
        }
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
    fn an_unknown_directory_is_dropped() {
        let f = fixture(&[]);
        f.add_photos();
        let service = WatcherService::start(&f.engine);
        service.handle_batch(vec![f.dir.path().join("elsewhere")]);
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }
}
