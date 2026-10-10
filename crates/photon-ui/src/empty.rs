//! What is said where there are no photos: nothing before the grid is known, why when the
//! library could not be read, and each view's own line when it is simply empty. The Svelte
//! UI's `grid-state.ts`, and the notices `Grid.svelte` wrote beside it.
//!
//! No egui here.

use crate::grid::labels::photo_count as counted;
use crate::nav::Place;
use photon_core::grid::GridView;
use photon_engine::engine::NOT_BUILT;

/// The grid on hand, as far as these rules need it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridState {
    pub version: u64,
    pub len: usize,
    /// Why the grid is empty, when it is only because the library could not be read at
    /// launch (`Engine::build_first_grid`).
    pub build_error: Option<String>,
}

/// Whether the engine has built a grid at all. `Engine::open` leaves the first build to its
/// startup thread and holds an empty index at `NOT_BUILT` until it lands.
pub fn grid_built(grid: &GridState) -> bool {
    grid.version > NOT_BUILT
}

/// What is said in place of photos when the library could not be read at launch.
pub fn build_failure(grid: &GridState) -> Option<String> {
    grid.build_error
        .as_ref()
        .map(|why| format!("photon could not read the library: {why}"))
}

/// Whether the grid should say it is empty. Not before the first build: the index is empty
/// then because nothing has been read, and on a large library the line stood on screen for
/// as long as the read took. Nor when that build failed: that grid is empty because
/// nothing *could* be read.
pub fn show_empty_notice(grid: &GridState) -> bool {
    grid_built(grid) && grid.len == 0 && grid.build_error.is_none()
}

/// The status bar's count, or nothing before the first build and after a failed one, when
/// "0 photos" would be the same misreading.
pub fn photo_count(grid: &GridState) -> Option<String> {
    (grid_built(grid) && grid.build_error.is_none()).then(|| counted(grid.len))
}

/// The line an empty view shows in place of its photos. All photos and Recent have none
/// here: an empty library says why in a panel of its own. Nor have the views that are
/// reached through a list the sidebar does not hold yet.
pub fn view_notice(grid: &GridState, place: &Place) -> Option<String> {
    if let Some(failure) = build_failure(grid) {
        return Some(failure);
    }
    if !show_empty_notice(grid) {
        return None;
    }
    match place.view {
        GridView::Starred => {
            Some("No starred photos. Star one in the viewer, or in Picasa.".to_owned())
        }
        GridView::Search => Some(format!("No photos match “{}”", place.arg)),
        GridView::Duplicates => {
            Some("No duplicates. Every photo in the library is the only copy of itself.".to_owned())
        }
        GridView::Videos => Some(
            "No videos. photon finds MP4, M4V, MOV and WebM files in your watched folders."
                .to_owned(),
        ),
        GridView::Hidden => Some(
            "No hidden photos. Right-click a photo and choose Hide to put it away here.".to_owned(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(version: u64, len: usize) -> GridState {
        GridState {
            version,
            len,
            build_error: None,
        }
    }

    #[test]
    fn nothing_is_said_of_a_grid_that_has_not_been_built() {
        let unbuilt = grid(NOT_BUILT, 0);
        assert!(!grid_built(&unbuilt));
        assert!(!show_empty_notice(&unbuilt));
        assert_eq!(photo_count(&unbuilt), None);
        assert_eq!(view_notice(&unbuilt, &Place::of(GridView::Starred)), None);
    }

    #[test]
    fn a_built_grid_is_counted_and_an_empty_one_says_so() {
        assert_eq!(photo_count(&grid(3, 1234)).as_deref(), Some("1,234 photos"));
        assert_eq!(photo_count(&grid(3, 0)).as_deref(), Some("0 photos"));
        assert!(show_empty_notice(&grid(3, 0)));
        assert!(!show_empty_notice(&grid(3, 1)));
    }

    // That grid is empty because nothing could be read: "No starred photos" would be a
    // statement about the library, and "0 photos" the same.
    #[test]
    fn a_library_that_could_not_be_read_says_that_and_nothing_else() {
        let failed = GridState {
            version: 1,
            len: 0,
            build_error: Some("disk I/O error".to_owned()),
        };
        assert!(!show_empty_notice(&failed));
        assert_eq!(photo_count(&failed), None);
        for view in [GridView::All, GridView::Starred] {
            assert_eq!(
                view_notice(&failed, &Place::of(view)).as_deref(),
                Some("photon could not read the library: disk I/O error")
            );
        }
    }

    #[test]
    fn an_empty_view_says_what_it_is_empty_of() {
        let empty = grid(2, 0);
        let said = |place: &Place| view_notice(&empty, place).unwrap_or_default();
        assert!(said(&Place::of(GridView::Starred)).starts_with("No starred photos."));
        assert_eq!(
            said(&Place::search("lake 2031")),
            "No photos match “lake 2031”"
        );
        assert!(said(&Place::of(GridView::Duplicates)).starts_with("No duplicates."));
        assert!(said(&Place::of(GridView::Videos)).starts_with("No videos."));
        assert!(said(&Place::of(GridView::Hidden)).starts_with("No hidden photos."));
        // An empty library has a panel of its own for these two.
        assert_eq!(view_notice(&empty, &Place::of(GridView::All)), None);
        assert_eq!(view_notice(&empty, &Place::of(GridView::Recent)), None);
        // And a view with photos has no line.
        assert_eq!(
            view_notice(&grid(2, 5), &Place::of(GridView::Starred)),
            None
        );
    }
}
