//! What the view of a photo's copies says of itself - `copies.ts`, with its tests' cases.
//! The view is `GridView::Copies`, the photo and its copies, opened from the info panel's
//! "Show N duplicates in the grid" and the tile menu's "Show N duplicates"; the sidebar
//! names it under Duplicates by the photo's file name.

use photon_core::grid::GridView;

/// Whether a tile with copies is marked so in `view`: not in Duplicates nor in a photo's
/// copies, where every photo has some - that is what put it there - and a mark on every
/// tile would say nothing.
pub fn marked(view: GridView) -> bool {
    !matches!(view, GridView::Duplicates | GridView::Copies)
}

/// The photo a Copies view is of, as the library last said it: the engine's
/// `commands::CopiesOf`, read with the sidebar's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopiesOf {
    pub id: i64,
    /// Empty once the photo has left the library: the library has no name for it then.
    pub file_name: String,
    /// The photo itself is gone - purged, or missing. The view's filter is keyed off its
    /// row, so the view is empty whatever its copies are doing.
    pub gone: bool,
    /// The user has hidden the photo: its copies stay in the view, and it does not.
    pub hidden: bool,
}

/// The name the view was opened with is kept once the library has none for the photo, so
/// the sidebar's row does not go blank under the user. `gone` is always the new answer's:
/// it is no name, and carried forward it would say "still here" after the very purge that
/// made it stale.
pub fn keep_name(prev: Option<&CopiesOf>, next: Option<CopiesOf>) -> Option<CopiesOf> {
    match (prev, next) {
        (Some(prev), Some(next)) if next.file_name.is_empty() && prev.id == next.id => {
            Some(CopiesOf {
                file_name: prev.file_name.clone(),
                ..next
            })
        }
        (_, next) => next,
    }
}

/// What the view says of itself, over an empty grid or under one that holds little: `len`
/// photos are in it, the first of them `first`.
///
/// Once the photo itself is gone its view is empty whatever its copies are doing, so the
/// count says nothing: the line is about the photo. A hidden photo is live and not in the
/// view, so what is left is its copies. Otherwise there is something to say only when the
/// view has shrunk to the photo alone or emptied; one photo left that is *not* it is
/// something the filter cannot make, and nothing is made up for it.
pub fn notice(of: Option<&CopiesOf>, len: usize, first: Option<i64>) -> Option<String> {
    let of = of?;
    let name = |or: &'static str| {
        if of.file_name.is_empty() {
            or.to_owned()
        } else {
            of.file_name.clone()
        }
    };
    if of.gone {
        return Some(format!(
            "{} is no longer in the library; its copies are under Duplicates.",
            name("This photo")
        ));
    }
    if of.hidden && len > 0 {
        return Some(format!(
            "{} is hidden; these are its copies. Unhide it from Hidden.",
            name("This photo")
        ));
    }
    if len == 0 || (len == 1 && first == Some(of.id)) {
        return Some(format!(
            "No other copies of {} any more.",
            name("this photo")
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(id: i64, file_name: &str, gone: bool, hidden: bool) -> CopiesOf {
        CopiesOf {
            id,
            file_name: file_name.to_owned(),
            gone,
            hidden,
        }
    }

    #[test]
    fn the_name_is_kept_when_the_photo_has_since_left_the_library() {
        let prev = of(7, "a.jpg", false, false);
        assert_eq!(
            keep_name(Some(&prev), Some(of(7, "", true, false))),
            Some(of(7, "a.jpg", true, false))
        );
        // Not one photo's name onto another.
        assert_eq!(
            keep_name(Some(&prev), Some(of(8, "", false, false))),
            Some(of(8, "", false, false))
        );
        // A fresh name is taken, and no view is no view.
        assert_eq!(
            keep_name(Some(&prev), Some(of(7, "b.jpg", false, false))),
            Some(of(7, "b.jpg", false, false))
        );
        assert_eq!(keep_name(Some(&prev), None), None);
        // `gone` is the new answer's, even while the old name is kept.
        assert_eq!(
            keep_name(
                Some(&of(7, "a.jpg", true, false)),
                Some(of(7, "", false, false))
            ),
            Some(of(7, "a.jpg", false, false))
        );
    }

    #[test]
    fn nothing_is_said_while_the_view_is_not_open() {
        assert_eq!(notice(None, 0, None), None);
    }

    #[test]
    fn a_photo_that_is_gone_is_said_to_be_whatever_is_left() {
        let gone = of(7, "a.jpg", true, false);
        let said = "a.jpg is no longer in the library; its copies are under Duplicates.";
        assert_eq!(notice(Some(&gone), 0, None).as_deref(), Some(said));
        assert_eq!(notice(Some(&gone), 2, Some(9)).as_deref(), Some(said));
        assert_eq!(
            notice(Some(&of(7, "", true, false)), 0, None).as_deref(),
            Some("This photo is no longer in the library; its copies are under Duplicates.")
        );
    }

    #[test]
    fn no_other_copies_is_said_of_an_empty_view_and_of_the_photo_alone() {
        let here = of(7, "a.jpg", false, false);
        let none = Some("No other copies of a.jpg any more.");
        assert_eq!(notice(Some(&here), 0, None).as_deref(), none);
        assert_eq!(
            notice(Some(&of(7, "", false, false)), 0, None).as_deref(),
            Some("No other copies of this photo any more.")
        );
        assert_eq!(notice(Some(&here), 1, Some(7)).as_deref(), none);
        // One left that is not the photo: nothing the filter makes, nothing said.
        assert_eq!(notice(Some(&here), 1, Some(9)), None);
        // More than the photo: nothing to say.
        assert_eq!(notice(Some(&here), 2, Some(7)), None);
    }

    #[test]
    fn a_hidden_photo_is_said_to_be_hidden_while_its_copies_are_in_the_view() {
        let hidden = of(7, "a.jpg", false, true);
        let said = Some("a.jpg is hidden; these are its copies. Unhide it from Hidden.");
        assert_eq!(notice(Some(&hidden), 1, Some(9)).as_deref(), said);
        assert_eq!(notice(Some(&hidden), 3, Some(9)).as_deref(), said);
        assert_eq!(
            notice(Some(&hidden), 0, None).as_deref(),
            Some("No other copies of a.jpg any more.")
        );
    }

    #[test]
    fn the_copies_mark_is_left_off_where_every_photo_has_copies() {
        assert!(marked(GridView::All));
        assert!(marked(GridView::Search));
        assert!(!marked(GridView::Duplicates));
        assert!(!marked(GridView::Copies));
    }

    // The words are the Svelte view's.
    #[test]
    fn the_words_are_the_svelte_views() {
        let svelte = include_str!("../../../ui/src/lib/copies.ts");
        for words in [
            "is no longer in the library; its copies are under Duplicates.",
            "is hidden; these are its copies. Unhide it from Hidden.",
            "No other copies of ${",
            "|| 'This photo'",
            "|| 'this photo'",
        ] {
            assert!(svelte.contains(words), "{words}");
        }
    }
}
