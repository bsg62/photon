//! Where the user is in the library, and where they have asked to be.
//!
//! The engine holds one view and one sort, and a step that changes either rebuilds the grid
//! on the thread that makes it. Steps are therefore made on `tasks::Queue`, in the order
//! asked, and this module holds what the interface knows meanwhile: the place the published
//! grid shows, and the steps asked that have not landed. It is the part of the Svelte UI's
//! `LibraryStore` that `switchView`, `viewChain` and `settledView` were.
//!
//! No egui here, and no engine: the application reads the engine's view when a grid is
//! published and tells this module.

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

pub struct Nav {
    /// What the published grid shows.
    settled: Place,
    sort: Sort,
    /// The steps asked and not yet answered, oldest first, each with its number.
    asked: VecDeque<(u64, Step)>,
}

impl Nav {
    pub fn new(settled: Place, sort: Sort) -> Self {
        Self {
            settled,
            sort,
            asked: VecDeque::new(),
        }
    }

    /// What the published grid shows.
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
            .map_or_else(|| self.settled.clone(), |(_, step)| step.leads_to())
    }

    /// Whether a step is on its way.
    pub fn busy(&self) -> bool {
        !self.asked.is_empty()
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

    /// The queue's answer to step `number`: what the engine said when it refused it, or
    /// nothing when it was made. Answers what to tell the user.
    ///
    /// A refused step changes nothing but that: the engine put its own state back
    /// (`rebuild_or_restore`), and the place this holds is the published grid's, which
    /// never moved.
    pub fn answered(&mut self, number: u64, refused: Option<String>) -> Option<String> {
        // Answers come in the order the steps were given, so everything up to this one
        // is done with.
        while self
            .asked
            .front()
            .is_some_and(|(asked, _)| *asked <= number)
        {
            self.asked.pop_front();
        }
        refused
    }

    /// The grid now published shows `place` under `sort`. Answers whether those are other
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
        assert_eq!(nav.answered(1, None), None);
        assert_eq!(nav.target(), Place::of(GridView::Recent));
        assert_eq!(nav.answered(2, None), None);
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
        let said = nav.answered(1, Some("the library is locked".to_owned()));
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
        assert!(nav.answered(1, Some("no".to_owned())).is_some());
        assert_eq!(nav.target(), Place::of(GridView::Recent));
    }

    #[test]
    fn a_step_the_queue_replaced_is_the_one_held() {
        let mut nav = nav();
        nav.asked(1, Step::Search("lak".to_owned()));
        // The same number: the queue put the later search in the first one's place.
        nav.asked(1, Step::Search("lake".to_owned()));
        assert_eq!(nav.target(), Place::search("lake"));
        nav.answered(1, None);
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
}
