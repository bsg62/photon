//! The People page's behaviour, apart from what draws it - `createPeoplePage` in
//! `people-page.svelte.ts`, with its tests' cases, and `nameChoice` from `people.ts`: the
//! sections as the library last answered, the groups hidden while a write on them is on
//! its way, and when the page is read again.
//!
//! The Svelte model awaits its reads; this one is told. The application asks for a read
//! with the number `reload` hands out and gives the answer back with it (`loaded`), and
//! gives a write's end to `named`: a number is when a read was *asked*, and a read asked
//! after a write ended has read the library after it.

use photon_core::library::{PageFace, PageGroup, PeoplePage as Page};
use std::collections::HashMap;

/// Faces a strip shows before "Show more".
pub const STRIP: usize = 12;

/// The least time between the starts of two reads that library changes ask for, in
/// milliseconds. A scan announces a data change with each rebuild, and each read reads every
/// visible face in the library: unthrottled, a burst of changes was a burst of whole reads,
/// redrawing the page faster than anyone can act on it.
pub const CHANGE_GAP_MS: f64 = 1000.0;

/// What a write refused because the group it named is gone is said as: a grouping run
/// emptied and deleted it, or another write merged it away, and the library's own words,
/// "that is not a named person", read as the user's mistake. Every such write reads the
/// page again, which is what the second half says.
pub const STALE_GROUP: &str = "That group changed; the page has been refreshed.";

/// A strip of faces on the page: a group's in one of its sections, or one of the two
/// strips that are no group's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Strip {
    Unnamed(i64),
    Suggestion(i64),
    Person(i64),
    Ignored(i64),
    /// The faces of unnamed groups of one.
    Single,
    /// The faces ignored one by one.
    IgnoredFaces,
}

/// What committing a typed name does, by the library's own rule (`library/people.rs`,
/// `clean` and `same_name`): trimmed, and compared without case - Unicode's, as Rust's
/// `to_lowercase` is, so "ÉMILE" is Émile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameChoice {
    Empty,
    /// The person being renamed, by exactly their own name: nothing to do.
    Same,
    New(String),
    /// A name someone already has, in any case: they are that person, not a second one.
    Merge {
        id: i64,
        name: String,
    },
}

/// What `typed` will do among `people`; `own` is the person being renamed, whom their own
/// name does not merge into. Only the case of their own name changing is a rename, which
/// the library allows: "anna" to "Anna" is how a name's case is put right.
pub fn name_choice(typed: &str, people: &[PageGroup], own: Option<i64>) -> NameChoice {
    let name = typed.trim();
    if name.is_empty() {
        return NameChoice::Empty;
    }
    let key = name.to_lowercase();
    let found = people.iter().find_map(|person| {
        let named = person.name.as_deref()?;
        (named.to_lowercase() == key).then_some((person.id, named))
    });
    match found {
        None => NameChoice::New(name.to_owned()),
        Some((id, named)) if Some(id) == own => {
            if named == name {
                NameChoice::Same
            } else {
                NameChoice::New(name.to_owned())
            }
        }
        Some((id, named)) => NameChoice::Merge {
            id,
            name: named.to_owned(),
        },
    }
}

/// The line under a name being typed: said before it is committed, because a name someone
/// has merges, and the user must see "Add to Anna" before pressing Enter.
pub fn name_hint(choice: &NameChoice) -> Option<String> {
    match choice {
        NameChoice::Merge { name, .. } => Some(format!("Add to {name} · Enter")),
        NameChoice::New(name) => Some(format!("New person “{name}” · Enter")),
        NameChoice::Empty | NameChoice::Same => None,
    }
}

/// The page's state between frames.
#[derive(Debug, Default)]
pub struct PeoplePage {
    page: Option<Page>,
    /// The number the last read was asked under; a write's end is stamped with it.
    tick: u64,
    /// Groups hidden while a write on them is on its way (`None`), and after it until a
    /// read asked after its end lands (the number it ended at). A read asked earlier may
    /// have read the library before the write, and would put back what the user has just
    /// named.
    hidden: HashMap<i64, Option<u64>>,
    /// When the last read was asked, in milliseconds on the frame's clock.
    last_start: Option<f64>,
    /// When a read that library changes put off is due.
    due: Option<f64>,
}

