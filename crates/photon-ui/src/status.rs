//! What the status bar says for a running scan, an export or the face pass: the wording
//! and the arithmetic of a line. `ui/src/lib/status.ts`, and the lines `StatusBar.svelte`
//! writes beside it.
//!
//! No egui here.

use crate::grid::labels::grouped;
use photon_core::library::WatchedFolder;
use photon_engine::events::{ExportProgress, FacePhase, FaceProgress, ScanProgressEvent};

/// Said while a watched folder cannot be watched live.
pub const LIVE_UPDATES_LIMITED: &str =
    "Live updates limited — photon will re-check these folders periodically.";

/// The bar beside a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bar {
    /// How far along, 0 to 1.
    Share(f64),
    /// Nothing to measure against: a folder's first scan. `beat` is how many reports the
    /// scan has made, which is all that moves the bar - it has no clock of its own, and a
    /// bar that moved by itself would draw a frame sixty times a second for as long as
    /// the scan ran. Reports, not files: the engine sends one every quarter of a second
    /// at most, whatever the scan has read meanwhile, and a bar placed by the count of
    /// files hopped about, backwards as often as forwards.
    Unknown { beat: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub label: String,
    pub bar: Option<Bar>,
}

/// The last part of a path, whichever way its separators lean: the folder's own name.
fn last_segment(path: &str) -> &str {
    path.split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
}

fn count(number: u64) -> String {
    grouped(usize::try_from(number).unwrap_or(usize::MAX))
}

/// A scan walks the disk and does not know its total ahead of time, so the count the
/// folder had before it stands in for one (`expected`). A rescan of an unchanged folder
/// therefore runs to exactly 100%; one with new photos runs past it, which the bar clamps
/// and the label shows honestly as "of ~N". Photos the scan has added or replaced are
/// called out because they are the point of watching. `beat` is how many reports the
/// scan has made, for the bar of one with nothing to be measured against.
pub fn scan_line(
    watched: &WatchedFolder,
    scan: &ScanProgressEvent,
    expected: Option<u64>,
    beat: u64,
) -> Line {
    let seen = count(scan.files_seen);
    let (mut label, bar) = match expected.filter(|expected| *expected > 0) {
        Some(expected) => {
            let share = (scan.files_seen as f64 / expected as f64).min(1.0);
            let percent = (share * 100.0).round();
            (
                format!("{seen} of ~{} files ({percent}%)", count(expected)),
                Bar::Share(share),
            )
        }
        None => (format!("{seen} files"), Bar::Unknown { beat }),
    };
    let moved = scan.added + scan.changed;
    if moved > 0 {
        label.push_str(&format!(", {} new or changed", count(moved)));
    }
    Line {
        label: format!("Scanning {}… {label}", last_segment(&watched.path)),
        bar: Some(bar),
    }
}

/// The line for a running face pass, or none when there is nothing to say: no pass, or
/// nothing to count. Detecting counts photos, and its total includes photos whose
/// thumbnail is not made yet, so the bar can wait short of its end while thumbnails are
/// rendering; recognising counts faces.
pub fn face_line(progress: Option<&FaceProgress>) -> Option<Line> {
    let progress = progress.filter(|progress| progress.running && progress.total > 0)?;
    let counted = format!("{} of {}", count(progress.checked), count(progress.total));
    Some(Line {
        label: match progress.phase {
            FacePhase::Recognising => format!("Recognising people: {counted} faces"),
            FacePhase::Detecting => format!("Finding faces: {counted}"),
        },
        bar: Some(Bar::Share(
            (progress.checked as f64 / progress.total as f64).min(1.0),
        )),
    })
}

