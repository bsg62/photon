//! Settings: which section is shown, what has been read of the settings it shows, and the
//! words - `Settings.svelte` and `settings.ts`. A dialog of the dialog layer
//! (`dialogs.rs`), drawn by `settings_view.rs`; the application reads and writes what it
//! shows (`app/settings_dialog.rs`).
//!
//! **Nothing is shown before the library has said it.** The interval, the shuffle and the
//! look-alikes are read as the dialog opens, and their controls take no press until then,
//! as the Svelte ones are disabled: a control that showed the default while the stored
//! value was on its way would be a value the library does not hold.

use crate::{
    tag_manager::Renamer,
    watched_list::{Adding, FolderRow},
};
use photon_core::library::{GridTile, LibraryStats, TagRule, ThemeChoice};
use std::collections::HashMap;

/// The sections, in the list's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Folders,
    Appearance,
    Tags,
    Slideshow,
    Duplicates,
    People,
    Statistics,
    Shortcuts,
    About,
}

pub const SECTIONS: [Section; 9] = [
    Section::Folders,
    Section::Appearance,
    Section::Tags,
    Section::Slideshow,
    Section::Duplicates,
    Section::People,
    Section::Statistics,
    Section::Shortcuts,
    Section::About,
];

impl Section {
    /// Its name in the list, and the heading over it.
    pub fn label(self) -> &'static str {
        match self {
            Section::Folders => "Folders",
            Section::Appearance => "Appearance",
            Section::Tags => "Tags",
            Section::Slideshow => "Slideshow",
            Section::Duplicates => "Duplicates",
            Section::People => "People",
            Section::Statistics => "Statistics",
            Section::Shortcuts => "Shortcuts",
            Section::About => "About",
        }
    }
}

pub const TITLE: &str = "Settings";
pub const CLOSE: &str = "Close settings";

pub const FOLDERS_HINT: &str =
    "photon watches these folders for photos. Removing one never touches its files.";
pub const TAGS_HINT: &str = "Tags are the keywords in your photos. Renaming or removing one changes how photon shows it; your files keep their keywords.";

pub const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::System, "System"),
    (ThemeChoice::Light, "Light"),
    (ThemeChoice::Dark, "Dark"),
];
pub const APPEARANCE_HINT: &str = "System follows your desktop. The photo viewer is always dark, so every photo is seen against the same ground.";
pub const PHOTO_SIZE: &str = "Photo size";
pub const PHOTO_SIZE_HINT: &str = "How large the grid draws each photo.";

pub const SLIDESHOW_HINT: &str = "Press S in the viewer to play the current view from the photo on screen. Space pauses, the arrow keys step, Escape ends it.";
pub const INTERVAL_BEFORE: &str = "Show each photo for";
pub const INTERVAL_AFTER: &str = "seconds";
pub const SHUFFLE: &str = "Shuffle: play the photos in a mixed order";

/// One choice of the look-alikes: the Hamming distance the library stores, which it
/// clamps, so there is no table of names to distances to keep in step with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Distance {
    pub value: i64,
    pub label: &'static str,
    pub hint: &'static str,
}

pub const DISTANCES: [Distance; 3] = [
    Distance {
        value: 0,
        label: "Off",
        hint: "Only byte-identical files count as duplicates.",
    },
    Distance {
        value: 7,
        label: "Conservative",
        hint: "Finds every close copy: resized, re-saved, sent through a chat app. The default.",
    },
    Distance {
        value: 10,
        label: "Loose",
        hint: "Also looks for more heavily altered copies, and finds most of those.",
    },
];
pub const LOOK_ALIKES: &str = "Find look-alikes";
pub const LOOK_ALIKES_HINT: &str = "The Duplicates view already finds files that are byte-for-byte the same. This widens it to photos that are the same picture after a resize or a re-save, compared as shown - an edit counts as the one photo it came from, not a look-alike of it.";

