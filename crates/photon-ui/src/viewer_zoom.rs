//! The zoom and the pan of the photo in the viewer: the zoom half of `nav.ts`, with its
//! tests' cases, and the two numbers themselves.
//!
//! The photo is fitted into its area and then drawn `zoom` times that size about the
//! middle of the area, moved by the pan: a point `q` of the fitted photo, measured from
//! that middle, is drawn at `pan + zoom * q`. Everything here is in points and measured
//! from the middle of the photo's area - not of the window, which is another place once
//! the info panel stands beside the photo.

/// The photo fitted into its area.
pub const MIN_ZOOM: f32 = 1.0;
/// Four times its fitted size, and no more.
pub const MAX_ZOOM: f32 = 4.0;
/// One press of `+` or `-`: a quarter more, or back by the same factor, so that as many
/// presses out as in end where they began.
pub const ZOOM_KEY_STEP: f32 = 1.25;
/// The most one turn of the wheel zooms by, as the web view's delta: `wheel_zoom_factor`.
pub const WHEEL_ZOOM_CAP: f32 = 20.0;
/// What a double click zooms to when the photo is shown at its own size or larger
/// already: there are no more pixels to see, and "closer" is still what was asked for.
pub const DOUBLE_CLICK_ZOOM: f32 = 2.0;

/// How far the photo is moved from the middle of its area, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pan {
    pub x: f32,
    pub y: f32,
}

/// Between fit and four times. A zoom that is no number is the fit.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_nan() {
        return MIN_ZOOM;
    }
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// What one turn of the wheel with Ctrl held multiplies the zoom by, from the delta a web
/// view gives it: about a hundred for a mouse's notch, away from the user below nought.
///
/// A trackpad's pinch arrives in a web view as a stream of such turns, each a small delta
/// made from the fingers' own scale - `-100 * ln(scale)` - so `exp(-delta / 100)` gives
/// the photo back the pinch as it was made. A mouse's notch read the same way would be
/// nearly three times a click; capped at `WHEEL_ZOOM_CAP` it is a step of a fifth.
/// Exponential, so the same way out and back end where they began. A delta that is no
/// number changes nothing.
pub fn wheel_zoom_factor(delta: f32) -> f32 {
    if !delta.is_finite() {
        return 1.0;
    }
    (-delta.clamp(-WHEEL_ZOOM_CAP, WHEEL_ZOOM_CAP) / 100.0).exp()
}

/// The zoom a double click on a fitted photo goes to: one pixel of the photo on one pixel
/// of the screen, which is what a photo is checked for sharpness at. `fitted` is the
/// photo's width as drawn at fit, in points, and `scale` how many pixels of the screen
/// one of those is. Clamped like every zoom, so a large photo in a small window stops at
/// `MAX_ZOOM`. A photo with no pixels to spare over its fitted size - a small one, or
/// anything not measured yet - gets `DOUBLE_CLICK_ZOOM` instead: a double click that
/// changed nothing, or a few hundredths, would read as broken.
pub fn actual_size_zoom(image_width: f32, fitted: f32, scale: f32) -> f32 {
    if !(image_width > 0.0 && fitted > 0.0 && scale > 0.0) {
        return DOUBLE_CLICK_ZOOM;
    }
    let actual = image_width / (fitted * scale);
    if actual > 1.1 {
        clamp_zoom(actual)
    } else {
        DOUBLE_CLICK_ZOOM
    }
}

/// Zooms from `zoom` to `next` keeping the photo under `point` where it is. The pan that
/// leaves `point` showing the same point of the photo at the new zoom is
/// `point - (point - pan) * next / zoom`. It is clamped like any pan, so zooming out from
/// a corner slides the photo back into its area and does not leave black beside it, and at
/// fit it is the middle whatever the point. A `point` of nought zooms about the middle.
pub fn zoom_at(zoom: f32, pan: Pan, next: f32, point: Pan, width: f32, height: f32) -> (f32, Pan) {
    let to = clamp_zoom(next);
    let by = to / zoom;
    let pan = clamp_pan(
        point.x - (point.x - pan.x) * by,
        point.y - (point.y - pan.y) * by,
        to,
        width,
        height,
    );
    (to, pan)
}

