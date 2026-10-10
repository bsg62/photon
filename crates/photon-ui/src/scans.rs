//! What photon is doing in the background, as the engine has reported it: the scans, an
//! export, the face pass, and which folders cannot be watched live. What the Svelte store
//! holds of these (`library.svelte.ts`), with its rules.
//!
//! The application gives every report to this and is told what to do next - read the
//! folder list again, count a folder's photos - which it does off its own thread.
//!
//! Not here: the note of a scan the interface was only *told* is running. The Svelte UI
//! asks which scans are running because the startup scans begin before a webview exists
//! and a report sent to no listener is lost; this interface's channel is made before the
//! engine is opened, so every report of every scan arrives.
//!
//! No egui here.

use crate::status::{LIVE_UPDATES_LIMITED, Line, export_line, face_line, scan_line};
use photon_core::library::WatchedFolder;
use photon_engine::events::{ExportProgress, FaceProgress, FolderStatus, ScanProgressEvent};
use std::collections::HashMap;

/// What a scan's report asks of the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asks {
    /// Read the folder list again.
    pub folders: bool,
    /// Count the folder's photos, for `expected_read`: this is the first report of a scan.
    pub expected: bool,
    /// Read the published grid again, and say so (`settled`).
    pub settle: bool,
}

#[derive(Default)]
pub struct Scans {
    /// The last report of each watched folder's scan.
    scans: HashMap<i64, ScanProgressEvent>,
    /// How many photos each folder held when its current scan started, for the bar: a
    /// scan does not know its total ahead of time, and the count before it is the best
    /// estimate. Absent for a folder's first scan.
    expected: HashMap<i64, u64>,
    /// The folders the system will not let photon watch live, from the last report of
    /// each: they fall back to periodic rescans.
    degraded: HashMap<i64, bool>,
    /// A scan has reported done over an empty grid, and the grid has not been read since.
    settling: bool,
    export: Option<ExportProgress>,
    faces: Option<FaceProgress>,
}

impl Scans {
    /// A scan's report. `listed` is whether the folder list holds its folder, and
    /// `photos` how many the grid on screen holds.
    pub fn scan(&mut self, event: ScanProgressEvent, listed: bool, photos: usize) -> Asks {
        let previous = self.scans.insert(event.watched_id, event);
        // The first report heard of this scan: none before it, or the end of the last.
        let first = previous.is_none_or(|previous| previous.done);
        let was_running = previous.is_some_and(|previous| !previous.done);
        // The engine announces a scan's rebuild and then its end, and the grid on screen
        // may be a moment behind both: an empty library would read as "no scan, no
        // photos" for that long, the moment before its photos appear. So the scan counts
        // as running until the grid has been read again. Only over an empty grid: nothing
        // else asks. And only for a scan that was heard running: an unplugged drive is
        // polled twice a minute, each poll a done and nothing else, and counted as a
        // scan ending each one turned the panel to "Looking for photos…".
        let settle = event.done && was_running && photos == 0;
        self.settling |= settle;
        Asks {
            // A scan ending may have added folders. One starting in a folder the list
            // does not hold: the engine watches the Pictures folder by itself on a first
            // run, after the list may have been read - asked on the scan's first report
            // only, since the list may still lack the folder when it comes back and the
            // scan reports many times a second.
            folders: event.done || (!listed && first),
            expected: !event.done && first,
            settle,
        }
    }

    /// The count of the folder's photos, read for the first report of its scan. That
    /// report arrives after the first batch has been written, so what the scan has
    /// already added is taken back out: on a brand-new folder the count would otherwise
    /// equal what was added, and the bar would read 100% from the first tick and then run
    /// past it. Nothing to measure against is recorded as absent.
    pub fn expected_read(&mut self, first: &ScanProgressEvent, photo_count: i64) {
        let before = photo_count.saturating_sub_unsigned(first.added);
        match u64::try_from(before) {
            Ok(before) if before > 0 => self.expected.insert(first.watched_id, before),
            _ => self.expected.remove(&first.watched_id),
        };
    }