/// What is said under the look-alikes of the distance the library holds: a distance none
/// of the three names is said as the default's, as the Svelte section says it.
pub fn distance_hint(stored: i64) -> &'static str {
    (DISTANCES.iter())
        .find(|distance| distance.value == stored)
        .unwrap_or(&DISTANCES[1])
        .hint
}

pub const FIND_FACES: &str = "Find faces";
pub const FIND_FACES_HINT: &str = "photon looks for faces in your photos and sorts them into groups, one per person as far as it can tell. Name them on the People page, in the sidebar, and find them with person:, has:face or faces:2+; the viewer's info panel shows them. It works in the background and can take hours on a large library; you can quit and it carries on next time. Until it has finished, a search for photos without faces also finds photos it has not reached yet.";
pub const FIND_FACES_PRIVATE: &str = "Everything stays on this computer. Switching this off deletes what photon found and the names you gave; faces named in Picasa are not affected.";
pub const FIND_FACES_BOX: &str = "Find faces in my photos";
/// The question's button that switches off, under the Svelte question's title; and the
/// one that does not.
pub const STOP_FINDING: &str = "Stop finding faces";
pub const CANCEL: &str = "Cancel";

/// What switching face detection off would delete that the user made: `people.ts`'s
/// `switchOffWarning`.
pub fn switch_off_warning(named: i64) -> String {
    let people = if named == 1 { "person" } else { "people" };
    format!("This deletes {named} {people} you named and everything photon found.")
}

pub const VERSION: &str = "Version";
pub const LIBRARY: &str = "Library";
pub const REVEAL: &str = "Reveal";
pub const MEMORY: &str = "Memory";
pub const LICENCE: &str = "Licence";

/// How often About reads photon's memory again while it is shown: `MEMORY_POLL_MS`. On
/// Linux each read walks the whole process table, so only while the section is shown.
pub const MEMORY_POLL_MS: f64 = 2000.0;

/// Whole megabytes below a gigabyte, gigabytes with one decimal above, in binary units:
/// `memoryAmount`.
pub fn memory_amount(bytes: u64) -> String {
    const MB: f64 = 1_048_576.0;
    const GB: f64 = 1_073_741_824.0;
    if (bytes as f64) < GB {
        format!("{} MB", (bytes as f64 / MB).round())
    } else {
        format!("{:.1} GB", bytes as f64 / GB)
    }
}

/// What the figure covers. The Svelte words name the web view, which this window has
/// none of: what it may count besides photon is a process photon started.
pub fn memory_scope(processes: u32) -> String {
    if processes <= 1 {
        "photon's own process".to_owned()
    } else {
        format!("photon and the processes it started, {processes} in all")
    }
}

/// What the stored settings were read as when the dialog opened, and what each write the
/// library answered has made of them since.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    /// Seconds a slideshow holds each photo.
    pub interval: i64,
    pub shuffle: bool,
    /// The look-alike distance.
    pub distance: i64,
}

/// What the dialog shows that the application holds, and gives it in every frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub theme: ThemeChoice,
    pub size: GridTile,
    /// Whether photon looks for faces: the engine's switch, a flag in memory.
    pub find_faces: bool,
    /// The face pass's line while it runs.
    pub faces_line: Option<String>,
    /// The watched folders, as their rows say them.
    pub folders: Vec<FolderRow>,
}

impl Default for Seen {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            size: GridTile::Medium,
            find_faces: false,
            faces_line: None,
            folders: Vec::new(),
        }
    }
}

/// What About says: read as the dialog opens, from what the engine holds in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct About {
    pub version: String,
    pub library: String,
    pub licence: String,
}

/// photon's memory, as last read, and how many processes it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub bytes: u64,
    pub processes: u32,
}