impl PeoplePage {
    /// The page as the library last answered: none until the first read lands.
    pub fn page(&self) -> Option<&Page> {
        self.page.as_ref()
    }

    /// A read asked for now, by the page itself - its opening, a write's end - which is
    /// never held back: the user is waiting on it. Answers the number to ask it under.
    pub fn reload(&mut self, now: f64) -> u64 {
        self.tick += 1;
        self.last_start = Some(now);
        self.due = None;
        self.tick
    }

    /// A change of the library's data: a read now if none began in the last
    /// `CHANGE_GAP_MS`, else one at the end of that gap, however many changes arrive
    /// meanwhile. A read the page's own write began counts, so the change that write
    /// announces waits its turn.
    pub fn changed(&mut self, now: f64) -> Option<u64> {
        if self.due.is_some() {
            return None;
        }
        match self.last_start {
            Some(last) if now < last + CHANGE_GAP_MS => {
                self.due = Some(last + CHANGE_GAP_MS);
                None
            }
            _ => Some(self.reload(now)),
        }
    }

    /// The read put off, once it is due.
    pub fn poll(&mut self, now: f64) -> Option<u64> {
        let due = self.due?;
        (now >= due).then(|| self.reload(now))
    }

    /// When the read put off is due, for a frame to be asked for then.
    pub fn due(&self) -> Option<f64> {
        self.due
    }

    /// The answer to the read asked under `number`. A reload keeps the order of the groups
    /// on screen: the library sorts Unnamed by size, and a read during a scan or a grouping
    /// run can reorder groups whose counts moved - which moved the row being typed in, and
    /// put another group under the pointer between press and release. Only the first
    /// answer is taken as sorted; new groups come after, in the library's order. People and
    /// Suggestions are by name, which only a rename moves.
    pub fn loaded(&mut self, number: u64, next: Page) {
        let shown = self.page.take();
        let unnamed = keep_order(shown.as_ref().map(|page| &page.unnamed), next.unnamed);
        let ignored = keep_order(
            shown.as_ref().map(|page| &page.ignored_groups),
            next.ignored_groups,
        );
        self.page = Some(Page {
            unnamed,
            ignored_groups: ignored,
            ..next
        });
        // What this read has seen land is shown as the library has it.
        self.hidden
            .retain(|_, ended| ended.is_none_or(|ended| ended >= number));
    }

    /// A group is being named: it leaves Unnamed at once, and its count with it.
    pub fn naming(&mut self, group: i64) {
        self.hidden.insert(group, None);
    }

    /// The write that named `group` is over: written, it stays hidden until a read asked
    /// after this lands; refused, it is put back. Either way the page is read again - a
    /// refusal can mean the page is stale - which the caller asks for with `reload`.
    pub fn named(&mut self, group: i64, written: bool) {
        if written {
            // Not a new number: a read asked from here on numbers itself past this one,
            // and one asked earlier carries a number at or below it, which `loaded` keeps.
            if let Some(ended) = self.hidden.get_mut(&group) {
                *ended = Some(self.tick);
            }
        } else {
            self.hidden.remove(&group);
        }
    }

    fn shown(&self, group: &PageGroup) -> bool {
        !self.hidden.contains_key(&group.id)
    }

    /// Unnamed groups, less those being named.
    pub fn unnamed(&self) -> Vec<&PageGroup> {
        let groups = self.page.iter().flat_map(|page| &page.unnamed);
        groups.filter(|group| self.shown(group)).collect()
    }

    /// Every unnamed group, listed or past the library's 200, less those being named: the
    /// listed ones leave at once, and so does their count.
    pub fn unnamed_count(&self) -> i64 {
        let Some(page) = &self.page else {
            return 0;
        };
        let leaving = page.unnamed.iter().filter(|g| !self.shown(g)).count();
        page.unnamed_count - leaving as i64
    }

