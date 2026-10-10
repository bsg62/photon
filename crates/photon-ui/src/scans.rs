//! What photon is doing in the background, as the engine has reported it: the scans, an
//! export, the face pass, and which folders cannot be watched live. What the Svelte store
//! holds of these (`library.svelte.ts`), with its rules.
//!
//! The application gives every report to this and is told what to do next - read the
//! folder list again, read the published grid again - which it does off its own thread.
//!
//! Not here: the note of a scan the interface was only *told* is running. The Svelte UI
//! asks which scans are running because the startup scans begin before a webview exists
//! and a report sent to no listener is lost; this interface's channel is made before the
//! engine is opened, so every report of every scan arrives.
//!
//! What is here and not there is the launch (`Launch`). The engine publishes its first
//! grid before it has settled which folders it watches and started their scans, and a
//! webview is nowhere near loaded by then; this interface is drawing.
//!
//! No egui here.

use crate::status::{LIVE_UPDATES_LIMITED, Line, export_line, face_line, scan_line};
use photon_core::library::WatchedFolder;
use photon_engine::events::{ExportProgress, FaceProgress, FolderStatus, ScanProgressEvent};
use std::collections::{HashMap, HashSet};

/// How far the launch has got, as far as an empty library needs to know.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Launch {
    /// The engine has not yet settled which folders it watches and started their first
    /// scans (`Engine::startup_scans_started`). The Pictures folder it watches by itself
    /// may not be watched yet, and a folder list read now may be from before it.
    #[default]
    Starting,
    /// It has, and the folder list has been asked for again.
    Listing,
    /// The list is read. Each folder it held has had a scan started, which reports at
    /// least its end unless it dies; these have not been heard from yet. A scan that
    /// panics reports nothing more, and an empty library waiting for it goes on looking
    /// until the folder is no longer watched (`listed`) - the engine does not say when a
    /// scan has died.
    Awaiting(HashSet<i64>),
}

/// What a scan's report asks of the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asks {
    /// Read the folder list again.
    pub folders: bool,
    /// Read the published grid again, and say so (`settled`).
    pub settle: bool,
}

#[derive(Default)]
pub struct Scans {
    /// The last report of each watched folder's scan.
    scans: HashMap<i64, ScanProgressEvent>,
    /// How many photos each folder held when its scan started, for the bar: a scan does
    /// not know its total ahead of time, and the count before it is the best estimate.
    /// The scan says it (`ScanProgressEvent::known`); counted here instead, after the
    /// scan's first report had been drawn, it held the scan's own first batches, and a
    /// first scan read "3,000 of ~500 files". Absent for a folder that held nothing.
    expected: HashMap<i64, u64>,
    /// How many reports each folder's current scan has made.
    heard: HashMap<i64, u64>,
    /// The folders the system will not let photon watch live, from the last report of
    /// each: they fall back to periodic rescans.
    degraded: HashMap<i64, bool>,
    /// A scan has reported done over an empty grid, and the grid has not been read since.
    settling: bool,
    export: Option<ExportProgress>,
    faces: Option<FaceProgress>,
    launch: Launch,
}

impl Scans {
    /// Whether the engine has yet to say that its first scans are started: asked of it
    /// until it has, since nothing reports the moment.
    pub fn starting(&self) -> bool {
        self.launch == Launch::Starting
    }

    /// The engine has settled its folders and started their scans. The folder list is to
    /// be read again, and the next one read is the one to go by (`listed`).
    pub fn started(&mut self) {
        if self.launch == Launch::Starting {
            self.launch = Launch::Listing;
        }
    }

    /// The folder list has been read. The first one read after the scans were started is
    /// the launch's: its folders are the ones whose scans are still owed a report.
    pub fn listed(&mut self, watched: &[WatchedFolder]) {
        match &mut self.launch {
            Launch::Starting => {}
            Launch::Listing => {
                let unheard = (watched.iter().map(|folder| folder.id))
                    .filter(|id| !self.scans.contains_key(id))
                    .collect();
                self.launch = Launch::Awaiting(unheard);
            }
            // A folder no longer watched is owed nothing: its scan may never have been
            // started, or may have been called off with the folder, and waited for it
            // would keep an empty library looking for good.
            Launch::Awaiting(unheard) => {
                unheard.retain(|id| watched.iter().any(|folder| folder.id == *id));
            }
        }
    }

    /// Whether it is known which folders photon watches: not before the list read after
    /// the first scans were started. Until then an empty list is "not read yet", and
    /// nothing may say that no folder is watched.
    pub fn known(&self) -> bool {
        matches!(self.launch, Launch::Awaiting(_))
    }