/// Keeps a panned photo covering its area, `width` by `height`. Drawn `zoom` times as
/// large it overflows the area by `size * (zoom - 1)`, half of it on each side, which is
/// how far either edge can travel before it would pull into view. At fit that is nought,
/// so this also holds the photo in the middle with no case of its own.
pub fn clamp_pan(x: f32, y: f32, zoom: f32, width: f32, height: f32) -> Pan {
    let most_x = (width * (zoom - 1.0) / 2.0).max(0.0);
    let most_y = (height * (zoom - 1.0) / 2.0).max(0.0);
    Pan {
        x: x.clamp(-most_x, most_x),
        y: y.clamp(-most_y, most_y),
    }
}

/// The zoom and the pan of the photo on screen. Every call is given the photo's area as
/// it is in the frame that calls, its width and its height: the pan is bounded by it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    zoom: f32,
    pan: Pan,
}

impl Default for Zoom {
    /// The photo fitted, in the middle.
    fn default() -> Self {
        Self {
            zoom: MIN_ZOOM,
            pan: Pan::default(),
        }
    }
}

impl Zoom {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn pan(&self) -> Pan {
        self.pan
    }

    /// Whether the photo is fitted: there is nothing to pan, and nowhere to zoom out to.
    pub fn is_fit(&self) -> bool {
        self.zoom <= MIN_ZOOM
    }

    /// Zooms to `next`, holding the photo under `point` still: the keys (about the
    /// middle, a point of nought), the wheel, a pinch and a double click (about the
    /// pointer). Answers how far the pan moved for it: a drag in progress is measured
    /// from where it began, and has to be moved along with the photo, or the pointer's
    /// next move puts the pan back where the drag had it and undoes the zoom's.
    pub fn to(&mut self, next: f32, point: Pan, (width, height): (f32, f32)) -> Pan {
        let before = self.pan;
        (self.zoom, self.pan) = zoom_at(self.zoom, self.pan, next, point, width, height);
        Pan {
            x: self.pan.x - before.x,
            y: self.pan.y - before.y,
        }
    }

    /// Zooms by `factor`, as `to` does.
    pub fn by(&mut self, factor: f32, point: Pan, area: (f32, f32)) -> Pan {
        self.to(self.zoom * factor, point, area)
    }

    /// The slider's: the zoom is `value`, and the pan stays where it is, pulled back in
    /// where it has to be - zooming out shrinks how far the photo may travel, and a pan
    /// that was within bounds at four times must not be left hanging off the edge.
    pub fn set(&mut self, value: f32, (width, height): (f32, f32)) {
        self.zoom = clamp_zoom(value);
        self.pan = clamp_pan(self.pan.x, self.pan.y, self.zoom, width, height);
    }

    /// The pan is `x`, `y`, as far as the photo may travel.
    pub fn pan_to(&mut self, x: f32, y: f32, (width, height): (f32, f32)) {
        self.pan = clamp_pan(x, y, self.zoom, width, height);
    }

