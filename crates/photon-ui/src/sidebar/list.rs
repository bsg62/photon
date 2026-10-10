//! The sidebar as one list: the views every library has, the albums, the saved searches,
//! the people and the tags under their headings, and the folders under their years.
//! `FolderTree.svelte`'s markup, as entries a view draws a window of.
//!
//! A library has thousands of folders, and the People and Tags lists are as long as the
//! user has made them. The Svelte list leans on the browser to skip what is off screen
//! (`content-visibility`); here the list is a flat sequence with a height to each entry
//! (`Stack`), and the view draws the entries a position and a height take in, as the grid
//! does. Building it is an allocation an entry, so it is built again only when something it
//! was built from has moved (`List::follow`), which a key compared every frame decides.
//!
//! No egui here: an entry is what it says and what a click on it would ask for.

use super::{
    folders::{arrange_folders, folder_rows},
    rows::{Counts, Fixed, ROW, Today, fixed_rows},
};
use crate::{
    nav::{Place, Step},
    window_layout::OpenGroups,
};
use jiff::tz::TimeZone;
use photon_core::{
    grid::{FolderTally, GridView},
    library::{AlbumSummary, Folder, Person, SavedSearch, TagCount},
    sort::Sort,
};
use std::{collections::HashMap, ops::Range};

/// The room above a group's heading, which is part of its entry.
pub const GROUP_GAP: f64 = 8.0;
/// A year's heading: twelve points above it, sixteen for the line, two under.
pub const YEAR: f64 = 30.0;
/// The room above the first entry and under the last.
pub const ABOVE: f64 = 8.0;
pub const BELOW: f64 = 12.0;

const NO_PEOPLE: &str = "No named people yet. Name the faces photon found on the People page; names Picasa recorded are listed here too.";
const NO_TAGS: &str = "No keywords. photon reads them from the photos themselves.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Albums,
    Searches,
    People,
    Tags,
}

/// What an entry is. Its identity too: a click is answered with it, and the view tells
/// one entry from another by it from frame to frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum What {
    Fixed(Fixed),
    /// A group's heading, which folds and unfolds it.
    Group(Group),
    Album(i64),
    Search {
        id: i64,
        query: String,
    },
    /// A person, by their key.
    Person(String),
    Tag(String),
    /// A year's heading over its folders. Not a button.
    Year(i16),
    Folder(i64),
    /// The line an unfolded group shows when it holds nothing.
    Note(Group),
}

impl What {
    /// What a click on the entry asks of the engine's view, for those that ask that. A
    /// folder asks for a place in the grid, a group for its fold, and neither is a step.
    pub fn step(&self, today: Today) -> Option<Step> {
        match self {
            What::Fixed(row) => row.step(today),
            What::Album(id) => Some(Step::Album(*id)),
            What::Search { query, .. } => Some(Step::Search(query.clone())),
            What::Person(key) => Some(Step::Person(key.clone())),
            What::Tag(tag) => Some(Step::Tag(tag.clone())),
            What::Group(_) | What::Year(_) | What::Folder(_) | What::Note(_) => None,
        }
    }
}

/// The number at an entry's right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Of(usize),
    /// Groups of faces waiting for a name, beside People.
    ToName(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    None,
    /// A group's heading: whether it is unfolded.
    Open(bool),
    /// An album mirrored from Picasa's INI: listed and shown, never changed here.
    Picasa,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub what: What,
    pub label: String,
    pub count: Option<Count>,
    /// Whether this is the view the user is in, or is going to.
    pub active: bool,
    /// What the entry says when the pointer rests on it.
    pub hint: String,
    pub detail: Detail,
}

/// The user's collections, as the engine lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collections {
    pub albums: Vec<AlbumSummary>,
    pub searches: Vec<SavedSearch>,
    pub people: Vec<Person>,
    /// Groups of faces waiting for a name.
    pub to_name: usize,
    pub tags: Vec<TagCount>,
}

/// What the application holds of the library for the list: the collections and the folder
/// list, each as it was last read. Too large to compare, so they come with a number that
/// moves whenever either is replaced - and they can be replaced in no other way, so that a
/// list built from the old ones cannot be left standing.
#[derive(Default)]
pub struct Held {
    collections: Collections,
    folders: HashMap<i64, Folder>,
    generation: u64,
}