/// The line for a running export.
pub fn export_line(progress: &ExportProgress) -> Line {
    let mut label = format!(
        "Exporting {} / {}…",
        grouped(progress.done),
        grouped(progress.total)
    );
    if progress.failed > 0 {
        label.push_str(&format!(
            " ({} could not be written)",
            grouped(progress.failed)
        ));
    }
    let share = if progress.total > 0 {
        (progress.done as f64 / progress.total as f64).min(1.0)
    } else {
        0.0
    };
    Line {
        label,
        bar: Some(Bar::Share(share)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watched() -> WatchedFolder {
        WatchedFolder {
            id: 3,
            path: "/home/dh/Pictures".to_owned(),
            online: true,
        }
    }

    fn scan(files_seen: u64, added: u64, changed: u64) -> ScanProgressEvent {
        ScanProgressEvent {
            watched_id: 3,
            files_seen,
            added,
            changed,
            done: false,
            cancelled: false,
            known: None,
        }
    }

    fn faces(phase: FacePhase, checked: u64, total: u64, running: bool) -> FaceProgress {
        FaceProgress {
            phase,
            checked,
            total,
            running,
        }
    }

    #[test]
    fn a_rescan_is_measured_against_the_count_the_folder_had_last_time() {
        assert_eq!(
            scan_line(&watched(), &scan(1250, 0, 0), Some(5000), 3),
            Line {
                label: "Scanning Pictures… 1,250 of ~5,000 files (25%)".to_owned(),
                bar: Some(Bar::Share(0.25)),
            }
        );
    }

    #[test]
    fn a_first_scan_has_nothing_to_be_measured_against() {
        assert_eq!(
            scan_line(&watched(), &scan(1250, 0, 0), None, 3),
            Line {
                label: "Scanning Pictures… 1,250 files".to_owned(),
                bar: Some(Bar::Unknown { beat: 3 }),
            }
        );
        let none = scan_line(&watched(), &scan(1250, 0, 0), Some(0), 4);
        assert_eq!(none.bar, Some(Bar::Unknown { beat: 4 }));
    }

    #[test]
    fn new_photos_push_the_scan_past_the_old_count_and_the_line_says_so() {
        let line = scan_line(&watched(), &scan(5200, 200, 0), Some(5000), 1);
        assert_eq!(line.bar, Some(Bar::Share(1.0)));
        assert_eq!(
            line.label,
            "Scanning Pictures… 5,200 of ~5,000 files (100%), 200 new or changed"
        );
    }

    #[test]
    fn added_and_changed_photos_are_counted_together() {
        assert_eq!(
            scan_line(&watched(), &scan(10, 2, 3), None, 1).label,
            "Scanning Pictures… 10 files, 5 new or changed"
        );
    }

    // A share on Windows, a folder with a slash at its end: the name is its last part.
    #[test]
    fn a_folder_is_called_by_the_last_part_of_its_path() {
        let named = |path: &str| {
            let watched = WatchedFolder {
                path: path.to_owned(),
                ..watched()
            };
            scan_line(&watched, &scan(1, 0, 0), None, 1).label
        };
        assert_eq!(named(r"\\nas\photos\2024"), "Scanning 2024… 1 files");
        assert_eq!(named("/mnt/photos/"), "Scanning photos… 1 files");
        assert_eq!(named(r"D:\Bilder"), "Scanning Bilder… 1 files");
    }

    #[test]
    fn the_face_pass_says_nothing_when_it_is_not_running() {
        assert_eq!(face_line(None), None);
        assert_eq!(
            face_line(Some(&faces(FacePhase::Detecting, 5, 10, false))),
            None
        );
    }

    #[test]
    fn detection_counts_photos_checked_of_all_photos() {
        assert_eq!(
            face_line(Some(&faces(FacePhase::Detecting, 12_400, 98_000, true))),
            Some(Line {
                label: "Finding faces: 12,400 of 98,000".to_owned(),
                bar: Some(Bar::Share(12_400.0 / 98_000.0)),
            })
        );
    }

    #[test]
    fn recognition_counts_faces_of_all_faces() {
        assert_eq!(
            face_line(Some(&faces(FacePhase::Recognising, 1200, 4300, true))),
            Some(Line {
                label: "Recognising people: 1,200 of 4,300 faces".to_owned(),
                bar: Some(Bar::Share(1200.0 / 4300.0)),
            })
        );
    }

    // An empty library, or a first event before anything is counted, must not draw a bar.
    #[test]
    fn the_face_pass_never_shows_none_of_none() {
        assert_eq!(
            face_line(Some(&faces(FacePhase::Detecting, 0, 0, true))),
            None
        );
    }

    #[test]
    fn the_face_pass_does_not_run_past_its_end() {
        let line = face_line(Some(&faces(FacePhase::Detecting, 11, 10, true))).unwrap();
        assert_eq!(line.bar, Some(Bar::Share(1.0)));
    }

    #[test]
    fn an_export_counts_what_it_is_done_with_and_says_what_it_could_not_write() {
        let running = ExportProgress {
            done: 1200,
            total: 4800,
            failed: 0,
        };
        assert_eq!(
            export_line(&running),
            Line {
                label: "Exporting 1,200 / 4,800…".to_owned(),
                bar: Some(Bar::Share(0.25)),
            }
        );
        let failing = ExportProgress {
            failed: 3,
            ..running
        };
        assert_eq!(
            export_line(&failing).label,
            "Exporting 1,200 / 4,800… (3 could not be written)"
        );
    }

    // The words are the Svelte footer's, until the switch-over.
    #[test]
    fn the_lines_are_worded_as_the_svelte_footer_words_them() {
        let footer = include_str!("../../../ui/src/components/StatusBar.svelte");
        assert!(footer.contains(LIVE_UPDATES_LIMITED));
        assert!(footer.contains("could not be written)"));
        let status = include_str!("../../../ui/src/lib/status.ts");
        for words in [
            "new or changed",
            "Recognising people: ${counted} faces",
            "Finding faces: ${counted}",
            "label: `Scanning ${name}… ${parts.join(', ')}`",
        ] {
            assert!(status.contains(words), "{words}");
        }
    }
}
