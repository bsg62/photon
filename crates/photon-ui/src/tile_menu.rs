//! The menu of a photo in the grid: which items it has, in which words, and which of
//! them work yet. `Grid.svelte`'s tile menu, item for item and string for string.
//!
//! In groups, as a file manager's menu is: the file itself, what the photo is marked as,
//! what it belongs to, where it goes. The items act on the whole selection - a
//! right-click outside it selects that photo first, so what the menu acts on is always
//! what is ringed.

use crate::{
    grid::labels::grouped,
    menu::{Item, Look},
    nav::Place,
};
use photon_core::{
    grid::GridView,
    library::{AlbumSummary, Person},
};

/// What an item of the tile's menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TileAction {
    Reveal,
    OpenInApp,
    CopyPhoto,
    /// Star the selection, or take its stars off.
    Star(bool),
    Compare,
    /// Hide the selection, or in Hidden bring it back.
    Hide(bool),
    AddKeyword,
    RemoveKeyword,
    AddToPerson,
    /// "Not this person", for the person whose view is shown: the id of a `p:` key.
    RemoveFromPerson(i64),
    RemoveFromAlbum(i64),
    Export,
    AddToAlbum(i64),
}

impl TileAction {
    /// Whether the item works yet. The others are in their place and take no press: what
    /// they open is a later sub-project's (the viewer's compare and its dialog that names
    /// a face, the platform's file manager and clipboard), and a menu that grew as they
    /// arrived would move every item under a hand that had learnt where it was.
    pub fn works(&self) -> bool {
        matches!(
            self,
            Self::Star(_)
                | Self::Hide(_)
                | Self::Export
                | Self::AddKeyword
                | Self::RemoveKeyword
                | Self::RemoveFromPerson(_)
                | Self::RemoveFromAlbum(_)
                | Self::AddToAlbum(_)
        )
    }
}

/// The photo a menu takes its direction from (`Selection::deciding`): the photo itself
/// when one is selected, and among several the one the star key goes by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deciding {
    pub video: bool,
    pub starred: bool,
}

/// What the menu is built from.
pub struct Facts<'a> {
    /// How many photos are selected.
    pub count: usize,
    /// The deciding photo. Of one photo it says which star item there is and whether
    /// the photo can be copied; of several, which star item the key would do, and the
    /// key's hint stands beside that one.
    pub deciding: Option<Deciding>,
    /// The view the grid shows.
    pub place: &'a Place,
    pub albums: &'a [AlbumSummary],
    pub people: &'a [Person],
    /// Whether photon finds faces: with the switch off there is no face to name.
    pub finds_faces: bool,
    /// Whether the keys are a Mac's: its chords are spelled with ⌘.
    pub mac: bool,
    /// Whether an export is running: one at a time, and "Export…" takes no press
    /// until the one that runs is over.
    pub exporting: bool,
}

/// The fewest and the most photos that can be compared side by side.
const COMPARE: std::ops::RangeInclusive<usize> = 2..=4;

/// "photo" for one and "12 photos" for several: what an item names as what it acts on.
pub fn subject(count: usize) -> String {
    if count == 1 {
        "photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

/// The album whose view is shown, when it is one of photon's own: only those offer
/// "Remove from". Asked this way round and not as "is it Picasa's": a Picasa album that
/// has just lost its last photo has left the list, and an id that is not listed must not
/// read as one that can be changed.
fn own_album<'a>(facts: &Facts<'a>) -> Option<&'a AlbumSummary> {
    let id: i64 = (facts.place.view == GridView::Album)
        .then(|| facts.place.arg.parse().ok())
        .flatten()?;
    (facts.albums.iter()).find(|album| album.id == id && !album.picasa)
}

/// The person whose view is shown, when photon named them: a `c:` key is a Picasa
/// contact, changed in Picasa. Their id and their name - none when the list no longer
/// has them, a person deleted while shown.
fn named_person<'a>(facts: &Facts<'a>) -> Option<(i64, &'a str)> {
    let key = (facts.place.view == GridView::Person).then_some(&facts.place.arg)?;
    let id = key.strip_prefix("p:")?.parse().ok()?;
    let name = (facts.people.iter())
        .find(|person| &person.key == key)
        .map_or("", |person| person.name.as_str());
    Some((id, name))
}

