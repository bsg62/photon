//! The scrollbar of a column painted by hand - the People page, a section of Settings,
//! the `?` sheet, the viewer's info panel - drawn and held as the grid's is: as wide
//! (`BAR_WIDTH`), its thumb as wide and never shorter than `MIN_THUMB`, a press on the
//! thumb holding it where it was taken and a press on the track bringing the thumb's middle
//! under the pointer, the column following the pointer for as long as the button is down.
//! The arithmetic is the grid's own (`grid::scroll::Scroll`).
//!
//! These columns had a mark three points wide that nothing could take hold of: a page of
//! people a library long could be gone through by the wheel alone (the first person to run
//! the People page on Windows found it, beside the grid's bar).

use crate::grid::scroll::{BAR_WIDTH, NOTCH, Notch, Scroll};
use eframe::egui::{self, Color32, Event, Id, Rect, Sense, pos2, vec2};

fn notch_id() -> Id {
    Id::new("wheel-notch")
}

/// What a notch of the wheel moves on this system, kept where every column can read it:
/// the application says it in every frame, beside what it tells egui for the grid.
pub fn keep_notch(ctx: &egui::Context, notch: Notch) {
    ctx.data_mut(|data| data.insert_temp(notch_id(), notch));
}

/// The points a notch moves a column of which `in_view` points are in view: the system's,
/// as the grid's (`keep_notch`); where Windows is set to "one screen at a time", a page
/// of the column's own, as WebView2 pages the element that scrolls - a page of the grid
/// skipped half a Settings section a notch.
pub(crate) fn notch_points(ctx: &egui::Context, in_view: f32) -> f32 {
    let notch = ctx.data(|data| data.get_temp::<Notch>(notch_id()));
    notch
        .unwrap_or(Notch::Points(NOTCH))
        .points(f64::from(in_view)) as f32
}

/// What a notch of the wheel moves a column by: what it moves the grid by, from the
/// system's setting (`notch_points`). A column moved forty points a notch at first,
/// whatever the system said, and on a Windows at its own three lines a notch the People
/// page went at two fifths of the grid's pace. A Windows set to scroll nothing a notch
/// scrolls no column either, as it scrolls no web page: the bar is the way then.
pub(crate) fn turned(ui: &egui::Ui, page: f32) -> f32 {
    let line = notch_points(ui.ctx(), page);
    ui.input(|input| {
        (input.events.iter())
            .map(|event| match event {
                Event::MouseWheel { unit, delta, .. } => match unit {
                    egui::MouseWheelUnit::Line => delta.y * line,
                    egui::MouseWheelUnit::Point => delta.y,
                    egui::MouseWheelUnit::Page => delta.y * page,
                },
                _ => 0.0,
            })
            .sum()
    })
}

/// The room the bar takes at `place`'s right edge, over its whole height.
pub(crate) fn track(place: Rect) -> Rect {
    Rect::from_min_max(
        pos2(place.right() - BAR_WIDTH as f32, place.top()),
        place.max,
    )
}

/// A column's bar, from frame to frame: where on the thumb the pointer took hold, while
/// it is held.
#[derive(Default)]
pub(crate) struct Bar {
    grab: Option<f64>,
}

impl Bar {
    /// A press on `track`, and the drag after it, for a column `content` tall of which
    /// `in_view` is in view, scrolled to `scroll`: the scroll it is moved to. Asked before
    /// the column is filled, so that what the column draws this frame is drawn where the
    /// bar has put it.
    pub(crate) fn hold(
        &mut self,
        ui: &egui::Ui,
        id: Id,
        track: Rect,
        scroll: f32,
        content: f32,
        in_view: f32,
    ) -> f32 {
        let mut at = extent(scroll, content, in_view);
        // Not a stop of the Tab key, as the grid's is not: the wheel and the keyboard's
        // own walk are the keyboard's ways down a column.
        let response = ui.interact(track, id, Sense::CLICK | Sense::DRAG);
        let length = f64::from(track.height());
        let held = (response.is_pointer_button_down_on())
            .then(|| response.interact_pointer_pos())
            .flatten();
        match (held, at.thumb(length)) {
            (Some(pointer), Some((start, thumb))) => {
                let y = f64::from(pointer.y - track.top());
                let on_thumb = (start..=start + thumb).contains(&y);
                let grab =
                    *(self.grab).get_or_insert(if on_thumb { y - start } else { thumb / 2.0 });
                at.set(at.position_for_thumb(length, y - grab));
            }
            _ => self.grab = None,
        }
        at.position() as f32
    }

