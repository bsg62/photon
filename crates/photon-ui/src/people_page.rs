//! The People page's behaviour, apart from what draws it - `createPeoplePage` in
//! `people-page.svelte.ts`, with its tests' cases, and `nameChoice` from `people.ts`: the
//! sections as the library last answered, the faces "Show more" has loaded on top, the
//! selection, the faces and groups hidden while a write on them is on its way, and when the
//! page is read again.
//!
//! The Svelte model awaits its reads and writes; this one is told. The application asks
//! for a read with what `reload` hands out and gives the answer back with its number
//! (`loaded`), and gives each write's end to the model: a number is when a read was
//! *asked*, and a read asked after a write ended has read the library after it.

use photon_core::library::{FaceFilter, PageFace, PageGroup, PeoplePage as Page};
use std::collections::{HashMap, HashSet};

/// Faces a strip shows before "Show more".
pub const STRIP: usize = 12;

/// Faces one "Show more" asks for, and one page of a reload's re-fetch: the most one
/// `person_faces` hands over (`MAX_FACE_PAGE` in `library/people.rs`). Asked for more, it
/// gives this many, so a re-fetch asked for in one call folded a strip back to it.
pub const MORE: usize = 200;

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

/// What can be done to faces selected in a strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceAction {
    Confirm,
    /// "Not this person".
    Reject,
    Ignore,
    Unignore,
}

impl FaceAction {
    /// The button's words (`FaceStrip.svelte`'s `ACTION_LABEL`).
    pub fn label(self) -> &'static str {
        match self {
            FaceAction::Confirm => "Confirm",
            FaceAction::Reject => "Not this person",
            FaceAction::Ignore => "Ignore these faces",
            FaceAction::Unignore => "Stop ignoring",
        }
    }
}

impl Strip {
    /// The group or person the strip is of.
    pub fn group(self) -> Option<i64> {
        match self {
            Strip::Unnamed(id) | Strip::Suggestion(id) | Strip::Person(id) | Strip::Ignored(id) => {
                Some(id)
            }
            Strip::Single | Strip::IgnoredFaces => None,
        }
    }

    /// Which of the group's faces the strip pages through: a person's confirmed ones under
    /// People, their unconfirmed ones under Suggestions, every one of an unnamed or an
    /// ignored group's.
    pub fn filter(self) -> FaceFilter {
        match self {
            Strip::Suggestion(_) => FaceFilter::Unconfirmed,
            Strip::Person(_) => FaceFilter::Confirmed,
            _ => FaceFilter::All,
        }
    }

    /// What its selected faces can be asked to do.
    pub fn actions(self) -> &'static [FaceAction] {
        match self {
            Strip::Unnamed(_) | Strip::Person(_) => &[FaceAction::Reject, FaceAction::Ignore],
            Strip::Suggestion(_) => &[FaceAction::Confirm, FaceAction::Reject, FaceAction::Ignore],
            Strip::Ignored(_) => &[],
            Strip::Single => &[FaceAction::Ignore],
            Strip::IgnoredFaces => &[FaceAction::Unignore],
        }
    }
}

/// The group a strip is of, in `page`.
pub fn group_in(page: &Page, strip: Strip) -> Option<&PageGroup> {
    let (list, id) = match strip {
        Strip::Unnamed(id) => (&page.unnamed, id),
        Strip::Suggestion(id) => (&page.suggestions, id),
        Strip::Person(id) => (&page.people, id),
        Strip::Ignored(id) => (&page.ignored_groups, id),
        Strip::Single | Strip::IgnoredFaces => return None,
    };
    list.iter().find(|group| group.id == id)
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
/// has merges, and the user must see "Add to Anna" before pressing Enter. A rename says it
/// is one.
pub fn name_hint(choice: &NameChoice, renaming: bool) -> Option<String> {
    match choice {
        NameChoice::Merge { name, .. } => Some(format!("Add to {name} · Enter")),
        NameChoice::New(name) if renaming => Some(format!("Rename to “{name}” · Enter")),
        NameChoice::New(name) => Some(format!("New person “{name}” · Enter")),
        NameChoice::Empty | NameChoice::Same => None,
    }
}

/// A read asked for: its number, and the faces "Show more" has loaded that it is to
/// fetch again - each strip's, by how many - so that a strip the user is working through
/// does not fold up under them at every write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub number: u64,
    pub wants: Vec<(Strip, usize)>,
}

/// A read's answer: the page, and the faces fetched again for each strip that had more
/// (a strip whose fetch failed is left out, and folds back to its first faces).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub page: Page,
    pub kept: HashMap<Strip, Vec<PageFace>>,
}

/// "Show more" asked of a strip: `limit` of the group's `filter`ed faces after `offset`,
/// for the page as it was opened (`opening`) and the strip as it stood (`generation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct More {
    pub opening: u64,
    pub generation: u64,
    pub strip: Strip,
    pub group: i64,
    pub filter: FaceFilter,
    pub offset: usize,
    pub limit: usize,
}

/// A write of faces the page hides at once: the faces' ids, and what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FacesWrite {
    Act {
        ids: Vec<i64>,
        action: FaceAction,
    },
    /// Single faces named: the first one's group named, the others' merged into it.
    NameSingles {
        groups: Vec<i64>,
        name: String,
        ids: Vec<i64>,
    },
}

