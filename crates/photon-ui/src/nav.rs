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
}

impl Step {
    /// Where the grid is once this step has landed.
    pub fn leads_to(&self) -> Place {
        match self {
            Step::View(view) => Place::of(*view),
            Step::Search(query) => Place::search(query),
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

pub struct Nav {
    /// What the grid on screen shows.
    settled: Place,
    sort: Sort,
    /// The steps asked and not yet answered, oldest first, each with its number.
    asked: VecDeque<(u64, Step)>,
    /// The steps made whose grids have not been taken yet, oldest first.
    landed: VecDeque<Landed>,
}

impl Nav {
    pub fn new(settled: Place, sort: Sort) -> Self {
        Self {
            settled,
            sort,
            asked: VecDeque::new(),
            landed: VecDeque::new(),
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
            .or_else(|| self.landed.back().map(|landed| landed.place.clone()))
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
            Some((last, held)) if *last == number => *held = step,
            _ => self.asked.push_back((number, step)),
        }
    }

    /// The queue's answer to step `number`: where it led, or what the engine said when it
    /// refused it. Answers what to tell the user.
    ///
    /// A refused step changes nothing but that: the engine put its own state back
    /// (`rebuild_or_restore`), and the grid on screen never moved.
    pub fn answered(&mut self, number: u64, outcome: Result<Landed, String>) -> Option<String> {
        // Answers come in the order the steps were given, so everything up to this one
        // is done with.
        while self
            .asked
            .front()
            .is_some_and(|(asked, _)| *asked <= number)
        {
            self.asked.pop_front();
        }
        match outcome {
            Ok(landed) => {
                self.landed.push_back(landed);
                None
            }
            Err(refused) => Some(refused),
        }
    }

    /// A grid has been published at `version`: may it be put on screen, and does it show
    /// other results than the one there?
    ///
    /// `None` is "not yet": a step is on its way and no answer vouches for this grid. It
    /// may be that step's own, published a moment before its answer arrives, and what it
    /// shows is known only from the answer. The grid on screen stays until then, which is
    /// at most the time of that step's rebuild.
    pub fn adopt(&mut self, version: u64) -> Option<bool> {
        // The latest step made that this grid can be showing.
        let Some(latest) = self
            .landed
            .iter()
            .rposition(|landed| landed.shown_at.is_none_or(|at| at <= version))
        else {
            // No step in between: the library changed under the view that is settled.
            let still = self.asked.is_empty() && self.landed.is_empty();
            return still.then_some(false);
        };
        // An answer vouches for the grid it names, and for a later one only when no step
        // is on its way: a later grid may be that step's own, in another view. Taken as
        // this answer's, it stood under the wrong name for good - the step's own answer,
        // arriving after it, found no new grid to put on screen.
        let named = self.landed[latest].shown_at == Some(version);
        if !self.asked.is_empty() && !named {
            return None;
        }
        let landed = self.landed.drain(..=latest).next_back()?;
        Some(self.settle(landed.place, landed.sort))
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
        assert_eq!(nav.adopt(2), Some(true));
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
        let said = nav.answered(1, Err("the library is locked".to_owned()));
        assert_eq!(said.as_deref(), Some("the library is locked"));
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
        assert_eq!(nav.adopt(1), Some(true));
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
        assert_eq!(nav.adopt(4), Some(false));
        assert_eq!(nav.settled(), &Place::of(GridView::All));
    }

    // The engine publishes a step's grid a moment before the step's answer is back, and a
    // scan may publish while a step is being made. Which of the two a grid is, and so what
    // it shows, is known only from the answer.
    #[test]
    fn a_grid_that_arrives_while_a_step_is_on_its_way_waits_for_the_steps_answer() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        assert_eq!(nav.adopt(5), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(nav.adopt(5), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
        // The same grid asked about again, or a later one in that view: the same results.
        assert_eq!(nav.adopt(5), Some(false));
        assert_eq!(nav.adopt(6), Some(false));
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
        assert_eq!(nav.adopt(5), Some(true));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
        assert_eq!(
            nav.target(),
            Place::of(GridView::All),
            "still on the way there"
        );

        // A scan's grid in Starred while All photos is still being built: no answer
        // vouches for it, and it waits.
        assert_eq!(nav.adopt(6), None);
        nav.answered(2, Ok(landed(GridView::All, 7)));
        assert_eq!(nav.adopt(7), Some(true));
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
        assert_eq!(nav.adopt(6), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        nav.answered(2, Ok(landed(GridView::Recent, 6)));
        assert_eq!(nav.adopt(6), Some(true));
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
        assert_eq!(nav.adopt(4), None);
        assert_eq!(nav.target(), Place::of(GridView::Starred));
        assert!(nav.busy());
        assert!(!nav.wants(&Step::View(GridView::Starred)));
        assert_eq!(nav.adopt(5), Some(true));
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
        assert_eq!(nav.adopt(6), Some(true));
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
        assert_eq!(nav.adopt(5), Some(true));
        nav.asked(2, Step::View(GridView::Hidden));
        assert!(nav.answered(2, Err("no".to_owned())).is_some());
        assert_eq!(nav.adopt(7), Some(false));
        assert_eq!(nav.settled(), &Place::of(GridView::Starred));
    }

    // A grid older than the one a landed step is shown by is not that step's.
    #[test]
    fn a_landed_step_is_not_shown_by_a_grid_from_before_it() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.answered(1, Ok(landed(GridView::Starred, 5)));
        assert_eq!(nav.adopt(4), None);
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        assert_eq!(nav.adopt(5), Some(true));
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
        assert_eq!(nav.adopt(3), Some(true), "another order is other results");
        assert_eq!(nav.sort(), by_name);
    }
}