    /// The thumb, over what the column has drawn: `rest` at rest, `held` while it is held.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        ui: &egui::Ui,
        track: Rect,
        scroll: f32,
        content: f32,
        in_view: f32,
        rest: Color32,
        held: Color32,
    ) {
        let at = extent(scroll, content, in_view);
        let Some((start, length)) = at.thumb(f64::from(track.height())) else {
            return;
        };
        let thumb = Rect::from_min_size(
            pos2(track.left() + 3.0, track.top() + start as f32),
            vec2(track.width() - 6.0, length as f32),
        );
        let tint = if self.grab.is_some() { held } else { rest };
        (ui.painter_at(track)).rect_filled(thumb, thumb.width() / 2.0, tint);
    }

    /// Whether the thumb is held: for a test.
    pub fn held(&self) -> bool {
        self.grab.is_some()
    }
}

fn extent(scroll: f32, content: f32, in_view: f32) -> Scroll {
    let mut at = Scroll::default();
    at.set_extent(f64::from(content), f64::from(in_view));
    at.set(f64::from(scroll));
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Modifiers, PointerButton, RawInput};

    const PLACE: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(300.0, 200.0),
    };

    /// One frame of a bar over a column 1,000 points tall with 200 in view, scrolled to
    /// `scroll`, given `events`: the scroll it was moved to.
    fn frame(ctx: &egui::Context, bar: &mut Bar, scroll: f32, events: Vec<Event>) -> f32 {
        let input = RawInput {
            screen_rect: Some(PLACE),
            events,
            ..RawInput::default()
        };
        let mut moved = scroll;
        let mut full = ctx.run_ui(input, |ui| {
            moved = bar.hold(ui, Id::new("bar"), track(PLACE), scroll, 1000.0, 200.0);
        });
        full.textures_delta.clear();
        moved
    }

    fn press(at: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    // The People page's bar was a mark nothing could take hold of: a press on the track
    // brings the thumb's middle under the pointer, and the column with it.
    #[test]
    fn a_press_on_the_track_takes_the_column_there() {
        let ctx = egui::Context::default();
        let mut bar = Bar::default();
        let foot = pos2(PLACE.right() - 6.0, PLACE.bottom() - 1.0);
        frame(&ctx, &mut bar, 0.0, vec![Event::PointerMoved(foot)]);
        let moved = frame(&ctx, &mut bar, 0.0, vec![press(foot, true)]);
        assert_eq!(moved, 800.0, "the column at its end");
        assert!(bar.held());
    }

    // A thumb taken hold of stays under the pointer where it was taken - near its top
    // here, not its middle - and the column follows it: forty points of the track's 160
    // of travel are a quarter of what there is to scroll.
    #[test]
    fn a_thumb_dragged_moves_the_column_by_as_much_of_it() {
        let ctx = egui::Context::default();
        let mut bar = Bar::default();
        let x = PLACE.right() - 6.0;
        // The thumb is forty points long, at the top: taken hold of ten points down it.
        let on = pos2(x, 10.0);
        frame(&ctx, &mut bar, 0.0, vec![Event::PointerMoved(on)]);
        let held = frame(&ctx, &mut bar, 0.0, vec![press(on, true)]);
        assert_eq!(held, 0.0, "taken hold of, not moved");
        let to = pos2(x, 50.0);
        let moved = frame(&ctx, &mut bar, held, vec![Event::PointerMoved(to)]);
        assert_eq!(moved, 200.0);
        let after = frame(&ctx, &mut bar, moved, vec![press(to, false)]);
        assert_eq!(after, 200.0);
        assert!(!bar.held(), "let go of");
    }

    // A notch is what the application says a notch moves the grid, in every frame.
    #[test]
    fn a_notch_moves_a_column_as_far_as_it_moves_the_grid() {
        let ctx = egui::Context::default();
        keep_notch(&ctx, Notch::Points(100.0));
        let input = RawInput {
            screen_rect: Some(PLACE),
            events: vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, -2.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            }],
            ..RawInput::default()
        };
        let mut moved = 0.0;
        let mut full = ctx.run_ui(input, |ui| moved = turned(ui, 200.0));
        full.textures_delta.clear();
        assert_eq!(moved, -200.0);
    }

    // Where Windows pages a notch, a column pages by its own height, not the grid's: a
    // page of the grid is more than a Settings section has in view.
    #[test]
    fn a_page_a_notch_is_a_page_of_the_column() {
        let ctx = egui::Context::default();
        keep_notch(&ctx, Notch::Page);
        assert_eq!(notch_points(&ctx, 446.0), 406.0);
        assert_eq!(notch_points(&ctx, 200.0), 175.0);
        keep_notch(&ctx, Notch::Points(0.0));
        assert_eq!(notch_points(&ctx, 446.0), 0.0);
    }
}
