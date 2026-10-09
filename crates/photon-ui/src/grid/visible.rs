//! Telling the engine's thumbnail queue what is on screen, once scrolling settles.
//!
//! The queue renders what is visible first (`ThumbService::set_visible`). Telling it on
//! every frame of a scroll would reorder it sixty times a second for rows nobody stops on,
//! so the photos in view are reported only once they have been the same for
//! `VISIBLE_DEBOUNCE_MS`, as the Svelte grid's timer did.

/// How long the photos in view must stay the same before they are reported.
pub const VISIBLE_DEBOUNCE_MS: f64 = 150.0;

#[derive(Debug, Default)]
pub struct VisibleReport {
    /// What is in view now, and since when.
    seen: Vec<i64>,
    since: f64,
    /// What the engine was last told.
    reported: Vec<i64>,
}

impl VisibleReport {
    /// The photos in view this frame, at `now` (milliseconds). Answers them when they are
    /// due to be reported: unchanged for the debounce, and not what was reported last.
    pub fn update(&mut self, on_screen: &[i64], now: f64) -> Option<&[i64]> {
        if self.seen != on_screen {
            self.seen = on_screen.to_vec();
            self.since = now;
        }
        if self.seen != self.reported && now - self.since >= VISIBLE_DEBOUNCE_MS {
            self.reported = self.seen.clone();
            return Some(&self.reported);
        }
        None
    }

    /// When `update` will next have something to report, if nothing changes: the frame to
    /// ask for, since a still grid draws no frame by itself.
    pub fn due_at(&self) -> Option<f64> {
        (self.seen != self.reported).then_some(self.since + VISIBLE_DEBOUNCE_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_in_view_is_reported_once_it_has_stayed_the_same() {
        let mut report = VisibleReport::default();
        assert_eq!(report.update(&[1, 2, 3], 1000.0), None);
        assert_eq!(report.due_at(), Some(1000.0 + VISIBLE_DEBOUNCE_MS));
        assert_eq!(
            report.update(&[1, 2, 3], 1000.0 + VISIBLE_DEBOUNCE_MS - 1.0),
            None
        );
        assert_eq!(
            report.update(&[1, 2, 3], 1000.0 + VISIBLE_DEBOUNCE_MS),
            Some(&[1, 2, 3][..])
        );
        // And once only.
        assert_eq!(report.update(&[1, 2, 3], 5000.0), None);
        assert_eq!(report.due_at(), None);
    }

    #[test]
    fn a_scroll_reports_nothing_until_it_stops() {
        let mut report = VisibleReport::default();
        let mut now = 0.0;
        for first in 0..100 {
            assert_eq!(report.update(&[first, first + 1], now), None);
            now += 16.0;
        }
        assert_eq!(
            report.update(&[99, 100], now + VISIBLE_DEBOUNCE_MS),
            Some(&[99, 100][..])
        );
    }

    #[test]
    fn coming_back_to_what_was_reported_reports_nothing() {
        let mut report = VisibleReport::default();
        report.update(&[1], 0.0);
        assert!(report.update(&[1], 200.0).is_some());
        report.update(&[2], 300.0);
        // Back before the other view was ever reported.
        assert_eq!(report.update(&[1], 320.0), None);
        assert_eq!(report.update(&[1], 900.0), None);
        assert_eq!(report.due_at(), None);
    }

    #[test]
    fn an_empty_grid_has_nothing_to_report() {
        let mut report = VisibleReport::default();
        assert_eq!(report.update(&[], 0.0), None);
        assert_eq!(report.update(&[], 1000.0), None);
        assert_eq!(report.due_at(), None);
    }
}
