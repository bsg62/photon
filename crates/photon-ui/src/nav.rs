//! Where the user is in the library, and where they have asked to be.
//!
//! The engine holds one view and one sort, and a step that changes either rebuilds the grid
//! on the thread that makes it. Steps are therefore made on `tasks::Queue`, in the order
//! asked, and this module holds what the interface knows meanwhile: the place the grid on
//! screen shows, and the steps asked that have not landed. It is the part of the Svelte UI's
//! `LibraryStore` that `switchView`, `viewChain` and `settledView` were.
//!
//! **What a grid shows is known from the step that built it, never read from the engine
//! when the grid arrives.** The engine's own view moves when a step *begins*, a whole
//! rebuild before its grid is published: with two steps on their way, the first's grid
//! arrives while the engine already says the second's view. Read then, the starred photos
//! were shown as All photos - at the place All photos had been scrolled to, under its
//! line. So a step's answer carries the place it led to and the version of the grid that
//! shows it (`Landed`), and a grid that arrives while a step is on its way is not taken
//! until an answer vouches for it (`adopt`). That holds as long as nothing but these steps
//! moves the engine's view, which is so: the queue is the only caller of its setters.
//!
//! **A folder is gone to in the grid that holds it.** A click on a folder from Starred asks
//! for All photos and then for a place in it, and a folder's offset means something only in
//! the grid it is looked up in: so the jump is kept with the number of the step it follows,
//! and `adopt` hands it out with that step's grid and with no other (`jump`). It is what
//! `enterFolder` and `returnToAll` await in the Svelte UI.
//!
//! No egui here, and no engine.

use photon_core::{
    grid::GridView,
    sort::{Grouping, Sort, SortKey},
};
use std::collections::VecDeque;

/// A view, and the argument that selects within it: a search's query, an album's id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub view: GridView,
    pub arg: String,
}

impl Place {
    /// A view that takes no argument.
    pub fn of(view: GridView) -> Self {
        Self {
            view,
            arg: String::new(),
        }
    }

    /// The search for `query`. A blank one is All photos, which is what the engine makes of
    /// it (`Engine::set_search_query`).
    pub fn search(query: &str) -> Self {
        if query.trim().is_empty() {
            Self::of(GridView::All)
        } else {
            Self {
                view: GridView::Search,
                arg: query.to_owned(),
            }
        }
    }
}

/// One thing asked of the engine's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// A view that takes no argument: All photos, Starred, Recent, Videos, Duplicates, Hidden.
    View(GridView),
    Search(String),
    Album(i64),
    /// A person's key: `p:<id>`, or `c:<hash>` for a Picasa contact no person is linked to.
    Person(String),
    Tag(String),
}

impl Step {
    /// Whether this is a search: the one kind of step a newer one of its kind takes the
    /// place of while it waits its turn. Each word typed while a rebuild runs asks for
    /// another search, and only the last is wanted; a view asked for is wanted even with
    /// another behind it, and so will a change of sort be.
    pub fn is_search(&self) -> bool {
        matches!(self, Step::Search(_))
    }

    /// Where the grid is once this step has landed.
    pub fn leads_to(&self) -> Place {
        match self {
            Step::View(view) => Place::of(*view),
            Step::Search(query) => Place::search(query),
            // The argument as the engine keeps it (`Engine::set_album_view`).
            Step::Album(id) => Place {
                view: GridView::Album,
                arg: id.to_string(),
            },
            Step::Person(key) => Place {
                view: GridView::Person,
                arg: key.clone(),
            },
            Step::Tag(tag) => Place {
                view: GridView::Tag,
                arg: tag.clone(),
            },
        }
    }
}

/// What a step came to, as the worker that made it read the engine when the step was
/// done and before the next one began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landed {
    pub place: Place,
    pub sort: Sort,
    /// The version of the grid that shows it (`Engine::set_view`'s answer), or none when
    /// the engine could not say.
    pub shown_at: Option<u64>,
}

/// A step the engine would not make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The step, when it is one this side still held.
    pub step: Option<Step>,
    /// What the engine said.
    pub said: String,
    /// What the grid shows once the engine has put itself back.
    pub shown: Place,
}

/// What taking a grid came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// Whether the grid shows other results than the one before it, in which case a
    /// position in the old ones means nothing.
    pub other_results: bool,
    /// The folder to go to in this grid: a jump that waited for it.
    pub jump: Option<i64>,
}

/// A folder to go to, in the grid of the step numbered `after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Jump {
    after: u64,
    folder: i64,
}

pub struct Nav {
    /// What the grid on screen shows.
    settled: Place,
    sort: Sort,
    /// The steps asked and not yet answered, oldest first, each with its number.
    asked: VecDeque<(u64, Step)>,
    /// The steps made whose grids have not been taken yet, oldest first, each with its
    /// number.
    landed: VecDeque<(u64, Landed)>,
    jump: Option<Jump>,
}