    /// How many groups the library lists in Unnamed, of `unnamed_count`.
    pub fn unnamed_listed(&self) -> usize {
        self.page.as_ref().map_or(0, |page| page.unnamed.len())
    }

    pub fn suggestions(&self) -> &[PageGroup] {
        self.page.as_ref().map_or(&[], |page| &page.suggestions)
    }

    pub fn people(&self) -> &[PageGroup] {
        self.page.as_ref().map_or(&[], |page| &page.people)
    }

    pub fn ignored_groups(&self) -> Vec<&PageGroup> {
        let groups = self.page.iter().flat_map(|page| &page.ignored_groups);
        groups.filter(|group| self.shown(group)).collect()
    }

    fn group(&self, strip: Strip) -> Option<&PageGroup> {
        let page = self.page.as_ref()?;
        let (list, id) = match strip {
            Strip::Unnamed(id) => (&page.unnamed, id),
            Strip::Suggestion(id) => (&page.suggestions, id),
            Strip::Person(id) => (&page.people, id),
            Strip::Ignored(id) => (&page.ignored_groups, id),
            Strip::Single | Strip::IgnoredFaces => return None,
        };
        list.iter().find(|group| group.id == id)
    }

    /// The faces a strip shows.
    pub fn faces(&self, strip: Strip) -> &[PageFace] {
        match (strip, &self.page) {
            (_, None) => &[],
            (Strip::Single, Some(page)) => &page.single_faces,
            (Strip::IgnoredFaces, Some(page)) => &page.ignored_faces,
            _ => self.group(strip).map_or(&[], |group| &group.faces),
        }
    }

    /// How many faces a strip stands for, shown or not.
    pub fn count(&self, strip: Strip) -> i64 {
        match (strip, &self.page) {
            (_, None) => 0,
            (Strip::Single, Some(page)) => page.single_count,
            (Strip::IgnoredFaces, Some(page)) => page.ignored_faces.len() as i64,
            _ => self.group(strip).map_or(0, |group| group.face_count),
        }
    }

    /// Whether the page, read, has nothing to show at all.
    pub fn empty(&self) -> bool {
        self.page.is_some()
            && self.unnamed().is_empty()
            && self.count(Strip::Single) == 0
            && self.suggestions().is_empty()
            && self.people().is_empty()
            && self.ignored_groups().is_empty()
            && self.count(Strip::IgnoredFaces) == 0
    }

    /// What a name typed for a group, or as `own`'s new name, will do.
    pub fn choice(&self, typed: &str, own: Option<i64>) -> NameChoice {
        name_choice(typed, self.people(), own)
    }
}

/// `next` in the order `shown` had them, the groups new to it after in the library's order.
fn keep_order(shown: Option<&Vec<PageGroup>>, mut next: Vec<PageGroup>) -> Vec<PageGroup> {
    let Some(shown) = shown else {
        return next;
    };
    let at: HashMap<i64, usize> = (shown.iter().enumerate())
        .map(|(n, group)| (group.id, n))
        .collect();
    // A stable sort: the known groups by their place, every new one after them, among
    // themselves as the library had them.
    next.sort_by_key(|group| at.get(&group.id).copied().unwrap_or(usize::MAX));
    next
}