impl FacesWrite {
    pub fn ids(&self) -> &[i64] {
        match self {
            FacesWrite::Act { ids, .. } | FacesWrite::NameSingles { ids, .. } => ids,
        }
    }
}

/// The page's state between frames.
#[derive(Debug, Default)]
pub struct PeoplePage {
    /// Which opening of the page this is: an answer to "Show more" asked of an earlier
    /// one is not this page's.
    opening: u64,
    /// Strips whose "Show more" is on its way: the next page starts where that one ends,
    /// which is not known until it lands.
    fetching: HashSet<Strip>,
    page: Option<Page>,
    /// Faces "Show more" loaded beyond each strip's first ones.
    extra: HashMap<Strip, Vec<PageFace>>,
    /// Moved by every "Show more" and "Show fewer" of a strip: a read keeps its re-fetch of
    /// a strip only if the strip's number is where it was when the read was asked, or a
    /// click made while the read was on its way would be undone by a re-fetch of what the
    /// strip held before it.
    generation: HashMap<Strip, u64>,
    /// The strips' numbers as the last read was asked.
    asked: (u64, HashMap<Strip, u64>),
    /// The faces selected, in one strip at a time.
    selection: Option<(Strip, Vec<i64>)>,
    /// The number the last read was asked under; a write's end is stamped with it.
    tick: u64,
    /// Groups and faces hidden while a write on them is on its way (`None`), and after it
    /// until a read asked after its end lands (the number it ended at). A read asked
    /// earlier may have read the library before the write, and would put back what the
    /// user has just named or taken away.
    hidden_groups: HashMap<i64, Option<u64>>,
    hidden_faces: HashMap<i64, Option<u64>>,
    /// When the last read was asked, in milliseconds on the frame's clock.
    last_start: Option<f64>,
    /// When a read that library changes put off is due.
    due: Option<f64>,
}

impl PeoplePage {
    /// The page at its `opening`th opening.
    pub fn new(opening: u64) -> Self {
        Self {
            opening,
            ..Self::default()
        }
    }

    /// Every face the page holds, shown or hidden for a write on its way, with what "Show
    /// more" loaded: what its crops are kept for.
    pub fn every_face(&self) -> impl Iterator<Item = &PageFace> {
        let page = self.page.iter().flat_map(|page| {
            let groups = (page.unnamed.iter())
                .chain(&page.suggestions)
                .chain(&page.people)
                .chain(&page.ignored_groups);
            (groups.flat_map(|group| &group.faces))
                .chain(&page.single_faces)
                .chain(&page.ignored_faces)
        });
        page.chain(self.extra.values().flatten())
    }

    /// The page as the library last answered: none until the first read lands.
    pub fn page(&self) -> Option<&Page> {
        self.page.as_ref()
    }

    /// A read asked for now, by the page itself - its opening, a write's end - which is
    /// never held back: the user is waiting on it.
    pub fn reload(&mut self, now: f64) -> Ask {
        self.tick += 1;
        self.last_start = Some(now);
        self.due = None;
        self.asked = (self.tick, self.generation.clone());
        let mut wants: Vec<(Strip, usize)> = (self.extra.iter())
            .filter(|(_, faces)| !faces.is_empty())
            .map(|(strip, faces)| (*strip, faces.len()))
            .collect();
        wants.sort_by_key(|(strip, _)| format!("{strip:?}"));
        Ask {
            number: self.tick,
            wants,
        }
    }