pub fn tile_menu(facts: &Facts<'_>) -> Vec<Item<TileAction>> {
    let pick = |label: String, hint: Option<&'static str>, look: Look, does: TileAction| {
        let works = does.works();
        Item::Pick {
            label,
            hint,
            works,
            look,
            does,
        }
    };
    let plain = |label: String, hint, does| pick(label, hint, Look::Plain, does);
    let subject = subject(facts.count);
    let mut items = Vec::new();

    if facts.count == 1 {
        let reveal = if facts.mac {
            "⌘+Shift+R"
        } else {
            "Ctrl+Shift+R"
        };
        items.push(plain(
            "Reveal in file manager".to_owned(),
            Some(reveal),
            TileAction::Reveal,
        ));
        items.push(plain(
            "Open in default app".to_owned(),
            None,
            TileAction::OpenInApp,
        ));
        // The clipboard holds one picture, and a video is not one.
        if !facts.deciding.is_some_and(|photo| photo.video) {
            let copy = if facts.mac { "⌘+C" } else { "Ctrl+C" };
            items.push(plain(
                "Copy photo".to_owned(),
                Some(copy),
                TileAction::CopyPhoto,
            ));
        }
        items.push(Item::Separator);
    }

    match facts.deciding.filter(|_| facts.count == 1) {
        // One photo whose star is known: the one item that changes it.
        Some(sole) => {
            let label = if sole.starred {
                "Unstar photo"
            } else {
                "Star photo"
            };
            items.push(plain(
                label.to_owned(),
                Some("."),
                TileAction::Star(!sole.starred),
            ));
        }
        // Several photos can be part starred, so both are offered, the key shown beside
        // the one it would do.
        None => {
            let starred = facts.deciding.is_some_and(|photo| photo.starred);
            let key = |star: bool| (star != starred).then_some(".");
            items.push(plain(
                format!("Star {subject}"),
                key(true),
                TileAction::Star(true),
            ));
            items.push(plain(
                format!("Unstar {subject}"),
                key(false),
                TileAction::Star(false),
            ));
        }
    }
    if COMPARE.contains(&facts.count) {
        items.push(plain(
            format!("Compare {subject}"),
            Some("C"),
            TileAction::Compare,
        ));
    }
    // Not "Delete": photon never deletes a photo. The file stays where it is, and the
    // Hidden view gives it back.
    let hidden = facts.place.view == GridView::Hidden;
    let hide = if hidden { "Unhide" } else { "Hide" };
    items.push(plain(
        format!("{hide} {subject}"),
        Some("H"),
        TileAction::Hide(!hidden),
    ));

    items.push(Item::Separator);
    items.push(plain(
        format!("Add keyword to {subject}…"),
        None,
        TileAction::AddKeyword,
    ));
    items.push(plain(
        format!("Remove keyword from {subject}…"),
        None,
        TileAction::RemoveKeyword,
    ));
    if facts.finds_faces {
        items.push(plain(
            format!("Add {subject} to a person…"),
            None,
            TileAction::AddToPerson,
        ));
    }
    if let Some((id, name)) = named_person(facts) {
        items.push(plain(
            format!("Remove {subject} from “{name}”"),
            None,
            TileAction::RemoveFromPerson(id),
        ));
    }
    if let Some(album) = own_album(facts) {
        items.push(plain(
            format!("Remove {subject} from “{}”", album.name),
            None,
            TileAction::RemoveFromAlbum(album.id),
        ));
    }

    items.push(Item::Separator);
    items.push(Item::Pick {
        label: format!("Export {subject}…"),
        hint: None,
        works: !facts.exporting,
        look: Look::Plain,
        does: TileAction::Export,
    });
    items.push(Item::Heading("Add to album"));
    // Adding is harmless twice over, so the album the photos are already in is not left
    // out: the grid's rows do not know their albums, and asking for each photo of a
    // selection to draw a menu would be a read for nothing.
    let own = facts.albums.iter().filter(|album| !album.picasa);
    let before = items.len();
    items.extend(own.map(|album| {
        pick(
            album.name.clone(),
            None,
            Look::Sub,
            TileAction::AddToAlbum(album.id),
        )
    }));
    if items.len() == before {
        items.push(Item::Note(NO_ALBUMS));
    }
    items
}