/// What a frame of the dialog asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Section(Section),
    Theme(ThemeChoice),
    Size(GridTile),
    /// What the interval's field holds now, and whether it is to be stored: Enter, or the
    /// field left.
    Interval {
        text: String,
        store: bool,
    },
    Shuffle(bool),
    Distance(i64),
    /// The Find faces box, ticked or not.
    FindFaces(bool),
    /// The question in a row answered, with what it offers or with Cancel.
    Yes,
    No,
    /// A watched folder scanned again.
    Rescan(i64),
    /// "Remove…" on a folder's row: its row asks.
    RemoveFolder {
        id: i64,
        path: String,
    },
    /// "Add folder…": the system's picker, the folder chosen watched; the field a path is
    /// typed in where no picker opens.
    AddOpen,
    /// "Choose…" beside the field: the system's folder picker.
    AddChoose,
    /// A watched folder's "Reveal": shown in the system's file manager.
    RevealWatched(i64),
    /// About's "Reveal": the library's file shown in the system's file manager.
    RevealLibrary,
    /// What the field holds now.
    AddPath(String),
    /// The path that has passed is to be added: Enter, or "Add".
    AddSend,
    /// The field closes: Escape while it has the keyboard, or its Cancel.
    AddClose,
    /// What the tags' filter holds now.
    Filter(String),
    /// "Rename" on a keyword's row, what its field holds, Enter, and Escape or the field
    /// left.
    RenameStart(String),
    RenameType(String),
    RenameCommit,
    RenameCancel,
    /// "Remove…" on a keyword's row: its row asks.
    RemoveTag(String),
    /// "Restore" on a change.
    Restore(String),
    /// A row of Statistics: Settings closes, and this is searched for.
    Search(String),
    /// The dialog closed by a press on the scrim in a frame that asked for something
    /// else as well: done after it.
    Close,
}

/// The interval's field holds at most this many digits: the library clamps to a minute,
/// and a field that took any number of them would hold a number no `i64` is.
const INTERVAL_DIGITS: usize = 4;

/// What is asked in a row - one dialog at a time, so a question is asked where it arises
/// and not in a dialog of its own, and one row at a time: asking another is the first
/// one's Cancel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// Find faces switched off with this many people named.
    SwitchOff(i64),
    RemoveFolder {
        id: i64,
        path: String,
    },
    RemoveTag(String),
    /// A keyword renamed onto one that is there, or listed under Changes: the words of
    /// the question, which the renamer holds.
    Rename(String),
}

impl Question {
    /// What the question says.
    pub fn words(&self) -> String {
        match self {
            Question::SwitchOff(named) => switch_off_warning(*named),
            Question::RemoveFolder { path, .. } => crate::watched_list::remove_question(path),
            Question::RemoveTag(tag) => crate::tag_manager::remove_question(tag),
            Question::Rename(words) => words.clone(),
        }
    }

    /// The button that does what is asked about.
    pub fn yes(&self) -> &'static str {
        match self {
            Question::SwitchOff(_) => STOP_FINDING,
            Question::RemoveFolder { .. } => crate::watched_list::REMOVE_YES,
            Question::RemoveTag(_) => crate::tag_manager::REMOVE_YES,
            Question::Rename(words) if words.starts_with("Merge") => crate::tag_manager::MERGE_YES,
            Question::Rename(_) => crate::tag_manager::RENAME_YES,
        }
    }
}

/// What Statistics has of the library.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Counted {
    #[default]
    NotAsked,
    Counting,
    Counted(Box<LibraryStats>),
    Failed,
}