    /// The published grid has been read again.
    pub fn settled(&mut self) {
        self.settling = false;
    }

    pub fn folder_status(&mut self, event: FolderStatus) {
        self.degraded.insert(event.watched_id, event.degraded);
    }

    /// `done == total` is the end whatever happened on the way, an export in which every
    /// photo failed included: the line always goes.
    pub fn export(&mut self, event: ExportProgress) {
        self.export = (event.done < event.total).then_some(event);
    }

    /// The pass's last report says it is no longer running, which is what `face_line`
    /// has no line for.
    pub fn face(&mut self, event: FaceProgress) {
        self.faces = Some(event);
    }

    /// Whether any scan is running, in a folder the list holds or not: what an empty
    /// library asks before it says that nothing has been found.
    pub fn scanning(&self) -> bool {
        self.settling || self.scans.values().any(|scan| !scan.done)
    }

    pub fn is_scanning(&self, watched: i64) -> bool {
        self.scans.get(&watched).is_some_and(|scan| !scan.done)
    }

    /// Whether a folder that is *still watched* relies on periodic rescans. Asked of the
    /// folder list and not of the reports alone: removing a folder sends no report of its
    /// status, and a stale one would keep the notice for good.
    pub fn any_degraded(&self, watched: &[WatchedFolder]) -> bool {
        watched
            .iter()
            .any(|folder| self.degraded.get(&folder.id).copied().unwrap_or(false))
    }