impl Nav {
    pub fn new(settled: Place, sort: Sort) -> Self {
        Self {
            settled,
            sort,
            asked: VecDeque::new(),
            landed: VecDeque::new(),
            jump: None,
        }
    }

    /// What the grid on screen shows.
    pub fn settled(&self) -> &Place {
        &self.settled
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    /// Where the user is, as they see it: where the last step asked leads, and the settled
    /// place when none is on its way. The sidebar marks this row, so a click is answered
    /// at once and not when a rebuild of the whole library has landed.
    pub fn target(&self) -> Place {
        self.asked
            .back()
            .map(|(_, step)| step.leads_to())
            // Made, and its grid not on screen yet: still where they are going.
            .or_else(|| self.landed.back().map(|(_, landed)| landed.place.clone()))
            .unwrap_or_else(|| self.settled.clone())
    }

    /// Whether a step is on its way: asked, or made and its grid not yet on screen.
    pub fn busy(&self) -> bool {
        !self.asked.is_empty() || !self.landed.is_empty()
    }

    /// Whether `step` leads anywhere the user is not already going. A click on the row of
    /// the view that is shown asks for nothing: All photos most of all, where the click
    /// means "back to where I was" and the user is there.
    pub fn wants(&self, step: &Step) -> bool {
        step.leads_to() != self.target()
    }

    /// `step` was given to the queue under `number`. A number already held is a step the
    /// queue put in another's place (`Queue::push_or_replace`).
    pub fn asked(&mut self, number: u64, step: Step) {
        match self.asked.back_mut() {
            Some((last, held)) if *last == number => {
                *held = step;
                // What lands under this number is no longer the grid a jump waited for.
                self.jump.take_if(|jump| jump.after == number);
            }
            _ => self.asked.push_back((number, step)),
        }
    }

    /// The step that brings the folders' own view on screen, when the user is not in it
    /// or on their way to it.
    ///
    /// The sidebar's folders are the folders of the view shown, and every view but Hidden
    /// is a part of All photos, so All holds the folder a click names. Hidden is left as it
    /// is: it is disjoint from All, and a folder whose photos are all hidden is not in All
    /// at all. Asked of where the user is going, not of the grid on screen: a search sent
    /// a moment before the click would otherwise read as All photos, skip the step, and
    /// then land and carry the jump into its own grid.
    pub fn home_of_folders(&self) -> Option<Step> {
        let home = matches!(self.target().view, GridView::All | GridView::Hidden);
        (!home).then_some(Step::View(GridView::All))
    }

    /// Goes to `folder`. Answers it when the grid on screen is the one to look it up in:
    /// no step is on its way. Otherwise it is kept for the grid of the last step asked
    /// (`adopt` answers it then), since a folder's offset means something only in the grid
    /// it is looked up in. The last folder asked for is the one gone to.
    pub fn jump(&mut self, folder: i64) -> Option<i64> {
        let last = self
            .asked
            .back()
            .map(|(number, _)| *number)
            .or_else(|| self.landed.back().map(|(number, _)| *number));
        self.jump = last.map(|after| Jump { after, folder });
        last.is_none().then_some(folder)
    }

    /// The queue's answer to step `number`: where it led, or what the engine said when it
    /// refused it. Answers a refusal, for the user to be told.
    ///
    /// A refused step changes nothing but that: the engine put its own state back
    /// (`rebuild_or_restore`), and the grid on screen never moved.
    pub fn answered(&mut self, number: u64, outcome: Result<Landed, String>) -> Option<Refused> {
        // Answers come in the order the steps were given, so everything up to this one
        // is done with.
        let mut step = None;
        while let Some((asked, held)) = self.asked.pop_front_if(|(asked, _)| *asked <= number) {
            step = (asked == number).then_some(held);
        }
        match outcome {
            Ok(landed) => {
                self.landed.push_back((number, landed));
                None
            }
            // A jump that waited for this step is never made: its number does not land.
            Err(said) => Some(Refused {
                step,
                said,
                // Where the engine put itself back to: the last step made, shown or not.
                shown: (self.landed.back())
                    .map_or_else(|| self.settled.clone(), |(_, landed)| landed.place.clone()),
            }),
        }
    }

    /// A grid has been published at `version`: may it be put on screen, and does it show
    /// other results than the one there?
    ///
    /// `None` is "not yet": a step is on its way and no answer vouches for this grid. It
    /// may be that step's own, published a moment before its answer arrives, and what it
    /// shows is known only from the answer. The grid on screen stays until then, which is
    /// at most the time of that step's rebuild.
    pub fn adopt(&mut self, version: u64) -> Option<Shown> {
        // The latest step made that this grid can be showing.
        let Some(latest) = self
            .landed
            .iter()
            .rposition(|(_, landed)| landed.shown_at.is_none_or(|at| at <= version))
        else {
            // No step in between: the library changed under the view that is settled.
            let still = self.asked.is_empty() && self.landed.is_empty();
            return still.then_some(Shown {
                other_results: false,
                jump: None,
            });
        };
        // An answer vouches for the grid it names, and for a later one only when no step
        // is on its way: a later grid may be that step's own, in another view. Taken as
        // this answer's, it stood under the wrong name for good - the step's own answer,
        // arriving after it, found no new grid to put on screen.
        let named = self.landed[latest].1.shown_at == Some(version);
        if !self.asked.is_empty() && !named {
            return None;
        }
        let (number, landed) = self.landed.drain(..=latest).next_back()?;
        // A jump is made in the grid of the step it waited for, and in no other: one that
        // waited for an earlier step than the one now shown is never made, its number not
        // coming again, and one that waits for a later step is still to come.
        let jump = self
            .jump
            .take_if(|jump| jump.after == number)
            .map(|jump| jump.folder);
        Some(Shown {
            other_results: self.settle(landed.place, landed.sort),
            jump,
        })
    }

    /// The grid on screen now shows `place` under `sort`. Answers whether those are other
    /// results than before, in which case a position in the old ones means nothing.
    pub fn settle(&mut self, place: Place, sort: Sort) -> bool {
        let before = view_key(&self.settled, self.sort);
        let changed = results_changed(&before, &view_key(&place, sort));
        self.settled = place;
        self.sort = sort;
        changed
    }
}

/// What `results_changed` compares: the view, its argument, and the order - the same
/// photos in another order are another list, and a scroll position in one is arbitrary in
/// the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewKey {
    place: Place,
    key: SortKey,
    reverse: bool,
    /// The grouping, only where it reorders the grid: by date, and not by folder, which
    /// is the view's own order. Under another key it is kept and ignored, and a change to
    /// it must not throw the position away.
    group: Option<Grouping>,
}

