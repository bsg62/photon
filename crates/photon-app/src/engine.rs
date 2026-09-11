//! The running library: photon-core services plus the current grid snapshot and the
//! background scans. Plain Rust, so it can be tested without a webview.

use crate::events::{Events, FolderStatus, LibraryChanged, ScanProgressEvent};
use parking_lot::{Mutex, RwLock};
use photon_core::{
    Result,
    grid::GridIndex,
    library::{Library, WatchedFolder},
    now_ms,
    scanner::{ScanOptions, ScanProgress, scan_watched},
    thumbs::{ThumbCache, ThumbService},
};
use std::{
    collections::HashMap,
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
    events: Arc<dyn Events>,
    scans: Mutex<HashMap<i64, RunningScan>>,
    next_token: AtomicU64,
}

impl Engine {
    pub fn open(config: EngineConfig, events: Arc<dyn Events>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&config.cache_dir)?;
        let lib = Arc::new(Library::open(&config.db_path)?);
        let cache = Arc::new(ThumbCache::new(config.cache_dir.clone()));
        let thumbs = ThumbService::start(lib.clone(), cache, config.workers);
        let mut excluded = vec![config.cache_dir.clone()];
        if let Some(data_dir) = config.db_path.parent() {
            excluded.push(data_dir.to_path_buf());
        }
        let grid = Arc::new(GridIndex::build(lib.grid_entries()?));
        Ok(Arc::new(Self {
            lib,
            thumbs,
            excluded,
            grid: RwLock::new((0, grid)),
            events,
            scans: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(0),
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
    pub fn refresh_grid(&self) -> Result<()> {
        let index = Arc::new(GridIndex::build(self.lib.grid_entries()?));
        let (version, len) = {
            let mut grid = self.grid.write();
            grid.0 += 1;
            grid.1 = index;
            (grid.0, grid.1.len())
        };
        self.events.library_changed(LibraryChanged { version, len });
        Ok(())
    }

    /// Validates and watches `path`, then starts its first scan.
    pub fn add_folder(self: &Arc<Self>, path: &Path) -> Result<WatchedFolder> {
        let watched = self.lib.add_watched_folder(path, &self.excluded)?;
        self.start_scan(watched.clone());
        Ok(watched)
    }

    /// Stops any scan of the folder, forgets it and everything under it, and refreshes the grid.
    pub fn remove_folder(&self, watched_id: i64) -> Result<()> {
        self.cancel_scan(watched_id);
        self.lib.remove_watched_folder(watched_id)?;
        self.refresh_grid()
    }

    /// Starts a background scan unless one is already running for this folder.
    pub fn start_scan(self: &Arc<Self>, watched: WatchedFolder) -> bool {
        let mut scans = self.scans.lock();
        if scans.contains_key(&watched.id) {
            return false;
        }
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let engine = Arc::clone(self);
        let thread_cancel = cancel.clone();
        let id = watched.id;
        let handle = std::thread::Builder::new()
            .name(format!("photon-scan-{id}"))
            .spawn(move || {
                engine.run_scan(&watched, thread_cancel);
                let mut scans = engine.scans.lock();
                if scans.get(&id).is_some_and(|r| r.token == token) {
                    scans.remove(&id);
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

    /// Cancels the folder's scan, if any, and waits for it to stop.
    pub fn cancel_scan(&self, watched_id: i64) {
        let running = self.scans.lock().remove(&watched_id);
        if let Some(mut running) = running {
            running.cancel.store(true, Ordering::Relaxed);
            if let Some(handle) = running.handle.take() {
                let _ = handle.join();
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

    /// Background start-up work: watch `pictures` if the library is empty, queue pending
    /// thumbnails, rescan every folder, then collect thumbnail garbage.
    pub fn startup(self: &Arc<Self>, pictures: Option<PathBuf>) -> JoinHandle<()> {
        let engine = Arc::clone(self);
        std::thread::Builder::new()
            .name("photon-startup".into())
            .spawn(move || {
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
                if let Err(err) = engine.thumbs.enqueue_pending() {
                    tracing::warn!(%err, "could not queue pending thumbnails");
                }
                for watched in engine.lib.watched_folders().unwrap_or_default() {
                    engine.start_scan(watched);
                }
                engine.wait_for_scans();
                match engine.thumbs.collect_garbage() {
                    Ok(removed) => tracing::info!(removed, "thumbnail garbage collected"),
                    Err(err) => tracing::warn!(%err, "thumbnail garbage collection failed"),
                }
            })
            .expect("failed to spawn startup thread")
    }

    /// Cancels all scans. Thumbnail workers stop when the engine is dropped.
    pub fn shutdown(&self) {
        let ids: Vec<i64> = self.scans.lock().keys().copied().collect();
        for id in ids {
            self.cancel_scan(id);
        }
    }

    fn run_scan(&self, watched: &WatchedFolder, cancel: Arc<AtomicBool>) {
        let options = ScanOptions {
            excluded: self.excluded.clone(),
            cancel,
        };
        let mut last = ScanProgress::default();
        let mut last_refresh = Instant::now();
        let mut refreshed_total = 0;
        let mut last_progress: Option<Instant> = None;
        let result = scan_watched(&self.lib, watched, now_ms(), &options, &mut |p| {
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
        });
        let cancelled = match &result {
            Ok(report) => report.cancelled,
            Err(err) => {
                tracing::warn!(watched_id = watched.id, %err, "scan failed");
                false
            }
        };
        if let Err(err) = self.refresh_grid() {
            tracing::warn!(%err, "grid refresh failed");
        }
        if let Err(err) = self.thumbs.enqueue_pending() {
            tracing::warn!(%err, "could not queue pending thumbnails");
        }
        if let Some(folder) = self
            .lib
            .watched_folders()
            .ok()
            .and_then(|all| all.into_iter().find(|w| w.id == watched.id))
        {
            self.events.folder_status(FolderStatus {
                watched_id: folder.id,
                online: folder.online,
            });
        }
        self.events
            .scan_progress(ScanProgressEvent::new(watched.id, &last, true, cancelled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Recorded;
    use crate::testutil::{fixture, jpeg};

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
            online: true
        })));
        assert!(!f.engine.is_scanning(watched.id));
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
        let data = f.dir.path().join("data");
        assert!(matches!(
            f.engine.add_folder(&cache),
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
        f.engine.startup(Some(f.photos.clone())).join().unwrap();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
        assert_eq!(f.engine.grid().1.len(), 1);

        let other = f.dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        f.engine.startup(Some(other)).join().unwrap();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
    }

    #[test]
    fn cancelling_an_idle_folder_is_a_no_op() {
        let f = fixture(&[]);
        f.engine.cancel_scan(42);
        f.engine.shutdown();
        assert!(!f.engine.is_scanning(42));
    }
}
