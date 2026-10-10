//! The search box's own state, apart from the field that draws it: its text, the send
//! that typing has made due, and what a view switch does to both. `search-box.svelte.ts`
//! on a clock that is a number.
//!
//! The sidebar has to be able to drop a send that is due: a click on Starred or a folder
//! switches the view, and a search that fired after it would re-enter Search with the text
//! it had caught and replace the grid the user had just asked for. Two views needing the
//! same `cancel` is why this is a module of its own.
//!
//! The box's text is only ever written from this side. The engine clears its query on any
//! view switch, and every switch is asked for by the interface, so the switch empties the
//! box itself (`leave`) rather than the box watching the engine's query and trying to tell
//! its own echo from a change made elsewhere - a guess that took the Svelte UI three
//! attempts and still swallowed characters.
//!
//! No egui here, and no engine: what is to be sent is handed back to the caller.

use photon_core::library::SavedSearch;

/// How long after the last key a typed search is sent.
pub const SEARCH_DEBOUNCE_MS: f64 = 150.0;

#[derive(Debug, Default)]
pub struct SearchBox {
    /// What the box holds. The field edits it in place and then says so (`typed`).
    pub text: String,
    /// When the text is to be sent, if typing has made a send due.
    due: Option<f64>,
    /// The text a view switch emptied the box of, and the number of the step that was
    /// the switch: put back if that step is refused.
    left: Option<(u64, String)>,
    /// The searches being saved: written, or on their way to be, and not yet in the
    /// lists as they were last read.
    saving: Vec<String>,
}

impl SearchBox {
    /// The text was changed by typing, at `now_ms`: it is sent once the typing has
    /// stopped for a moment, and a send already due is put off with it.
    pub fn typed(&mut self, now_ms: f64) {
        self.due = Some(now_ms + SEARCH_DEBOUNCE_MS);
    }

    /// When a send is due: the frame to ask for, since a still window draws none.
    pub fn due_at(&self) -> Option<f64> {
        self.due
    }

    /// The text to send, when its time has come.
    pub fn take_due(&mut self, now_ms: f64) -> Option<String> {
        self.due.take_if(|at| *at <= now_ms)?;
        Some(self.text.clone())
    }

    /// Drops a send that is due rather than making it. Whatever switches the view does
    /// this first; see the module's comment.
    pub fn cancel(&mut self) {
        self.due = None;
    }

    /// The box is to hold `query`, which the caller sends now: a saved search's row, a
    /// link elsewhere in the interface. A send that was due is dropped first, or the text
    /// the user had half typed would land after this and replace it.
    pub fn search(&mut self, query: &str) {
        self.cancel();
        self.text = query.to_owned();
    }

    /// Empties the box; the caller sends the empty query. The send that was due is
    /// dropped, or it would land after the clear and put the grid straight back into the
    /// search.
    pub fn clear(&mut self) {
        self.cancel();
        self.text.clear();
    }

    /// The view is switching away, by step `step`, which clears the engine's query: the
    /// box empties to match, and a send that was due is dropped so that it cannot
    /// re-enter Search behind the switch.
    pub fn leave(&mut self, step: u64) {
        self.cancel();
        self.left = Some((step, std::mem::take(&mut self.text)));
    }

    /// Step `step` was refused, and the grid still shows the search for `shown` (nothing,
    /// outside a search).
    ///
    /// A refused switch puts back the text it emptied the box of - but only while the box
    /// is still empty, since anything typed since is what the user wants now. A refused
    /// search (`asked` is its query) leaves the grid on what it showed, and the box shows
    /// that too: left on the refused text it would offer to save a search that filters
    /// nothing, and Escape would then move the grid. Unless the user has typed on.
    pub fn refused(&mut self, step: u64, asked: Option<&str>, shown: &str) {
        if let Some((_, left)) = self.left.take_if(|(left_by, _)| *left_by == step) {
            if self.text.is_empty() {
                self.text = left;
            }
        } else if asked == Some(self.text.as_str()) {
            self.text = shown.to_owned();
        }
    }
}

impl SearchBox {
    /// Whether the bookmark can save what the box holds: a search that is neither saved
    /// nor being saved. The lists are read a moment after the write, and until they are
    /// the search does not look saved; a second press in that moment saved it twice.
    pub fn can_save(&self, searches: &[SavedSearch]) -> bool {
        let query = default_search_name(&self.text);
        can_save_search(searches, &self.text) && !self.saving.iter().any(|saving| saving == query)
    }