/// The open dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub section: Section,
    pub seen: Seen,
    pub about: About,
    /// None until read.
    stored: Option<Stored>,
    /// What the interval's field holds.
    interval: String,
    /// The library is being asked how many people are named, before Find faces is
    /// switched off.
    counting: bool,
    /// The question a row asks.
    question: Option<Question>,
    /// What the tags' filter holds.
    pub filter: String,
    /// The keyword being renamed in its row.
    pub renamer: Renamer,
    /// The field a folder to add is typed in, while it is open.
    pub adding: Option<Adding>,
    /// The changes made to keywords so far, and each watched folder's count of photos:
    /// read as the dialog opens and at every change of the library's data. None until
    /// read.
    pub rules: Option<Vec<TagRule>>,
    pub counts: HashMap<i64, i64>,
    pub memory: Option<Memory>,
    /// The library counted, for Statistics: read when the section is first shown and not
    /// before - it reads every photo, and most openings of Settings are for something
    /// else - and not again while the dialog is open: it is a look at the library, not a
    /// gauge. A failed count is asked again when the section is left and come back to.
    pub stats: Counted,
    /// Which opening of Settings this is, in the session: what an answer to something
    /// asked in it is told apart by from one asked in a Settings closed since.
    pub opening: u64,
}

impl Settings {
    pub fn new(section: Section, about: About) -> Self {
        Self {
            section,
            seen: Seen::default(),
            about,
            stored: None,
            interval: String::new(),
            counting: false,
            question: None,
            filter: String::new(),
            renamer: Renamer::default(),
            adding: None,
            rules: None,
            counts: HashMap::new(),
            memory: None,
            stats: Counted::NotAsked,
            opening: 0,
        }
    }

    pub fn stored(&self) -> Option<Stored> {
        self.stored
    }

    /// What the library holds, read as the dialog opened.
    pub fn read(&mut self, stored: Stored) {
        self.stored = Some(stored);
        self.interval = stored.interval.to_string();
    }

    pub fn interval(&self) -> &str {
        &self.interval
    }

    /// What the field was typed to: its digits, and no more of them than an interval
    /// needs. Nothing is typed into it before the stored interval is known.
    pub fn type_interval(&mut self, text: &str) {
        if self.stored.is_none() {
            return;
        }
        self.interval = (text.chars())
            .filter(char::is_ascii_digit)
            .take(INTERVAL_DIGITS)
            .collect();
    }

    /// The seconds to store, when the field holds a number other than the stored one.
    /// A field that holds none goes back to what is stored, as the Svelte field does:
    /// not on every letter, which would fight "15" on its way through "1", but when it is
    /// left or Enter is pressed.
    pub fn commit_interval(&mut self) -> Option<i64> {
        let stored = self.stored?;
        match self.interval.parse::<i64>() {
            Ok(seconds) if seconds != stored.interval => Some(seconds),
            _ => {
                self.interval = stored.interval.to_string();
                None
            }
        }
    }

    /// The library stored an interval, clamped to what it takes, sent from the opening
    /// numbered `opening`: the field shows it. One sent from a Settings closed since is
    /// what the library holds now, and this one's field shows it only where nothing
    /// has been typed into it - over what is being typed, the answer would put back a
    /// number nobody typed here.
    pub fn interval_stored(&mut self, seconds: i64, opening: u64) {
        let Some(stored) = &mut self.stored else {
            return;
        };
        if opening == self.opening || self.interval == stored.interval.to_string() {
            self.interval = seconds.to_string();
        }
        stored.interval = seconds;
    }

    /// The library would not store the interval sent from the opening numbered
    /// `opening`: if that is this one, the field shows what the library holds.
    pub fn interval_refused(&mut self, opening: u64) {
        if let Some(stored) = self.stored
            && opening == self.opening
        {
            self.interval = stored.interval.to_string();
        }
    }

    pub fn shuffle_stored(&mut self, shuffle: bool) {
        if let Some(stored) = &mut self.stored {
            stored.shuffle = shuffle;
        }
    }

    pub fn distance_stored(&mut self, distance: i64) {
        if let Some(stored) = &mut self.stored {
            stored.distance = distance;
        }
    }

    /// Find faces unticked: whether the library is to be asked how many people are
    /// named. Not while it is being asked already, nor with the question up.
    pub fn ask_off(&mut self) -> bool {
        if self.counting || matches!(self.question, Some(Question::SwitchOff(_))) {
            return false;
        }
        self.counting = true;
        true
    }