pub fn view_key(place: &Place, sort: Sort) -> ViewKey {
    let grouped = sort.key == SortKey::Date && sort.group != Grouping::Folder;
    ViewKey {
        place: place.clone(),
        key: sort.key,
        reverse: sort.reverse,
        group: grouped.then_some(sort.group),
    }
}

/// Whether the grid runs folder by folder, each under its header. The place photon
/// remembers in the library is a folder, so it is a place in this arrangement only: under
/// any other a jump to that folder lands on one of its photos, somewhere, not where the
/// user was.
pub fn laid_out_by_folder(sort: Sort) -> bool {
    sort.key == SortKey::Date && sort.group == Grouping::Folder
}

/// The folder at the top of All photos, as photon remembers it: for the next launch, and
/// for the way back from an excursion.
///
/// It is read from the library once, before the first frame, and kept here from then on:
/// the application is the only thing that writes it, so what it wrote is what is stored.
/// That is also why the way back needs no read before its step, where the Svelte UI has
/// to read the setting before the switch can overwrite it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastFolder {
    folder: Option<i64>,
    /// Whether the launch has yet to go back to it.
    restoring: bool,
    /// Whether no folder is kept at all (`off`).
    off: bool,
}

impl LastFolder {
    /// `stored` is what the last session left in the library.
    pub fn new(stored: Option<i64>) -> Self {
        Self {
            folder: stored,
            restoring: true,
            off: false,
        }
    }

    /// No folder kept at all: none gone back to, none remembered. For a run of the gate,
    /// whose programme starts from the top of the grid, over a library the other
    /// application has left a folder in and that the next run has to find as it was.
    pub fn off() -> Self {
        Self {
            folder: None,
            restoring: false,
            off: true,
        }
    }

    /// The folder for the launch to go back to, answered once: in the frame the first
    /// grid with photos is on screen, and before that grid is drawn - so there is no
    /// frame in which the grid stands at its top and `at_top` remembers the library's
    /// first folder over the one the user left.
    ///
    /// An empty grid is waited out: the engine starts on an index it has not built, and
    /// on a first run the scan is still working; asked of an empty index, every folder is
    /// gone. A user who has already gone to another view is left there, and under an
    /// order that is not by folder the launch opens at the top.
    pub fn restore(&mut self, photos: usize, view: GridView, sort: Sort) -> Option<i64> {
        if !self.restoring || photos == 0 {
            return None;
        }
        self.restoring = false;
        (view == GridView::All).then(|| self.left(sort)).flatten()
    }

    /// The grid shows `view` with `top` at its top. Answers the folder to store, when it
    /// is another than the one remembered.
    ///
    /// Only in All photos: Starred, Recent and a search are excursions, and one of them
    /// overwriting this would lose the place the user was browsing. Only when it becomes
    /// another folder, which is a handful of times a session, where the position changes
    /// on every frame of a flick: this is a write to the library. Under an order that
    /// names no folder at the top there is nothing to remember, which is why the place
    /// survives an excursion into another order.
    pub fn at_top(&mut self, view: GridView, top: Option<i64>) -> Option<i64> {
        let folder = top.filter(|_| view == GridView::All && !self.off)?;
        (self.folder != Some(folder)).then(|| {
            self.folder = Some(folder);
            folder
        })
    }