/// "1 group" / "12 groups", with the count grouped in thousands.
pub fn counted(n: i64, one: &str, many: &str) -> String {
    let words = if n == 1 { one } else { many };
    let n = crate::grid::labels::grouped(usize::try_from(n).unwrap_or(0));
    format!("{n} {words}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(id: i64) -> PageFace {
        PageFace {
            id,
            item_id: id,
            thumb_key: format!("k{id}"),
            confirmed: false,
            person_id: None,
        }
    }

    fn group(id: i64, name: Option<&str>, faces: Vec<PageFace>, face_count: i64) -> PageGroup {
        PageGroup {
            id,
            name: name.map(str::to_owned),
            face_count,
            faces,
            offer: None,
        }
    }

    fn two(id: i64, face_count: i64) -> PageGroup {
        group(id, None, vec![face(1), face(2)], face_count)
    }

    fn answer(unnamed: Vec<PageGroup>) -> Page {
        Page {
            unnamed_count: unnamed.len() as i64,
            unnamed,
            ..Page::default()
        }
    }

    fn loaded(model: &mut PeoplePage, page: Page) {
        let number = model.reload(0.0);
        model.loaded(number, page);
    }

    fn ids(groups: &[&PageGroup]) -> Vec<i64> {
        groups.iter().map(|group| group.id).collect()
    }

    #[test]
    fn the_sections_are_read() {
        let mut model = PeoplePage::default();
        assert!(model.page().is_none());
        assert!(!model.empty(), "not known to be empty before it is read");
        loaded(
            &mut model,
            answer(vec![group(1, None, vec![face(1), face(2), face(3)], 5)]),
        );
        let faces: Vec<i64> = model
            .faces(Strip::Unnamed(1))
            .iter()
            .map(|f| f.id)
            .collect();
        assert_eq!(faces, [1, 2, 3]);
        assert_eq!(model.count(Strip::Unnamed(1)), 5);
        assert!(!model.empty());
        loaded(&mut model, Page::default());
        assert!(model.empty());
    }

    #[test]
    fn a_reload_keeps_the_order_of_the_groups_already_shown() {
        let mut model = PeoplePage::default();
        let mut first = answer(vec![two(1, 9), two(2, 5), two(3, 3)]);
        first.ignored_groups = vec![two(7, 4), two(8, 2)];
        loaded(&mut model, first);
        // Group 2 has grown past group 1, group 3 has gone, group 4 is new and the
        // largest; the ignored groups' counts have swapped and group 9 is new.
        let mut next = answer(vec![two(4, 20), two(2, 12), two(1, 9)]);
        next.ignored_groups = vec![two(9, 6), two(8, 5), two(7, 4)];
        loaded(&mut model, next);
        assert_eq!(ids(&model.unnamed()), [1, 2, 4]);
        let counts: Vec<i64> = model.unnamed().iter().map(|g| g.face_count).collect();
        assert_eq!(counts, [9, 12, 20]);
        assert_eq!(ids(&model.ignored_groups()), [7, 8, 9]);
    }

    #[test]
    fn new_groups_keep_the_librarys_order_among_themselves() {
        let mut model = PeoplePage::default();
        loaded(&mut model, answer(vec![two(1, 3)]));
        loaded(&mut model, answer(vec![two(6, 9), two(5, 7), two(1, 3)]));
        assert_eq!(ids(&model.unnamed()), [1, 6, 5]);
    }

    #[test]
    fn every_unnamed_group_is_counted_less_those_being_named() {
        let mut model = PeoplePage::default();
        let mut page = answer(vec![two(1, 2), two(2, 2)]);
        page.unnamed_count = 250;
        loaded(&mut model, page);
        assert_eq!(model.unnamed_count(), 250);
        model.naming(1);
        assert_eq!(model.unnamed_count(), 249);
        assert_eq!(ids(&model.unnamed()), [2]);
        assert_eq!(model.unnamed_listed(), 2);
    }

    // Named, the group stays away until a read asked after the write lands; one asked
    // while it was on its way read the library before it, and does not bring it back.
    #[test]
    fn a_group_named_stays_away_until_a_read_asked_after_the_write_lands() {
        let mut model = PeoplePage::default();
        loaded(&mut model, answer(vec![two(1, 2)]));
        model.naming(1);
        let early = model.reload(10.0);
        model.named(1, true);
        model.loaded(early, answer(vec![two(1, 2)]));
        assert!(model.unnamed().is_empty(), "a read from before the write");
        let after = model.reload(20.0);
        model.loaded(after, answer(vec![two(1, 2)]));
        assert_eq!(ids(&model.unnamed()), [1], "the library has the last word");
    }

    #[test]
    fn a_name_refused_puts_the_group_back() {
        let mut model = PeoplePage::default();
        loaded(&mut model, answer(vec![two(1, 2)]));
        model.naming(1);
        assert!(model.unnamed().is_empty());
        model.named(1, false);
        assert_eq!(ids(&model.unnamed()), [1]);
    }

    #[test]
    fn library_changes_read_at_most_once_a_gap_the_last_of_a_burst_at_its_end() {
        let mut model = PeoplePage::default();
        // At once: the page opens on this call.
        assert_eq!(model.changed(0.0), Some(1));
        for at in [100.0, 200.0, 300.0, 400.0, 500.0] {
            assert_eq!(model.changed(at), None);
        }
        assert_eq!(model.due(), Some(CHANGE_GAP_MS));
        assert_eq!(model.poll(CHANGE_GAP_MS - 1.0), None);
        assert_eq!(model.poll(CHANGE_GAP_MS), Some(2));
        assert_eq!(model.due(), None);
        // A change long after the last read is not held back.
        assert_eq!(model.changed(6.0 * CHANGE_GAP_MS), Some(3));
    }

    // The page's own writes read at once, and the change each announces waits its gap.
    #[test]
    fn the_pages_own_reads_are_not_held_back_and_the_change_they_announce_waits() {
        let mut model = PeoplePage::default();
        assert_eq!(model.changed(0.0), Some(1));
        assert_eq!(model.reload(5.0 * CHANGE_GAP_MS), 2);
        assert_eq!(model.changed(5.0 * CHANGE_GAP_MS + 10.0), None);
        assert_eq!(model.poll(6.0 * CHANGE_GAP_MS), Some(3));
    }

    #[test]
    fn a_name_typed_is_new_a_merge_or_nothing() {
        let people = vec![
            group(3, Some("Anna"), Vec::new(), 0),
            group(5, Some("Émile"), Vec::new(), 0),
            group(9, None, Vec::new(), 0),
        ];
        assert_eq!(name_choice("   ", &people, None), NameChoice::Empty);
        assert_eq!(
            name_choice("  Ben ", &people, None),
            NameChoice::New("Ben".to_owned())
        );
        let anna = NameChoice::Merge {
            id: 3,
            name: "Anna".to_owned(),
        };
        assert_eq!(name_choice("anna", &people, None), anna);
        assert_eq!(
            name_choice("ÉMILE", &people, None),
            NameChoice::Merge {
                id: 5,
                name: "Émile".to_owned()
            }
        );
        assert_eq!(name_choice(" Anna ", &people, Some(3)), NameChoice::Same);
        assert_eq!(
            name_choice("ANNA", &people, Some(3)),
            NameChoice::New("ANNA".to_owned())
        );
        assert_eq!(
            name_choice("null", &people, None),
            NameChoice::New("null".to_owned())
        );
        assert_eq!(name_hint(&anna).as_deref(), Some("Add to Anna · Enter"));
        assert_eq!(
            name_hint(&NameChoice::New("Ben".to_owned())).as_deref(),
            Some("New person “Ben” · Enter")
        );
        assert_eq!(name_hint(&NameChoice::Empty), None);
    }

    // The words are the Svelte page's.
    #[test]
    fn the_words_are_the_svelte_pages() {
        let model = include_str!("../../../ui/src/lib/people-page.svelte.ts");
        assert!(model.contains(STALE_GROUP));
        assert!(model.contains("export const STRIP = 12;"));
        assert!(model.contains("export const CHANGE_GAP_MS = 1000;"));
        let name_box = include_str!("../../../ui/src/components/NameBox.svelte");
        for words in [
            "`Add to ${choice.name}`",
            "`New person “${choice.name}”`",
            "· Enter",
        ] {
            assert!(name_box.contains(words), "{words}");
        }
    }
}