    /// The library says how many people are named: whether to switch off now. With
    /// none it is no question, as in the Svelte section; with some the row asks first.
    pub fn named(&mut self, named: i64) -> bool {
        self.counting = false;
        if named > 0 {
            self.ask(Question::SwitchOff(named));
            false
        } else {
            true
        }
    }

    /// The count could not be read: nothing is switched off, and nothing asked.
    pub fn uncounted(&mut self) {
        self.counting = false;
    }

    /// The question a row asks, while it is asked.
    pub fn question(&self) -> Option<&Question> {
        self.question.as_ref()
    }

    /// A row asks `question`, in the place of any other: a rename's own question,
    /// replaced, is that rename declined.
    pub fn ask(&mut self, question: Question) {
        if let Some(Question::Rename(_)) = self.question {
            self.renamer.declined();
        }
        self.question = Some(question);
    }

    /// The question answered, either way: what it was.
    pub fn answered(&mut self) -> Option<Question> {
        self.question.take()
    }

    /// The question let go of, as its Cancel lets go of it: another keyword's rename begun
    /// under a rename's question, whose Merge would then have nothing to agree to. (One
    /// left standing in another section is no harm: it takes no Escape while it is not
    /// on screen - `SettingsView::claims_escape` - and is there again when its section is.)
    pub fn let_go_of_question(&mut self) {
        if let Some(Question::Rename(_)) = self.question.take() {
            self.renamer.declined();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about() -> About {
        About {
            version: "0.53.1".to_owned(),
            library: "/home/ada/.local/share/photon/library.db".to_owned(),
            licence: "MIT".to_owned(),
        }
    }

    fn read() -> Settings {
        let mut settings = Settings::new(Section::Slideshow, about());
        settings.read(Stored {
            interval: 4,
            shuffle: false,
            distance: 7,
        });
        settings
    }

    // A question says its words and offers its own button; another question in its
    // place, over a rename's, is that rename declined - its name typed stays, and the
    // field takes typing again.
    #[test]
    fn a_question_in_the_place_of_a_renames_declines_it() {
        let mut settings = read();
        assert_eq!(Question::SwitchOff(2).yes(), STOP_FINDING);
        assert_eq!(
            Question::RemoveFolder {
                id: 1,
                path: "/p".to_owned()
            }
            .words(),
            "Remove “/p” from photon? Your files stay where they are."
        );
        let tags = [photon_core::library::TagCount {
            tag: "lake".to_owned(),
            count: 1,
            total: 1,
        }];
        settings.renamer.start("hiking");
        settings.renamer.type_in("lake");
        let crate::tag_manager::Committed::Asks(words) = settings.renamer.commit(&tags, &[]) else {
            panic!("a merge asks");
        };
        settings.ask(Question::Rename(words));
        assert_eq!(settings.question().map(Question::yes), Some("Merge"));
        settings.ask(Question::RemoveTag("lake".to_owned()));
        assert_eq!(settings.renamer.asking(), None);
        settings.renamer.type_in("lakes");
        assert_eq!(settings.renamer.text(), "lakes");
    }

    #[test]
    fn the_switch_off_warning_counts_the_people() {
        assert_eq!(
            switch_off_warning(1),
            "This deletes 1 person you named and everything photon found."
        );
        assert_eq!(
            switch_off_warning(3),
            "This deletes 3 people you named and everything photon found."
        );
    }

    // `settings.test.ts`'s cases.
    #[test]
    fn memory_is_megabytes_below_a_gigabyte_and_gigabytes_above() {
        assert_eq!(memory_amount(0), "0 MB");
        assert_eq!(memory_amount(1_572_864), "2 MB");
        assert_eq!(memory_amount(350 * 1_048_576), "350 MB");
        assert_eq!(memory_amount(1_073_741_823), "1024 MB");
        assert_eq!(memory_amount(1_073_741_824), "1.0 GB");
        assert_eq!(memory_amount(1_288_490_189), "1.2 GB");
    }

    #[test]
    fn the_memory_says_what_it_covers() {
        assert_eq!(memory_scope(1), "photon's own process");
        assert_eq!(
            memory_scope(3),
            "photon and the processes it started, 3 in all"
        );
    }

    #[test]
    fn a_distance_none_of_the_three_names_is_said_as_the_default() {
        assert_eq!(distance_hint(0), DISTANCES[0].hint);
        assert_eq!(distance_hint(10), DISTANCES[2].hint);
        assert_eq!(distance_hint(5), DISTANCES[1].hint);
    }

    // Nothing is typed into a field whose stored value is not known: what it would hold
    // is not the library's.
    #[test]
    fn the_interval_takes_nothing_before_it_is_read_and_digits_after() {
        let mut settings = Settings::new(Section::Slideshow, about());
        settings.type_interval("9");
        assert_eq!(settings.interval(), "");
        assert_eq!(settings.commit_interval(), None);
        settings.read(Stored {
            interval: 4,
            shuffle: false,
            distance: 7,
        });
        assert_eq!(settings.interval(), "4");
        settings.type_interval("1a5-");
        assert_eq!(settings.interval(), "15");
        settings.type_interval("123456");
        assert_eq!(settings.interval(), "1234");
    }

    // Stored when it is another number; a field with none, or the stored one, writes
    // nothing and shows what is stored.
    #[test]
    fn the_interval_is_stored_when_it_is_another_number() {
        let mut settings = read();
        settings.type_interval("15");
        assert_eq!(settings.commit_interval(), Some(15));
        // The library clamps, and the field shows what it stored.
        settings.interval_stored(60, settings.opening);
        assert_eq!(settings.interval(), "60");
        assert_eq!(settings.stored().map(|s| s.interval), Some(60));

        settings.type_interval("");
        assert_eq!(settings.commit_interval(), None);
        assert_eq!(settings.interval(), "60");
        settings.type_interval("60");
        assert_eq!(settings.commit_interval(), None);

        settings.type_interval("8");
        assert_eq!(settings.commit_interval(), Some(8));
        settings.interval_refused(settings.opening);
        assert_eq!(settings.interval(), "60");
    }

    // An answer to an interval sent from a Settings closed since is what the library
    // holds, and does not write over what is being typed in this one; a refusal of it
    // says nothing here.
    #[test]
    fn an_interval_answered_for_another_opening_leaves_what_is_typed() {
        let mut settings = read();
        settings.opening = 2;
        settings.interval_stored(9, 1);
        assert_eq!(
            (settings.interval(), settings.stored().map(|s| s.interval)),
            ("9", Some(9)),
            "nothing typed here: the field shows what the library holds"
        );
        settings.type_interval("30");
        settings.interval_stored(12, 1);
        assert_eq!(
            (settings.interval(), settings.stored().map(|s| s.interval)),
            ("30", Some(12))
        );
        settings.interval_refused(1);
        assert_eq!(settings.interval(), "30");
        assert_eq!(settings.commit_interval(), Some(30));
    }

    // Switching off asks the library how many people are named, once; with none it
    // switches off, with some the row asks.
    #[test]
    fn switching_off_asks_only_when_people_are_named() {
        let mut settings = read();
        assert!(settings.ask_off());
        assert!(
            !settings.ask_off(),
            "asked again while the count is on its way"
        );
        assert!(settings.named(0));
        assert_eq!(settings.question(), None);

        assert!(settings.ask_off());
        assert!(!settings.named(2));
        assert_eq!(settings.question(), Some(&Question::SwitchOff(2)));
        assert!(!settings.ask_off(), "asked again under the question");
        settings.answered();
        assert_eq!(settings.question(), None);
        assert!(settings.ask_off());
        settings.uncounted();
        assert!(settings.ask_off());
    }
}
