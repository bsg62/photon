//! Where the grid is, and its scrollbar.
//!
//! The position is an `f64` in the layout's own coordinates (`layout::Row::top`), and the
//! view draws each visible row at `row.top - position`. The grid's rows are never put in a
//! scrolling container: egui's `ScrollArea` keeps its offset in an `f32`, which counts
//! pixels exactly only to 16,777,216, and a large library at large tiles is more than twice
//! that tall. So there is no scroll map here, as `ui/src/lib/scroll-map.ts` is for a
//! browser's 33,554,428px box: nothing has a limit to map around.

/// The room the scrollbar takes at the grid's right edge. Always taken, whether or not
/// there is anything to scroll: a bar that came with the overflow would narrow the tiles,
/// which shortens the grid, which takes the overflow away.
pub const BAR_WIDTH: f64 = 12.0;
/// The shortest the thumb is drawn, so there is something to take hold of in a library
/// thousands of viewports tall.
pub const MIN_THUMB: f64 = 32.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    position: f64,
    total: f64,
    viewport: f64,
}

impl Scroll {
    pub fn position(&self) -> f64 {
        self.position
    }

    /// The furthest the grid scrolls: the layout's last pixel at the viewport's bottom edge.
    pub fn max(&self) -> f64 {
        (self.total - self.viewport).max(0.0)
    }

    /// The layout's height and the viewport's, on every frame. A position past the new end
    /// is held to it.
    pub fn set_extent(&mut self, total: f64, viewport: f64) {
        self.total = total.max(0.0);
        self.viewport = viewport.max(0.0);
        self.position = self.position.clamp(0.0, self.max());
    }

    pub fn set(&mut self, position: f64) {
        // `clamp` panics on NaN bounds and passes a NaN value through; a NaN position would
        // draw nothing, for ever.
        self.position = if position.is_nan() {
            0.0
        } else {
            position.clamp(0.0, self.max())
        };
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.set(self.position + delta);
    }

    /// The thumb on a track `track` long, as `(start, length)`; `None` when the layout fits
    /// the viewport and there is nothing to scroll.
    pub fn thumb(&self, track: f64) -> Option<(f64, f64)> {
        let max = self.max();
        if max <= 0.0 || track <= 0.0 {
            return None;
        }
        let length = (track * self.viewport / self.total)
            .max(MIN_THUMB)
            .min(track);
        Some(((track - length) * (self.position / max), length))
    }

    /// The position a thumb whose top is at `start` stands for: the inverse of `thumb`,
    /// exact at both ends, so a thumb dragged to the bottom is the last row.
    pub fn position_for_thumb(&self, track: f64, start: f64) -> f64 {
        let Some((_, length)) = self.thumb(track) else {
            return 0.0;
        };
        let travel = track - length;
        if travel <= 0.0 {
            return 0.0;
        }
        self.max() * (start / travel).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taller than twice what an `f32` counts exactly, and than a browser's box.
    const TALL: f64 = 40_000_000.0;
    const VIEWPORT: f64 = 1000.0;

    fn tall() -> Scroll {
        let mut scroll = Scroll::default();
        scroll.set_extent(TALL, VIEWPORT);
        scroll
    }

    // `scroll-probe`'s successor: every pixel of a library this tall is a place the grid
    // can be. In an `f32` the step below 33,000,000 is 2 and above it 4.
    #[test]
    fn a_step_of_one_pixel_moves_one_pixel_anywhere_in_a_tall_library() {
        let mut scroll = tall();
        for from in [0.0, 16_777_216.0, 33_554_428.0, TALL - VIEWPORT - 1.0] {
            scroll.set(from);
            scroll.scroll_by(1.0);
            assert_eq!(scroll.position(), from + 1.0, "from {from}");
        }
    }

    #[test]
    fn the_end_of_a_tall_library_is_reachable_and_is_the_end() {
        let mut scroll = tall();
        scroll.set(f64::INFINITY);
        assert_eq!(scroll.position(), TALL - VIEWPORT);
        scroll.scroll_by(500.0);
        assert_eq!(scroll.position(), TALL - VIEWPORT);
        scroll.set(-5.0);
        assert_eq!(scroll.position(), 0.0);
        scroll.set(f64::NAN);
        assert_eq!(scroll.position(), 0.0);
    }

    #[test]
    fn a_layout_that_shrinks_holds_the_position_to_its_new_end() {
        let mut scroll = tall();
        scroll.set(30_000_000.0);
        scroll.set_extent(5000.0, VIEWPORT);
        assert_eq!(scroll.position(), 4000.0);
        // And one that fits the viewport has nowhere to be but the top.
        scroll.set_extent(800.0, VIEWPORT);
        assert_eq!(scroll.position(), 0.0);
        assert_eq!(scroll.max(), 0.0);
    }

    #[test]
    fn the_thumb_is_at_the_tracks_ends_at_the_layouts_ends() {
        let mut scroll = tall();
        let track = 900.0;
        // 900 * 1000 / 40,000,000 is far under the minimum.
        assert_eq!(scroll.thumb(track), Some((0.0, MIN_THUMB)));
        scroll.set(f64::INFINITY);
        assert_eq!(scroll.thumb(track), Some((track - MIN_THUMB, MIN_THUMB)));
    }

    #[test]
    fn a_thumb_dragged_to_an_end_is_that_end_exactly() {
        let scroll = tall();
        let track = 900.0;
        assert_eq!(scroll.position_for_thumb(track, 0.0), 0.0);
        assert_eq!(
            scroll.position_for_thumb(track, track - MIN_THUMB),
            TALL - VIEWPORT
        );
        // Past either end of the track is the end, not beyond it.
        assert_eq!(scroll.position_for_thumb(track, 5000.0), TALL - VIEWPORT);
        assert_eq!(scroll.position_for_thumb(track, -40.0), 0.0);
    }

    #[test]
    fn the_thumb_and_the_position_are_each_others_inverse() {
        let mut scroll = Scroll::default();
        scroll.set_extent(10_000.0, VIEWPORT);
        let track = 1000.0;
        // A tenth of the layout is in view: the thumb is a tenth of the track.
        assert_eq!(scroll.thumb(track), Some((0.0, 100.0)));
        for position in [0.0, 1234.0, 4500.0, 9000.0] {
            scroll.set(position);
            let (start, _) = scroll.thumb(track).unwrap();
            assert!((scroll.position_for_thumb(track, start) - position).abs() < 1e-6);
        }
    }

    #[test]
    fn there_is_no_thumb_when_everything_fits() {
        let mut scroll = Scroll::default();
        scroll.set_extent(500.0, VIEWPORT);
        assert_eq!(scroll.thumb(900.0), None);
        assert_eq!(scroll.position_for_thumb(900.0, 300.0), 0.0);
        assert_eq!(Scroll::default().thumb(900.0), None);
    }
}
