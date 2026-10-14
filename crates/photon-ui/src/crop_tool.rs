//! The crop tool's state: the rectangle being drawn, the ratio it is held to, and the
//! drag in progress. `createCropTool`, with its tests' cases; the geometry is `crop.rs`.

use crate::crop::{
    ASPECTS, FULL, Handle, Rect, drag_rect, fit_aspect, fraction_ratio, from_wire, to_wire,
};

/// A drag: what is held, the rectangle as it was when the button went down, and where
/// it went down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    handle: Handle,
    from: Rect,
    at: (f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropTool {
    rect: Rect,
    /// Which of `ASPECTS`.
    aspect: usize,
    /// Width over height of the picture the rectangle is drawn on.
    image_aspect: f64,
    drag: Option<Drag>,
}

impl CropTool {
    /// Opens on the crop the photo has, so that cropping again adjusts the rectangle
    /// and does not crop what is left. Always free: the stored crop has no memory of
    /// the ratio it was drawn with, and holding it to one would reshape it before the
    /// user had touched it. `width` and `height` are the picture's, turned and not
    /// cropped.
    pub fn begin(crop: Option<[u16; 4]>, width: u32, height: u32) -> Self {
        Self {
            rect: from_wire(crop),
            aspect: 0,
            image_aspect: if width > 0 && height > 0 {
                f64::from(width) / f64::from(height)
            } else {
                1.0
            },
            drag: None,
        }
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Which of `ASPECTS` the rectangle is held to.
    pub fn aspect(&self) -> usize {
        self.aspect
    }

    /// What is being dragged, if anything is.
    pub fn dragging(&self) -> Option<Handle> {
        self.drag.map(|drag| drag.handle)
    }

    fn k(&self) -> Option<f64> {
        fraction_ratio(ASPECTS[self.aspect].1, self.image_aspect)
    }

    /// Choosing a ratio reshapes the rectangle to it at once, inside what is drawn.
    pub fn set_aspect(&mut self, index: usize) {
        if index >= ASPECTS.len() {
            return;
        }
        self.aspect = index;
        self.rect = fit_aspect(self.rect, self.k());
    }

    /// Back to the whole picture, which is written as no crop.
    pub fn clear(&mut self) {
        self.aspect = 0;
        self.rect = FULL;
    }

    /// `x`, `y`: where the button went down, in points.
    pub fn start_drag(&mut self, handle: Handle, x: f64, y: f64) {
        self.drag = Some(Drag {
            handle,
            from: self.rect,
            at: (x, y),
        });
    }

    /// `x`, `y` in the points `start_drag` was given; `width`, `height` the size the
    /// picture is drawn at. The rectangle is worked out again from where the drag began
    /// every time, so what the picture's edge swallows of one move is not lost to the
    /// next.
    pub fn drag_to(&mut self, x: f64, y: f64, width: f64, height: f64) {
        let Some(drag) = self.drag else {
            return;
        };
        // (Not `width <= 0.0`: a size that is not a number is no size either.)
        if width.is_nan() || height.is_nan() || width <= 0.0 || height <= 0.0 {
            return;
        }
        let (dx, dy) = ((x - drag.at.0) / width, (y - drag.at.1) / height);
        self.rect = drag_rect(drag.from, drag.handle, dx, dy, self.k());
    }

    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    /// The rectangle for `set_item_edit`; none is the whole picture.
    pub fn wire(&self) -> Option<[u16; 4]> {
        to_wire(self.rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> usize {
        (ASPECTS.iter())
            .position(|(label, _)| *label == "1:1")
            .unwrap()
    }

    fn close(a: f64, b: f64, digits: i32) -> bool {
        (a - b).abs() < 0.5 * 10f64.powi(-digits)
    }

    #[test]
    fn it_opens_on_the_crop_the_photo_has_and_free() {
        let tool = CropTool::begin(Some([32768, 0, 65535, 65535]), 300, 200);
        assert!(close(tool.rect().left, 0.5, 4));
        assert_eq!(tool.aspect(), 0);
        assert_eq!(tool.wire(), Some([32768, 0, 65535, 65535]));
        assert_eq!(tool.dragging(), None);
    }

    #[test]
    fn an_uncropped_photo_opens_on_the_whole_picture_which_is_written_as_no_crop() {
        assert_eq!(CropTool::begin(None, 300, 200).wire(), None);
    }

    // Far past the right edge, then back to 100 points right of the press. Added up move
    // by move, the rectangle would end 900 points short; from the press it ends 100 out.
    #[test]
    fn a_drag_is_from_where_the_press_began_so_the_edge_does_not_eat_the_way_back() {
        let mut tool = CropTool::begin(Some([0, 0, 32768, 65535]), 1000, 1000);
        tool.start_drag(Handle::E, 100.0, 100.0);
        assert_eq!(tool.dragging(), Some(Handle::E));
        tool.drag_to(2000.0, 100.0, 1000.0, 1000.0);
        assert_eq!(tool.rect().right, 1.0);
        tool.drag_to(200.0, 100.0, 1000.0, 1000.0);
        assert!(close(tool.rect().right, 0.6, 4));
        tool.end_drag();
        tool.drag_to(900.0, 100.0, 1000.0, 1000.0);
        assert!(close(tool.rect().right, 0.6, 4), "no drag, nothing moves");
    }

    // A square of a 3:2 picture is two thirds of its width and all of its height.
    #[test]
    fn a_ratio_reshapes_the_rectangle_in_pixels_and_not_in_fractions() {
        let mut tool = CropTool::begin(None, 300, 200);
        tool.set_aspect(square());
        assert!(close(tool.rect().width(), 2.0 / 3.0, 9));
        assert!(close(tool.rect().height(), 1.0, 9));
        tool.start_drag(Handle::Se, 0.0, 0.0);
        tool.drag_to(-60.0, 0.0, 300.0, 200.0);
        let w = tool.rect().width() * 300.0;
        let h = tool.rect().height() * 200.0;
        assert!(close(w, h, 6), "{w} by {h}");
    }

    #[test]
    fn clearing_goes_back_to_the_whole_picture_and_lets_the_ratio_go() {
        let mut tool = CropTool::begin(Some([1000, 1000, 30000, 30000]), 300, 200);
        tool.set_aspect(square());
        tool.clear();
        assert_eq!(tool.wire(), None);
        assert_eq!(tool.aspect(), 0);
    }

    #[test]
    fn a_ratio_that_is_not_there_and_a_drag_over_nothing_change_nothing() {
        let mut tool = CropTool::begin(None, 300, 200);
        tool.set_aspect(99);
        assert_eq!(tool.aspect(), 0);
        tool.start_drag(Handle::E, 0.0, 0.0);
        tool.drag_to(-50.0, 0.0, 0.0, 0.0);
        assert_eq!(tool.wire(), None);
        tool.drag_to(-50.0, 0.0, f64::NAN, 100.0);
        assert_eq!(tool.wire(), None);
    }

    // A picture whose size is not known is taken for a square one: the tool still opens.
    #[test]
    fn a_picture_with_no_size_is_taken_for_a_square() {
        let mut tool = CropTool::begin(None, 0, 200);
        tool.set_aspect(square());
        assert_eq!(tool.rect(), FULL);
    }
}
