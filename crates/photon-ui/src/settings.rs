//! Settings: which section is shown, what has been read of the settings it shows, and the
//! words - `Settings.svelte` and `settings.ts`. A dialog of the dialog layer
//! (`dialogs.rs`), drawn by `settings_view.rs`; the application reads and writes what it
//! shows (`app/settings.rs`).
//!
//! **Nothing is shown before the library has said it.** The interval, the shuffle and the
//! look-alikes are read as the dialog opens, and their controls take no press until then,
//! as the Svelte ones are disabled: a control that showed the default while the stored
//! value was on its way would be a value the library does not hold.

use photon_core::library::{GridTile, ThemeChoice};

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
}

impl Default for Seen {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            size: GridTile::Medium,
            find_faces: false,
            faces_line: None,
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
    /// The question answered: switch off, deleting the people named; or keep them.
    SwitchOff,
    KeepFaces,
}

/// The interval's field holds at most this many digits: the library clamps to a minute,
/// and a field that took any number of them would hold a number no `i64` is.
const INTERVAL_DIGITS: usize = 4;

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
    /// The People row's question: how many people switching off would delete.
    question: Option<i64>,
    pub memory: Option<Memory>,
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
            memory: None,
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

    /// The library stored an interval, clamped to what it takes: the field shows it.
    pub fn interval_stored(&mut self, seconds: i64) {
        if let Some(stored) = &mut self.stored {
            stored.interval = seconds;
        }
        self.interval = seconds.to_string();
    }

    /// The library would not store the interval: the field shows what it holds.
    pub fn interval_refused(&mut self) {
        if let Some(stored) = self.stored {
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
        if self.counting || self.question.is_some() {
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
            self.question = Some(named);
            false
        } else {
            true
        }
    }

    /// The count could not be read: nothing is switched off, and nothing asked.
    pub fn uncounted(&mut self) {
        self.counting = false;
    }

    /// The row's question, while it is asked: how many people switching off deletes.
    pub fn question(&self) -> Option<i64> {
        self.question
    }

    /// The question answered, either way.
    pub fn answered(&mut self) {
        self.question = None;
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

    // The words are the Svelte component's: read out of it, its lines joined, since a
    // paragraph there runs over several.
    #[test]
    fn the_words_are_the_svelte_dialogs() {
        let source = include_str!("../../../ui/src/components/Settings.svelte");
        let joined = (source.lines())
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");
        let mut words = vec![
            FOLDERS_HINT,
            TAGS_HINT,
            APPEARANCE_HINT,
            PHOTO_SIZE_HINT,
            SLIDESHOW_HINT,
            INTERVAL_BEFORE,
            SHUFFLE,
            LOOK_ALIKES_HINT,
            FIND_FACES_HINT,
            FIND_FACES_PRIVATE,
            FIND_FACES_BOX,
            STOP_FINDING,
        ];
        words.extend(SECTIONS.iter().map(|section| section.label()));
        words.extend(THEMES.iter().map(|(_, label)| *label));
        words.extend(DISTANCES.iter().flat_map(|d| [d.label, d.hint]));
        for words in words {
            assert!(
                joined.contains(words),
                "Settings.svelte no longer says {words:?}"
            );
        }
        let people = include_str!("../../../ui/src/lib/people.ts");
        assert!(
            people.lines().any(|line| line.contains(
                "`This deletes ${named} ${named === 1 ? 'person' : 'people'} you named and everything photon found.`"
            )),
            "people.ts no longer says the switch-off warning"
        );
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
        settings.interval_stored(60);
        assert_eq!(settings.interval(), "60");
        assert_eq!(settings.stored().map(|s| s.interval), Some(60));

        settings.type_interval("");
        assert_eq!(settings.commit_interval(), None);
        assert_eq!(settings.interval(), "60");
        settings.type_interval("60");
        assert_eq!(settings.commit_interval(), None);

        settings.type_interval("8");
        assert_eq!(settings.commit_interval(), Some(8));
        settings.interval_refused();
        assert_eq!(settings.interval(), "60");
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
        assert_eq!(settings.question(), Some(2));
        assert!(!settings.ask_off(), "asked again under the question");
        settings.answered();
        assert_eq!(settings.question(), None);
        assert!(settings.ask_off());
        settings.uncounted();
        assert!(settings.ask_off());
    }
}
