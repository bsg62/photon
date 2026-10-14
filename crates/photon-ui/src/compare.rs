//! Compare's state: two to four photos side by side, at one zoom over one point -
//! `createCompare` and `differingFacts`, with their tests' cases.
//!
//! **The zoom and the pan are held once, not per pane**, and that is the feature: two
//! photos at 200% on the same eye is the comparison a person is trying to make. They are
//! the viewer's own `Zoom`, measured in a pane's size, so the arithmetic - the point a
//! zoom holds still, the pan's bounds - is the viewer's and is tested there.

use crate::{
    viewer_caption::{format_taken, oriented},
    viewer_zoom::{Pan, Zoom},
};

/// Two is the fewest that is a comparison; four is where a 2x2 stops being legible at any
/// window size worth supporting.
pub const MIN_PANES: usize = 2;
pub const MAX_PANES: usize = 4;

pub fn can_compare(count: usize) -> bool {
    (MIN_PANES..=MAX_PANES).contains(&count)
}

/// What a pane is of, as the library said it when compare opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: i64,
    pub file_name: String,
    /// As stored: an unedited photo's are the file's, with the orientation beside them.
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    /// The key of the picture it is: which preview and which full picture are its.
    pub key: u64,
    pub starred: bool,
}

/// What a pane says of itself beyond its file name: empty where every pane agrees.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// "4000 × 3000".
    pub size: String,
    pub taken: String,
}

/// What distinguishes each pane from its neighbours. Only a fact the panes disagree about
/// is said, and then on every pane: a lone number beside three blanks says nothing about
/// the blanks. Sizes are compared as they are seen, turned by the orientation: a portrait
/// and a landscape of one camera are not "the same size".
pub fn differing_facts(panes: &[Pane]) -> Vec<Facts> {
    let seen = |pane: &Pane| oriented(pane.width, pane.height, pane.orientation);
    let differ = |of: &dyn Fn(&Pane) -> String| {
        panes
            .first()
            .is_some_and(|first| panes.iter().any(|pane| of(pane) != of(first)))
    };
    let size = |pane: &Pane| {
        let (width, height) = seen(pane);
        format!("{width} × {height}")
    };
    let taken = |pane: &Pane| format_taken(pane.taken_at);
    let (sizes, takens) = (differ(&size), differ(&taken));
    (panes.iter())
        .map(|pane| Facts {
            size: if sizes { size(pane) } else { String::new() },
            taken: if takens { taken(pane) } else { String::new() },
        })
        .collect()
}

/// Two to four photos side by side, with the pane the keys act on and the one zoom every
/// pane is drawn at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compare {
    panes: Vec<Pane>,
    focus: usize,
    zoom: Zoom,
}

impl Compare {
    /// Compare, open on `panes` with the first one's keys - or none, for a count that is
    /// no comparison.
    pub fn open(panes: Vec<Pane>) -> Option<Self> {
        can_compare(panes.len()).then_some(Self {
            panes,
            focus: 0,
            zoom: Zoom::default(),
        })
    }

    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    /// The pane the keys act on.
    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn zoom(&self) -> &Zoom {
        &self.zoom
    }

    /// The zoom, to be changed in a frame, with a pane's size.
    pub fn zoom_mut(&mut self) -> &mut Zoom {
        &mut self.zoom
    }

    /// A pane's star, as the library has it since.
    pub fn starred(&mut self, id: i64, starred: bool) {
        for pane in self.panes.iter_mut().filter(|pane| pane.id == id) {
            pane.starred = starred;
        }
    }

    /// A pane's photo under another key - turned or rewritten elsewhere: answers which
    /// pane, for its full picture to be made again.
    pub fn rekey(&mut self, id: i64, key: u64) -> Option<usize> {
        let n = self
            .panes
            .iter()
            .position(|pane| pane.id == id && pane.key != key)?;
        self.panes[n].key = key;
        Some(n)
    }

    /// Out of range is no move: `3` is a key a person can press over two panes.
    pub fn focus_pane(&mut self, index: usize) {
        if index < self.panes.len() {
            self.focus = index;
        }
    }

    pub fn next_pane(&mut self) {
        self.focus = (self.focus + 1) % self.panes.len().max(1);
    }

    pub fn prev_pane(&mut self) {
        let len = self.panes.len().max(1);
        self.focus = (self.focus + len - 1) % len;
    }

    /// Whether a pane's full picture is wanted: once the zoom has left the fit, every
    /// pane's. Fitted, a pane is drawn from its preview, 1600 pixels - twice or three
    /// times a pane in a window of ordinary size, drawn from one level.
    pub fn wants_full(&self) -> bool {
        !self.zoom.is_fit()
    }

