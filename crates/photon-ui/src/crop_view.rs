//! The crop tool, drawn over the picture: what is outside the rectangle dimmed, the
//! rectangle's line, its eight handles - and the drags that move them, read from the
//! button's own events.
//!
//! The rectangle is in fractions of the picture the tool is drawn on (`crop.rs`), and
//! that picture's place on screen is handed in: a drag is so many points, and a point
//! is so much of the picture as it is drawn.
//!
//! **A drag is read from the button going down and coming up**, as the grid's band and
//! the viewer's pan are, and has their endings: the button up, the window left, a touch
//! the system cancels. egui's own drag ends at the release of any button and never
//! hears of a button let go of over another window.

use crate::{
    crop::{HANDLES, Handle, Rect as Crop},
    crop_tool::CropTool,
    theme::{
        apply::color,
        tokens::{PHOTO_LINE, SCRIM, SHADOW_INK},
    },
};
use eframe::egui::{
    self, CursorIcon, Event, PointerButton, Pos2, Rect, Stroke, StrokeKind, TouchPhase, pos2, vec2,
};

/// A handle as it is drawn, and as it is caught: larger than it looks, so that it can
/// be.
const HANDLE: f32 = 8.0;
const HANDLE_REACH: f32 = 22.0;

/// What the view is told besides the tool.
pub struct CropData<'a> {
    /// Where the picture the rectangle is drawn on stands: turned, not cropped, fitted.
    pub picture: Rect,
    /// What lies over the picture: the tool's own bar. A press there is not the
    /// rectangle's, though the rectangle may reach under it.
    pub over: &'a [Rect],
    /// The tool takes no drag: its write is on its way.
    pub busy: bool,
}

/// The rectangle on screen.
fn on_screen(rect: Crop, picture: Rect) -> Rect {
    let at = |x: f64, y: f64| {
        pos2(
            picture.left() + x as f32 * picture.width(),
            picture.top() + y as f32 * picture.height(),
        )
    };
    Rect::from_min_max(at(rect.left, rect.top), at(rect.right, rect.bottom))
}

/// Where a handle stands on the rectangle as drawn.
fn handle_at(handle: Handle, rect: Rect) -> Pos2 {
    let (x, y) = handle.place();
    pos2(
        rect.left() + x as f32 * rect.width(),
        rect.top() + y as f32 * rect.height(),
    )
}

/// What a press at `at` takes hold of: a handle, within its reach - a corner before an
/// edge where the two are near enough to overlap, as the later of two elements is the
/// one a pointer meets - else the rectangle itself, else nothing.
fn grip(at: Pos2, rect: Rect) -> Option<Handle> {
    let caught = |handle: &&Handle| {
        Rect::from_center_size(handle_at(**handle, rect), vec2(HANDLE_REACH, HANDLE_REACH))
            .contains(at)
    };
    (HANDLES.iter().rev().find(caught).copied())
        .or_else(|| rect.contains(at).then_some(Handle::Move))
}

/// The pointer over a grip, or holding one.
fn cursor(handle: Handle) -> CursorIcon {
    match handle {
        Handle::N | Handle::S => CursorIcon::ResizeVertical,
        Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
        Handle::Nw | Handle::Se => CursorIcon::ResizeNwSe,
        Handle::Ne | Handle::Sw => CursorIcon::ResizeNeSw,
        Handle::Move => CursorIcon::Move,
    }
}