    /// The area is another size: a pan that was within bounds in the larger one is
    /// pulled back in.
    pub fn held_to(&mut self, (width, height): (f32, f32)) {
        self.pan = clamp_pan(self.pan.x, self.pan.y, self.zoom, width, height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: (f32, f32) = (800.0, 600.0);

    fn pan(x: f32, y: f32) -> Pan {
        Pan { x, y }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_zoom_is_kept_between_fit_and_four_times() {
        assert_eq!(clamp_zoom(0.2), 1.0);
        assert_eq!(clamp_zoom(1.0), 1.0);
        assert_eq!(clamp_zoom(2.5), 2.5);
        assert_eq!(clamp_zoom(4.0), 4.0);
        assert_eq!(clamp_zoom(9.0), 4.0);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
    }

    #[test]
    fn a_pan_goes_only_as_far_as_the_zoomed_photo_overflows_its_area() {
        // At twice an 800 by 600 area the photo is 1600 by 1200, so half of the
        // overflow - 400 across and 300 down - is as far as either edge can travel.
        assert_eq!(
            clamp_pan(1000.0, 1000.0, 2.0, 800.0, 600.0),
            pan(400.0, 300.0)
        );
        assert_eq!(
            clamp_pan(-1000.0, -1000.0, 2.0, 800.0, 600.0),
            pan(-400.0, -300.0)
        );
        assert_eq!(clamp_pan(100.0, 50.0, 2.0, 800.0, 600.0), pan(100.0, 50.0));
        // At fit there is nothing to pan: the photo is held in the middle.
        assert_eq!(clamp_pan(250.0, 250.0, 1.0, 800.0, 600.0), pan(0.0, 0.0));
    }

    #[test]
    fn a_zoom_keeps_the_photo_under_the_pointer_where_it_is() {
        // At fit, the pointer 200 right of the middle and 100 below is over the fitted
        // photo's point (200, 100). At twice, that point is drawn at pan + 2 * (200, 100),
        // so the pan that leaves it under the pointer is (-200, -100).
        assert_eq!(
            zoom_at(1.0, pan(0.0, 0.0), 2.0, pan(200.0, 100.0), 800.0, 600.0),
            (2.0, pan(-200.0, -100.0))
        );
        // From a photo zoomed and panned already: the point under the pointer is
        // (100 - 40) / 2 = 30, and at three times it must still be drawn at 100.
        assert_eq!(
            zoom_at(2.0, pan(40.0, 0.0), 3.0, pan(100.0, 0.0), 800.0, 600.0),
            (3.0, pan(10.0, 0.0))
        );
    }

    #[test]
    fn a_zoom_with_no_point_is_about_the_middle() {
        // The pan grows with the zoom, so what is in the middle stays there.
        assert_eq!(
            zoom_at(2.0, pan(100.0, -50.0), 4.0, pan(0.0, 0.0), 800.0, 600.0),
            (4.0, pan(200.0, -100.0))
        );
    }

    #[test]
    fn a_zoom_never_leaves_the_photo_short_of_its_area() {
        // Zooming out from a corner: about the middle the pan would be (600, 450), past
        // what a photo at twice can travel. It slides back in.
        assert_eq!(
            zoom_at(4.0, pan(1200.0, 900.0), 2.0, pan(0.0, 0.0), 800.0, 600.0).1,
            pan(400.0, 300.0)
        );
        // And zooming out under a pointer at the far side, which asks for more still.
        assert_eq!(
            zoom_at(4.0, pan(1200.0, 0.0), 2.0, pan(-400.0, 0.0), 800.0, 600.0).1,
            pan(400.0, 0.0)
        );
    }

    #[test]
    fn the_bottom_of_the_range_is_the_fit_in_the_middle_whatever_the_point() {
        for next in [1.0, 0.2] {
            assert_eq!(
                zoom_at(
                    3.0,
                    pan(250.0, -90.0),
                    next,
                    pan(300.0, 200.0),
                    800.0,
                    600.0
                ),
                (1.0, pan(0.0, 0.0))
            );
        }
    }

    #[test]
    fn the_top_of_the_range_is_panned_for_the_zoom_it_stopped_at() {
        // Asked for eight times it gets four - and the pan is four's, not eight's pulled
        // in: the point under the pointer has to stay there at the zoom reached.
        assert_eq!(
            zoom_at(2.0, pan(0.0, 0.0), 8.0, pan(100.0, 0.0), 800.0, 600.0),
            (MAX_ZOOM, pan(-100.0, 0.0))
        );
    }

    #[test]
    fn a_double_click_goes_to_one_pixel_of_the_photo_on_one_of_the_screen() {
        // 4,000 wide, drawn 1,600 wide at fit: two and a half times shows it pixel for
        // pixel.
        assert_eq!(actual_size_zoom(4000.0, 1600.0, 1.0), 2.5);
        // A screen that draws two pixels a point needs half the zoom.
        assert_eq!(actual_size_zoom(6400.0, 1600.0, 2.0), 2.0);
        // It stops at the most the viewer zooms to.
        assert_eq!(actual_size_zoom(6000.0, 1000.0, 1.0), MAX_ZOOM);
    }

    #[test]
    fn a_double_click_still_zooms_a_photo_with_no_more_pixels_to_show() {
        // Smaller than its fitted size, exactly it, or a hair over: a double click that
        // moved nothing, or a few hundredths, would read as broken.
        for width in [800.0, 1600.0, 1700.0] {
            assert_eq!(actual_size_zoom(width, 1600.0, 1.0), DOUBLE_CLICK_ZOOM);
        }
        // And before anything has been measured.
        assert_eq!(actual_size_zoom(0.0, 1600.0, 1.0), DOUBLE_CLICK_ZOOM);
        assert_eq!(actual_size_zoom(4000.0, 0.0, 1.0), DOUBLE_CLICK_ZOOM);
        assert_eq!(actual_size_zoom(4000.0, 1600.0, 0.0), DOUBLE_CLICK_ZOOM);
    }

    #[test]
    fn the_wheel_gives_a_pinch_back_as_it_was_made() {
        // A pinch to a tenth more arrives as a delta of -100 * ln(1.1).
        assert!(close(wheel_zoom_factor(-100.0 * 1.1_f32.ln()), 1.1));
        assert!(close(wheel_zoom_factor(100.0 * 1.1_f32.ln()), 1.0 / 1.1));
        // The same way out and back ends where it began.
        assert!(close(wheel_zoom_factor(-7.0) * wheel_zoom_factor(7.0), 1.0));
    }

    #[test]
    fn a_notch_of_a_mouse_is_a_step_of_a_fifth_and_not_nearly_three_times() {
        assert!(close(wheel_zoom_factor(-100.0), 0.2_f32.exp()));
        assert!(close(wheel_zoom_factor(100.0), (-0.2_f32).exp()));
        // However hard the wheel is flicked.
        assert_eq!(wheel_zoom_factor(-1200.0), wheel_zoom_factor(-100.0));
        // A delta of nothing, or one that is no number, changes nothing.
        assert_eq!(wheel_zoom_factor(0.0), 1.0);
        assert_eq!(wheel_zoom_factor(f32::NAN), 1.0);
        assert_eq!(wheel_zoom_factor(f32::INFINITY), 1.0);
    }

    #[test]
    fn it_begins_fitted_and_a_zoom_answers_how_far_the_pan_moved() {
        let mut zoom = Zoom::default();
        assert!(zoom.is_fit());
        assert_eq!((zoom.zoom(), zoom.pan()), (1.0, pan(0.0, 0.0)));
        // About the pointer: the pan moves, and that is said.
        assert_eq!(zoom.to(2.0, pan(200.0, 100.0), AREA), pan(-200.0, -100.0));
        assert_eq!((zoom.zoom(), zoom.pan()), (2.0, pan(-200.0, -100.0)));
        assert!(!zoom.is_fit());
        // By a factor, from where it is: about the middle the pan grows with it.
        assert_eq!(zoom.by(1.5, pan(0.0, 0.0), AREA), pan(-100.0, -50.0));
        assert_eq!((zoom.zoom(), zoom.pan()), (3.0, pan(-300.0, -150.0)));
        // Out again, to the fit.
        zoom.by(0.1, pan(0.0, 0.0), AREA);
        assert_eq!(zoom, Zoom::default());
    }

    #[test]
    fn the_slider_sets_the_zoom_and_leaves_the_pan_where_it_may_stay() {
        let mut zoom = Zoom::default();
        zoom.to(4.0, pan(0.0, 0.0), AREA);
        zoom.pan_to(1000.0, 100.0, AREA);
        // As far as a photo at four times may travel: (800 * 3) / 2 across.
        assert_eq!(zoom.pan(), pan(1000.0, 100.0));
        zoom.pan_to(2000.0, -2000.0, AREA);
        assert_eq!(zoom.pan(), pan(1200.0, -900.0));
        // Turned down to twice: the pan is not scaled, only pulled in.
        zoom.pan_to(300.0, 100.0, AREA);
        zoom.set(2.0, AREA);
        assert_eq!((zoom.zoom(), zoom.pan()), (2.0, pan(300.0, 100.0)));
        zoom.pan_to(400.0, 300.0, AREA);
        zoom.set(1.5, AREA);
        assert_eq!((zoom.zoom(), zoom.pan()), (1.5, pan(200.0, 150.0)));
        // Past either end of the range it stops there.
        zoom.set(9.0, AREA);
        assert_eq!(zoom.zoom(), MAX_ZOOM);
        zoom.set(0.0, AREA);
        assert_eq!(zoom, Zoom::default());
    }

    #[test]
    fn a_smaller_area_pulls_the_pan_back_in() {
        let mut zoom = Zoom::default();
        zoom.to(2.0, pan(0.0, 0.0), AREA);
        zoom.pan_to(400.0, 300.0, AREA);
        zoom.held_to((600.0, 600.0));
        assert_eq!(zoom.pan(), pan(300.0, 300.0));
        // A larger one moves nothing.
        zoom.held_to((2000.0, 2000.0));
        assert_eq!(zoom.pan(), pan(300.0, 300.0));
    }
}