    /// The status bar's lines, in the Svelte footer's order: the notice, the export, a
    /// line for each running scan in the watched folders' order, the face pass.
    pub fn lines(&self, watched: &[WatchedFolder]) -> Vec<Line> {
        let mut lines = Vec::new();
        if self.any_degraded(watched) {
            lines.push(Line {
                label: LIVE_UPDATES_LIMITED.to_owned(),
                bar: None,
            });
        }
        lines.extend(self.export.as_ref().map(export_line));
        for folder in watched {
            if let Some(scan) = self.scans.get(&folder.id).filter(|scan| !scan.done) {
                let expected = self.expected.get(&folder.id).copied();
                lines.push(scan_line(folder, scan, expected));
            }
        }
        lines.extend(face_line(self.faces.as_ref()));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Bar;
    use photon_engine::events::FacePhase;

    fn report(watched_id: i64, files_seen: u64, added: u64, done: bool) -> ScanProgressEvent {
        ScanProgressEvent {
            watched_id,
            files_seen,
            added,
            changed: 0,
            done,
            cancelled: false,
        }
    }

    fn folder(id: i64, path: &str) -> WatchedFolder {
        WatchedFolder {
            id,
            path: path.to_owned(),
            online: true,
        }
    }

    #[test]
    fn the_count_a_scan_is_measured_against_is_read_once_when_it_starts() {
        let mut scans = Scans::default();
        // The first report of a scan arrives after its first batch: 200 of the 5,000 rows
        // counted were added by this very scan, so 4,800 is what it started with.
        let first = report(1, 200, 200, false);
        assert!(scans.scan(first, true, 5000).expected);
        scans.expected_read(&first, 5000);
        let line = &scans.lines(&[folder(1, "/a")])[0];
        assert_eq!(
            line.label,
            "Scanning a… 200 of ~4,800 files (4%), 200 new or changed"
        );

        // Later reports of the same scan do not ask again: the count would drift upwards
        // with every batch and the bar would never reach the end.
        assert!(!scans.scan(report(1, 900, 300, false), true, 5000).expected);
        assert!(!scans.scan(report(1, 5000, 300, true), true, 5000).expected);
        // The next scan of the folder asks afresh.
        assert!(scans.scan(report(1, 64, 0, false), true, 5000).expected);
    }

    #[test]
    fn a_brand_new_folder_has_nothing_to_be_measured_against() {
        let mut scans = Scans::default();
        // Everything counted so far is this scan's own work.
        let first = report(2, 150, 150, false);
        scans.scan(first, true, 0);
        scans.expected_read(&first, 150);
        let line = &scans.lines(&[folder(2, "/new")])[0];
        assert_eq!(line.bar, Some(Bar::Unknown { beat: 150 }));
        // And a count that came to less than nothing is nothing.
        scans.expected_read(&first, 10);
        assert_eq!(scans.lines(&[folder(2, "/new")])[0].bar, line.bar);
        // A count there was, gone with the next scan that has none.
        scans.expected_read(&report(2, 1, 0, false), 40);
        assert!(matches!(
            scans.lines(&[folder(2, "/new")])[0].bar,
            Some(Bar::Share(_))
        ));
        scans.expected_read(&first, 150);
        assert_eq!(scans.lines(&[folder(2, "/new")])[0].bar, line.bar);
    }

    #[test]
    fn photon_is_scanning_while_any_scan_has_not_reported_done() {
        let mut scans = Scans::default();
        assert!(!scans.scanning());
        // Folder 7 is not in the list: the engine added it after the list was read.
        scans.scan(report(7, 1, 0, false), false, 10);
        assert!(scans.scanning() && scans.is_scanning(7) && !scans.is_scanning(1));
        scans.scan(report(7, 9, 2, true), false, 10);
        assert!(!scans.scanning() && !scans.is_scanning(7));
    }

    // The engine watches the Pictures folder by itself on a first run, after the list may
    // have been read: without this the status bar showed no scan and the sidebar said "No
    // folders yet" until the scan was done.
    #[test]
    fn the_folder_list_is_read_again_for_a_scan_in_a_folder_it_does_not_hold_once() {
        let mut scans = Scans::default();
        assert!(scans.scan(report(7, 1, 0, false), false, 0).folders);
        // The list may still not hold the folder when it comes back, and the scan reports
        // many times a second.
        assert!(!scans.scan(report(7, 2, 0, false), false, 0).folders);
        assert!(!scans.scan(report(7, 3, 0, false), false, 0).folders);
        // Its end is a reason of its own.
        assert!(scans.scan(report(7, 3, 0, true), false, 0).folders);
    }

    #[test]
    fn the_folder_list_is_not_read_again_for_a_scan_in_a_folder_it_holds_until_it_ends() {
        let mut scans = Scans::default();
        assert!(!scans.scan(report(7, 1, 0, false), true, 0).folders);
        assert!(!scans.scan(report(7, 2, 0, false), true, 0).folders);
        assert!(scans.scan(report(7, 2, 0, true), true, 0).folders);
    }

    // The engine announces the rebuild and then the scan's end, and the grid on screen
    // can be a moment behind both: for that long an empty grid and no scan read as "no
    // photos found", the moment before the photos appeared.
    #[test]
    fn an_empty_librarys_scan_counts_as_running_until_the_grid_has_been_read_again() {
        let mut scans = Scans::default();
        scans.scan(report(7, 0, 0, false), true, 0);
        let end = scans.scan(report(7, 5, 5, true), true, 0);
        assert!(end.settle);
        assert!(scans.scanning());
        assert!(!scans.is_scanning(7), "its line is gone all the same");
        scans.settled();
        assert!(!scans.scanning());
        // Over a grid with photos in it nothing waits for anything.
        scans.scan(report(7, 0, 0, false), true, 5);
        assert!(!scans.scan(report(7, 5, 0, true), true, 5).settle);
        assert!(!scans.scanning());
    }

    // An unplugged drive is polled twice a minute, and each poll sends a done and nothing
    // else. Counted as a scan that had just ended, each one turned an empty library's
    // panel to "Looking for photos…", every thirty seconds.
    #[test]
    fn a_done_with_no_scan_heard_running_is_not_a_scan_ending() {
        let mut scans = Scans::default();
        for _ in 0..2 {
            let poll = scans.scan(report(7, 0, 0, true), true, 0);
            // Nor one starting: there is no bar to count a folder's photos for.
            assert!(!poll.settle && !poll.expected);
            assert!(!scans.scanning());
        }
    }

    #[test]
    fn live_updates_are_limited_while_a_folder_still_watched_is_degraded() {
        let status = |watched_id, degraded| FolderStatus {
            watched_id,
            online: true,
            degraded,
        };
        let both = [folder(1, "/a"), folder(2, "/b")];
        let mut scans = Scans::default();
        assert!(!scans.any_degraded(&both));
        scans.folder_status(status(1, true));
        assert!(scans.any_degraded(&both));
        assert_eq!(scans.lines(&both)[0].label, LIVE_UPDATES_LIMITED);
        assert_eq!(scans.lines(&both)[0].bar, None);
        // The other folder is fine, and that changes nothing.
        scans.folder_status(status(2, false));
        assert!(scans.any_degraded(&both));
        // Removing a folder sends no report of its status: the notice goes with the
        // folder, not with a report that never comes.
        assert!(!scans.any_degraded(&both[1..]));
        assert!(scans.lines(&both[1..]).is_empty());
        scans.folder_status(status(1, false));
        assert!(!scans.any_degraded(&both));
    }

    // It has to go at the end whatever happened on the way, an export in which every
    // photo failed included.
    #[test]
    fn an_export_has_its_line_while_it_runs_and_none_at_its_end() {
        let mut scans = Scans::default();
        scans.export(ExportProgress {
            done: 3,
            total: 12,
            failed: 0,
        });
        assert_eq!(scans.lines(&[])[0].label, "Exporting 3 / 12…");
        // What could not be written is among what it is done with, not beside it.
        scans.export(ExportProgress {
            done: 6,
            total: 12,
            failed: 6,
        });
        assert_eq!(scans.lines(&[]).len(), 1);
        scans.export(ExportProgress {
            done: 12,
            total: 12,
            failed: 12,
        });
        assert!(scans.lines(&[]).is_empty());
    }

    #[test]
    fn the_face_pass_has_its_line_until_its_last_report() {
        let pass = |running| FaceProgress {
            phase: FacePhase::Detecting,
            checked: 5,
            total: 10,
            running,
        };
        let mut scans = Scans::default();
        scans.face(pass(true));
        assert_eq!(scans.lines(&[])[0].label, "Finding faces: 5 of 10");
        scans.face(pass(false));
        assert!(scans.lines(&[]).is_empty());
    }

    #[test]
    fn the_lines_are_the_notice_the_export_the_scans_in_the_folders_order_and_the_faces() {
        let mut scans = Scans::default();
        scans.face(FaceProgress {
            phase: FacePhase::Recognising,
            checked: 1,
            total: 2,
            running: true,
        });
        scans.scan(report(9, 9, 0, false), true, 1);
        scans.scan(report(4, 4, 0, false), true, 1);
        // Done, and one in a folder the list does not hold: neither has a line.
        scans.scan(report(5, 5, 0, true), true, 1);
        scans.scan(report(77, 1, 0, false), false, 1);
        scans.export(ExportProgress {
            done: 1,
            total: 2,
            failed: 0,
        });
        scans.folder_status(FolderStatus {
            watched_id: 4,
            online: true,
            degraded: true,
        });
        let watched = [folder(4, "/four"), folder(5, "/five"), folder(9, "/nine")];
        let labels: Vec<String> = (scans.lines(&watched).into_iter())
            .map(|line| line.label)
            .collect();
        assert_eq!(
            labels,
            [
                LIVE_UPDATES_LIMITED,
                "Exporting 1 / 2…",
                "Scanning four… 4 files",
                "Scanning nine… 9 files",
                "Recognising people: 1 of 2 faces",
            ]
        );
    }
}