/// Draws the tool over the picture and takes the frame's drags. The rectangle drawn is
/// the one the frame's drags have left.
pub fn show(ui: &egui::Ui, tool: &mut CropTool, data: &CropData<'_>) {
    let picture = data.picture;
    let (events, pointer) = ui.input(|input| (input.events.clone(), input.pointer.latest_pos()));
    // Whether a press at `at` is the tool's: under nothing of the tool's own that lies
    // over the picture, and under nothing else that lies over the viewer - asked of the
    // layer egui finds on top there, the one judge (`grid/view.rs`).
    // (`layer_id_at` locks egui's memory: not inside an `input` closure.)
    let free = |at: Pos2| {
        !data.over.iter().any(|rect| rect.contains(at))
            && ui.ctx().layer_id_at(at) == Some(ui.layer_id())
    };
    for event in &events {
        match event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                ..
            } => {
                // A press with a drag still held: its release was never heard of.
                tool.end_drag();
                if data.busy || !free(*pos) {
                    continue;
                }
                if let Some(handle) = grip(*pos, on_screen(tool.rect(), picture)) {
                    tool.start_drag(handle, f64::from(pos.x), f64::from(pos.y));
                }
            }
            Event::PointerMoved(to) => tool.drag_to(
                f64::from(to.x),
                f64::from(to.y),
                f64::from(picture.width()),
                f64::from(picture.height()),
            ),
            // The button up; or the gesture taken away with no release - the window
            // left, a touch the system cancels. The rectangle stays where the drag had
            // brought it.
            Event::PointerButton {
                button: PointerButton::Primary,
                pressed: false,
                ..
            }
            | Event::WindowFocused(false)
            | Event::Touch {
                phase: TouchPhase::Cancel,
                ..
            } => tool.end_drag(),
            _ => {}
        }
    }
    // A tool whose write is on its way holds nothing.
    if data.busy {
        tool.end_drag();
    }

    let rect = on_screen(tool.rect(), picture);
    let painter = ui.painter().with_clip_rect(picture);
    // What is outside the rectangle, dimmed: the picture's four margins round it.
    let scrim = color(SCRIM);
    for margin in [
        Rect::from_min_max(picture.min, pos2(picture.right(), rect.top())),
        Rect::from_min_max(pos2(picture.left(), rect.bottom()), picture.max),
        Rect::from_min_max(
            pos2(picture.left(), rect.top()),
            pos2(rect.left(), rect.bottom()),
        ),
        Rect::from_min_max(
            pos2(rect.right(), rect.top()),
            pos2(picture.right(), rect.bottom()),
        ),
    ] {
        if margin.is_positive() {
            painter.rect_filled(margin, 0.0, scrim);
        }
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, color(PHOTO_LINE)),
        StrokeKind::Inside,
    );
    // The handles stand on the line, half of each outside the picture where the
    // rectangle is the whole of it: drawn through the viewer's own painter, not the
    // one cut off at the picture.
    let painter = ui.painter();
    for handle in HANDLES {
        let square = Rect::from_center_size(handle_at(handle, rect), vec2(HANDLE, HANDLE));
        painter.rect_stroke(
            square,
            1.0,
            Stroke::new(1.0, color(SHADOW_INK)),
            StrokeKind::Outside,
        );
        painter.rect_filled(square, 1.0, color(PHOTO_LINE));
    }

    // The pointer says what it would take hold of, and what it holds.
    let held = tool.dragging();
    let over = pointer
        .filter(|at| !data.busy && free(*at))
        .and_then(|at| grip(at, rect));
    if let Some(handle) = held.or(over) {
        ui.ctx().set_cursor_icon(cursor(handle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, Modifiers, RawInput};

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    /// The picture: 800 points by 400, from (100, 100), in a window of 1000 by 700.
    const PICTURE: Rect = Rect {
        min: pos2(100.0, 100.0),
        max: pos2(900.0, 500.0),
    };

    struct Fixture {
        ctx: egui::Context,
        tool: CropTool,
        over: Vec<Rect>,
        busy: bool,
        time: f64,
        /// Filled rectangles and lined ones of the last frame, each with its colour.
        fills: Vec<(Rect, Color32)>,
        lines: Vec<(Rect, Color32)>,
        cursor: CursorIcon,
        repaint: std::time::Duration,
    }

    impl Fixture {
        /// The tool on a picture twice as wide as tall, cropped to its middle half.
        fn new() -> Self {
            let mut f = Self {
                ctx: egui::Context::default(),
                tool: CropTool::begin(Some([16384, 16384, 49151, 49151]), 800, 400),
                over: Vec::new(),
                busy: false,
                time: 0.0,
                fills: Vec::new(),
                lines: Vec::new(),
                cursor: CursorIcon::Default,
                repaint: std::time::Duration::MAX,
            };
            f.frame(Vec::new());
            f
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let (tool, over, busy) = (&mut self.tool, &self.over, self.busy);
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let data = CropData {
                            picture: PICTURE,
                            over,
                            busy,
                        };
                        show(ui, tool, &data);
                    });
            });
            full.textures_delta.clear();
            self.cursor = full.platform_output.cursor_icon;
            self.fills.clear();
            self.lines.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Rect(rect) = shape {
                        if rect.fill != Color32::TRANSPARENT {
                            self.fills.push((rect.rect, rect.fill));
                        }
                        if rect.stroke.width > 0.0 {
                            self.lines.push((rect.rect, rect.stroke.color));
                        }
                    }
                });
            }
            self.repaint = full
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay);
        }

        fn button(&mut self, at: Pos2, pressed: bool) {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn point(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
        }

        /// A press at `from`, the pointer taken to `to`, the button still down.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.point(from);
            self.button(from, true);
            self.point(to);
        }

        /// The rectangle as drawn: the one with the photo's line round it.
        fn line(&self) -> Rect {
            (self.lines.iter())
                .find(|(_, tint)| *tint == color(PHOTO_LINE))
                .map(|(rect, _)| *rect)
                .expect("the rectangle's line")
        }
    }

    fn near(a: Rect, b: Rect) -> bool {
        let close = |a: f32, b: f32| (a - b).abs() < 0.05;
        close(a.left(), b.left())
            && close(a.top(), b.top())
            && close(a.right(), b.right())
            && close(a.bottom(), b.bottom())
    }

    /// The middle half of the picture, on screen.
    const MIDDLE: Rect = Rect {
        min: pos2(300.0, 200.0),
        max: pos2(700.0, 400.0),
    };

    // The rectangle stands where its fractions are on the picture as it is drawn, with
    // the photo's line round it; what is outside it is dimmed, up to the picture's edge
    // and no further; and its eight handles stand on its corners and the middles of its
    // sides.
    #[test]
    fn the_rectangle_its_dimming_and_its_handles_are_drawn_on_the_picture() {
        let f = Fixture::new();
        assert!(near(f.line(), MIDDLE), "{:?}", f.line());
        let dim: Vec<Rect> = (f.fills.iter())
            .filter(|(_, fill)| *fill == color(SCRIM))
            .map(|(rect, _)| *rect)
            .collect();
        assert_eq!(dim.len(), 4, "{dim:?}");
        for margin in &dim {
            assert!(PICTURE.expand(0.05).contains_rect(*margin), "{margin:?}");
            assert!(
                !margin.shrink(0.5).intersects(MIDDLE.shrink(0.5)),
                "{margin:?} lies over the rectangle"
            );
        }
        // Together they are all of the picture but the rectangle.
        let area: f32 = dim.iter().map(|margin| margin.area()).sum();
        let left = PICTURE.area() - f.line().area();
        assert!((area - left).abs() < 1.0, "{area} of {left}");
        let handles: Vec<Pos2> = (f.fills.iter())
            .filter(|(rect, fill)| *fill == color(PHOTO_LINE) && rect.width() == HANDLE)
            .map(|(rect, _)| rect.center())
            .collect();
        assert_eq!(handles.len(), 8);
        for expected in [
            pos2(500.0, 200.0),
            pos2(500.0, 400.0),
            pos2(700.0, 300.0),
            pos2(300.0, 300.0),
            pos2(700.0, 200.0),
            pos2(300.0, 200.0),
            pos2(700.0, 400.0),
            pos2(300.0, 400.0),
        ] {
            assert!(
                handles.iter().any(|at| at.distance(expected) < 0.05),
                "a handle at {expected:?}: {handles:?}"
            );
        }
    }

    // The whole picture has nothing dimmed, and its handles on the picture's own edge.
    #[test]
    fn the_whole_picture_has_no_dimming() {
        let mut f = Fixture::new();
        f.tool.clear();
        f.frame(Vec::new());
        assert!(near(f.line(), PICTURE));
        assert!(f.fills.iter().all(|(_, fill)| *fill != color(SCRIM)));
    }

    // A handle dragged moves its side by as much of the picture as the pointer moved on
    // it: eighty points right on a picture eight hundred wide is a tenth.
    #[test]
    fn a_handle_dragged_moves_its_sides_with_the_pointer() {
        let mut f = Fixture::new();
        f.drag(pos2(700.0, 300.0), pos2(780.0, 340.0));
        assert_eq!(f.tool.dragging(), Some(Handle::E));
        assert!(near(
            f.line(),
            Rect::from_min_max(MIDDLE.min, pos2(780.0, 400.0))
        ));
        f.button(pos2(780.0, 340.0), false);
        assert_eq!(f.tool.dragging(), None);
        // A corner, within its reach and not on its dot: both its sides.
        f.drag(pos2(292.0, 208.0), pos2(252.0, 168.0));
        assert_eq!(f.tool.dragging(), Some(Handle::Nw));
        assert!(near(
            f.line(),
            Rect::from_min_max(pos2(260.0, 160.0), pos2(780.0, 400.0))
        ));
    }

    // A press inside the rectangle takes the whole of it along, and it stops at the
    // picture's edge.
    #[test]
    fn the_rectangle_is_moved_from_inside_it_and_stays_on_the_picture() {
        let mut f = Fixture::new();
        f.drag(pos2(500.0, 300.0), pos2(550.0, 280.0));
        assert_eq!(f.tool.dragging(), Some(Handle::Move));
        assert!(near(f.line(), MIDDLE.translate(vec2(50.0, -20.0))));
        f.point(pos2(2000.0, 280.0));
        assert!(near(
            f.line(),
            Rect::from_min_max(pos2(500.0, 180.0), pos2(900.0, 380.0))
        ));
    }

    // Where a corner's reach and an edge's meet, the corner has the press: on a small
    // rectangle every edge's handle is within reach of a corner's.
    #[test]
    fn a_corner_has_a_press_that_an_edge_could_have_too() {
        let small = Rect::from_min_size(pos2(400.0, 300.0), vec2(16.0, 16.0));
        // Above the top side, twelve points from its right end: within reach of the
        // side's own handle, four points away, and of the corner's.
        assert_eq!(grip(pos2(412.0, 289.5), small), Some(Handle::Ne));
        // Within reach of both upper corners, the one that is made later has it.
        assert_eq!(grip(pos2(408.0, 289.5), small), Some(Handle::Nw));
        assert_eq!(grip(pos2(389.5, 289.5), small), Some(Handle::Nw));
        // In a rectangle of ordinary size each has its own.
        assert_eq!(grip(pos2(500.0, 195.0), MIDDLE), Some(Handle::N));
        assert_eq!(grip(pos2(500.0, 300.0), MIDDLE), Some(Handle::Move));
        assert_eq!(grip(pos2(200.0, 300.0), MIDDLE), None);
    }

    // A press outside the rectangle and its handles takes hold of nothing, and neither
    // does one on what lies over the picture, though the rectangle reaches under it.
    #[test]
    fn a_press_beside_the_rectangle_or_on_what_lies_over_it_holds_nothing() {
        let mut f = Fixture::new();
        f.drag(pos2(150.0, 150.0), pos2(400.0, 300.0));
        assert_eq!(f.tool.dragging(), None);
        assert!(near(f.line(), MIDDLE));
        f.button(pos2(400.0, 300.0), false);
        // The tool's bar over the rectangle's foot.
        f.over = vec![Rect::from_min_max(pos2(400.0, 380.0), pos2(600.0, 420.0))];
        f.drag(pos2(500.0, 400.0), pos2(500.0, 450.0));
        assert_eq!(f.tool.dragging(), None);
        assert!(near(f.line(), MIDDLE));
        f.button(pos2(500.0, 450.0), false);
        // Beside the bar, the same side's handle is the rectangle's.
        f.drag(pos2(700.0, 400.0), pos2(700.0, 450.0));
        assert_eq!(f.tool.dragging(), Some(Handle::Se));
    }

    // A drag has three endings: the button up, the window left, a touch the system
    // cancels. After each the pointer moves nothing.
    #[test]
    fn a_drag_ends_when_the_button_comes_up_the_window_is_left_or_a_touch_is_cancelled() {
        let cancel = Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(0),
            phase: TouchPhase::Cancel,
            pos: pos2(0.0, 0.0),
            force: None,
        };
        let up = Event::PointerButton {
            pos: pos2(740.0, 300.0),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        };
        for ending in [up, Event::WindowFocused(false), cancel] {
            let mut f = Fixture::new();
            f.drag(pos2(700.0, 300.0), pos2(740.0, 300.0));
            assert_eq!(f.tool.dragging(), Some(Handle::E));
            f.frame(vec![ending.clone()]);
            assert_eq!(f.tool.dragging(), None, "{ending:?}");
            f.point(pos2(860.0, 300.0));
            assert!(
                near(f.line(), Rect::from_min_max(MIDDLE.min, pos2(740.0, 400.0))),
                "{ending:?}"
            );
        }
    }

    // A press that finds a drag still held - its release was never heard of - begins
    // anew from where it goes down, and not from where the lost drag began.
    #[test]
    fn a_press_under_a_drag_whose_release_was_lost_begins_anew() {
        let mut f = Fixture::new();
        f.drag(pos2(700.0, 300.0), pos2(740.0, 300.0));
        f.button(pos2(150.0, 150.0), true);
        assert_eq!(
            f.tool.dragging(),
            None,
            "beside the rectangle: nothing held"
        );
        f.point(pos2(860.0, 300.0));
        assert!(near(
            f.line(),
            Rect::from_min_max(MIDDLE.min, pos2(740.0, 400.0))
        ));
    }

    // While its write is on its way the tool takes no drag, and lets go of one it held.
    #[test]
    fn a_busy_tool_takes_no_drag_and_holds_none() {
        let mut f = Fixture::new();
        f.busy = true;
        f.drag(pos2(700.0, 300.0), pos2(780.0, 300.0));
        assert_eq!(f.tool.dragging(), None);
        assert!(near(f.line(), MIDDLE));
        f.button(pos2(780.0, 300.0), false);
        f.busy = false;
        f.drag(pos2(700.0, 300.0), pos2(740.0, 300.0));
        assert_eq!(f.tool.dragging(), Some(Handle::E));
        f.busy = true;
        f.frame(Vec::new());
        assert_eq!(f.tool.dragging(), None);
    }

    // The pointer says what it would take hold of: a handle's own arrow over it, the
    // four-way one inside the rectangle, nothing beside it - and what it holds, wherever
    // it has gone with it.
    #[test]
    fn the_pointer_says_what_it_would_hold_and_what_it_holds() {
        let mut f = Fixture::new();
        for (at, cursor) in [
            (pos2(500.0, 200.0), CursorIcon::ResizeVertical),
            (pos2(300.0, 300.0), CursorIcon::ResizeHorizontal),
            (pos2(300.0, 200.0), CursorIcon::ResizeNwSe),
            (pos2(700.0, 200.0), CursorIcon::ResizeNeSw),
            (pos2(500.0, 300.0), CursorIcon::Move),
            (pos2(150.0, 150.0), CursorIcon::Default),
        ] {
            f.point(at);
            f.frame(Vec::new());
            assert_eq!(f.cursor, cursor, "at {at:?}");
        }
        f.drag(pos2(700.0, 300.0), pos2(150.0, 150.0));
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::ResizeHorizontal, "held, beside it");
        f.button(pos2(150.0, 150.0), false);
        f.busy = true;
        f.point(pos2(500.0, 300.0));
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Default, "a busy tool offers nothing");
    }

    // A tool that stands asks for no frame.
    #[test]
    fn a_tool_that_stands_asks_for_no_frame() {
        let mut f = Fixture::new();
        f.point(pos2(500.0, 300.0));
        for _ in 0..5 {
            f.frame(Vec::new());
        }
        assert_eq!(f.repaint, std::time::Duration::MAX);
    }
}