    /// The bookmark was pressed. Answers the search to write, under its own text - the
    /// sidebar is where it gets a friendlier name - or nothing when it is being saved
    /// already or there is none.
    pub fn save(&mut self) -> Option<String> {
        let query = default_search_name(&self.text).to_owned();
        if query.is_empty() || self.saving.contains(&query) {
            return None;
        }
        self.saving.push(query.clone());
        Some(query)
    }

    /// The library would not save `query`.
    pub fn save_refused(&mut self, query: &str) {
        self.saving.retain(|saving| saving != query);
    }

    /// The lists were read: what they hold is saved, and no longer being saved.
    pub fn lists_read(&mut self, searches: &[SavedSearch]) {
        (self.saving).retain(|saving| !searches.iter().any(|search| search.query == *saving));
    }
}

/// The saved search the box already holds, if any.
///
/// Exact after trimming, and with case: the grammar reads `OR` and `AND` as operators only
/// in capitals, so `lake or pond` and `lake OR pond` are two searches and can be saved
/// apart. Trimmed as `create_saved_search` stores a query, or one typed with a space after
/// it would look unsaved right after being saved.
pub fn saved_search_for<'a>(searches: &'a [SavedSearch], query: &str) -> Option<&'a SavedSearch> {
    let wanted = query.trim();
    if wanted.is_empty() {
        return None;
    }
    searches.iter().find(|search| search.query == wanted)
}

/// Whether the bookmark can save what the box holds: there has to be a query, and it must
/// not be saved already. A saved one leaves the button filled and taking no press rather
/// than removing it - a saved search is deleted from the sidebar, which asks first.
pub fn can_save_search(searches: &[SavedSearch], query: &str) -> bool {
    !query.trim().is_empty() && saved_search_for(searches, query).is_none()
}