    /// The folder to come back to from an excursion, where it is a place: under `sort`
    /// the grid runs folder by folder. Under any other All photos opens at its top.
    pub fn left(&self, sort: Sort) -> Option<i64> {
        self.folder.filter(|_| laid_out_by_folder(sort))
    }
}

/// Whether the grid shows a different list of photos than it did. Keyed on the view alone
/// it would miss a search refined within Search, or one album replacing another.
pub fn results_changed(before: &ViewKey, now: &ViewKey) -> bool {
    before != now
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> Nav {
        Nav::new(Place::of(GridView::All), Sort::default())
    }

    /// A step to `view` that the engine made, shown by the grid at `version`.
    fn landed(view: GridView, version: u64) -> Landed {
        Landed {
            place: Place::of(view),
            sort: Sort::default(),
            shown_at: Some(version),
        }
    }

    /// Whether the grid at `version` is taken, and as other results.
    fn adopt(nav: &mut Nav, version: u64) -> Option<bool> {
        nav.adopt(version).map(|shown| shown.other_results)
    }

    /// The folder a grid taken at `version` is to go to.
    fn jump_at(nav: &mut Nav, version: u64) -> Option<i64> {
        nav.adopt(version).and_then(|shown| shown.jump)
    }

    fn sorted(key: SortKey, group: Grouping) -> Sort {
        Sort {
            key,
            reverse: false,
            group,
        }
    }

    #[test]
    fn with_nothing_asked_the_user_is_where_the_grid_is() {
        let nav = nav();
        assert_eq!(nav.target(), Place::of(GridView::All));
        assert!(!nav.busy());
    }

    // A rebuild of a large library takes a moment, and the click must show before it
    // lands: the row marked is the one gone to.
    #[test]
    fn a_step_asked_is_where_the_user_is_going_before_it_lands() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        assert_eq!(nav.target(), Place::of(GridView::Starred));
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        assert!(nav.busy());

        // A second click before the first has landed: the last asked is where they go.
        nav.asked(2, Step::View(GridView::Recent));
        assert_eq!(nav.target(), Place::of(GridView::Recent));
        assert_eq!(nav.answered(1, Ok(landed(GridView::Starred, 1))), None);
        assert_eq!(nav.target(), Place::of(GridView::Recent));
        assert_eq!(nav.answered(2, Ok(landed(GridView::Recent, 2))), None);
        // Made, and on its way to the screen until its grid is taken.
        assert!(nav.busy());
        assert_eq!(adopt(&mut nav, 2), Some(true));
        assert!(!nav.busy());
    }

    #[test]
    fn a_step_to_where_the_user_already_is_asks_for_nothing() {
        let mut nav = nav();
        assert!(!nav.wants(&Step::View(GridView::All)));
        assert!(nav.wants(&Step::View(GridView::Starred)));
        // Asked and not yet landed: the user is going there, and a second click on the
        // same row is not a second rebuild.
        nav.asked(1, Step::View(GridView::Starred));
        assert!(!nav.wants(&Step::View(GridView::Starred)));
        assert!(nav.wants(&Step::View(GridView::All)));
        // The same search is not wanted twice, and another one is.
        nav.asked(2, Step::Search("lake".to_owned()));
        assert!(!nav.wants(&Step::Search("lake".to_owned())));
        assert!(nav.wants(&Step::Search("lake 2024".to_owned())));
    }

    #[test]
    fn a_refused_step_is_said_and_leaves_the_user_where_the_grid_is() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        let refused = nav.answered(1, Err("the library is locked".to_owned()));
        let refused = refused.expect("a refusal is said");
        assert_eq!(refused.said, "the library is locked");
        assert_eq!(nav.target(), Place::of(GridView::All));
        assert!(!nav.busy());
    }

    // Starred refused while Recent is still on its way: the user is going to Recent.
    #[test]
    fn a_refusal_does_not_take_back_a_later_step() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::Recent));
        assert!(nav.answered(1, Err("no".to_owned())).is_some());
        assert_eq!(nav.target(), Place::of(GridView::Recent));
    }

    // The search box has to tell a refused search from a refused switch, and to show what
    // the grid still shows: the step is named, with where the engine put itself back to.
    #[test]
    fn a_refusal_names_its_step_and_where_the_grid_is_left() {
        let mut nav = nav();
        nav.asked(1, Step::Search("lake".to_owned()));
        nav.answered(
            1,
            Ok(Landed {
                place: Place::search("lake"),
                sort: Sort::default(),
                shown_at: Some(1),
            }),
        );
        nav.asked(2, Step::Search("beach".to_owned()));
        let refused = nav.answered(2, Err("no".to_owned())).unwrap();
        assert_eq!(refused.step, Some(Step::Search("beach".to_owned())));
        // Back to the search before it, whose grid is not even on screen yet.
        assert_eq!(refused.shown, Place::search("lake"));

        nav.asked(3, Step::View(GridView::Starred));
        let refused = nav.answered(3, Err("no".to_owned())).unwrap();
        assert_eq!(refused.step, Some(Step::View(GridView::Starred)));
        // An answer for a step nobody holds names none - not the step before it either.
        nav.asked(4, Step::View(GridView::Recent));
        let refused = nav.answered(9, Err("no".to_owned())).unwrap();
        assert_eq!(refused.step, None);
    }

    #[test]
    fn only_a_search_gives_way_to_a_newer_one_while_it_waits() {
        assert!(Step::Search("lake".to_owned()).is_search());
        assert!(Step::Search(String::new()).is_search());
        for step in [
            Step::View(GridView::Starred),
            Step::Album(4),
            Step::Person("p:7".to_owned()),
            Step::Tag("lake".to_owned()),
        ] {
            assert!(!step.is_search(), "{step:?}");
        }
    }

    #[test]
    fn a_step_the_queue_replaced_is_the_one_held() {
        let mut nav = nav();
        nav.asked(1, Step::Search("lak".to_owned()));
        // The same number: the queue put the later search in the first one's place.
        nav.asked(1, Step::Search("lake".to_owned()));
        assert_eq!(nav.target(), Place::search("lake"));
        nav.answered(
            1,
            Ok(Landed {
                place: Place::search("lake"),
                sort: Sort::default(),
                shown_at: Some(1),
            }),
        );
        assert_eq!(adopt(&mut nav, 1), Some(true));
        assert_eq!(nav.settled(), &Place::search("lake"));
        assert!(!nav.busy());
    }

    #[test]
    fn a_blank_search_leads_to_all_photos() {
        assert_eq!(
            Step::Search("  ".to_owned()).leads_to(),
            Place::of(GridView::All)
        );
        assert_eq!(
            Step::Search("lake".to_owned()).leads_to(),
            Place {
                view: GridView::Search,
                arg: "lake".to_owned()
            }
        );
    }

    #[test]
    fn other_results_are_another_view_or_another_argument() {
        let mut nav = nav();
        assert!(nav.settle(Place::of(GridView::Starred), Sort::default()));
        // The same view published again - a star, a scan - is the same list.
        assert!(!nav.settle(Place::of(GridView::Starred), Sort::default()));
        assert!(nav.settle(Place::search("lake"), Sort::default()));
        // A search refined within Search.
        assert!(nav.settle(Place::search("lake 2024"), Sort::default()));
        assert!(!nav.settle(Place::search("lake 2024"), Sort::default()));
        assert_eq!(nav.settled(), &Place::search("lake 2024"));
    }

    #[test]
    fn a_new_sort_is_other_results_and_the_same_sort_published_again_is_not() {
        let mut nav = nav();
        let by_name = sorted(SortKey::Name, Grouping::Folder);
        assert!(nav.settle(Place::of(GridView::All), by_name));
        assert!(!nav.settle(Place::of(GridView::All), by_name));
        let reversed = Sort {
            reverse: true,
            ..by_name
        };
        assert!(nav.settle(Place::of(GridView::All), reversed));
        assert_eq!(nav.sort(), reversed);
    }

    // The grouping reorders the grid only by date. Under another key it is ignored, and a
    // change to it must not throw the scroll position away.
    #[test]
    fn a_new_grouping_is_other_results_by_date_and_nothing_under_another_key() {
        let all = Place::of(GridView::All);
        let key = |key, group| view_key(&all, sorted(key, group));
        assert!(results_changed(
            &key(SortKey::Date, Grouping::Folder),
            &key(SortKey::Date, Grouping::Month)
        ));
        assert!(results_changed(
            &key(SortKey::Date, Grouping::Month),
            &key(SortKey::Date, Grouping::None)
        ));
        assert!(!results_changed(
            &key(SortKey::Name, Grouping::Folder),
            &key(SortKey::Name, Grouping::Month)
        ));
        assert!(!results_changed(
            &key(SortKey::Size, Grouping::Day),
            &key(SortKey::Size, Grouping::None)
        ));
    }

    // A scan, a star, a hide: the library changed under the view that is shown. With no
    // step on its way the grid is taken, and it is the same results.
    #[test]
    fn a_grid_published_with_no_step_on_its_way_is_taken_as_the_view_that_is_shown() {
        let mut nav = nav();
        assert_eq!(adopt(&mut nav, 4), Some(false));
        assert_eq!(nav.settled(), &Place::of(GridView::All));
    }

    // The engine publishes a step's grid a moment before the step's answer is back, and a
    // scan may publish while a step is being made. Which of the two a grid is, and so what
    // it shows, is known only from the answer.
    #[test]
    fn a_grid_that_arrives_while_a_step_is_on_its_way_waits_for_the_steps_answer() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        assert_eq!(adopt(&mut nav, 5), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 5), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
        // The same grid asked about again, or a later one in that view: the same results.
        assert_eq!(adopt(&mut nav, 5), Some(false));
        assert_eq!(adopt(&mut nav, 6), Some(false));
    }

    // Starred, then All photos before Starred has landed. Starred's grid arrives while All
    // photos is being built - when the engine already says All photos. It is Starred's
    // grid: other results than the ones on screen, shown as Starred.
    #[test]
    fn the_first_of_two_steps_is_shown_as_itself_while_the_second_is_built() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::All));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 5), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
        assert_eq!(
            nav.target(),
            Place::of(GridView::All),
            "still on the way there"
        );

        // A scan's grid in Starred while All photos is still being built: no answer
        // vouches for it, and it waits.
        assert_eq!(adopt(&mut nav, 6), None);
        nav.answered(2, Ok(landed(GridView::All, 7)));
        assert_eq!(adopt(&mut nav, 7), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::All));
    }

    // The first step's answer is back and its grid was never drawn; the grid published now
    // is newer than the one that answer vouches for, and the second step is still on its
    // way. It may be the second's own - the engine publishes a moment before it answers -
    // so it is not taken as the first's: taken so, the starred photos stood under "All
    // photos" for good, the answer that followed finding nothing new to put on screen.
    #[test]
    fn a_grid_newer_than_the_last_answer_waits_while_a_step_is_on_its_way() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::Recent));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 6), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        nav.answered(2, Ok(landed(GridView::Recent, 6)));
        assert_eq!(adopt(&mut nav, 6), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Recent));
        assert!(!nav.busy());
    }

    // Answered, and its grid not on screen yet: the user is still going there. Read as
    // "nothing on its way", the row marked fell back to the view on screen for a frame.
    #[test]
    fn a_step_answered_and_not_yet_shown_is_still_where_the_user_is_going() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 4), None);
        assert_eq!(nav.target(), Place::of(GridView::Starred));
        assert!(nav.busy());
        assert!(!nav.wants(&Step::View(GridView::Starred)));
        assert_eq!(adopt(&mut nav, 5), Some(true));
        assert!(!nav.busy());
    }

    // Both answers back before the interface drew a frame: the grid published is the
    // second's, and the first's is never shown.
    #[test]
    fn two_steps_landed_between_two_frames_are_the_second() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::Recent));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        nav.answered(2, Ok(landed(GridView::Recent, 6)));
        assert_eq!(adopt(&mut nav, 6), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Recent));
        assert!(!nav.busy());
    }

    // The engine put its own view back and published that. Nothing on screen changed
    // view, so nothing is thrown back to its top.
    #[test]
    fn the_grid_after_a_refused_step_is_the_view_that_was_shown() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 5), Some(true));
        nav.asked(2, Step::View(GridView::Hidden));
        assert!(nav.answered(2, Err("no".to_owned())).is_some());
        assert_eq!(adopt(&mut nav, 7), Some(false));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
    }

    // A grid older than the one a landed step is shown by is not that step's.
    #[test]
    fn a_landed_step_is_not_shown_by_a_grid_from_before_it() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(adopt(&mut nav, 4), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        assert_eq!(adopt(&mut nav, 5), Some(true));
    }

    // The sort a step led to is the step's too.
    #[test]
    fn a_landed_step_brings_its_sort() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::All));
        let by_name = sorted(SortKey::Name, Grouping::Folder);
        nav.answered(
            1,
            Ok(Landed {
                place: Place::of(GridView::All),
                sort: by_name,
                shown_at: Some(3),
            }),
        );
        assert_eq!(
            adopt(&mut nav, 3),
            Some(true),
            "another order is other results"
        );
        assert_eq!(nav.sort(), by_name);
    }

    #[test]
    fn a_step_to_an_album_a_person_or_a_tag_leads_where_the_engine_puts_it() {
        // `Engine::set_album_view` keeps the id as the view's argument, in digits.
        assert_eq!(
            Step::Album(42).leads_to(),
            Place {
                view: GridView::Album,
                arg: "42".to_owned()
            }
        );
        assert_eq!(
            Step::Person("p:7".to_owned()).leads_to(),
            Place {
                view: GridView::Person,
                arg: "p:7".to_owned()
            }
        );
        assert_eq!(
            Step::Tag("lake".to_owned()).leads_to(),
            Place {
                view: GridView::Tag,
                arg: "lake".to_owned()
            }
        );
    }

    // The sidebar's folders are the folders of the view shown, and every view but Hidden
    // is a part of All photos: All holds the folder jumped to. Hidden is disjoint from it,
    // and a folder whose photos are all hidden is not in All at all.
    #[test]
    fn a_folder_is_gone_to_in_all_photos_or_in_hidden() {
        let mut nav = nav();
        assert_eq!(nav.home_of_folders(), None, "already in All photos");
        for view in [GridView::Starred, GridView::Recent, GridView::Album] {
            nav.settle(Place::of(view), Sort::default());
            assert_eq!(nav.home_of_folders(), Some(Step::View(GridView::All)));
        }
        nav.settle(Place::search("lake"), Sort::default());
        assert_eq!(nav.home_of_folders(), Some(Step::View(GridView::All)));
        nav.settle(Place::of(GridView::Hidden), Sort::default());
        assert_eq!(nav.home_of_folders(), None);
        // It is where the user is going that counts: a search sent a moment ago must not
        // read as All photos and carry the jump into its own grid.
        nav.settle(Place::of(GridView::All), Sort::default());
        nav.asked(1, Step::Search("lake".to_owned()));
        assert_eq!(nav.home_of_folders(), Some(Step::View(GridView::All)));
    }

    #[test]
    fn a_jump_with_no_step_on_its_way_is_made_at_once() {
        let mut nav = nav();
        assert_eq!(nav.jump(30), Some(30));
        // And nothing of it is kept for a later grid.
        assert_eq!(jump_at(&mut nav, 4), None);
    }

    // A folder's offset means something only in the grid it is looked up in. Behind a step
    // to All photos the jump waits for that step's grid, and for no grid before it.
    #[test]
    fn a_jump_behind_a_step_waits_for_that_steps_grid() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Starred), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        // A scan's grid in Starred meanwhile is not the one.
        assert_eq!(nav.adopt(4), None);
        nav.answered(1, Ok(landed(GridView::All, 5)));
        let shown = nav.adopt(5).expect("the step's grid is taken");
        assert_eq!(shown.jump, Some(30));
        assert!(shown.other_results);
        // Once.
        assert_eq!(jump_at(&mut nav, 6), None);
    }

    // The step is made and answered, and its grid not on screen yet: the jump still waits
    // for it and is not made in the grid that is.
    #[test]
    fn a_jump_behind_a_step_that_has_been_answered_waits_for_its_grid() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Starred), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        nav.answered(1, Ok(landed(GridView::All, 5)));
        assert_eq!(nav.jump(30), None);
        assert_eq!(jump_at(&mut nav, 5), Some(30));
    }

    // The engine would not go to All photos: the grid on screen is still the excursion's,
    // and the folder's place in it is not what was asked for.
    #[test]
    fn a_refused_step_takes_its_jump_with_it() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Starred), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        assert!(nav.answered(1, Err("no".to_owned())).is_some());
        assert_eq!(jump_at(&mut nav, 6), None);
        // A refusal of an earlier step is not this one's.
        nav.asked(2, Step::View(GridView::Recent));
        nav.asked(3, Step::View(GridView::All));
        assert_eq!(nav.jump(31), None);
        assert!(nav.answered(2, Err("no".to_owned())).is_some());
        nav.answered(3, Ok(landed(GridView::All, 8)));
        assert_eq!(jump_at(&mut nav, 8), Some(31));
    }

    // A folder, then Starred, both landed before a frame was drawn: Starred is what is on
    // screen, and the folder's place in All photos means nothing there.
    #[test]
    fn a_later_step_shown_at_once_drops_the_jump() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Recent), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        nav.asked(2, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::All, 5)));
        nav.answered(2, Ok(landed(GridView::Starred, 6)));
        let shown = nav.adopt(6).expect("taken");
        assert_eq!(shown.jump, None);
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
        assert_eq!(jump_at(&mut nav, 7), None, "and it does not come back");
    }

    // Shown by itself, with the later step still being made, the jump's own grid is the
    // one on screen: the jump is made in it.
    #[test]
    fn a_jump_is_made_in_its_own_grid_with_a_later_step_behind_it() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Recent), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        nav.asked(2, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::All, 5)));
        assert_eq!(jump_at(&mut nav, 5), Some(30));
    }

    // A folder clicked behind two steps: the first step's grid, shown by itself, is not
    // the one the folder is looked up in, and taking it must not lose the jump.
    #[test]
    fn a_jump_waits_through_the_grid_of_a_step_before_its_own() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(jump_at(&mut nav, 5), None);
        nav.answered(2, Ok(landed(GridView::All, 6)));
        assert_eq!(jump_at(&mut nav, 6), Some(30));
    }

    // The queue put another step in the place of the one waited for: what lands under
    // that number is not the grid the folder was to be looked up in.
    #[test]
    fn a_step_put_in_the_place_of_the_one_waited_for_drops_the_jump() {
        let mut nav = nav();
        nav.asked(1, Step::Search("lak".to_owned()));
        assert_eq!(nav.jump(30), None);
        nav.asked(1, Step::Search("lake".to_owned()));
        nav.answered(
            1,
            Ok(Landed {
                place: Place::search("lake"),
                sort: Sort::default(),
                shown_at: Some(3),
            }),
        );
        assert_eq!(jump_at(&mut nav, 3), None);
    }

    #[test]
    fn the_last_folder_asked_for_is_the_one_gone_to() {
        let mut nav = nav();
        nav.settle(Place::of(GridView::Starred), Sort::default());
        nav.asked(1, Step::View(GridView::All));
        assert_eq!(nav.jump(30), None);
        assert_eq!(nav.jump(31), None);
        nav.answered(1, Ok(landed(GridView::All, 5)));
        assert_eq!(jump_at(&mut nav, 5), Some(31));
    }

    // The place photon remembers in the library is a folder, which is a place only where
    // the grid runs folder by folder.
    #[test]
    fn the_grid_runs_folder_by_folder_by_date_under_the_folder_grouping_alone() {
        assert!(laid_out_by_folder(sorted(SortKey::Date, Grouping::Folder)));
        assert!(!laid_out_by_folder(sorted(SortKey::Date, Grouping::Month)));
        assert!(!laid_out_by_folder(sorted(SortKey::Date, Grouping::None)));
        assert!(!laid_out_by_folder(sorted(SortKey::Name, Grouping::Folder)));
        // Turned over it still runs folder by folder.
        assert!(laid_out_by_folder(Sort {
            reverse: true,
            ..sorted(SortKey::Date, Grouping::Folder)
        }));
    }

    fn by_folder() -> Sort {
        sorted(SortKey::Date, Grouping::Folder)
    }

    // An empty grid is waited out: the engine starts on an index it has not built, and on
    // a first run the scan is still working. Asked of an empty index every folder is gone.
    #[test]
    fn the_launch_goes_back_to_the_folder_left_once_there_is_a_grid_to_go_back_in() {
        let mut last = LastFolder::new(Some(30));
        assert_eq!(last.restore(0, GridView::All, by_folder()), None);
        assert_eq!(last.restore(0, GridView::All, by_folder()), None);
        assert_eq!(last.restore(900, GridView::All, by_folder()), Some(30));
        // Once: after it the grid is the user's.
        assert_eq!(last.restore(900, GridView::All, by_folder()), None);
        // And with nothing remembered there is nowhere to go.
        assert_eq!(
            LastFolder::new(None).restore(900, GridView::All, by_folder()),
            None
        );
    }

    // The folder is a place in All photos as it runs folder by folder. A user who has
    // already gone elsewhere is left there, and under another order the launch opens at
    // the top - and neither is made up for later.
    #[test]
    fn the_launch_goes_back_only_in_all_photos_laid_out_by_folder() {
        let mut gone = LastFolder::new(Some(30));
        assert_eq!(gone.restore(40, GridView::Starred, by_folder()), None);
        assert_eq!(gone.restore(900, GridView::All, by_folder()), None);
        let mut by_month = LastFolder::new(Some(30));
        let month = sorted(SortKey::Date, Grouping::Month);
        assert_eq!(by_month.restore(900, GridView::All, month), None);
        assert_eq!(by_month.restore(900, GridView::All, by_folder()), None);
    }

    #[test]
    fn the_folder_at_the_top_of_all_photos_is_remembered_when_it_becomes_another() {
        let mut last = LastFolder::new(Some(30));
        // The one remembered already is not written again: the position changes on
        // every frame of a flick, and this is a write to the library.
        assert_eq!(last.at_top(GridView::All, Some(30)), None);
        assert_eq!(last.at_top(GridView::All, Some(31)), Some(31));
        assert_eq!(last.at_top(GridView::All, Some(31)), None);
        assert_eq!(last.left(by_folder()), Some(31));
        // No folder at the top - an empty grid, an order that names none - is nothing to
        // remember, and does not forget.
        assert_eq!(last.at_top(GridView::All, None), None);
        assert_eq!(last.left(by_folder()), Some(31));
    }

    // Starred, Recent, a search: one of them overwriting the folder would lose the place
    // the user was browsing.
    #[test]
    fn an_excursion_does_not_change_the_folder_remembered() {
        let mut last = LastFolder::new(Some(30));
        for view in [GridView::Starred, GridView::Recent, GridView::Search] {
            assert_eq!(last.at_top(view, Some(44)), None, "{view:?}");
        }
        assert_eq!(last.left(by_folder()), Some(30));
    }

    // Under a month grouping a jump to the folder would land on one of its photos
    // wherever the order put it: somewhere, not where the user was.
    #[test]
    fn the_folder_left_is_a_place_only_where_the_grid_runs_folder_by_folder() {
        let last = LastFolder::new(Some(30));
        assert_eq!(last.left(by_folder()), Some(30));
        assert_eq!(last.left(sorted(SortKey::Date, Grouping::Month)), None);
        assert_eq!(last.left(sorted(SortKey::Name, Grouping::Folder)), None);
        assert_eq!(LastFolder::new(None).left(by_folder()), None);
    }

    // The gate measures a grid that starts at its top, over a library the other
    // application has left a folder in: a measured run neither goes back nor remembers.
    #[test]
    fn a_folder_that_is_not_kept_is_neither_gone_back_to_nor_remembered() {
        let mut off = LastFolder::off();
        assert_eq!(off.restore(900, GridView::All, by_folder()), None);
        assert_eq!(off.at_top(GridView::All, Some(31)), None);
        assert_eq!(off.left(by_folder()), None);
        assert_eq!(off.at_top(GridView::All, Some(32)), None);
    }
}