    /// A change of the library's data: a read now if none began in the last
    /// `CHANGE_GAP_MS`, else one at the end of that gap, however many changes arrive
    /// meanwhile. A read the page's own write began counts, so the change that write
    /// announces waits its turn.
    pub fn changed(&mut self, now: f64) -> Option<Ask> {
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
    pub fn poll(&mut self, now: f64) -> Option<Ask> {
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
    pub fn loaded(&mut self, number: u64, loaded: Loaded) {
        let Loaded {
            page: next,
            mut kept,
        } = loaded;
        // A strip the user expanded or folded since the read was asked stays as they
        // left it: its re-fetch is of what it held before.
        let before = (self.asked.0 == number).then_some(&self.asked.1);
        let strips: HashSet<Strip> = kept.keys().chain(self.extra.keys()).copied().collect();
        for strip in strips {
            let now = self.generation.get(&strip).copied().unwrap_or(0);
            let then = before
                .and_then(|before| before.get(&strip))
                .copied()
                .unwrap_or(0);
            if now == then {
                continue;
            }
            match self.extra.get(&strip) {
                Some(faces) => {
                    kept.insert(strip, faces.clone());
                }
                None => {
                    kept.remove(&strip);
                }
            }
        }
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
        self.extra = kept;
        // What this read has seen land is shown as the library has it.
        let settled = |ended: &mut Option<u64>| ended.is_none_or(|ended| ended >= number);
        self.hidden_groups.retain(|_, ended| settled(ended));
        self.hidden_faces.retain(|_, ended| settled(ended));
        // And a selection keeps only the faces still shown.
        if let Some((strip, ids)) = self.selection.take() {
            let present: HashSet<i64> = self.faces(strip).iter().map(|face| face.id).collect();
            let ids: Vec<i64> = ids.into_iter().filter(|id| present.contains(id)).collect();
            if !ids.is_empty() {
                self.selection = Some((strip, ids));
            }
        }
    }

    /// The writes on their way are not to be heard of - one panicked, and its answer
    /// names nothing: every group and face still hidden for one comes back.
    pub fn forget_writes(&mut self) {
        self.hidden_groups.retain(|_, ended| ended.is_some());
        self.hidden_faces.retain(|_, ended| ended.is_some());
    }

    /// A group is being written - named, ignored or no longer: it leaves its section at
    /// once, and its count with it.
    pub fn naming(&mut self, group: i64) {
        self.hidden_groups.insert(group, None);
    }

    /// The write on `group` is over: written, it stays hidden until a read asked after this
    /// lands; refused, it is put back. Either way the page is read again - a refusal can
    /// mean the page is stale - which the caller asks for with `reload`.
    pub fn named(&mut self, group: i64, written: bool) {
        ended(&mut self.hidden_groups, &[group], written, self.tick);
    }

    /// The write on faces `ids` is over, as `named` for a group.
    pub fn faces_done(&mut self, ids: &[i64], written: bool) {
        ended(&mut self.hidden_faces, ids, written, self.tick);
    }

    fn group_shown(&self, group: &PageGroup) -> bool {
        !self.hidden_groups.contains_key(&group.id)
    }

    fn face_shown(&self, face: &PageFace) -> bool {
        !self.hidden_faces.contains_key(&face.id)
    }

    /// Unnamed groups, less those being written.
    pub fn unnamed(&self) -> Vec<&PageGroup> {
        let groups = self.page.iter().flat_map(|page| &page.unnamed);
        groups.filter(|group| self.group_shown(group)).collect()
    }

    /// Every unnamed group, listed or past the library's 200, less those being written: the
    /// listed ones leave at once, and so does their count.
    pub fn unnamed_count(&self) -> i64 {
        let Some(page) = &self.page else {
            return 0;
        };
        let leaving = page.unnamed.iter().filter(|g| !self.group_shown(g)).count();
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
        groups.filter(|group| self.group_shown(group)).collect()
    }

    fn base(&self, strip: Strip) -> &[PageFace] {
        match (strip, &self.page) {
            (_, None) => &[],
            (Strip::Single, Some(page)) => &page.single_faces,
            (Strip::IgnoredFaces, Some(page)) => &page.ignored_faces,
            (_, Some(page)) => group_in(page, strip).map_or(&[], |group| &group.faces),
        }
    }

    /// The strip's faces with what "Show more" loaded, each face once: the extra faces are
    /// a later read than the strip's first ones, and a face that moved between the two
    /// reads can be in both.
    fn listed(&self, strip: Strip) -> Vec<&PageFace> {
        let mut seen = HashSet::new();
        let extra = self.extra.get(&strip).into_iter().flatten();
        (self.base(strip).iter().chain(extra))
            .filter(|face| seen.insert(face.id))
            .collect()
    }

    /// The faces a strip shows: less those a write is taking away.
    pub fn faces(&self, strip: Strip) -> Vec<&PageFace> {
        let listed = self.listed(strip);
        listed
            .into_iter()
            .filter(|face| self.face_shown(face))
            .collect()
    }

    /// How many faces a strip stands for, shown or not, less those a write is taking away.
    pub fn count(&self, strip: Strip) -> i64 {
        let gone = (self.listed(strip).iter())
            .filter(|face| !self.face_shown(face))
            .count() as i64;
        let all = match (strip, &self.page) {
            (_, None) => 0,
            (Strip::Single, Some(page)) => page.single_count,
            (Strip::IgnoredFaces, Some(page)) => page.ignored_faces.len() as i64,
            (_, Some(page)) => group_in(page, strip).map_or(0, |group| group.face_count),
        };
        all - gone
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

    /// Whether "Show more" is offered: a group's strip with faces left to show. Never for
    /// the single or ignored faces, which the library lists up to its own cap.
    pub fn can_show_more(&self, strip: Strip) -> bool {
        strip.group().is_some() && (self.faces(strip).len() as i64) < self.count(strip)
    }

    /// Whether "Show more" has loaded faces, for "Show fewer".
    pub fn is_expanded(&self, strip: Strip) -> bool {
        self.extra
            .get(&strip)
            .is_some_and(|faces| !faces.is_empty())
    }

    /// What "Show more" asks of the library for `strip`: a page after what it holds -
    /// nothing while one is on its way, whose end is where the next would start: asked
    /// again from what is held, a second press fetched the same page, and the one after
    /// it started two pages on, leaving a page out.
    pub fn show_more(&mut self, strip: Strip) -> Option<More> {
        if self.fetching.contains(&strip) {
            return None;
        }
        let group = group_in(self.page.as_ref()?, strip)?;
        let held = self.extra.get(&strip).map_or(0, Vec::len);
        let generation = self.generation.get(&strip).copied().unwrap_or(0);
        let offset = group.faces.len() + held;
        let id = group.id;
        self.fetching.insert(strip);
        Some(More {
            opening: self.opening,
            generation,
            strip,
            group: id,
            filter: strip.filter(),
            offset,
            limit: MORE,
        })
    }

    /// The page "Show more" asked for, landed: for the strip as it was asked of. Shown
    /// fewer of since, or a page of an earlier opening, it is no longer wanted.
    pub fn more_landed(&mut self, more: More, faces: Vec<PageFace>) {
        if !self.more_over(more) {
            return;
        }
        if self.generation.get(&more.strip).copied().unwrap_or(0) != more.generation {
            return;
        }
        self.extra.entry(more.strip).or_default().extend(faces);
        self.bump(more.strip);
    }

    /// "Show more" could not be fetched: it can be asked again.
    pub fn more_failed(&mut self, more: More) {
        self.more_over(more);
    }

    /// Whether `more` is this opening's, and its strip no longer waiting on it.
    fn more_over(&mut self, more: More) -> bool {
        more.opening == self.opening && self.fetching.remove(&more.strip)
    }

    /// The strip folds back to its first faces, and a face picked among those it no
    /// longer shows is no longer picked: the bar would act on faces nobody can see.
    pub fn show_fewer(&mut self, strip: Strip) {
        self.extra.remove(&strip);
        self.bump(strip);
        if let Some((at, ids)) = &mut self.selection
            && *at == strip
        {
            let shown: HashSet<i64> = (self.page.iter())
                .flat_map(|page| group_in(page, strip))
                .flat_map(|group| &group.faces)
                .map(|face| face.id)
                .collect();
            ids.retain(|id| shown.contains(id));
            if ids.is_empty() {
                self.selection = None;
            }
        }
    }

    fn bump(&mut self, strip: Strip) {
        *self.generation.entry(strip).or_default() += 1;
    }

    /// The faces selected in `strip`, in the order they were picked.
    pub fn selected(&self, strip: Strip) -> &[i64] {
        match &self.selection {
            Some((at, ids)) if *at == strip => ids,
            _ => &[],
        }
    }

    pub fn is_selected(&self, strip: Strip, face: i64) -> bool {
        self.selected(strip).contains(&face)
    }

    /// A face picked or put back: in one strip at a time, a pick in another starting over.
    pub fn toggle(&mut self, strip: Strip, face: i64) {
        match &mut self.selection {
            Some((at, ids)) if *at == strip => {
                if let Some(n) = ids.iter().position(|id| *id == face) {
                    ids.remove(n);
                    if ids.is_empty() {
                        self.selection = None;
                    }
                } else {
                    ids.push(face);
                }
            }
            _ => self.selection = Some((strip, vec![face])),
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// `action` asked of the faces selected in `strip`: they leave the strip at once and the
    /// selection is let go of. Answers the write, or nothing with nothing selected there.
    pub fn act(&mut self, strip: Strip, action: FaceAction) -> Option<FacesWrite> {
        let ids = self.selected(strip).to_vec();
        if ids.is_empty() {
            return None;
        }
        self.selection = None;
        self.hide_faces(&ids);
        Some(FacesWrite::Act { ids, action })
    }

    /// "Confirm all": the person's suggestions on screen, not their whole list - what the
    /// user has looked at.
    pub fn confirm_all(&mut self, person: i64) -> Option<FacesWrite> {
        let ids: Vec<i64> = (self.faces(Strip::Suggestion(person)).iter())
            .map(|face| face.id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        self.hide_faces(&ids);
        Some(FacesWrite::Act {
            ids,
            action: FaceAction::Confirm,
        })
    }

    /// "Confirm all" when every suggestion is on screen, else how many are.
    pub fn confirm_all_label(&self, person: i64) -> String {
        let strip = Strip::Suggestion(person);
        let shown = self.faces(strip).len();
        if (shown as i64) < self.count(strip) {
            format!("Confirm these {shown}")
        } else {
            "Confirm all".to_owned()
        }
    }

    /// The single faces selected, named `typed`: the first one's group named, the others'
    /// merged into the person it made.
    pub fn name_singles(&mut self, typed: &str) -> Option<FacesWrite> {
        let name = typed.trim();
        let chosen: Vec<(i64, Option<i64>)> = (self.faces(Strip::Single).iter())
            .filter(|face| self.is_selected(Strip::Single, face.id))
            .map(|face| (face.id, face.person_id))
            .collect();
        let groups: Vec<i64> = chosen.iter().filter_map(|(_, group)| *group).collect();
        if name.is_empty() || groups.is_empty() {
            return None;
        }
        self.selection = None;
        let ids: Vec<i64> = chosen.iter().map(|(id, _)| *id).collect();
        self.hide_faces(&ids);
        Some(FacesWrite::NameSingles {
            groups,
            name: name.to_owned(),
            ids,
        })
    }

    fn hide_faces(&mut self, ids: &[i64]) {
        for id in ids {
            self.hidden_faces.insert(*id, None);
        }
    }

    /// The name `person` is to have: what was typed, trimmed - nothing for no name or for
    /// their own exactly.
    pub fn rename(&self, person: i64, typed: &str) -> Option<String> {
        match self.choice(typed, Some(person)) {
            NameChoice::Empty | NameChoice::Same => None,
            NameChoice::New(_) | NameChoice::Merge { .. } => Some(typed.trim().to_owned()),
        }
    }

    /// The named person's name, or nothing for one the page does not list.
    pub fn name_of(&self, person: i64) -> Option<&str> {
        (self.people().iter())
            .find(|named| named.id == person)
            .and_then(|named| named.name.as_deref())
    }
}

/// A write's end for the ids hidden under it: stamped with the last read's number when it
/// was written, put back when it was refused.
fn ended(hidden: &mut HashMap<i64, Option<u64>>, ids: &[i64], written: bool, tick: u64) {
    for id in ids {
        if written {
            // Not a new number: a read asked from here on numbers itself past this one,
            // and one asked earlier carries a number at or below it, which `loaded` keeps.
            if let Some(ended) = hidden.get_mut(id) {
                *ended = Some(tick);
            }
        } else {
            hidden.remove(id);
        }
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

/// The question asked before one person is merged into another.
pub fn merge_question(from: &str, into: &str) -> String {
    format!("Merge “{from}” into “{into}”? Their faces become {into}'s. This cannot be undone.")
}

/// The question asked before a person is deleted.
pub fn delete_question(name: &str) -> String {
    format!("Delete “{name}”? Their faces stay together as a group with no name.")
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

    fn faces(ids: impl IntoIterator<Item = i64>) -> Vec<PageFace> {
        ids.into_iter().map(face).collect()
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
        let ask = model.reload(0.0);
        model.loaded(
            ask.number,
            Loaded {
                page,
                kept: HashMap::new(),
            },
        );
    }

    fn ids(groups: &[&PageGroup]) -> Vec<i64> {
        groups.iter().map(|group| group.id).collect()
    }

    fn shown(model: &PeoplePage, strip: Strip) -> Vec<i64> {
        model.faces(strip).iter().map(|face| face.id).collect()
    }

    const A: Strip = Strip::Unnamed(1);

    fn unnamed_page() -> Page {
        answer(vec![group(1, None, faces([1, 2, 3]), 5)])
    }

    /// The library's `person_faces`: faces `offset..` of a group of `total`, at most `MORE`
    /// a call however many are asked for, as `MAX_FACE_PAGE` clamps them.
    fn clamped(total: usize, offset: usize, limit: usize) -> Vec<PageFace> {
        let n = limit.min(MORE).min(total.saturating_sub(offset));
        faces((offset..offset + n).map(|n| n as i64))
    }

    /// What a read's worker does with `wants` over `page`, from a group of `total` faces.
    fn refetched(
        page: &Page,
        wants: &[(Strip, usize)],
        total: usize,
    ) -> HashMap<Strip, Vec<PageFace>> {
        let mut kept = HashMap::new();
        for (strip, want) in wants {
            let Some(group) = group_in(page, *strip) else {
                continue;
            };
            let mut got: Vec<PageFace> = Vec::new();
            while got.len() < *want {
                let limit = MORE.min(want - got.len());
                let page = clamped(total, group.faces.len() + got.len(), limit);
                let short = page.len() < limit;
                got.extend(page);
                if short {
                    break;
                }
            }
            kept.insert(*strip, got);
        }
        kept
    }

    #[test]
    fn the_sections_are_read() {
        let mut model = PeoplePage::default();
        assert!(model.page().is_none());
        assert!(!model.empty(), "not known to be empty before it is read");
        loaded(&mut model, unnamed_page());
        assert_eq!(shown(&model, A), [1, 2, 3]);
        assert_eq!(model.count(A), 5);
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
        let early = model.reload(10.0).number;
        model.named(1, true);
        let page = |model: &mut PeoplePage, number| {
            let page = answer(vec![two(1, 2)]);
            model.loaded(
                number,
                Loaded {
                    page,
                    kept: HashMap::new(),
                },
            );
        };
        page(&mut model, early);
        assert!(model.unnamed().is_empty(), "a read from before the write");
        let after = model.reload(20.0).number;
        page(&mut model, after);
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
        assert_eq!(model.changed(0.0).map(|ask| ask.number), Some(1));
        for at in [100.0, 200.0, 300.0, 400.0, 500.0] {
            assert_eq!(model.changed(at), None);
        }
        assert_eq!(model.due(), Some(CHANGE_GAP_MS));
        assert_eq!(model.poll(CHANGE_GAP_MS - 1.0), None);
        assert_eq!(model.poll(CHANGE_GAP_MS).map(|ask| ask.number), Some(2));
        assert_eq!(model.due(), None);
        // A change long after the last read is not held back.
        assert_eq!(
            model.changed(6.0 * CHANGE_GAP_MS).map(|ask| ask.number),
            Some(3)
        );
    }

    // The page's own writes read at once, and the change each announces waits its gap.
    #[test]
    fn the_pages_own_reads_are_not_held_back_and_the_change_they_announce_waits() {
        let mut model = PeoplePage::default();
        assert!(model.changed(0.0).is_some());
        assert_eq!(model.reload(5.0 * CHANGE_GAP_MS).number, 2);
        assert_eq!(model.changed(5.0 * CHANGE_GAP_MS + 10.0), None);
        assert_eq!(
            model.poll(6.0 * CHANGE_GAP_MS).map(|ask| ask.number),
            Some(3)
        );
    }

    #[test]
    fn faces_are_selected_within_one_strip_at_a_time() {
        let mut model = PeoplePage::default();
        let b = Strip::Person(2);
        model.toggle(A, 1);
        model.toggle(A, 2);
        assert_eq!(model.selected(A), [1, 2]);
        model.toggle(b, 7);
        assert_eq!(model.selected(A), [] as [i64; 0]);
        assert_eq!(model.selected(b), [7]);
        model.toggle(b, 7);
        assert_eq!(model.selected(b), [] as [i64; 0]);
    }

    #[test]
    fn each_section_offers_its_own_actions() {
        use FaceAction::*;
        assert_eq!(Strip::Unnamed(1).actions(), [Reject, Ignore]);
        assert_eq!(Strip::Suggestion(1).actions(), [Confirm, Reject, Ignore]);
        assert_eq!(Strip::Person(1).actions(), [Reject, Ignore]);
        assert_eq!(Strip::Single.actions(), [Ignore]);
        assert_eq!(Strip::IgnoredFaces.actions(), [Unignore]);
        assert_eq!(Strip::Ignored(1).actions(), [] as [FaceAction; 0]);
    }

    // An action takes its faces away at once and lets go of the selection; written, they
    // stay away until a read asked after it; refused, they come back.
    #[test]
    fn an_action_hides_its_faces_until_its_write_is_answered() {
        let mut model = PeoplePage::default();
        loaded(&mut model, unnamed_page());
        model.toggle(A, 1);
        model.toggle(A, 2);
        let write = model.act(A, FaceAction::Reject).unwrap();
        assert_eq!(
            write,
            FacesWrite::Act {
                ids: vec![1, 2],
                action: FaceAction::Reject
            }
        );
        assert_eq!(shown(&model, A), [3]);
        assert_eq!(model.count(A), 3);
        assert_eq!(model.selected(A), [] as [i64; 0]);
        // A read asked while the write was on its way does not bring them back.
        let early = model.reload(1.0).number;
        model.faces_done(write.ids(), true);
        model.loaded(
            early,
            Loaded {
                page: unnamed_page(),
                kept: HashMap::new(),
            },
        );
        assert_eq!(shown(&model, A), [3]);
        // One asked after it shows what the library says.
        loaded(&mut model, unnamed_page());
        assert_eq!(shown(&model, A), [1, 2, 3]);
        // Refused, they are back at once.
        model.toggle(A, 1);
        let write = model.act(A, FaceAction::Reject).unwrap();
        model.faces_done(write.ids(), false);
        assert_eq!(shown(&model, A), [1, 2, 3]);
        assert_eq!(model.act(A, FaceAction::Reject), None, "nothing selected");
    }

    #[test]
    fn confirm_all_confirms_the_faces_on_screen() {
        let mut model = PeoplePage::default();
        let twelve = faces(1..=12);
        let page = Page {
            suggestions: vec![group(4, Some("Ben"), twelve.clone(), 30)],
            ..Page::default()
        };
        loaded(&mut model, page);
        assert_eq!(model.confirm_all_label(4), "Confirm these 12");
        assert_eq!(
            model.confirm_all(4),
            Some(FacesWrite::Act {
                ids: (1..=12).collect(),
                action: FaceAction::Confirm
            })
        );
        let mut all = PeoplePage::default();
        let page = Page {
            suggestions: vec![group(4, Some("Ben"), twelve, 12)],
            ..Page::default()
        };
        loaded(&mut all, page);
        assert_eq!(all.confirm_all_label(4), "Confirm all");
    }

    // "Show more" asks for the group's next page in the section's own faces; "Show fewer"
    // folds it back.
    #[test]
    fn show_more_pages_through_the_group_with_the_sections_filter() {
        let mut model = PeoplePage::default();
        let strip = |n: i64| faces((0..STRIP as i64).map(|i| n * 100 + i));
        let page = Page {
            people: vec![group(1, Some("Anna"), strip(1), 40)],
            suggestions: vec![group(2, Some("Ben"), strip(2), 40)],
            unnamed: vec![group(3, None, strip(3), 40)],
            ignored_groups: vec![group(4, None, strip(4), 40)],
            ..Page::default()
        };
        loaded(&mut model, page);
        let person = Strip::Person(1);
        let asked = model.show_more(person).unwrap();
        assert_eq!(
            (asked.group, asked.filter, asked.offset, asked.limit),
            (1, FaceFilter::Confirmed, STRIP, MORE)
        );
        model.more_landed(asked, faces([900, 901]));
        assert!(shown(&model, person).contains(&901));
        assert_eq!(model.faces(person).len(), STRIP + 2);
        assert_eq!(model.show_more(person).unwrap().offset, STRIP + 2);
        let mut filter = |strip| model.show_more(strip).unwrap().filter;
        assert_eq!(filter(Strip::Suggestion(2)), FaceFilter::Unconfirmed);
        assert_eq!(filter(Strip::Unnamed(3)), FaceFilter::All);
        assert_eq!(filter(Strip::Ignored(4)), FaceFilter::All);
        assert!(model.is_expanded(person));
        model.show_fewer(person);
        assert_eq!(model.faces(person).len(), STRIP);
        assert!(!model.is_expanded(person));
        assert!(model.can_show_more(person));
        assert!(!model.can_show_more(Strip::Single));
    }

    // A read fetches again every page "Show more" loaded, a page at a time, and stops at a
    // short page: a strip the user is working through does not fold up under them.
    #[test]
    fn a_reload_keeps_what_show_more_loaded() {
        let mut model = PeoplePage::default();
        let page = || answer(vec![group(1, None, faces(0..STRIP as i64), 1000)]);
        loaded(&mut model, page());
        for _ in 0..3 {
            let more = model.show_more(A).unwrap();
            model.more_landed(more, clamped(1000, more.offset, more.limit));
        }
        assert_eq!(model.faces(A).len(), STRIP + 3 * MORE);
        let ask = model.reload(1.0);
        assert_eq!(ask.wants, [(A, 3 * MORE)]);
        let kept = refetched(&page(), &ask.wants, 1000);
        model.loaded(ask.number, Loaded { page: page(), kept });
        assert_eq!(
            shown(&model, A),
            (0..(STRIP + 3 * MORE) as i64).collect::<Vec<_>>()
        );
        // Faces left since: the group now ends 250 faces in.
        let ask = model.reload(2.0);
        let kept = refetched(&page(), &ask.wants, 250);
        model.loaded(ask.number, Loaded { page: page(), kept });
        assert_eq!(model.faces(A).len(), 250);
    }

    // A strip folded or expanded while a read was on its way stays as the user left it.
    #[test]
    fn a_strip_changed_while_a_read_is_on_its_way_stays_as_it_was_left() {
        let mut model = PeoplePage::default();
        let page = || answer(vec![group(1, None, faces(0..STRIP as i64), 1000)]);
        loaded(&mut model, page());
        let more = model.show_more(A).unwrap();
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        // Folded meanwhile.
        let ask = model.reload(1.0);
        model.show_fewer(A);
        let kept = refetched(&page(), &ask.wants, 1000);
        model.loaded(ask.number, Loaded { page: page(), kept });
        assert!(!model.is_expanded(A));
        assert_eq!(model.faces(A).len(), STRIP);
        // Expanded meanwhile, a second page on the first.
        let more = model.show_more(A).unwrap();
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        let ask = model.reload(2.0);
        let more = model.show_more(A).unwrap();
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        let kept = refetched(&page(), &ask.wants, 1000);
        model.loaded(ask.number, Loaded { page: page(), kept });
        assert_eq!(model.faces(A).len(), STRIP + 2 * MORE);
    }

    #[test]
    fn a_face_read_by_both_the_strip_and_show_more_is_listed_once() {
        let mut model = PeoplePage::default();
        loaded(&mut model, unnamed_page());
        let more = model.show_more(A).unwrap();
        model.more_landed(more, faces([3, 4]));
        assert_eq!(shown(&model, A), [1, 2, 3, 4]);
        assert_eq!(model.count(A), 5);
    }

    // "Show more" pressed again while its page is on its way asks nothing: the next page
    // starts where that one ends. Asked from what was held, the second press fetched the
    // same page and the third started two pages on, leaving one out.
    #[test]
    fn show_more_waits_for_the_page_on_its_way() {
        let page = || answer(vec![group(1, None, faces(0..STRIP as i64), 1000)]);
        let mut model = PeoplePage::default();
        loaded(&mut model, page());
        let first = model.show_more(A).unwrap();
        assert_eq!(model.show_more(A), None);
        model.more_landed(first, clamped(1000, first.offset, first.limit));
        assert_eq!(model.show_more(A).unwrap().offset, STRIP + MORE);
        // A fetch that failed can be asked again.
        let mut model = PeoplePage::default();
        loaded(&mut model, page());
        let failed = model.show_more(A).unwrap();
        model.more_failed(failed);
        assert_eq!(model.show_more(A).unwrap().offset, STRIP);
    }

    // A page that lands after "Show fewer", or for an earlier opening of the page, is not
    // put in the strip: the user folded it, or never asked this page for it.
    #[test]
    fn a_page_no_longer_wanted_is_dropped() {
        let page = || answer(vec![group(1, None, faces(0..STRIP as i64), 1000)]);
        let mut model = PeoplePage::default();
        loaded(&mut model, page());
        let more = model.show_more(A).unwrap();
        model.show_fewer(A);
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        assert!(
            !model.is_expanded(A),
            "folded, and opened by the page that landed"
        );
        let mut earlier = PeoplePage::new(1);
        loaded(&mut earlier, page());
        let more = earlier.show_more(A).unwrap();
        let mut model = PeoplePage::new(2);
        loaded(&mut model, page());
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        assert!(
            !model.is_expanded(A),
            "an earlier opening's page landed in this one"
        );
    }

    // "Show fewer" lets go of the faces picked among those it no longer shows: the bar
    // would count them and act on faces nobody can see.
    #[test]
    fn show_fewer_lets_go_of_the_faces_it_hides() {
        let mut model = PeoplePage::default();
        loaded(
            &mut model,
            answer(vec![group(1, None, faces(0..STRIP as i64), 1000)]),
        );
        let more = model.show_more(A).unwrap();
        model.more_landed(more, clamped(1000, more.offset, more.limit));
        model.toggle(A, 1);
        model.toggle(A, STRIP as i64 + 3);
        model.show_fewer(A);
        assert_eq!(model.selected(A), [1]);
    }

    // A strip whose fetch failed is left out of `kept`, and folds back; the others stay.
    #[test]
    fn a_strip_whose_refetch_failed_folds_back_alone() {
        let mut model = PeoplePage::default();
        let strip = |n: i64| faces((0..STRIP as i64).map(|i| n * 100 + i));
        let page = || Page {
            people: vec![
                group(1, Some("Anna"), strip(1), 40),
                group(2, Some("Ben"), strip(2), 40),
            ],
            ..Page::default()
        };
        loaded(&mut model, page());
        for (n, landed) in [(1, [900, 901]), (2, [950, 951])] {
            let more = model.show_more(Strip::Person(n)).unwrap();
            model.more_landed(more, faces(landed));
        }
        let ask = model.reload(1.0);
        let kept = HashMap::from([(Strip::Person(2), faces([950, 951]))]);
        model.loaded(ask.number, Loaded { page: page(), kept });
        assert_eq!(model.faces(Strip::Person(1)).len(), STRIP);
        assert_eq!(model.faces(Strip::Person(2)).len(), STRIP + 2);
    }

    #[test]
    fn a_selection_loses_faces_a_reload_no_longer_shows() {
        let mut model = PeoplePage::default();
        loaded(&mut model, unnamed_page());
        model.toggle(A, 1);
        model.toggle(A, 2);
        loaded(&mut model, answer(vec![group(1, None, faces([1, 3]), 4)]));
        assert_eq!(model.selected(A), [1]);
    }

    // Single faces named: the first one's group is named, the others' merged into it.
    #[test]
    fn single_faces_are_named_by_their_groups() {
        let mut model = PeoplePage::default();
        let mut singles = faces([1, 2, 3]);
        for (face, group) in singles.iter_mut().zip([21, 22, 23]) {
            face.person_id = Some(group);
        }
        let page = Page {
            single_faces: singles,
            single_count: 3,
            ..Page::default()
        };
        loaded(&mut model, page);
        assert_eq!(model.name_singles("Ben"), None, "nothing selected");
        for id in [1, 2, 3] {
            model.toggle(Strip::Single, id);
        }
        assert_eq!(model.name_singles("  "), None);
        assert_eq!(
            model.name_singles(" Ben "),
            Some(FacesWrite::NameSingles {
                groups: vec![21, 22, 23],
                name: "Ben".to_owned(),
                ids: vec![1, 2, 3]
            })
        );
        assert!(model.faces(Strip::Single).is_empty());
        assert_eq!(model.selected(Strip::Single), [] as [i64; 0]);
    }

    #[test]
    fn renaming_to_ones_own_name_does_nothing() {
        let mut model = PeoplePage::default();
        let page = Page {
            people: vec![group(3, Some("Anna"), Vec::new(), 0)],
            ..Page::default()
        };
        loaded(&mut model, page);
        assert_eq!(model.rename(3, " Anna "), None);
        assert_eq!(model.rename(3, "ANNA").as_deref(), Some("ANNA"));
        assert_eq!(model.rename(3, " Anne ").as_deref(), Some("Anne"));
        assert_eq!(model.rename(3, "  "), None);
        assert_eq!(model.name_of(3), Some("Anna"));
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
        assert_eq!(
            name_hint(&anna, false).as_deref(),
            Some("Add to Anna · Enter")
        );
        let ben = NameChoice::New("Ben".to_owned());
        assert_eq!(
            name_hint(&ben, false).as_deref(),
            Some("New person “Ben” · Enter")
        );
        assert_eq!(
            name_hint(&ben, true).as_deref(),
            Some("Rename to “Ben” · Enter")
        );
        assert_eq!(name_hint(&NameChoice::Empty, false), None);
    }

    // The words are the Svelte page's.
    #[test]
    fn the_words_are_the_svelte_pages() {
        let model = include_str!("../../../ui/src/lib/people-page.svelte.ts");
        assert!(model.contains(STALE_GROUP));
        assert!(model.contains("export const STRIP = 12;"));
        assert!(model.contains("export const MORE = 200;"));
        assert!(model.contains("export const CHANGE_GAP_MS = 1000;"));
        assert!(model.contains("`Confirm these ${n}`"));
        assert!(model.contains(
            "`Merge “${nameOf(from)}” into “${nameOf(into)}”? Their faces become ${nameOf(into)}'s. This cannot be undone.`"
        ));
        assert!(model.contains(
            "`Delete “${nameOf(person)}”? Their faces stay together as a group with no name.`"
        ));
        assert_eq!(
            merge_question("Anna", "Ben"),
            "Merge “Anna” into “Ben”? Their faces become Ben's. This cannot be undone."
        );
        let strip = include_str!("../../../ui/src/components/FaceStrip.svelte");
        for action in [
            FaceAction::Confirm,
            FaceAction::Reject,
            FaceAction::Ignore,
            FaceAction::Unignore,
        ] {
            assert!(
                strip.contains(&format!("'{}'", action.label())),
                "{action:?}"
            );
        }
        let name_box = include_str!("../../../ui/src/components/NameBox.svelte");
        for words in [
            "`Add to ${choice.name}`",
            "`New person “${choice.name}”`",
            "`Rename to “${choice.name}”`",
            "· Enter",
        ] {
            assert!(name_box.contains(words), "{words}");
        }
    }
}