pub const NO_ALBUMS: &str = "No albums of your own yet — create one in the sidebar.";

/// "1 photo", or "12 photos": a count that a message names.
fn counted(count: usize) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

/// What is said when fewer stars were written than were asked for, and nothing when all
/// were. The engine skips a folder whose `.picasa.ini` it cannot write and answers with
/// how many landed, so a read-only folder in the selection costs a message and not the
/// other photos.
pub fn stars_missed(asked: usize, done: usize, starred: bool) -> Option<String> {
    let way = if starred { "starred" } else { "unstarred" };
    (done < asked).then(|| {
        format!(
            "{} of {} could not be {way}",
            counted(asked - done),
            grouped(asked)
        )
    })
}

/// What is said after photos were taken from a person: `people.ts`'s `removedMessage`.
/// `asked` is how many photos were given. A photo Picasa names the person on stays
/// theirs, since photon never writes Picasa's names, and this is how the user learns why
/// it did not leave the grid. Neither removed nor kept means none of the photos was
/// theirs by the time the write landed.
pub fn removed_message(name: &str, removed: usize, kept: usize, asked: usize) -> String {
    match (removed, kept) {
        (0, 0) => {
            let these = if asked == 1 {
                "this photo"
            } else {
                "these photos"
            };
            format!("{name} is no longer on {these}: nothing to remove.")
        }
        (removed, 0) => format!("Removed {} from {name}.", counted(removed)),
        (0, 1) => format!("1 stays with {name}: Picasa names {name} on it."),
        (0, kept) => format!(
            "{} stay with {name}: Picasa names {name} on them.",
            grouped(kept)
        ),
        (removed, kept) => {
            let (stay, it) = if kept == 1 {
                ("stays", "it")
            } else {
                ("stay", "them")
            };
            format!(
                "Removed {} from {name}. {} {stay}: Picasa names {name} on {it}.",
                counted(removed),
                grouped(kept)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: i64, name: &str, picasa: bool) -> AlbumSummary {
        AlbumSummary {
            id,
            name: name.to_owned(),
            count: 3,
            picasa,
        }
    }

    fn person(key: &str, name: &str) -> Person {
        Person {
            key: key.to_owned(),
            name: name.to_owned(),
            count: 3,
        }
    }

    struct World {
        count: usize,
        deciding: Option<Deciding>,
        place: Place,
        albums: Vec<AlbumSummary>,
        people: Vec<Person>,
        finds_faces: bool,
        mac: bool,
        exporting: bool,
    }

    impl World {
        /// One photo, not starred, in All photos, in a library with an album of its own
        /// and one of Picasa's.
        fn one() -> Self {
            Self {
                count: 1,
                deciding: Some(Deciding {
                    video: false,
                    starred: false,
                }),
                place: Place::of(GridView::All),
                albums: vec![album(4, "Lisbon", false), album(9, "From Picasa", true)],
                people: vec![person("p:7", "Anna"), person("c:ab12", "Ben")],
                finds_faces: false,
                mac: false,
                exporting: false,
            }
        }

        fn several(count: usize) -> Self {
            Self {
                count,
                ..Self::one()
            }
        }

        fn menu(&self) -> Vec<Item<TileAction>> {
            tile_menu(&Facts {
                count: self.count,
                deciding: self.deciding,
                place: &self.place,
                albums: &self.albums,
                people: &self.people,
                finds_faces: self.finds_faces,
                mac: self.mac,
                exporting: self.exporting,
            })
        }

        /// The menu as lines: a label with its hint and whether it works, `---` for a
        /// separator, a heading in brackets, a note in parentheses.
        fn lines(&self) -> Vec<String> {
            (self.menu().iter())
                .map(|item| match item {
                    Item::Pick {
                        label,
                        hint,
                        works,
                        look,
                        ..
                    } => {
                        let hint = hint.map_or(String::new(), |hint| format!(" [{hint}]"));
                        let off = if *works { "" } else { " (off)" };
                        let indent = if *look == Look::Sub { "  " } else { "" };
                        format!("{indent}{label}{hint}{off}")
                    }
                    Item::Separator => "---".to_owned(),
                    Item::Heading(heading) => format!("<{heading}>"),
                    Item::Note(note) => format!("({note})"),
                })
                .collect()
        }

        fn does(&self, label: &str) -> Option<TileAction> {
            self.menu().into_iter().find_map(|item| match item {
                Item::Pick {
                    label: at, does, ..
                } if at == label => Some(does),
                _ => None,
            })
        }
    }

    #[test]
    fn the_menu_of_one_photo() {
        assert_eq!(
            World::one().lines(),
            [
                "Reveal in file manager [Ctrl+Shift+R] (off)",
                "Open in default app (off)",
                "Copy photo [Ctrl+C] (off)",
                "---",
                "Star photo [.]",
                "Hide photo [H]",
                "---",
                "Add keyword to photo…",
                "Remove keyword from photo…",
                "---",
                "Export photo…",
                "<Add to album>",
                "  Lisbon",
            ]
        );
    }

    #[test]
    fn the_one_star_item_is_the_one_that_changes_it() {
        let mut w = World::one();
        assert_eq!(w.does("Star photo"), Some(TileAction::Star(true)));
        assert_eq!(w.does("Unstar photo"), None);
        w.deciding = Some(Deciding {
            video: false,
            starred: true,
        });
        assert_eq!(w.does("Unstar photo"), Some(TileAction::Star(false)));
        assert_eq!(w.does("Star photo"), None);
    }

    // The clipboard holds one picture, and a video is not one.
    #[test]
    fn a_video_is_not_offered_to_be_copied() {
        let mut w = World::one();
        w.deciding = Some(Deciding {
            video: true,
            starred: false,
        });
        assert!(!w.lines().iter().any(|line| line.starts_with("Copy photo")));
        assert!(w.lines().iter().any(|line| line.starts_with("Reveal")));
    }

    #[test]
    fn a_macs_chords_are_spelled_with_its_key() {
        let mut w = World::one();
        w.mac = true;
        let lines = w.lines();
        assert_eq!(lines[0], "Reveal in file manager [⌘+Shift+R] (off)");
        assert_eq!(lines[2], "Copy photo [⌘+C] (off)");
    }

    // Several photos can be part starred, so both are offered; the key's hint is beside
    // the one the key would do, which the deciding photo says.
    #[test]
    fn the_menu_of_several_photos() {
        let mut w = World::several(3);
        assert_eq!(
            w.lines(),
            [
                "Star 3 photos [.]",
                "Unstar 3 photos",
                "Compare 3 photos [C] (off)",
                "Hide 3 photos [H]",
                "---",
                "Add keyword to 3 photos…",
                "Remove keyword from 3 photos…",
                "---",
                "Export 3 photos…",
                "<Add to album>",
                "  Lisbon",
            ]
        );
        assert_eq!(w.does("Star 3 photos"), Some(TileAction::Star(true)));
        assert_eq!(w.does("Unstar 3 photos"), Some(TileAction::Star(false)));
        w.deciding = Some(Deciding {
            video: false,
            starred: true,
        });
        let lines = w.lines();
        assert_eq!(lines[0], "Star 3 photos");
        assert_eq!(lines[1], "Unstar 3 photos [.]");
    }

    #[test]
    fn two_to_four_photos_can_be_compared() {
        let compares = |count| {
            let lines = World::several(count).lines();
            lines.iter().any(|line| line.starts_with("Compare"))
        };
        assert!(
            !World::one()
                .lines()
                .iter()
                .any(|line| line.starts_with("Compare"))
        );
        assert!(compares(2));
        assert!(compares(4));
        assert!(!compares(5));
    }

    #[test]
    fn a_count_is_written_as_the_grid_writes_one() {
        assert_eq!(subject(1), "photo");
        assert_eq!(subject(2), "2 photos");
        assert_eq!(subject(12_345), "12,345 photos");
        let lines = World::several(12_345).lines();
        assert_eq!(lines[0], "Star 12,345 photos [.]");
    }

    #[test]
    fn in_hidden_the_item_brings_the_photos_back() {
        let mut w = World::one();
        assert_eq!(w.does("Hide photo"), Some(TileAction::Hide(true)));
        w.place = Place::of(GridView::Hidden);
        assert_eq!(w.does("Unhide photo"), Some(TileAction::Hide(false)));
        assert_eq!(w.does("Hide photo"), None);
        assert!(w.lines().contains(&"Unhide photo [H]".to_owned()));
    }

    #[test]
    fn an_album_of_ones_own_offers_to_take_the_photos_out_and_picasas_does_not() {
        let mut w = World::several(2);
        let removes = |w: &World| {
            let lines = w.lines();
            lines.into_iter().find(|line| line.contains(" from “"))
        };
        assert_eq!(removes(&w), None);
        w.place = Place {
            view: GridView::Album,
            arg: "4".to_owned(),
        };
        assert_eq!(
            removes(&w).as_deref(),
            Some("Remove 2 photos from “Lisbon”")
        );
        assert_eq!(
            w.does("Remove 2 photos from “Lisbon”"),
            Some(TileAction::RemoveFromAlbum(4))
        );
        // Picasa's are changed in Picasa.
        w.place.arg = "9".to_owned();
        assert_eq!(removes(&w), None);
        // And an album the list does not hold is not one to change.
        w.place.arg = "77".to_owned();
        assert_eq!(removes(&w), None);
        // The same id as a tag's name is not an album.
        w.place = Place {
            view: GridView::Tag,
            arg: "4".to_owned(),
        };
        assert_eq!(removes(&w), None);
    }

    #[test]
    fn a_person_photon_named_offers_to_take_the_photos_off_them() {
        let mut w = World::one();
        w.place = Place {
            view: GridView::Person,
            arg: "p:7".to_owned(),
        };
        assert!(w.lines().contains(&"Remove photo from “Anna”".to_owned()));
        assert_eq!(
            w.does("Remove photo from “Anna”"),
            Some(TileAction::RemoveFromPerson(7))
        );
        // A Picasa contact no person is linked to is changed in Picasa.
        w.place.arg = "c:ab12".to_owned();
        assert!(!w.lines().iter().any(|line| line.contains(" from “")));
        // A search for the same letters is not that person's view.
        w.place = Place {
            view: GridView::Search,
            arg: "p:7".to_owned(),
        };
        assert!(!w.lines().iter().any(|line| line.contains(" from “")));
        w.place.view = GridView::Person;
        // A person deleted while shown has no name left, and can still be taken off.
        w.place.arg = "p:99".to_owned();
        assert_eq!(
            w.does("Remove photo from “”"),
            Some(TileAction::RemoveFromPerson(99))
        );
    }

    // With the switch off photon has no face to name, and every photo would come back
    // "no unnamed face".
    #[test]
    fn naming_a_person_is_offered_only_while_photon_finds_faces() {
        let mut w = World::several(2);
        assert!(!w.lines().iter().any(|line| line.contains("to a person")));
        w.finds_faces = true;
        let lines = w.lines();
        let at = lines
            .iter()
            .position(|line| line == "Add 2 photos to a person… (off)");
        // After the keywords, before the album's "Remove from".
        assert_eq!(at, Some(7));
    }

    #[test]
    fn every_album_of_ones_own_is_offered_and_a_library_with_none_says_so() {
        let mut w = World::one();
        w.albums = vec![
            album(4, "Lisbon", false),
            album(9, "From Picasa", true),
            album(5, "To print", false),
        ];
        let lines = w.lines();
        assert_eq!(
            &lines[lines.len() - 3..],
            ["<Add to album>", "  Lisbon", "  To print"]
        );
        assert_eq!(w.does("To print"), Some(TileAction::AddToAlbum(5)));
        // The album shown is offered too: adding twice is harmless.
        w.place = Place {
            view: GridView::Album,
            arg: "4".to_owned(),
        };
        assert_eq!(w.does("Lisbon"), Some(TileAction::AddToAlbum(4)));

        w.albums = vec![album(9, "From Picasa", true)];
        let lines = w.lines();
        assert_eq!(
            &lines[lines.len() - 2..],
            [
                "<Add to album>",
                "(No albums of your own yet — create one in the sidebar.)"
            ]
        );
    }

    #[test]
    fn what_writes_works_and_what_waits_for_a_later_surface_does_not() {
        for works in [
            TileAction::Star(true),
            TileAction::Hide(false),
            TileAction::RemoveFromPerson(1),
            TileAction::RemoveFromAlbum(1),
            TileAction::AddToAlbum(1),
            TileAction::AddKeyword,
            TileAction::RemoveKeyword,
            TileAction::Export,
        ] {
            assert!(works.works(), "{works:?}");
        }
        for waits in [
            TileAction::Reveal,
            TileAction::OpenInApp,
            TileAction::CopyPhoto,
            TileAction::Compare,
            TileAction::AddToPerson,
        ] {
            assert!(!waits.works(), "{waits:?}");
        }
    }

    // One export at a time: the one that runs has the status bar's line, and a second
    // asked for behind it would start when the first had been forgotten.
    #[test]
    fn export_takes_no_press_while_an_export_runs() {
        let mut w = World::one();
        assert_eq!(w.does("Export photo…"), Some(TileAction::Export));
        assert!(w.lines().contains(&"Export photo…".to_owned()));
        w.exporting = true;
        assert!(w.lines().contains(&"Export photo… (off)".to_owned()));
        // Nothing else waits for it.
        assert_eq!(w.does("Hide photo"), Some(TileAction::Hide(true)));
    }

    // The words are the Svelte menu's: read out of the component that draws it.
    #[test]
    fn the_words_are_the_svelte_menus() {
        let svelte = include_str!("../../../ui/src/components/Grid.svelte");
        for words in [
            "Reveal in file manager",
            "Open in default app",
            "Copy photo",
            "'Unstar photo' : 'Star photo'",
            "Star {subject}",
            "Unstar {subject}",
            "Compare {subject}",
            "Unhide {subject}",
            "Hide {subject}",
            "Add keyword to {subject}…",
            "Remove keyword from {subject}…",
            "Add {subject} to a person…",
            "Remove {subject} from “{library.personName(library.info.person)}”",
            "Remove {subject} from “{library.albumName(albumId)}”",
            "Export {subject}…",
            ">Add to album<",
            NO_ALBUMS,
        ] {
            assert!(
                svelte.lines().any(|line| line.contains(words)),
                "Grid.svelte no longer says {words:?}"
            );
        }
    }

    #[test]
    fn stars_that_could_not_be_written_are_counted_of_those_asked() {
        assert_eq!(stars_missed(12, 12, true), None);
        assert_eq!(
            stars_missed(12, 9, true).as_deref(),
            Some("3 photos of 12 could not be starred")
        );
        assert_eq!(
            stars_missed(1, 0, false).as_deref(),
            Some("1 photo of 1 could not be unstarred")
        );
        assert_eq!(
            stars_missed(12_000, 500, true).as_deref(),
            Some("11,500 photos of 12,000 could not be starred")
        );
    }

    // people.test.ts's cases.
    #[test]
    fn taking_photos_from_a_person_says_what_came_of_it() {
        assert_eq!(
            removed_message("Anna", 3, 0, 3),
            "Removed 3 photos from Anna."
        );
        assert_eq!(
            removed_message("Anna", 1, 2, 3),
            "Removed 1 photo from Anna. 2 stay: Picasa names Anna on them."
        );
        assert_eq!(
            removed_message("Anna", 2, 1, 3),
            "Removed 2 photos from Anna. 1 stays: Picasa names Anna on it."
        );
        assert_eq!(
            removed_message("Anna", 0, 2, 2),
            "2 stay with Anna: Picasa names Anna on them."
        );
        assert_eq!(
            removed_message("Anna", 0, 1, 1),
            "1 stays with Anna: Picasa names Anna on it."
        );
        // A stale "Not Anna": her face was found again between the menu and the click.
        assert_eq!(
            removed_message("Anna", 0, 0, 1),
            "Anna is no longer on this photo: nothing to remove."
        );
        for several in [2, 4] {
            assert_eq!(
                removed_message("Anna", 0, 0, several),
                "Anna is no longer on these photos: nothing to remove."
            );
        }
    }
}
