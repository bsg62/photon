//! Settings' Folders: what is said of each watched folder, and the field a folder to add
//! is typed in - `settings.ts`'s `folderStatus` and `photoCountLabel`, and the Svelte
//! dialog's words.
//!
//! **Every watched folder is listed, whether or not it holds photos.** One that is
//! offline, or has never been scanned, has no section in the grid and so no row in the
//! sidebar: this list is the only place it can be rescanned or removed.
//!
//! The Svelte dialog adds a folder through the system's folder picker; this one has that
//! (`platform.rs`) and a field for its path beside it, where a folder chosen is put - the
//! one way to add a folder where no picker can be opened - with the export's field's rules
//! (`export_dialog.rs`): asked about as it is typed, off the window's thread, an answer
//! counting only for the text the field still holds, and what was said of the last path
//! standing while the next is asked about.

use crate::{export_dialog::Dest, status::last_segment, tag_manager::photos};
use photon_core::library::WatchedFolder;

pub const NO_FOLDERS: &str = "No folders yet. Add one to start building your library.";
pub const RESCAN: &str = "Rescan";
/// What a Rescan that takes no press says under the pointer.
pub const BEING_SCANNED: &str = "This folder is being scanned";
pub const REVEAL: &str = "Reveal";
pub const REMOVE: &str = "Remove…";
pub const REMOVE_YES: &str = "Remove";
pub const ADD_FOLDER: &str = "Add folder…";
pub const ADD: &str = "Add";
/// The picker's button, which waits for the picker.
pub const CHOOSE: &str = "Choose…";
/// The field's name, for whoever reads the window without seeing it.
pub const FIELD: &str = "Folder to add";
pub const PLACEHOLDER: &str = "Path of a folder";

/// The question asked in a folder's row before it is removed: photon never touches the
/// folder's files.
pub fn remove_question(path: &str) -> String {
    format!("Remove “{path}” from photon? Your files stay where they are.")
}

/// How a watched folder is doing, as its row says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Scanning,
    Degraded,
    Offline,
    Online,
}

/// One line for a watched folder. A running scan says the most about what the user is
/// waiting for, so it comes first; live updates limited matter only while the drive is
/// there to watch, so offline comes before them.
pub fn folder_status(online: bool, scanning: Option<u64>, degraded: bool) -> (Kind, String) {
    if let Some(seen) = scanning {
        let seen = crate::grid::labels::grouped(usize::try_from(seen).unwrap_or(usize::MAX));
        return (Kind::Scanning, format!("Scanning… {seen} files"));
    }
    if !online {
        return (Kind::Offline, "Offline".to_owned());
    }
    if degraded {
        return (Kind::Degraded, "Live updates limited".to_owned());
    }
    (Kind::Online, "Online".to_owned())
}

/// A watched folder as its row says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderRow {
    pub id: i64,
    /// The folder's own name, over its whole path.
    pub name: String,
    pub path: String,
    pub online: bool,
    pub kind: Kind,
    /// Its status, and its count of photos once that has been read - a number is shown
    /// beside the thing it was read with, or not at all, where the Svelte list says "0
    /// photos" until it is.
    pub details: String,
    /// Whether it is being scanned: its Rescan takes no press, `rescan_folder` doing
    /// nothing while a scan of it runs and saying nothing back.
    pub scanning: bool,
    /// Whether its removal has been asked for and the list not read since: the row stays
    /// until then, and offers nothing - a watched folder's number is handed out again once
    /// it is free, and a second Remove… of the row while the first was on its way removed
    /// whatever folder had been added meanwhile under that number.
    pub going: bool,
}

/// The row of `folder`: `scanning` with how many files its scan has seen.
pub fn row(
    folder: &WatchedFolder,
    scanning: Option<u64>,
    degraded: bool,
    count: Option<i64>,
) -> FolderRow {
    let (kind, status) = folder_status(folder.online, scanning, degraded);
    let details = match count {
        Some(count) => format!("{status} · {}", photos(count)),
        None => status,
    };
    FolderRow {
        id: folder.id,
        name: last_segment(&folder.path).to_owned(),
        path: folder.path.clone(),
        online: folder.online,
        kind,
        details,
        scanning: scanning.is_some(),
        going: false,
    }
}

/// The field a folder to add is typed in, while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adding {
    path: String,
    dest: Dest,
    /// What the last answer said against a path, or the library against the last one
    /// sent: kept while the next path is asked about.
    said: Option<String>,
    /// The number the path was sent under, while the library is adding it.
    sent: Option<u64>,
    sends: u64,
}

impl Default for Adding {
    fn default() -> Self {
        Self {
            path: String::new(),
            dest: Dest::Empty,
            said: None,
            sent: None,
            sends: 0,
        }
    }
}

impl Adding {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn dest(&self) -> &Dest {
        &self.dest
    }

    pub fn problem(&self) -> Option<&str> {
        self.said.as_deref()
    }