impl Held {
    pub fn collections(&self) -> &Collections {
        &self.collections
    }

    pub fn folders(&self) -> &HashMap<i64, Folder> {
        &self.folders
    }

    pub fn set_collections(&mut self, collections: Collections) {
        self.collections = collections;
        self.generation += 1;
    }

    pub fn set_folders(&mut self, folders: Vec<Folder>) {
        self.folders = folders
            .into_iter()
            .map(|folder| (folder.id, folder))
            .collect();
        self.generation += 1;
    }
}

/// Everything the list is built from.
pub struct Sources<'a> {
    pub counts: &'a Counts,
    /// Where the user is, or is going.
    pub at: &'a Place,
    pub today: Today,
    pub open: OpenGroups,
    pub sort: Sort,
    pub held: &'a Held,
    /// The published grid's folders, and the generation of its layout, which moves when
    /// they do (`Engine::published`).
    pub tallies: &'a [FolderTally],
    pub layout_gen: u64,
    pub zone: &'a TimeZone,
}

/// What a list was built from, as far as that can change: compared, never read.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    counts: Counts,
    at: Place,
    today: Today,
    open: OpenGroups,
    sort: Sort,
    held: u64,
    layout_gen: u64,
}

impl Sources<'_> {
    fn key(&self) -> Key {
        Key {
            counts: self.counts.clone(),
            at: self.at.clone(),
            today: self.today,
            open: self.open,
            sort: self.sort,
            held: self.held.generation,
            layout_gen: self.layout_gen,
        }
    }
}

fn count(number: i64) -> usize {
    usize::try_from(number).unwrap_or(0)
}

#[derive(Default)]
pub struct List {
    pub entries: Vec<Entry>,
    /// Moves every time the entries are built again.
    pub generation: u64,
    key: Option<Key>,
    /// Where each folder's entry is.
    folders: HashMap<i64, usize>,
}

impl List {
    /// Builds the list again when `sources` are not what it was last built from, and
    /// answers whether it did.
    pub fn follow(&mut self, sources: &Sources<'_>) -> bool {
        let key = sources.key();
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.entries = entries(sources);
        self.folders = (self.entries.iter().enumerate())
            .filter_map(|(index, entry)| match entry.what {
                What::Folder(id) => Some((id, index)),
                _ => None,
            })
            .collect();
        self.key = Some(key);
        self.generation += 1;
        true
    }

    /// Where `folder_id`'s entry is, when the list holds the folder.
    pub fn folder(&self, folder_id: i64) -> Option<usize> {
        self.folders.get(&folder_id).copied()
    }
}

fn plain(what: What, label: &str, count: Option<Count>, active: bool, hint: &str) -> Entry {
    Entry {
        what,
        label: label.to_owned(),
        count,
        active,
        hint: hint.to_owned(),
        detail: Detail::None,
    }
}

fn heading(group: Group, label: &str, count: Count, open: bool) -> Entry {
    Entry {
        detail: Detail::Open(open),
        ..plain(What::Group(group), label, Some(count), false, "")
    }
}