/// The name the bookmark saves a search under: the query itself, which is what the
/// sidebar then shows, so the button never has to stop and ask.
pub fn default_search_name(query: &str) -> &str {
    query.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(text: &str) -> SearchBox {
        SearchBox {
            text: text.to_owned(),
            ..SearchBox::default()
        }
    }

    #[test]
    fn a_typed_query_is_sent_once_the_typing_has_stopped_for_a_moment() {
        let mut search = holding("bea");
        search.typed(1000.0);
        assert_eq!(search.due_at(), Some(1150.0));
        assert_eq!(search.take_due(1100.0), None);
        // Another key: the send is put off, and it is the whole text that is sent.
        search.text.push_str("ch");
        search.typed(1100.0);
        assert_eq!(search.take_due(1150.0), None);
        assert_eq!(search.take_due(1250.0).as_deref(), Some("beach"));
        // Once.
        assert_eq!(search.due_at(), None);
        assert_eq!(search.take_due(5000.0), None);
    }

    #[test]
    fn a_send_that_is_due_is_dropped_on_cancel_so_a_view_switch_is_not_undone_by_it() {
        let mut search = holding("beach");
        search.typed(0.0);
        search.cancel();
        assert_eq!(search.due_at(), None);
        assert_eq!(search.take_due(1500.0), None);
        assert_eq!(search.text, "beach", "the text is the user's still");
    }

    #[test]
    fn clearing_empties_the_box_and_drops_the_send_that_would_land_after_it() {
        let mut search = holding("beach");
        search.typed(0.0);
        search.clear();
        assert_eq!(search.text, "");
        assert_eq!(search.take_due(1500.0), None);
    }

    // Without the cancel, "bea" fires after the link's query and replaces it.
    #[test]
    fn a_linked_search_is_what_the_box_holds_and_drops_what_was_half_typed() {
        let mut search = holding("bea");
        search.typed(0.0);
        search.search("camera:\"NIKON D750\"");
        assert_eq!(search.text, "camera:\"NIKON D750\"");
        assert_eq!(search.take_due(1500.0), None);
    }

    // The engine clears its query on a view switch; a box still holding "beach" would
    // offer a search that no longer filters anything.
    #[test]
    fn the_box_is_emptied_when_the_view_is_left_and_the_send_that_was_due_is_dropped() {
        let mut search = holding("beach");
        search.typed(0.0);
        search.leave(7);
        assert_eq!(search.text, "");
        assert_eq!(search.take_due(1500.0), None);
    }

    #[test]
    fn a_refused_switch_puts_the_text_back() {
        let mut search = holding("beach");
        search.leave(7);
        // Another step refused is not this one.
        search.refused(6, None, "");
        assert_eq!(search.text, "");
        search.refused(7, None, "");
        assert_eq!(search.text, "beach");
        // Once: refused again, there is nothing more to put back.
        search.text.clear();
        search.refused(7, None, "");
        assert_eq!(search.text, "");
    }

    #[test]
    fn what_was_typed_after_leaving_is_kept_when_the_switch_is_refused() {
        let mut search = holding("beach");
        search.leave(7);
        search.text = "hut".to_owned();
        search.refused(7, None, "");
        assert_eq!(search.text, "hut");
    }

    // The grid still shows "lake"; a box left on "beach" would offer to save a search
    // that filters nothing, and Escape would then move the grid.
    #[test]
    fn a_refused_search_leaves_the_box_showing_what_the_grid_shows() {
        let mut search = SearchBox::default();
        search.search("beach");
        search.refused(3, Some("beach"), "lake");
        assert_eq!(search.text, "lake");
        // Refused from outside a search, the grid shows none.
        search.search("beach");
        search.refused(4, Some("beach"), "");
        assert_eq!(search.text, "");
    }

    #[test]
    fn what_was_typed_on_is_kept_after_a_refused_search() {
        let mut search = SearchBox::default();
        search.search("beach");
        search.text = "beaches".to_owned();
        search.refused(3, Some("beach"), "lake");
        assert_eq!(search.text, "beaches");
    }

    fn saved(queries: &[&str]) -> Vec<SavedSearch> {
        (queries.iter().zip(1..))
            .map(|(query, id)| SavedSearch {
                id,
                name: format!("search {id}"),
                query: (*query).to_owned(),
                created_ms: 0,
            })
            .collect()
    }

    #[test]
    fn the_saved_search_the_box_holds_is_found_by_its_query() {
        let searches = saved(&["lake 2024", "lake OR pond"]);
        assert_eq!(
            saved_search_for(&searches, "lake 2024").map(|s| s.id),
            Some(1)
        );
        // Space around the typed query is ignored, as saving ignores it.
        assert_eq!(
            saved_search_for(&searches, "  lake 2024 ").map(|s| s.id),
            Some(1)
        );
        assert_eq!(saved_search_for(&searches, "lake"), None);
        // A capitalised operator is not a lowercase word.
        assert_eq!(saved_search_for(&searches, "lake or pond"), None);
        assert_eq!(
            saved_search_for(&searches, "lake OR pond").map(|s| s.id),
            Some(2)
        );
        // An empty box holds none, even if a blank one had been saved.
        assert_eq!(saved_search_for(&saved(&[""]), "   "), None);
    }

    // The lists come back a moment after the write, and until they do the search does
    // not look saved: a second press in that moment saved it twice.
    #[test]
    fn a_search_being_saved_is_not_saved_again() {
        let mut search = holding(" lake 2024 ");
        let none = saved(&[]);
        assert!(search.can_save(&none));
        assert_eq!(
            search.save().as_deref(),
            Some("lake 2024"),
            "trimmed, as it is stored"
        );
        assert!(!search.can_save(&none));
        assert_eq!(search.save(), None);
        // Another search is another matter.
        search.text = "pond".to_owned();
        assert!(search.can_save(&none));
        // The lists hold it now: it is saved, and no longer being saved.
        search.text = "lake 2024".to_owned();
        let lists = saved(&["lake 2024"]);
        search.lists_read(&lists);
        assert!(!search.can_save(&lists));
        // Deleted again elsewhere, it can be saved again.
        search.lists_read(&none);
        assert!(search.can_save(&none));
    }

    // The library would not have it: the button is the user's again, to try once more.
    #[test]
    fn a_save_that_was_refused_can_be_tried_again() {
        let mut search = holding("lake 2024");
        let none = saved(&[]);
        assert_eq!(search.save().as_deref(), Some("lake 2024"));
        search.save_refused("lake 2024");
        assert!(search.can_save(&none));
        assert_eq!(search.save().as_deref(), Some("lake 2024"));
    }

    #[test]
    fn a_search_can_be_saved_when_there_is_one_and_it_is_not_saved_already() {
        let searches = saved(&["lake 2024"]);
        assert!(!can_save_search(&searches, ""));
        assert!(!can_save_search(&searches, "   "));
        // Saved already: the button takes no press, rather than being a toggle.
        assert!(!can_save_search(&searches, "lake 2024 "));
        assert!(can_save_search(&searches, "pond"));
        assert_eq!(default_search_name("  lake 2024 "), "lake 2024");
    }
}