    pub fn busy(&self) -> bool {
        self.sent.is_some()
    }

    /// The field holds `text` now. Answers the text to ask about: none for an empty
    /// field, of which nothing is said, nor while a path is being added.
    pub fn type_in(&mut self, text: String) -> Option<String> {
        if self.busy() {
            return None;
        }
        self.path = text;
        if self.path.trim().is_empty() {
            self.dest = Dest::Empty;
            self.said = None;
            return None;
        }
        self.dest = Dest::Checking;
        Some(self.path.clone())
    }

    /// The answer about `asked`: the folder, or why not. It counts only for the text the
    /// field still holds.
    pub fn checked(&mut self, asked: &str, answer: Result<String, String>) {
        if asked != self.path || self.dest != Dest::Checking {
            return;
        }
        self.said = answer.as_ref().err().cloned();
        self.dest = match answer {
            Ok(folder) => Dest::Passed(folder),
            Err(why) => Dest::Refused(why),
        };
    }

    /// The folder to add, and the number it is sent under: none until a path has
    /// passed, and none while one is on its way.
    pub fn send(&mut self) -> Option<(String, u64)> {
        let Dest::Passed(folder) = &self.dest else {
            return None;
        };
        if self.busy() {
            return None;
        }
        self.sends += 1;
        self.sent = Some(self.sends);
        Some((folder.clone(), self.sends))
    }

    /// The library refused the folder sent under `sent` - inside one already watched,
    /// say: its words stand under the field, which keeps the path to be put right.
    pub fn refused(&mut self, sent: u64, said: String) {
        if self.sent == Some(sent) {
            self.sent = None;
            self.said = Some(said);
            self.dest = Dest::Refused(self.said.clone().unwrap_or_default());
        }
    }

    /// Whether the answer to `sent` is this field's: the one it is waiting for.
    pub fn waits_for(&self, sent: u64) -> bool {
        self.sent == Some(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `settings.test.ts`'s cases: a scan first, then offline, then the watcher.
    #[test]
    fn a_scan_is_said_first_and_offline_before_live_updates() {
        assert_eq!(
            folder_status(true, Some(1234), true),
            (Kind::Scanning, "Scanning… 1,234 files".to_owned())
        );
        assert_eq!(
            folder_status(false, Some(3), false).0,
            Kind::Scanning,
            "a scan of a folder last found offline"
        );
        assert_eq!(folder_status(false, None, true).0, Kind::Offline);
        assert_eq!(folder_status(true, None, true).0, Kind::Degraded);
        assert_eq!(folder_status(true, None, false).0, Kind::Online);
    }

    #[test]
    fn a_row_says_its_folders_name_its_status_and_its_count_once_read() {
        let folder = WatchedFolder {
            id: 3,
            path: "/home/ada/Pictures/Summer".to_owned(),
            online: true,
        };
        let read = row(&folder, None, false, Some(1234));
        assert_eq!(
            (read.name.as_str(), read.details.as_str(), read.scanning),
            ("Summer", "Online · 1,234 photos", false)
        );
        assert_eq!(row(&folder, None, false, None).details, "Online");
        assert!(row(&folder, Some(5), false, Some(1)).scanning);
    }

    // As the export's field: an answer counts for the text it was about; nothing is
    // sent before a path has passed, nor twice while one is on its way.
    #[test]
    fn a_folder_is_added_once_its_path_has_passed() {
        let mut adding = Adding::default();
        assert_eq!(adding.send(), None);
        assert_eq!(
            adding.type_in("/photos".to_owned()),
            Some("/photos".to_owned())
        );
        assert_eq!(
            adding.type_in("/photos/2024".to_owned()),
            Some("/photos/2024".to_owned())
        );
        adding.checked("/photos", Ok("/photos".to_owned()));
        assert_eq!(
            adding.dest(),
            &Dest::Checking,
            "an answer about another text"
        );
        adding.checked("/photos/2024", Ok("/photos/2024".to_owned()));
        let (folder, sent) = adding.send().expect("passed");
        assert_eq!(folder, "/photos/2024");
        assert_eq!(adding.send(), None, "on its way");
        assert_eq!(
            adding.type_in("/x".to_owned()),
            None,
            "no typing while it is added"
        );
        adding.refused(sent, "that folder is already watched".to_owned());
        assert_eq!(adding.problem(), Some("that folder is already watched"));
        assert_eq!(adding.path(), "/photos/2024");
        assert!(!adding.busy());
    }

    #[test]
    fn what_was_said_stands_until_the_next_answer() {
        let mut adding = Adding::default();
        adding.type_in("/nowhere".to_owned());
        adding.checked(
            "/nowhere",
            Err("There is no folder at this path".to_owned()),
        );
        adding.type_in("/nowhere2".to_owned());
        assert_eq!(adding.problem(), Some("There is no folder at this path"));
        adding.type_in(String::new());
        assert_eq!(adding.problem(), None, "an empty field says nothing");
    }
}