/// The entries, top to bottom.
fn entries(sources: &Sources<'_>) -> Vec<Entry> {
    let at = sources.at;
    let Collections {
        albums,
        searches,
        people,
        to_name,
        tags,
    } = sources.held.collections();
    let open = sources.open;

    let mut entries: Vec<Entry> = fixed_rows(sources.counts, at, sources.today)
        .into_iter()
        .map(|row| {
            let count = row.count.map(Count::Of);
            plain(
                What::Fixed(row.what),
                &row.label,
                count,
                row.active,
                &row.hint,
            )
        })
        .collect();

    // Albums: photon's own, and Picasa's, mirrored from its INI and never changed here.
    entries.push(heading(
        Group::Albums,
        "Albums",
        Count::Of(albums.len()),
        open.albums,
    ));
    if open.albums {
        entries.extend(albums.iter().map(|album| {
            let active = at.view == GridView::Album && at.arg == album.id.to_string();
            let counted = Some(Count::Of(count(album.count)));
            Entry {
                detail: if album.picasa {
                    Detail::Picasa
                } else {
                    Detail::None
                },
                ..plain(
                    What::Album(album.id),
                    &album.name,
                    counted,
                    active,
                    &album.name,
                )
            }
        }));
    }

    // Saved searches: a name over a query, run again on every visit. No count - one would
    // cost a pass over the library for every row on every change. And no heading while
    // there is none: it is the bookmark in the search box that makes one.
    if !searches.is_empty() {
        entries.push(heading(
            Group::Searches,
            "Searches",
            Count::Of(searches.len()),
            open.searches,
        ));
    }
    if open.searches {
        entries.extend(searches.iter().map(|search| {
            // The view shown when the search is exactly its query: refined by a word it
            // is another search.
            let active = at.view == GridView::Search && at.arg == search.query;
            let hint = if search.name == search.query {
                search.query.clone()
            } else {
                format!("{} — {}", search.name, search.query)
            };
            let what = What::Search {
                id: search.id,
                query: search.query.clone(),
            };
            plain(what, &search.name, None, active, &hint)
        }));
    }

    // People: those the user named among the faces photon found, and Picasa's contacts no
    // such person is linked to.
    let waiting = if *to_name > 0 {
        Count::ToName(*to_name)
    } else {
        Count::Of(people.len())
    };
    entries.push(heading(Group::People, "People", waiting, open.people));
    if open.people {
        entries.extend(people.iter().map(|person| {
            let active = at.view == GridView::Person && at.arg == person.key;
            let counted = Some(Count::Of(count(person.count)));
            plain(
                What::Person(person.key.clone()),
                &person.name,
                counted,
                active,
                &person.name,
            )
        }));
        if people.is_empty() {
            entries.push(plain(What::Note(Group::People), NO_PEOPLE, None, false, ""));
        }
    }

    // Tags: the keywords on at least one photo that is not hidden. One on hidden photos
    // alone would open an empty view.
    let shown: Vec<&TagCount> = tags.iter().filter(|tag| tag.count > 0).collect();
    entries.push(heading(
        Group::Tags,
        "Tags",
        Count::Of(shown.len()),
        open.tags,
    ));
    if open.tags {
        entries.extend(shown.iter().map(|tag| {
            let active = at.view == GridView::Tag && at.arg == tag.tag;
            let counted = Some(Count::Of(count(tag.count)));
            plain(
                What::Tag(tag.tag.clone()),
                &tag.tag,
                counted,
                active,
                &tag.tag,
            )
        }));
        if shown.is_empty() {
            entries.push(plain(What::Note(Group::Tags), NO_TAGS, None, false, ""));
        }
    }

    // The folders that hold photos in the view, in the user's sort: under the years of
    // their oldest photos by date, one headerless list otherwise.
    let folders = sources.held.folders();
    let rows = folder_rows(sources.tallies, folders, sources.zone);
    for group in arrange_folders(rows, sources.sort) {
        if let Some(year) = group.year {
            entries.push(plain(What::Year(year), &year.to_string(), None, false, ""));
        }
        entries.extend(group.rows.into_iter().map(|row| {
            // The path is what keeps the real name in sight beside an alias.
            let path = (folders.get(&row.folder_id)).map_or("", |folder| &folder.path);
            let counted = Some(Count::Of(row.count));
            plain(What::Folder(row.folder_id), &row.name, counted, false, path)
        }));
    }
    entries
}

/// How tall an entry is, for all but a note, whose words are wrapped to the list's width
/// and so measured where they are drawn.
pub fn height(what: &What) -> Option<f64> {
    match what {
        What::Group(_) => Some(GROUP_GAP + f64::from(ROW)),
        What::Year(_) => Some(YEAR),
        What::Note(_) => None,
        _ => Some(f64::from(ROW)),
    }
}

/// Where each entry of a list is, top to bottom.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    /// The top of each entry, and after the last one its bottom.
    edges: Vec<f64>,
    total: f64,
}