    /// A scan's report. `listed` is whether the folder list holds its folder, and
    /// `photos` how many the grid on screen holds.
    pub fn scan(&mut self, event: ScanProgressEvent, listed: bool, photos: usize) -> Asks {
        if let Launch::Awaiting(unheard) = &mut self.launch {
            unheard.remove(&event.watched_id);
        }
        let previous = self.scans.insert(event.watched_id, event);
        // The first report heard of this scan: none before it, or the end of the last.
        let first = previous.is_none_or(|previous| previous.done);
        let was_running = previous.is_some_and(|previous| !previous.done);
        let heard = self.heard.entry(event.watched_id).or_default();
        *heard = if first { 1 } else { *heard + 1 };
        // What the scan says its folder held. Its opening report does not know yet, and
        // leaves the count of the scan before standing for the moment until it does.
        match event.known {
            Some(0) => {
                self.expected.remove(&event.watched_id);
            }
            Some(held) => {
                self.expected.insert(event.watched_id, held);
            }
            None => {}
        }
        // The engine announces a scan's rebuild and then its end, and the grid on screen
        // can be behind both: a grid published while a step is on its way is not put on
        // screen until that step has answered (`Nav::adopt`), and for that long an empty
        // library would read as "no scan, no photos", with its photos already found. So
        // the scan counts as running until the grid has been read again. Only over an
        // empty grid: nothing else asks. And only for a scan that was heard running: an
        // unplugged drive is polled twice a minute, each poll a done and nothing else,
        // and counted as a scan ending each one turned the panel to "Looking for
        // photos…".
        let settle = event.done && was_running && photos == 0;
        self.settling |= settle;
        Asks {
            // A scan ending may have added folders. One starting in a folder the list
            // does not hold: the engine watches the Pictures folder by itself on a first
            // run, after the list may have been read - asked on the scan's first report
            // only, since the list may still lack the folder when it comes back and the
            // scan reports several times a second.
            folders: event.done || (!listed && first),
            settle,
        }
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
    ///
    /// A folder watched at launch whose scan has not been heard from counts: its scan is
    /// started, and has yet to find its root and say so.
    pub fn scanning(&self) -> bool {
        let awaited = matches!(&self.launch, Launch::Awaiting(unheard) if !unheard.is_empty());
        awaited || self.settling || self.scans.values().any(|scan| !scan.done)
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
                let beat = self.heard.get(&folder.id).copied().unwrap_or(0);
                lines.push(scan_line(folder, scan, expected, beat));
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
            known: None,
        }
    }

    /// `report`, from a scan that knows by now what its folder held.
    fn measured(report: ScanProgressEvent, held: u64) -> ScanProgressEvent {
        ScanProgressEvent {
            known: Some(held),
            ..report
        }
    }

    fn folder(id: i64, path: &str) -> WatchedFolder {
        WatchedFolder {
            id,
            path: path.to_owned(),
            online: true,
        }
    }

    // The scan says what its folder held before it, in every report but its first.
    #[test]
    fn a_scan_is_measured_against_what_it_says_its_folder_held() {
        let mut scans = Scans::default();
        let watched = [folder(1, "/a")];
        let line = |scans: &Scans| scans.lines(&watched)[0].clone();
        // Its opening report does not know yet.
        scans.scan(report(1, 0, 0, false), true, 4800);
        assert_eq!(line(&scans).label, "Scanning a… 0 files");
        scans.scan(measured(report(1, 200, 200, false), 4800), true, 4800);
        assert_eq!(
            line(&scans).label,
            "Scanning a… 200 of ~4,800 files (4%), 200 new or changed"
        );
        scans.scan(measured(report(1, 5000, 200, true), 4800), true, 5000);

        // The next scan's opening report leaves that count standing for the moment it
        // takes the scan to say its own.
        scans.scan(report(1, 0, 0, false), true, 5000);
        assert_eq!(line(&scans).label, "Scanning a… 0 of ~4,800 files (0%)");
        scans.scan(measured(report(1, 0, 0, false), 5000), true, 5000);
        assert_eq!(line(&scans).label, "Scanning a… 0 of ~5,000 files (0%)");
    }

