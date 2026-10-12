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

/// How far one notch of a mouse wheel moves what is under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notch {
    /// So many points.
    Points(f64),
    /// A page of what is in view: Windows' "one screen at a time".
    Page,
}

/// What egui moves a notch by, which is also WebKitGTK's: the web view of Linux.
pub const NOTCH: f64 = 40.0;
/// What WebView2 moves a page by for each line of Windows' setting of lines a notch: a
/// hundred pixels at the three lines Windows comes with (Chromium's own constant).
pub const WINDOWS_LINE: f64 = 100.0 / 3.0;
/// The lines a notch Windows comes with, for when its setting cannot be read.
pub const WINDOWS_LINES: u32 = 3;
/// What Windows says for "one screen at a time" where it says lines (`WHEEL_PAGESCROLL`).
pub const WINDOWS_PAGE: u32 = u32::MAX;

/// How far one notch of a mouse wheel moves what is under it: as far as the web view of
/// the system moved it. `lines` is Windows' setting of lines a notch, where it could be
/// read (`wheel::lines`).
///
/// A wheel reports notches and leaves the distance to the application. egui's own answer
/// is forty points, which is also what WebKitGTK scrolls a page by - so on Linux the
/// native grid moved as the Tauri photon's did. WebView2 scrolls a hundred pixels a notch
/// at Windows' own setting of three lines, and there the native grid was two and a half
/// times slower than the one it replaces: the first thing said by the first person to run
/// it. A hundred for everyone was the first answer, and the second thing said was that it
/// was still not the web view's speed with the setting turned up: WebView2 goes by the
/// setting, a third of a hundred pixels for each line, a page where the setting is "one
/// screen at a time", and nothing where it is nought.
pub fn wheel_notch(windows: bool, lines: Option<u32>) -> Notch {
    if !windows {
        return Notch::Points(NOTCH);
    }
    match lines.unwrap_or(WINDOWS_LINES) {
        WINDOWS_PAGE => Notch::Page,
        lines => Notch::Points(f64::from(lines) * WINDOWS_LINE),
    }
}

impl Notch {
    /// The points a notch moves a list of which `in_view` points are in view. A page is
    /// what Chromium turns: all but forty points of it, or seven eighths where that is
    /// more, so that the last row read is still there to go on from.
    pub fn points(self, in_view: f64) -> f64 {
        match self {
            Self::Points(points) => points,
            Self::Page => (in_view - 40.0).max(in_view * 0.875).max(1.0),
        }
    }
}

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

    // WebView2 moves a page a hundred pixels for a notch of the wheel, at Windows' own
    // setting of three lines a notch; WebKitGTK moves it forty. The first person to run
    // the native grid on Windows found it "very slow" beside the Tauri photon: egui's
    // forty, where the web view's hundred had been.
    #[test]
    fn a_wheel_notch_moves_the_grid_as_far_as_the_web_view_moved_it() {
        assert_eq!(wheel_notch(true, Some(3)), Notch::Points(100.0));
        assert_eq!(wheel_notch(false, None), Notch::Points(40.0));
        // Whatever is said of lines where Windows is not the system: nothing asks there.
        assert_eq!(wheel_notch(false, Some(9)), Notch::Points(40.0));
    }

    // The second thing said by the first person to run it: with Windows' lines a notch
    // turned up, a hundred points was still not what the web view moved.
    #[test]
    fn on_windows_a_notch_goes_by_the_systems_lines_a_notch() {
        assert_eq!(wheel_notch(true, Some(6)), Notch::Points(200.0));
        assert_eq!(wheel_notch(true, Some(1)), Notch::Points(100.0 / 3.0));
        assert_eq!(wheel_notch(true, Some(100)), Notch::Points(10_000.0 / 3.0));
        // Nought is the wheel switched off, there as in the web view.
        assert_eq!(wheel_notch(true, Some(0)), Notch::Points(0.0));
        // A setting that could not be read is the one Windows comes with.
        assert_eq!(wheel_notch(true, None), Notch::Points(100.0));
        // "One screen at a time."
        assert_eq!(wheel_notch(true, Some(u32::MAX)), Notch::Page);
    }

    #[test]
    fn a_page_is_what_is_in_view_less_what_is_kept_to_go_on_from() {
        assert_eq!(Notch::Points(100.0).points(631.0), 100.0);
        assert_eq!(Notch::Page.points(631.0), 591.0);
        // Seven eighths of a list too low for forty points to be an eighth of it.
        assert_eq!(Notch::Page.points(200.0), 175.0);
        // And never nothing, of a list with nothing in view.
        assert_eq!(Notch::Page.points(0.0), 1.0);
    }

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