impl Stack {
    /// Entries of `heights`, with `above` over the first and `below` under the last.
    pub fn new(heights: impl IntoIterator<Item = f64>, above: f64, below: f64) -> Self {
        let mut edges = vec![above];
        let mut edge = above;
        for height in heights {
            edge += height.max(0.0);
            edges.push(edge);
        }
        Self {
            total: edge + below,
            edges,
        }
    }

    pub fn len(&self) -> usize {
        self.edges.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The height of everything, the room above and below included.
    pub fn total(&self) -> f64 {
        self.total
    }

    pub fn top(&self, index: usize) -> f64 {
        self.edges[index]
    }

    pub fn bottom(&self, index: usize) -> f64 {
        self.edges[index + 1]
    }

    /// The entries any part of which lies between `from` and `to`.
    pub fn range(&self, from: f64, to: f64) -> Range<usize> {
        let tops = &self.edges[..self.len()];
        // The last entry that begins at or before `from` is the first that can reach
        // into it.
        let start = tops.partition_point(|&top| top <= from).saturating_sub(1);
        // A view of no height holds nothing, not the entry its edge is in.
        let end = if to > from {
            tops.partition_point(|&top| top < to)
        } else {
            start
        };
        start..end.max(start)
    }
}

/// Where to put a list so that what lies between `top` and `bottom` is in view, by the
/// least movement, or nothing when it already is: `scrollIntoView`'s `nearest`.
pub fn into_view(top: f64, bottom: f64, position: f64, viewport: f64) -> Option<f64> {
    if top < position {
        Some(top)
    } else if bottom > position + viewport {
        // Not past its own top, for something taller than the view.
        Some((bottom - viewport).min(top))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::sort::{Grouping, SortKey};

    const TODAY: Today = Today { month: 7, day: 4 };
    /// Noon UTC on 2024-06-01 and on 2019-02-02.
    const IN_2024: i64 = 1_717_243_200;
    const IN_2019: i64 = 1_549_108_800;

    fn folder(id: i64, name: &str) -> (i64, Folder) {
        let folder = Folder {
            id,
            watched_id: 1,
            parent_id: Some(1),
            path: format!("/photos/{name}"),
            name: name.to_owned(),
            hidden: false,
            alias: None,
        };
        (id, folder)
    }

    fn tally(folder_id: i64, count: usize, taken_at_min: i64) -> FolderTally {
        FolderTally {
            folder_id,
            count,
            taken_at_min,
            bytes: 100 * folder_id,
            modified_ms: 0,
        }
    }

    fn collections() -> Collections {
        let album = |id, name: &str, count, picasa| AlbumSummary {
            id,
            name: name.to_owned(),
            count,
            picasa,
        };
        let tag = |tag: &str, count| TagCount {
            tag: tag.to_owned(),
            count,
            total: count.max(1),
        };
        Collections {
            albums: vec![
                album(4, "Best of", 12, false),
                album(9, "From Picasa", 3, true),
            ],
            searches: vec![SavedSearch {
                id: 2,
                name: "Lakes".to_owned(),
                query: "lake 2024".to_owned(),
                created_ms: 0,
            }],
            people: vec![Person {
                key: "p:7".to_owned(),
                name: "Anna".to_owned(),
                count: 31,
            }],
            to_name: 0,
            tags: vec![tag("coast", 5), tag("put away", 0), tag("lake", 2)],
        }
    }

    /// Everything a list is built from, owned, so that a test changes one thing.
    struct World {
        counts: Counts,
        at: Place,
        open: OpenGroups,
        sort: Sort,
        held: Held,
        tallies: Vec<FolderTally>,
        layout_gen: u64,
        zone: TimeZone,
    }

    impl World {
        fn new() -> Self {
            Self {
                counts: Counts::default(),
                at: Place::of(GridView::All),
                open: OpenGroups {
                    albums: true,
                    searches: true,
                    people: true,
                    tags: true,
                },
                sort: Sort::default(),
                held: Held {
                    collections: collections(),
                    folders: [folder(2, "rome"), folder(3, "oslo"), folder(4, "old")].into(),
                    generation: 1,
                },
                tallies: vec![
                    tally(2, 12, IN_2024),
                    tally(3, 3, IN_2024 + 86_400),
                    tally(4, 40, IN_2019),
                ],
                layout_gen: 1,
                zone: TimeZone::UTC,
            }
        }

        fn sources(&self) -> Sources<'_> {
            Sources {
                counts: &self.counts,
                at: &self.at,
                today: TODAY,
                open: self.open,
                sort: self.sort,
                held: &self.held,
                tallies: &self.tallies,
                layout_gen: self.layout_gen,
                zone: &self.zone,
            }
        }

        fn list(&self) -> List {
            let mut list = List::default();
            list.follow(&self.sources());
            list
        }

        fn labels(&self) -> Vec<String> {
            (self.list().entries.into_iter())
                .map(|entry| entry.label)
                .collect()
        }

        fn entry(&self, what: &What) -> Entry {
            (self.list().entries.into_iter())
                .find(|entry| entry.what == *what)
                .unwrap_or_else(|| panic!("no entry {what:?}"))
        }

        fn holds(&self, what: &What) -> bool {
            self.list().entries.iter().any(|entry| entry.what == *what)
        }
    }

    fn search() -> What {
        What::Search {
            id: 2,
            query: "lake 2024".to_owned(),
        }
    }

    #[test]
    fn the_list_runs_from_the_views_through_the_collections_to_the_folders() {
        let world = World::new();
        assert_eq!(
            world.labels(),
            [
                "All photos",
                "Starred",
                "Recent",
                "On this day",
                "Albums",
                "Best of",
                "From Picasa",
                "Searches",
                "Lakes",
                "People",
                "Anna",
                "Tags",
                "coast",
                "lake",
                "2024",
                "oslo",
                "rome",
                "2019",
                "old",
            ]
        );
    }

    #[test]
    fn a_heading_counts_what_is_under_it_and_says_whether_it_is_open() {
        let mut world = World::new();
        let albums = world.entry(&What::Group(Group::Albums));
        assert_eq!(albums.count, Some(Count::Of(2)));
        assert_eq!(albums.detail, Detail::Open(true));
        assert_eq!(
            world.entry(&What::Group(Group::Searches)).count,
            Some(Count::Of(1))
        );
        // The keyword on hidden photos alone is not counted, as it is not listed.
        assert_eq!(
            world.entry(&What::Group(Group::Tags)).count,
            Some(Count::Of(2))
        );
        assert!(!world.holds(&What::Tag("put away".to_owned())));

        world.open.albums = false;
        let folded = world.entry(&What::Group(Group::Albums));
        assert_eq!(folded.detail, Detail::Open(false));
        assert_eq!(folded.count, Some(Count::Of(2)), "counted folded too");
        assert!(!world.holds(&What::Album(4)));
        assert!(world.holds(&search()), "the other groups stay as they are");
    }

    #[test]
    fn each_group_folds_by_itself() {
        for (group, under) in [
            (Group::Albums, What::Album(4)),
            (Group::Searches, search()),
            (Group::People, What::Person("p:7".to_owned())),
            (Group::Tags, What::Tag("coast".to_owned())),
        ] {
            let mut world = World::new();
            assert!(world.holds(&under));
            match group {
                Group::Albums => world.open.albums = false,
                Group::Searches => world.open.searches = false,
                Group::People => world.open.people = false,
                Group::Tags => world.open.tags = false,
            }
            assert!(!world.holds(&under), "{group:?} folded");
            assert!(world.holds(&What::Group(group)), "its heading stays");
        }
    }

    // It is the bookmark in the search box that makes one; a heading over nothing would
    // offer nothing.
    #[test]
    fn searches_have_no_heading_while_there_is_none() {
        let mut world = World::new();
        world.held.collections.searches.clear();
        assert!(!world.holds(&What::Group(Group::Searches)));
        // Albums are made from the list itself, so its heading is always there.
        world.held.collections.albums.clear();
        assert_eq!(
            world.entry(&What::Group(Group::Albums)).count,
            Some(Count::Of(0))
        );
    }

    #[test]
    fn people_are_counted_until_there_are_faces_to_name() {
        let mut world = World::new();
        let people = What::Group(Group::People);
        assert_eq!(world.entry(&people).count, Some(Count::Of(1)));
        world.held.collections.to_name = 14;
        assert_eq!(world.entry(&people).count, Some(Count::ToName(14)));
    }

    #[test]
    fn an_open_group_with_nothing_in_it_says_so_and_a_folded_one_says_nothing() {
        let mut world = World::new();
        world.held.collections.people.clear();
        // Every keyword is on hidden photos alone.
        world.held.collections.tags.retain(|tag| tag.count == 0);
        let people = world.entry(&What::Note(Group::People));
        assert!(people.label.starts_with("No named people yet."));
        let tags = world.entry(&What::Note(Group::Tags));
        assert_eq!(
            tags.label,
            "No keywords. photon reads them from the photos themselves."
        );
        world.open.people = false;
        world.open.tags = false;
        assert!(!world.holds(&What::Note(Group::People)));
        assert!(!world.holds(&What::Note(Group::Tags)));
        // With someone in it there is nothing to say.
        let full = World::new();
        assert!(!full.holds(&What::Note(Group::People)));
        assert!(!full.holds(&What::Note(Group::Tags)));
    }

    #[test]
    fn an_album_carries_its_count_and_picasas_its_mark() {
        let world = World::new();
        let own = world.entry(&What::Album(4));
        assert_eq!((own.count, own.detail), (Some(Count::Of(12)), Detail::None));
        assert_eq!(world.entry(&What::Album(9)).detail, Detail::Picasa);
        assert_eq!(
            world.entry(&What::Person("p:7".to_owned())).count,
            Some(Count::Of(31))
        );
        assert_eq!(
            world.entry(&What::Tag("coast".to_owned())).count,
            Some(Count::Of(5))
        );
        // A saved search has no count.
        assert_eq!(world.entry(&search()).count, None);
    }

    #[test]
    fn the_entry_of_the_view_shown_is_the_active_one() {
        let at = |view, arg: &str| Place {
            view,
            arg: arg.to_owned(),
        };
        let cases = [
            (at(GridView::Album, "4"), What::Album(4)),
            (at(GridView::Person, "p:7"), What::Person("p:7".to_owned())),
            (at(GridView::Tag, "coast"), What::Tag("coast".to_owned())),
            (Place::search("lake 2024"), search()),
            (Place::of(GridView::Starred), What::Fixed(Fixed::Starred)),
        ];
        for (place, what) in &cases {
            let mut world = World::new();
            world.at = place.clone();
            let active: Vec<What> = (world.list().entries.into_iter())
                .filter(|entry| entry.active)
                .map(|entry| entry.what)
                .collect();
            assert_eq!(active, std::slice::from_ref(what), "at {place:?}");
        }
        // Another album, and a view whose argument only looks like an album's id.
        let mut world = World::new();
        world.at = at(GridView::Album, "9");
        assert!(!world.entry(&What::Album(4)).active);
        world.at = at(GridView::Tag, "4");
        assert!(!world.entry(&What::Album(4)).active);
    }

    // Refined by a word it is another search, and the saved one is not where the user is.
    #[test]
    fn a_saved_search_is_active_only_when_the_search_is_exactly_its_query() {
        let mut world = World::new();
        world.at = Place::search("lake 2024 anna");
        assert!(!world.entry(&search()).active);
        world.at = Place::search("lake 2024");
        assert!(world.entry(&search()).active);
    }

    #[test]
    fn an_entry_says_what_it_is_when_the_pointer_rests_on_it() {
        let mut world = World::new();
        assert_eq!(world.entry(&search()).hint, "Lakes — lake 2024");
        // A search saved under its own text says it once.
        world.held.collections.searches[0].name = "lake 2024".to_owned();
        assert_eq!(world.entry(&search()).hint, "lake 2024");
        // A folder says where it is, which keeps its real name in sight beside an alias.
        world.held.folders.get_mut(&2).unwrap().alias = Some("Roma".to_owned());
        let rome = world.entry(&What::Folder(2));
        assert_eq!(
            (rome.label.as_str(), rome.hint.as_str()),
            ("Roma", "/photos/rome")
        );
        assert_eq!(world.entry(&What::Album(4)).hint, "Best of");
    }

    #[test]
    fn folders_are_under_their_years_by_date_and_under_none_by_another_key() {
        let mut world = World::new();
        let folders = |world: &World| -> Vec<What> {
            (world.list().entries.into_iter())
                .map(|entry| entry.what)
                .filter(|what| matches!(what, What::Year(_) | What::Folder(_)))
                .collect()
        };
        assert_eq!(
            folders(&world),
            [
                What::Year(2024),
                What::Folder(3),
                What::Folder(2),
                What::Year(2019),
                What::Folder(4),
            ]
        );
        world.sort = Sort {
            key: SortKey::Size,
            reverse: false,
            group: Grouping::Folder,
        };
        // The biggest first, and no heading to split the order.
        assert_eq!(
            folders(&world),
            [What::Folder(4), What::Folder(3), What::Folder(2)]
        );
        assert_eq!(world.entry(&What::Folder(4)).count, Some(Count::Of(40)));
    }

    #[test]
    fn a_folder_the_list_has_not_read_yet_is_a_row_with_no_name() {
        let mut world = World::new();
        world.held.folders.remove(&3);
        let unnamed = world.entry(&What::Folder(3));
        assert_eq!((unnamed.label.as_str(), unnamed.hint.as_str()), ("", ""));
    }

    #[test]
    fn a_folders_entry_is_found_by_the_folder() {
        let list = World::new().list();
        let at = list.folder(2).expect("rome is listed");
        assert_eq!(list.entries[at].what, What::Folder(2));
        assert_eq!(list.folder(99), None);
    }

    #[test]
    fn a_click_asks_for_the_view_an_entry_names() {
        assert_eq!(What::Album(4).step(TODAY), Some(Step::Album(4)));
        assert_eq!(
            search().step(TODAY),
            Some(Step::Search("lake 2024".to_owned()))
        );
        assert_eq!(
            What::Person("p:7".to_owned()).step(TODAY),
            Some(Step::Person("p:7".to_owned()))
        );
        assert_eq!(
            What::Tag("coast".to_owned()).step(TODAY),
            Some(Step::Tag("coast".to_owned()))
        );
        assert_eq!(
            What::Fixed(Fixed::OnThisDay).step(TODAY),
            Some(Step::Search("on:07-04".to_owned()))
        );
        // A folder is a place in the grid, a heading a fold: neither is a view.
        for what in [
            What::Folder(2),
            What::Group(Group::Albums),
            What::Year(2024),
            What::Note(Group::Tags),
        ] {
            assert_eq!(what.step(TODAY), None, "{what:?}");
        }
    }

    // Building it is an allocation an entry: five thousand folders, a hundred and twenty
    // times a second while the grid scrolls.
    #[test]
    fn the_list_is_built_again_only_when_something_it_was_built_from_moved() {
        let mut world = World::new();
        let mut list = List::default();
        assert!(list.follow(&world.sources()), "the first time");
        let first = list.generation;
        assert!(!list.follow(&world.sources()));
        assert_eq!(list.generation, first);

        let mut moved = |world: &World, what: &str| {
            assert!(list.follow(&world.sources()), "{what}");
            assert!(!list.follow(&world.sources()), "{what}, once");
        };
        world.open.tags = false;
        moved(&world, "a fold");
        world.at = Place::of(GridView::Starred);
        moved(&world, "another view");
        world.counts.starred = 3;
        moved(&world, "a count");
        world.sort.reverse = true;
        moved(&world, "the sort");
        world.held.set_collections(collections());
        moved(&world, "the collections");
        world.held.set_folders(vec![folder(2, "rome").1]);
        moved(&world, "the folder list");
        world.layout_gen += 1;
        moved(&world, "the grid's folders");
        assert_eq!(list.generation, first + 7);
    }

    // The collections and the folder list are too large to compare, and are replaced only
    // by a setter that says so: the list drawn is of the ones that are held.
    #[test]
    fn collections_and_folders_replaced_are_what_the_list_is_built_from() {
        let mut world = World::new();
        let mut list = world.list();
        let holds = |list: &List, what: What| list.entries.iter().any(|entry| entry.what == what);
        assert!(holds(&list, What::Album(4)));

        let mut fewer = collections();
        fewer.albums.clear();
        world.held.set_collections(fewer);
        assert!(list.follow(&world.sources()));
        assert!(!holds(&list, What::Album(4)));
        assert!(world.held.collections().albums.is_empty());

        let mut renamed = folder(2, "rome").1;
        renamed.alias = Some("Roma".to_owned());
        world.held.set_folders(vec![renamed]);
        assert!(list.follow(&world.sources()));
        let rome = &list.entries[list.folder(2).expect("still listed")];
        assert_eq!(rome.label, "Roma");
        assert_eq!(
            world.held.folders().len(),
            1,
            "the list read is the whole of it"
        );
    }

    #[test]
    fn an_entry_is_as_tall_as_its_kind() {
        assert_eq!(height(&What::Folder(1)), Some(28.0));
        assert_eq!(height(&What::Fixed(Fixed::All)), Some(28.0));
        assert_eq!(height(&What::Group(Group::Tags)), Some(36.0));
        assert_eq!(height(&What::Year(2024)), Some(30.0));
        // Its words are wrapped to the list's width: measured where they are drawn.
        assert_eq!(height(&What::Note(Group::Tags)), None);
    }

    #[test]
    fn entries_are_stacked_under_the_room_above_them() {
        let stack = Stack::new([28.0, 36.0, 30.0], 8.0, 12.0);
        assert_eq!(stack.len(), 3);
        assert_eq!((stack.top(0), stack.bottom(0)), (8.0, 36.0));
        assert_eq!((stack.top(1), stack.bottom(1)), (36.0, 72.0));
        assert_eq!((stack.top(2), stack.bottom(2)), (72.0, 102.0));
        assert_eq!(stack.total(), 114.0);
        // Nothing listed is the room above and below, and no entry.
        let none = Stack::new([], 8.0, 12.0);
        assert!(none.is_empty());
        assert_eq!(none.total(), 20.0);
        assert_eq!(none.range(0.0, 500.0), 0..0);
    }

    #[test]
    fn the_entries_in_view_are_those_any_part_of_which_is() {
        // Tops at 8, 36, 64, 92; the last ends at 120.
        let stack = Stack::new([28.0; 4], 8.0, 12.0);
        assert_eq!(stack.range(0.0, 132.0), 0..4);
        // The top edge inside the second entry, the bottom edge inside the third.
        assert_eq!(stack.range(40.0, 70.0), 1..3);
        // An entry that ends exactly at the top edge is out, one that begins exactly at
        // the bottom edge too.
        assert_eq!(stack.range(36.0, 64.0), 1..2);
        // In the room above the first entry, and past the last.
        assert_eq!(stack.range(0.0, 8.0), 0..0);
        assert_eq!(stack.range(0.0, 9.0), 0..1);
        assert_eq!(stack.range(119.0, 400.0), 3..4);
        assert_eq!(stack.range(500.0, 900.0), 3..4);
        // A view of no height holds nothing.
        assert_eq!(stack.range(50.0, 50.0), 1..1);
    }

    #[test]
    fn a_row_out_of_view_is_brought_in_by_the_least_movement() {
        // A view 100 tall at 200: it shows 200 to 300.
        assert_eq!(into_view(220.0, 248.0, 200.0, 100.0), None);
        assert_eq!(into_view(200.0, 228.0, 200.0, 100.0), None);
        assert_eq!(into_view(272.0, 300.0, 200.0, 100.0), None);
        // Above: its top to the view's top. Below: its bottom to the view's bottom.
        assert_eq!(into_view(150.0, 178.0, 200.0, 100.0), Some(150.0));
        assert_eq!(into_view(190.0, 218.0, 200.0, 100.0), Some(190.0));
        assert_eq!(into_view(280.0, 308.0, 200.0, 100.0), Some(208.0));
        assert_eq!(into_view(500.0, 528.0, 200.0, 100.0), Some(428.0));
        // Taller than the view: its top is what is shown.
        assert_eq!(into_view(400.0, 600.0, 200.0, 100.0), Some(400.0));
    }
}
