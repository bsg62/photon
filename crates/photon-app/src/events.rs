//! Events the engine sends to the UI. The Tauri implementation lives in `app.rs`.

use photon_core::scanner::ScanProgress;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub version: u64,
    pub len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressEvent {
    pub watched_id: i64,
    pub files_seen: u64,
    pub added: u64,
    pub changed: u64,
    pub done: bool,
    pub cancelled: bool,
}

impl ScanProgressEvent {
    pub fn new(watched_id: i64, p: &ScanProgress, done: bool, cancelled: bool) -> Self {
        Self {
            watched_id,
            files_seen: p.files_seen,
            added: p.added,
            changed: p.changed,
            done,
            cancelled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    pub watched_id: i64,
    pub online: bool,
    /// True while the OS wouldn't let photon watch this folder, so it's relying on
    /// periodic rescans instead of live filesystem events.
    pub degraded: bool,
}

pub trait Events: Send + Sync + 'static {
    fn library_changed(&self, event: LibraryChanged);
    fn scan_progress(&self, event: ScanProgressEvent);
    fn folder_status(&self, event: FolderStatus);
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    Library(LibraryChanged),
    Scan(ScanProgressEvent),
    Folder(FolderStatus),
}

/// Test sink that keeps every event.
#[cfg(test)]
#[derive(Default)]
pub struct Recorder(pub parking_lot::Mutex<Vec<Recorded>>);

#[cfg(test)]
impl Recorder {
    pub fn all(&self) -> Vec<Recorded> {
        self.0.lock().clone()
    }
}

#[cfg(test)]
impl Events for Recorder {
    fn library_changed(&self, e: LibraryChanged) {
        self.0.lock().push(Recorded::Library(e));
    }
    fn scan_progress(&self, e: ScanProgressEvent) {
        self.0.lock().push(Recorded::Scan(e));
    }
    fn folder_status(&self, e: FolderStatus) {
        self.0.lock().push(Recorded::Folder(e));
    }
}