    // A first scan, and the scan of a folder that has since lost every photo.
    #[test]
    fn a_folder_that_held_nothing_has_nothing_to_be_measured_against() {
        let mut scans = Scans::default();
        let watched = [folder(2, "/new")];
        let bar = |scans: &Scans| scans.lines(&watched)[0].bar;
        scans.scan(measured(report(2, 150, 150, false), 0), true, 0);
        assert_eq!(bar(&scans), Some(Bar::Unknown { beat: 1 }));
        // However much it has added by now, it is measured against nothing.
        scans.scan(measured(report(2, 3000, 3000, false), 0), true, 0);
        assert_eq!(bar(&scans), Some(Bar::Unknown { beat: 2 }));
        scans.scan(measured(report(2, 3000, 3000, true), 0), true, 3000);
        // A count there was is gone with a scan that says there is none.
        scans.scan(measured(report(2, 1, 0, false), 40), true, 0);
        assert!(matches!(bar(&scans), Some(Bar::Share(_))));
        scans.scan(measured(report(2, 2, 0, false), 0), true, 0);
        assert_eq!(bar(&scans), Some(Bar::Unknown { beat: 2 }));
    }

    // The bar of a scan with nothing to be measured against steps once for each report
    // heard of it, whatever the report counts, and starts again with the next scan.
    #[test]
    fn the_bar_of_a_first_scan_steps_once_a_report() {
        let mut scans = Scans::default();
        let watched = [folder(3, "/c")];
        let beat = |scans: &Scans| match scans.lines(&watched)[0].bar {
            Some(Bar::Unknown { beat }) => beat,
            other => panic!("{other:?}"),
        };
        let mut seen = Vec::new();
        for files in [0, 2500, 2500, 4500, 7500] {
            scans.scan(report(3, files, 0, false), true, 0);
            seen.push(beat(&scans));
        }
        assert_eq!(seen, [1, 2, 3, 4, 5]);
        scans.scan(report(3, 7500, 0, true), true, 0);
        scans.scan(report(3, 0, 0, false), true, 0);
        assert_eq!(beat(&scans), 1);
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
            assert!(!poll.settle);
            assert!(!scans.scanning());
        }
    }

    // The engine publishes its first grid before it has settled which folders it watches
    // and started their scans, and this interface is there to draw it: for those
    // moments an empty library is only known to be empty. Said then, "add a folder"
    // stood over a Pictures folder about to be watched, and "has found no photos" over
    // a folder whose scan was a moment from starting.
    #[test]
    fn nothing_is_known_of_the_folders_until_the_list_read_after_the_first_scans_started() {
        let mut scans = Scans::default();
        assert!(scans.starting() && !scans.known());
        // A list read before the engine has started its scans may be from before the
        // Pictures folder was watched: it settles nothing.
        scans.listed(&[]);
        assert!(scans.starting() && !scans.known());
        scans.started();
        assert!(!scans.starting() && !scans.known());
        scans.listed(&[folder(1, "/a")]);
        assert!(scans.known());
    }

    // After that, each folder watched at launch has had a scan started, which reports at
    // least its end: until it has been heard from, photon is looking.
    #[test]
    fn photon_is_looking_until_every_folder_watched_at_launch_has_been_heard_from() {
        let mut scans = Scans::default();
        // One scan was quick, and has reported before the list arrived.
        scans.scan(report(2, 0, 0, true), false, 0);
        scans.started();
        assert!(!scans.scanning(), "nothing is known to be looked in yet");
        scans.listed(&[folder(1, "/a"), folder(2, "/b"), folder(3, "/c")]);
        assert!(scans.scanning());
        // An unplugged drive's scan says only that it is done.
        scans.scan(report(3, 0, 0, true), true, 0);
        assert!(scans.scanning(), "the first folder has not been heard from");
        scans.scan(report(1, 0, 0, false), true, 0);
        assert!(scans.scanning(), "and now it is running");
        scans.scan(report(1, 4, 0, true), true, 4);
        assert!(!scans.scanning());
        // Only the folders of that first list are waited for: one watched later is not
        // owed a report by a launch it was no part of.
        scans.listed(&[folder(1, "/a"), folder(9, "/later")]);
        assert!(!scans.scanning());
        // And the launch is over once: told again that the scans are started, what is
        // known stays known.
        scans.started();
        assert!(scans.known());
    }

    // A folder that is no longer watched is owed no report: its scan may never have been
    // started, or been called off with the folder.
    #[test]
    fn a_folder_no_longer_watched_is_not_waited_for() {
        let mut scans = Scans::default();
        scans.started();
        scans.listed(&[folder(1, "/a"), folder(2, "/b")]);
        scans.scan(report(1, 0, 0, true), true, 0);
        assert!(scans.scanning(), "the second has not been heard from");
        scans.listed(&[folder(1, "/a")]);
        assert!(!scans.scanning());
    }

    #[test]
    fn a_library_that_watches_nothing_at_launch_is_looking_for_nothing() {
        let mut scans = Scans::default();
        scans.started();
        scans.listed(&[]);
        assert!(scans.known() && !scans.scanning());
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