    /// The pan, as numbers.
    pub fn pan(&self) -> Pan {
        self.zoom.pan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer_zoom::{MAX_ZOOM, MIN_ZOOM};

    fn pane(id: i64) -> Pane {
        Pane {
            id,
            file_name: format!("IMG_{id}.jpg"),
            width: 4000,
            height: 3000,
            orientation: 1,
            taken_at: 1_700_000_000 + id,
            key: id as u64,
            starred: false,
        }
    }

    fn sized(id: i64, width: u32, height: u32, taken_at: i64) -> Pane {
        Pane {
            width,
            height,
            taken_at,
            ..pane(id)
        }
    }

    fn opened(ids: &[i64]) -> Compare {
        Compare::open(ids.iter().map(|id| pane(*id)).collect()).unwrap()
    }

    #[test]
    fn two_to_four_are_a_comparison_and_nothing_else() {
        assert!(!can_compare(0) && !can_compare(1) && !can_compare(5));
        assert!(can_compare(MIN_PANES) && can_compare(3) && can_compare(MAX_PANES));
        assert!(Compare::open(vec![pane(1)]).is_none());
        assert!(Compare::open((1..=5).map(pane).collect()).is_none());
    }

    #[test]
    fn it_opens_with_a_pane_for_each_photo_the_first_with_the_keys_at_fit() {
        let c = opened(&[7, 8, 9]);
        assert_eq!(
            c.panes().iter().map(|p| p.id).collect::<Vec<_>>(),
            [7, 8, 9]
        );
        assert_eq!(c.focus(), 0);
        assert!(c.zoom().is_fit());
        assert!(!c.wants_full());
    }

    #[test]
    fn the_zoom_is_one_for_every_pane_and_the_viewers_range() {
        let mut c = opened(&[1, 2]);
        c.zoom_mut().by(100.0, Pan::default(), (800.0, 600.0));
        assert_eq!(c.zoom().zoom(), MAX_ZOOM);
        c.zoom_mut().by(0.0001, Pan::default(), (800.0, 600.0));
        assert_eq!(c.zoom().zoom(), MIN_ZOOM);
    }

    // The case the Svelte store shipped wrong: a second notch about the same point keeps
    // it still too (the viewer's `zoom_at`).
    #[test]
    fn a_second_zoom_about_the_same_point_keeps_it_still() {
        let mut c = opened(&[1, 2]);
        let at = Pan { x: 100.0, y: 50.0 };
        c.zoom_mut().by(2.0, at, (800.0, 600.0));
        c.zoom_mut().by(2.0, at, (800.0, 600.0));
        assert_eq!(c.zoom().zoom(), 4.0);
        assert!((c.pan().x + 300.0).abs() < 1e-3 && (c.pan().y + 150.0).abs() < 1e-3);
    }

    #[test]
    fn the_keys_move_by_number_and_round_both_ways() {
        let mut c = opened(&[1, 2, 3]);
        c.focus_pane(2);
        assert_eq!(c.focus(), 2);
        c.next_pane();
        assert_eq!(c.focus(), 0);
        c.focus_pane(9);
        assert_eq!(c.focus(), 0);
        c.prev_pane();
        assert_eq!(c.focus(), 2);
        c.prev_pane();
        assert_eq!(c.focus(), 1);
    }

    #[test]
    fn every_pane_wants_its_full_picture_once_zoomed_and_none_at_fit() {
        let mut c = opened(&[1, 2, 3]);
        c.zoom_mut().by(2.0, Pan::default(), (800.0, 600.0));
        assert!(c.wants_full());
        c.zoom_mut().set(MIN_ZOOM, (800.0, 600.0));
        assert!(!c.wants_full());
    }

    #[test]
    fn a_star_changes_its_pane_alone() {
        let mut c = opened(&[1, 2]);
        c.starred(2, true);
        assert_eq!(
            c.panes().iter().map(|p| p.starred).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn nothing_is_said_where_every_pane_agrees() {
        let facts = differing_facts(&[sized(1, 4000, 3000, 100), sized(2, 4000, 3000, 100)]);
        assert_eq!(facts, vec![Facts::default(), Facts::default()]);
    }

    #[test]
    fn every_panes_size_is_said_once_any_differs() {
        let facts = differing_facts(&[sized(1, 4000, 3000, 100), sized(2, 2000, 1500, 100)]);
        let sizes: Vec<&str> = facts.iter().map(|f| f.size.as_str()).collect();
        assert_eq!(sizes, ["4000 × 3000", "2000 × 1500"]);
        assert!(facts.iter().all(|f| f.taken.is_empty()));
    }

    #[test]
    fn every_panes_time_is_said_once_any_differs() {
        let facts = differing_facts(&[
            sized(1, 4000, 3000, 1_718_454_645),
            sized(2, 4000, 3000, 1_718_454_700),
        ]);
        let taken: Vec<&str> = facts.iter().map(|f| f.taken.as_str()).collect();
        assert_eq!(taken, ["Jun 15, 2024, 12:30 PM", "Jun 15, 2024, 12:31 PM"]);
        assert!(facts.iter().all(|f| f.size.is_empty()));
    }

    #[test]
    fn a_fact_is_said_on_every_pane_when_one_of_three_differs() {
        let facts = differing_facts(&[
            sized(1, 4000, 3000, 100),
            sized(2, 4000, 3000, 100),
            sized(3, 4000, 2250, 100),
        ]);
        let sizes: Vec<&str> = facts.iter().map(|f| f.size.as_str()).collect();
        assert_eq!(sizes, ["4000 × 3000", "4000 × 3000", "4000 × 2250"]);
    }

    #[test]
    fn a_quarter_turned_photo_is_compared_and_said_as_it_is_seen() {
        let portrait = Pane {
            orientation: 6,
            ..sized(1, 4000, 3000, 100)
        };
        let facts = differing_facts(&[portrait, sized(2, 4000, 3000, 100)]);
        let sizes: Vec<&str> = facts.iter().map(|f| f.size.as_str()).collect();
        assert_eq!(sizes, ["3000 × 4000", "4000 × 3000"]);
    }

    #[test]
    fn a_width_alone_is_a_difference() {
        let facts = differing_facts(&[sized(1, 4000, 3000, 100), sized(2, 3000, 3000, 100)]);
        let sizes: Vec<&str> = facts.iter().map(|f| f.size.as_str()).collect();
        assert_eq!(sizes, ["4000 × 3000", "3000 × 3000"]);
    }

    #[test]
    fn fewer_than_two_panes_have_nothing_to_compare() {
        assert_eq!(
            differing_facts(&[sized(1, 4000, 3000, 100)]),
            vec![Facts::default()]
        );
        assert_eq!(differing_facts(&[]), Vec::<Facts>::new());
    }
}
