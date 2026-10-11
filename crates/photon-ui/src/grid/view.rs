//! The grid: lays the published index out, moves through it, and draws what is in view.
//!
//! One function does all three in that order, every frame. That order is why several rules
//! of the Svelte grid have no successor here (`placeIn`, `restoredTo`, `viewTop`): they
//! bridged the render in which the tiles changed and the effect that moved the viewport
//! after it, and here the place is found again before anything is drawn.

use super::{
    header,
    layout::{
        GAP, HEADER, NavKey, PinnedHeader, Row, RowKind, Span, build_rows, columns_for,
        defers_thumbs, edge_scroll_speed, has_header, header_rows, held_place, hold_at,
        items_in_rect, move_from, page_move, pin_at, pin_top, pinned_header, row_of_item,
        row_width, scroll_into_grid, scroll_to_start, shows_timeline, tile_at, tile_for, tile_row,
        tile_width, top_folder_id, total_height, visible_range, wanted_range,
    },
    motion::{Direction, Motion, ScrollSpeed},
    scroll::{BAR_WIDTH, Scroll},
    tile,
    timeline::{
        TIMELINE_WIDTH, YearMark, labelled_marks, scroll_top_for, strip_y, year_at, year_marks,
        year_runs,
    },
};
use crate::{
    selection::Selection,
    theme::{
        apply::{color, menu_shadow, palette},
        fonts,
        tokens::{R, S, T},
    },
    thumbs::{loader::Want, shown::Thumbs},
};
use eframe::egui::{
    self, Align2, CursorIcon, Event, Key, Modifiers, PointerButton, Rect, Sense, Stroke,
    StrokeKind, TouchPhase, UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};
use jiff::tz::TimeZone;
use photon_core::{
    grid::GridIndex,
    library::{Folder, GridTile},
};
use std::{collections::HashMap, time::Duration};

/// The least a page key scrolls a grid with nothing selected in it.
const LEAST_PAGE: f64 = 40.0;
/// How near the top or the bottom of the grid a rubber band scrolls it, and how fast at
/// the most, in points a second. The Svelte grid's numbers.
const BAND_EDGE: f64 = 48.0;
const BAND_SCROLL_MAX: f64 = 1400.0;
/// The longest frame a band's scroll is worked out for, in seconds: a frame that took
/// half a second - the window dragged, the machine asleep - is not half a second of
/// scroll.
const BAND_FRAME_MAX: f64 = 0.05;

/// A press of the primary button on the photos, from the frame the button goes down in
/// to the frame it comes up in: a click if the pointer stays where it went down, a
/// rubber band once it has gone further than a click may.
///
/// Kept from the button's own events and not from what egui calls dragged, which says
/// less than it seems to: it ends at the release of *any* button and at Escape with
/// anything held, and it goes stale for good after a release that is never heard of.
#[derive(Clone, Copy, Debug)]
struct Press {
    /// Where the button went down, in the window's coordinates.
    at: egui::Pos2,
    /// And in the layout's, as the grid stood in that frame: the corner a band keeps
    /// its grip by while the grid scrolls under it - also before it is a band, when the
    /// wheel turns under a button that is down and still.
    origin: (f64, f64),
    /// What was held as it went down: a band begun with Ctrl, Cmd or Shift adds.
    modifiers: Modifiers,
    /// Whether it has been a band. One that has is no click when the button comes up,
    /// and one whose band was abandoned draws no second: the rest of it is nobody's.
    banded: bool,
}

/// A rubber band being drawn, from its press's origin to the pointer.
#[derive(Clone, Copy, Debug)]
struct Band {
    /// Where the pointer was last seen, in the window's coordinates: the far corner is
    /// worked out from it again in every frame, the grid having moved under a pointer
    /// that holds still. Kept because a pointer that leaves the window is no longer seen
    /// at all, and is then where it left - at the edge it is scrolling towards.
    pointer: egui::Pos2,
    /// When the last frame of its scroll was drawn, in seconds.
    scrolled_at: Option<f64>,
}

/// A band's rectangle in the layout's coordinates, for a grid drawn in `area` at
/// `position`.
fn span(press: &Press, band: &Band, area: Rect, position: f64) -> Span {
    Span {
        x0: press.origin.0,
        y0: press.origin.1,
        x1: f64::from(band.pointer.x - area.left()),
        y1: f64::from(band.pointer.y - area.top()) + position,
    }
}

/// Strip points between two printed years: a line of the label's type plus air.
const MIN_LABEL_GAP: f64 = 16.0;
/// The line a year is printed in, its top where the year begins.
const LABEL_LINE: f32 = 14.0;

/// Sets how far a notch of the mouse wheel scrolls, for everything in `ctx` that scrolls:
/// the grid and the sidebar's list alike. Once, at startup (`scroll::WHEEL_NOTCH`).
pub fn set_wheel_notch(ctx: &egui::Context, points: f64) {
    ctx.options_mut(|options| options.input_options.line_scroll_speed = points as f32);
}

/// What the grid draws this frame.
pub struct GridData<'a> {
    /// Moves when a publish changes the sections (`Engine::published`): what the rows are
    /// rebuilt on, where a new version alone - a star, a new thumbnail - is not.
    pub layout_gen: u64,
    pub index: &'a GridIndex,
    pub folders: &'a HashMap<i64, Folder>,
    pub size: GridTile,
    /// The viewer's zone, for the month a folder's header names.
    pub zone: &'a TimeZone,
    /// Whether the grid shows All photos: there Ctrl+A is the section under the lead,
    /// and everywhere else the whole view (`Selection::select_all`).
    pub library: bool,
    /// Whether the keys may be the grid's at all: not while something lies open over the
    /// photos that has no keyboard of its own to say so with (the search box's help).
    pub keys: bool,
}

/// A key the grid answers, read from the frame's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridKey {
    Move(NavKey),
    /// Page Down, or Page Up.
    Page {
        down: bool,
    },
    SelectAll,
    Clear,
}

/// The keys of this frame that are the grid's, in the order they were pressed, each with
/// whether Shift was held.
///
/// A key held down repeats, and each repeat moves again - but not Escape's: Escape in an
/// empty search box hands the keys to the grid, and a key still held from there must not
/// take the selection with it. A chord with Ctrl, Cmd or Alt is the system's or another
/// part of the window's, but for Ctrl/Cmd+A.
fn grid_keys(input: &egui::InputState) -> Vec<(GridKey, bool)> {
    let mut keys = Vec::new();
    for event in &input.events {
        let Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        let chord = modifiers.command || modifiers.ctrl || modifiers.alt;
        let grid_key = match key {
            Key::A if modifiers.command && !modifiers.alt => GridKey::SelectAll,
            _ if chord => continue,
            Key::ArrowLeft => GridKey::Move(NavKey::Left),
            Key::ArrowRight => GridKey::Move(NavKey::Right),
            Key::ArrowUp => GridKey::Move(NavKey::Up),
            Key::ArrowDown => GridKey::Move(NavKey::Down),
            Key::Home => GridKey::Move(NavKey::Home),
            Key::End => GridKey::Move(NavKey::End),
            Key::PageDown => GridKey::Page { down: true },
            Key::PageUp => GridKey::Page { down: false },
            Key::Escape if !repeat => GridKey::Clear,
            _ => continue,
        };
        keys.push((grid_key, modifiers.shift));
    }
    keys
}

/// What a frame of the grid came to.
#[derive(Clone, Debug, PartialEq)]
pub struct GridOutput {
    /// The photos in view, in grid order: what the engine's thumbnail queue is told to
    /// put first.
    pub on_screen: Vec<i64>,
    pub pinned: Option<PinnedHeader>,
    /// Whether the rows were built again this frame.
    pub rebuilt: bool,
    pub position: f64,
    /// Whether every tile in view has its picture, or the mark that it will never have one.
    pub settled: bool,
    /// How many tiles in view show that mark.
    pub marked: usize,
    /// Whether a picture the grid wants, in view or just outside it, is still on its way:
    /// being read, or read and not yet uploaded.
    pub loading: bool,
    /// The folder whose photos are at the top of the grid while it runs folder by folder,
    /// and none otherwise: what the sidebar marks and the next launch comes back to.
    pub top_folder: Option<i64>,
    /// Whether the year strip stands beside the grid.
    pub strip: bool,
    /// Whether a rubber band is being drawn: nothing opens over one.
    pub banding: bool,
}

/// Where a photo asked for is put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Its row at the top of the grid, under whatever lies over the top.
    Start,
    /// Its row in view, by the least movement, and none when it already is.
    Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LaidOut {
    layout_gen: u64,
    columns: usize,
    tile: f64,
}

#[derive(Default)]
pub struct GridView {
    scroll: Scroll,
    speed: ScrollSpeed,
    rows: Vec<Row>,
    headers: Vec<usize>,
    laid_out: Option<LaidOut>,
    /// Where on the thumb the pointer took hold, while the scrollbar is held.
    grab: Option<f64>,
    /// A place asked for from outside (`move_to`), until the next frame takes it.
    asked: Option<f64>,
    /// A photo asked for from outside (`go_to`), until the next frame has laid the rows
    /// out and knows where it is.
    wanted: Option<(usize, Align)>,
    /// How many runs of a year the sections make, and the layout generation that was
    /// counted in: asked before the rows are built, since it decides how wide they are.
    years: Option<(u64, usize)>,
    /// Where each run of a year begins in the rows: what the strip prints, built with them.
    marks: Vec<YearMark>,
    /// Whether the strip is held.
    scrubbing: bool,
    /// Where the photos were drawn in the last frame, and the position they were drawn
    /// at: what `rect_of` answers from.
    drawn: Option<(Rect, f64)>,
    /// Whether something else had the keyboard when the grid was last drawn.
    elsewhere: bool,
    /// The press of the primary button on the photos that is going on, and its band.
    press: Option<Press>,
    band: Option<Band>,
    /// Whether Escape was down when the grid was last drawn.
    escape_down: bool,
}

impl GridView {
    /// Puts the grid at `position` without that counting as a scroll: for a test that
    /// needs a place, not a move.
    #[cfg(test)]
    fn scroll_to(&mut self, position: f64) {
        self.scroll.set(position);
    }

    /// Asks for the grid to be at `position`, as the gate's programme does. Taken with
    /// the next frame's input, so the move is a scroll like the wheel's or a key's: set
    /// here and now, the grid was moved without knowing it, and drew every frame of a
    /// sweep as if it had always been standing there.
    pub fn move_to(&mut self, position: f64) {
        self.asked = Some(position);
    }

    /// Puts the grid at its top, which is no scroll: the photos shown are another list -
    /// another view, another search, another order - and a place in the old one names
    /// nothing in this one.
    pub fn to_top(&mut self) {
        self.asked = None;
        self.wanted = None;
        self.scroll.set(0.0);
    }

    /// Asks for the photo at grid offset `offset` to be shown: the Svelte grid's
    /// `scrollToOffset`. Taken by the next frame, once the rows are laid out for the index
    /// the offset was looked up in - the caller holds both, and gives them in one frame.
    /// A place, like the top, and not a scroll.
    pub fn go_to(&mut self, offset: usize, align: Align) {
        self.wanted = Some((offset, align));
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll.scroll_by(delta);
    }

    pub fn max_position(&self) -> f64 {
        self.scroll.max()
    }

    /// Where the photo at grid offset `offset` was drawn in the last frame, in view or
    /// not: what a test of the whole application presses.
    pub fn rect_of(&self, offset: usize) -> Option<Rect> {
        let (area, position) = self.drawn?;
        let laid = self.laid_out?;
        let row = self.rows[row_of_item(&self.rows, offset)?];
        let column = (offset - row.first) as f64;
        let x = area.left() + (GAP + column * tile_row(laid.tile)) as f32;
        let y = area.top() + (row.top - position) as f32;
        Some(Rect::from_min_size(
            pos2(x, y),
            Vec2::splat(laid.tile as f32),
        ))
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &GridData<'_>,
        thumbs: &mut Thumbs,
        selection: &mut Selection,
    ) -> GridOutput {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let viewport = f64::from(rect.height()).max(0.0);
        // The strip at the right edge, the scrollbar beside it, the photos in what is left.
        let strip = self.strip(data, rect, viewport);
        let right = strip.map_or(rect.right(), |strip| strip.left());
        let bar = Rect::from_min_max(
            pos2(right - BAR_WIDTH as f32, rect.top()),
            pos2(right, rect.bottom()),
        );
        let area = Rect::from_min_max(rect.min, pos2(bar.left(), rect.bottom()));

        let rebuilt = self.lay_out(data, f64::from(area.width()), viewport);
        if let Some((offset, align)) = self.wanted.take() {
            self.place(data.index, offset, align, viewport);
        }

        // Measured after the layout: a place restored across a resize is not a scroll.
        let before = self.scroll.position();
        self.take_input(ui, rect, bar, strip, viewport);
        self.take_picks(ui, area, data, selection, viewport);
        let now = ui.input(|input| input.time) * 1000.0;
        if self.scroll.position() != before {
            self.speed.sample(self.scroll.position(), now, viewport);
        }
        self.speed.tick(now);
        if let Some(at) = self.speed.settles_at() {
            // Nothing else would draw the frame in which the scroll counts as over.
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ui.ctx().request_repaint_after(wait);
        }

        let top = self.scroll.position();
        let motion = self.speed.motion();
        let (first, last) = visible_range(&self.rows, top, viewport, 0.0);
        let (wanted, on_screen) = self.wanted(data.index, top, viewport, motion, (first, last));
        let loading = thumbs.frame(ui.ctx(), &wanted, defers_thumbs(motion, viewport));

        let (settled, marked) = self.draw(ui, area, data, thumbs, selection, (first, last));
        self.drawn = Some((area, top));
        self.draw_band(ui, area, top);
        let pinned = pinned_header(&self.rows, &self.headers, top);
        if let Some(pinned) = pinned
            && let Some(section) = data.index.sections().get(pinned.section)
        {
            let heading = header::heading(section, data.folders, data.zone);
            // The whole header, at the place the push has taken it to, cut at the grid's
            // edge: painted in only the part still in view, its words stayed where the
            // part was and were gone as soon as the push began.
            let at = pos2(area.left(), area.top() + pinned.y as f32);
            let rect = Rect::from_min_size(at, vec2(area.width(), HEADER as f32));
            let clipped = clipped_to(ui, area);
            header::paint(&clipped, rect, &heading, true, palette(ui.ctx()));
        }
        self.draw_scrollbar(ui, bar);
        if let Some(strip) = strip {
            self.draw_strip(ui, strip, top);
        }

        GridOutput {
            on_screen,
            pinned,
            rebuilt,
            position: top,
            settled,
            marked,
            loading,
            top_folder: top_folder_id(&self.rows, data.index.sections(), top),
            strip: strip.is_some(),
            banding: self.band.is_some(),
        }
    }

    /// Where the year strip stands in `rect`, when it is shown: asked of the grid as it
    /// would be beside the strip (`shows_timeline`), before the rows are laid out for the
    /// room that answer leaves them.
    fn strip(&mut self, data: &GridData<'_>, rect: Rect, viewport: f64) -> Option<Rect> {
        let sections = data.index.sections();
        let years = match self.years {
            Some((counted_in, years)) if counted_in == data.layout_gen => years,
            _ => {
                let years = year_runs(sections, data.zone);
                self.years = Some((data.layout_gen, years));
                years
            }
        };
        let outer = f64::from(rect.width());
        let nominal = tile_width(data.size);
        shows_timeline(sections, years, outer, BAR_WIDTH, viewport, nominal).then(|| {
            Rect::from_min_max(
                pos2(rect.right() - TIMELINE_WIDTH as f32, rect.top()),
                rect.max,
            )
        })
    }

    /// Puts the grid where the photo at `offset` is: one asked for from outside (`go_to`),
    /// or the one a key has just moved the lead to. One the rows do not hold - its folder
    /// left the view between the click and the grid - moves nothing, and is not kept for
    /// a grid that does.
    fn place(&mut self, index: &GridIndex, offset: usize, align: Align, viewport: f64) {
        let Some(row) = row_of_item(&self.rows, offset).map(|at| self.rows[at]) else {
            return;
        };
        // A grid with headers has one over its top edge - a section's own, or the pinned
        // copy - and a row starts, or is in view, only below that.
        let headed = index.sections().get(row.section).is_some_and(has_header);
        let inset = if headed { HEADER } else { 0.0 };
        let to = match align {
            Align::Start => Some(scroll_to_start(&row, inset)),
            Align::Nearest => scroll_into_grid(&row, self.scroll.position(), viewport, inset),
        };
        if let Some(to) = to {
            self.scroll.set(to);
        }
    }

    /// Builds the rows again when the sections, the column count or the tile changed.
    ///
    /// Tiles fill the row, so every row's height moves with every pixel of a resize and a
    /// position kept as a number would name another photo: across a change of the tile or
    /// the columns the place is read from the rows as they were (`pin_at`) and found in the
    /// rows as they are (`pin_top`). Across a rebuilt index alone it is kept as the number
    /// it was, held to the new end, as the Svelte grid keeps it.
    ///
    /// A new index can change the tile too, by bringing the year strip or taking it away,
    /// and then the place is the photo's offset carried from the old index into the new
    /// one - right while nothing arrived before that photo, and off by what did when
    /// something has, which is as far as the kept number is off across any new index.
    /// Neither follows the photo by its id.
    fn lay_out(&mut self, data: &GridData<'_>, width: f64, viewport: f64) -> bool {
        let nominal = tile_width(data.size);
        let row = row_width(width);
        let now = LaidOut {
            layout_gen: data.layout_gen,
            columns: columns_for(row, nominal),
            tile: tile_for(row, nominal),
        };
        let rebuilt = self.laid_out != Some(now);
        let mut pin = None;
        // The corner of a press or a band is a place in the rows as they were: it is
        // found again in the rows as they are, on the photo it was on.
        let held = (self.press.zip(self.laid_out))
            .filter(|_| rebuilt)
            .and_then(|(press, was)| hold_at(&self.rows, press.origin.0, press.origin.1, was.tile));
        if rebuilt {
            let resized = self
                .laid_out
                .is_some_and(|was| was.columns != now.columns || was.tile != now.tile);
            if resized {
                pin = pin_at(&self.rows, self.scroll.position());
            }
            self.rows = build_rows(data.index.sections(), now.columns, now.tile);
            self.headers = header_rows(&self.rows);
            self.marks = year_marks(data.index.sections(), &self.rows, data.zone);
            self.laid_out = Some(now);
        }
        self.scroll.set_extent(total_height(&self.rows), viewport);
        if let Some(top) = pin.and_then(|pin| pin_top(&self.rows, pin)) {
            self.scroll.set(top);
        }
        if let Some(press) = &mut self.press
            && let Some(place) = held.and_then(|hold| held_place(&self.rows, hold, now.tile))
        {
            press.origin = place;
        }
        rebuilt
    }

    fn take_input(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        bar: Rect,
        strip: Option<Rect>,
        viewport: f64,
    ) {
        if let Some(position) = self.asked.take() {
            self.scroll.set(position);
        }
        if ui.rect_contains_pointer(rect) {
            // Positive moves the content down, which is towards the top of the grid.
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.scroll.scroll_by(-f64::from(delta));
            }
        }

        // A press on the thumb holds it where it was taken; a press on the track brings
        // the thumb's middle under the pointer. Either way the grid follows the pointer
        // for as long as the button is down.
        let response = ui.interact(bar, ui.id().with("grid-scrollbar"), Sense::click_and_drag());
        let track = f64::from(bar.height());
        let held = response
            .is_pointer_button_down_on()
            .then(|| response.interact_pointer_pos())
            .flatten();
        match (held, self.scroll.thumb(track)) {
            (Some(pointer), Some((start, length))) => {
                let y = f64::from(pointer.y - bar.top());
                let on_thumb = (start..=start + length).contains(&y);
                let grab =
                    *self
                        .grab
                        .get_or_insert(if on_thumb { y - start } else { length / 2.0 });
                self.scroll
                    .set(self.scroll.position_for_thumb(track, y - grab));
            }
            _ => self.grab = None,
        }

        // A press on the strip puts the grid at that share of its layout, and the grid
        // follows the pointer for as long as the button is down, on the strip or off it:
        // it is forty-four points wide, and a drag strays. Not in the tab order - the
        // sidebar's years are the keyboard's way to a year.
        self.scrubbing = false;
        if let Some(strip) = strip {
            let response = ui.interact(
                strip,
                ui.id().with("grid-timeline"),
                Sense::CLICK | Sense::DRAG,
            );
            let here = year_at(&self.marks, self.scroll.position());
            response.widget_info(|| {
                WidgetInfo::slider(true, here.unwrap_or_default() as f64, "Timeline")
            });
            let held = (response.is_pointer_button_down_on()
                && ui.input(|input| input.pointer.primary_down()))
            .then(|| response.interact_pointer_pos())
            .flatten();
            if let Some(pointer) = held {
                let y = f64::from(pointer.y - strip.top());
                let total = total_height(&self.rows);
                self.scroll.set(scroll_top_for(
                    y,
                    f64::from(strip.height()),
                    total,
                    viewport,
                ));
                self.scrubbing = true;
            }
        }
    }

    /// What the mouse and the keys pick this frame.
    ///
    /// A click on a photo selects it: alone, or with Ctrl/Cmd in or out of what is
    /// selected, or with Shift everything from the anchor to it - and Shift wins when both
    /// are held, as in every file manager. A click that is on no photo - a gutter, a
    /// header, the room beside a short row - selects nothing and clears nothing.
    ///
    /// The arrows, Home and End move the lead and select the photo it lands on; with
    /// Shift they select from the anchor to there, as a Shift+click does, and with
    /// nothing selected there is no anchor to select from and the key is the plain one.
    /// The page keys do the same a screenful at a time - but with nothing selected they
    /// scroll: a move would start from the first photo, the top of the library from
    /// wherever the wheel had got to. The grid follows the lead into sight.
    ///
    /// **The keys are the grid's only while nothing else has the keyboard**, and had
    /// none when the grid was last drawn. Not "while no text field has it": the splitter
    /// answers the arrows, a control with its list open the arrows, Home, End and the
    /// letters, a button the Tab key reached lets the arrows walk on from it - and the
    /// grid, drawn after all of them in the same frame, answered each of those keys a
    /// second time, moving or clearing a selection behind what the user was working.
    /// And not "has it now" alone: egui takes the keyboard off a widget at Escape
    /// before anything is drawn, so in the frame of that Escape nothing has it, and
    /// the key that left a control cleared the selection too. A click on a photo gives
    /// the keys back: egui takes the keyboard off whatever had it at a click elsewhere.
    fn take_picks(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        data: &GridData<'_>,
        selection: &mut Selection,
        viewport: f64,
    ) {
        let Some(laid) = self.laid_out else {
            return;
        };
        let index = data.index;
        // No stop of the Tab key: the grid has the keys whenever nothing else does.
        let response = ui.interact(
            area,
            ui.id().with("grid-photos"),
            Sense::CLICK | Sense::DRAG,
        );
        if let Some((pointer, modifiers)) =
            self.take_pointer(ui, area, &response, index, selection, laid.tile)
        {
            let top = self.scroll.position();
            // The pinned header lies over a row of photos, and a press on it is the
            // header's: the photo under it is one nobody can see. Its full height from
            // the top, wherever it is drawn: while the next header pushes it out, what
            // shows under it in those points is that header's own row, which is no
            // photo either.
            let on_pinned = pinned_header(&self.rows, &self.headers, top).is_some()
                && f64::from(pointer.y - area.top()) < HEADER;
            let x = f64::from(pointer.x - area.left());
            let y = f64::from(pointer.y - area.top()) + top;
            if !on_pinned && let Some(offset) = tile_at(&self.rows, x, y, laid.tile) {
                if modifiers.shift {
                    selection.extend(index, offset);
                } else if modifiers.command {
                    selection.toggle(index, offset);
                } else {
                    selection.select(index, offset);
                }
            }
        }

        let elsewhere = ui.memory(|memory| memory.focused().is_some());
        let was_elsewhere = std::mem::replace(&mut self.elsewhere, elsewhere);
        // A drag owns the grid while it lasts: Escape abandons it (`take_band`) and
        // every other key is ignored, not acted on under a rectangle that is selecting.
        if elsewhere || was_elsewhere || !data.keys || self.band.is_some() {
            return;
        }
        for (key, shift) in ui.input(grid_keys) {
            let lead = selection.lead().map(|lead| lead.offset);
            let to = match key {
                GridKey::SelectAll => {
                    selection.select_all(index, data.library);
                    continue;
                }
                GridKey::Clear => {
                    selection.clear();
                    continue;
                }
                GridKey::Page { down } => match lead {
                    Some(lead) => page_move(&self.rows, lead, down, viewport),
                    None => {
                        let page = (viewport - tile_row(laid.tile)).max(LEAST_PAGE);
                        self.scroll.scroll_by(if down { page } else { -page });
                        continue;
                    }
                },
                GridKey::Move(key) => move_from(lead, key, index.sections(), laid.columns),
            };
            if shift && lead.is_some() {
                selection.extend(index, to);
            } else {
                selection.select(index, to);
            }
            self.place(index, to, Align::Nearest, viewport);
        }
    }

    /// The primary button on the photos: a click, answered as where and with what
    /// held, or a rubber band - a press that goes further than a click may draws a
    /// rectangle, and what it touches is selected, in place of what was or, with Ctrl,
    /// Cmd or Shift held as the button went down, as well.
    ///
    /// **Both are read from the button's own events, not from what egui calls clicked
    /// and dragged.** egui's click has a time limit, so a press held still for a second
    /// selected nothing; and its drag ends at the release of any button and at Escape
    /// with anything held, and after a release that is never heard of it is stale for
    /// good, so the next drag began no band (a review found each, with a test). A press
    /// is this widget's when the button goes down with the pointer over the photos and
    /// nothing above them.
    ///
    /// **A band has one teardown, and every ending comes through it**:
    /// - the button comes up: what the band covers stays selected;
    /// - Escape, whatever is held and whoever read the key first: what was selected
    ///   before the band is put back, and the key is nobody's after that - left in the
    ///   frame, the keys below would clear the selection the band has just restored;
    /// - the gesture is taken away - the window loses the focus with the button down, or
    ///   the system cancels a touch: the same. Neither sends a release. A button let go
    ///   of over another window may never be heard of, and a band that outlived it
    ///   would go on scrolling to the end of the library, selecting all the way: what
    ///   the Svelte grid's did when a touch was taken from it;
    /// - the selection is cleared under it by a grid of other results: nothing is put
    ///   back, the band having begun in the grid before.
    fn take_pointer(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        response: &egui::Response,
        index: &GridIndex,
        selection: &mut Selection,
        tile: f64,
    ) -> Option<(egui::Pos2, Modifiers)> {
        let top = self.scroll.position();
        let (buttons, pointer, taken, escape) = ui.input(|input| {
            let buttons: Vec<(bool, egui::Pos2, Modifiers)> = (input.events.iter())
                .filter_map(|event| match event {
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers,
                    } => Some((*pressed, *pos, *modifiers)),
                    _ => None,
                })
                .collect();
            let taken = input.events.iter().any(|event| {
                let cancelled = matches!(
                    event,
                    Event::Touch {
                        phase: TouchPhase::Cancel,
                        ..
                    }
                );
                cancelled || matches!(event, Event::WindowFocused(false))
            });
            (
                buttons,
                input.pointer.latest_pos(),
                taken,
                input.key_down(Key::Escape),
            )
        });
        // Escape, by the key being down that was not: whoever read its event first.
        let escaped = escape && !std::mem::replace(&mut self.escape_down, escape);

        if taken {
            self.abandon(selection);
            self.press = None;
        }
        let mut clicked = None;
        for (pressed, at, modifiers) in buttons {
            if pressed {
                // A press with a band still held: its release was never heard of.
                self.abandon(selection);
                let y = f64::from(at.y - area.top());
                // Not from the pinned header: it lies over a row of photos, and a band
                // from it would begin on photos nobody can see. Nor in a grid with no
                // photos, where the shell draws what an empty library says.
                let on_pinned =
                    pinned_header(&self.rows, &self.headers, top).is_some() && y < HEADER;
                let photos = response.contains_pointer() && !on_pinned && !index.is_empty();
                self.press = photos.then_some(Press {
                    at,
                    origin: (f64::from(at.x - area.left()), y + top),
                    modifiers,
                    banded: false,
                });
            } else if let Some(press) = self.press.take() {
                match self.band.take() {
                    // (One whose selection was cleared under it ends as nothing:
                    // `band_end` finds no band to end.)
                    Some(band) => {
                        let covered =
                            items_in_rect(&self.rows, span(&press, &band, area, top), tile);
                        selection.band_end(index, &covered);
                    }
                    // It never was a band: a click, however long the button was down.
                    None if !press.banded => clicked = Some((at, modifiers)),
                    None => {}
                }
            }
        }

        let Some(mut press) = self.press else {
            return clicked;
        };
        // The selection was cleared under the band: it began in another grid.
        if self.band.is_some() && !selection.banding() {
            self.band = None;
        }
        if escaped && self.band.is_some() {
            self.abandon(selection);
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        Event::Key {
                            key: Key::Escape,
                            pressed: true,
                            ..
                        }
                    )
                });
            });
        }
        // It is a band once the pointer has gone further from where the button went
        // down than a click may; a press held still, however long, is none.
        let reach = ui
            .ctx()
            .options(|options| options.input_options.max_click_dist);
        if self.band.is_none()
            && !press.banded
            && let Some(pointer) = pointer.filter(|pointer| pointer.distance(press.at) > reach)
        {
            press.banded = true;
            self.press = Some(press);
            selection.band_begin(press.modifiers.command || press.modifiers.shift);
            self.band = Some(Band {
                pointer,
                scrolled_at: None,
            });
            // A drag on the photos gives them the keys, as a click on one does: egui
            // takes the keyboard off a widget at a click elsewhere, not at a press that
            // becomes a drag, and the Escape that abandons this band would be a text
            // field's to answer first.
            if let Some(focused) = ui.memory(|memory| memory.focused()) {
                ui.memory_mut(|memory| memory.surrender_focus(focused));
            }
        }
        let Some(mut band) = self.band else {
            return clicked;
        };
        if let Some(pointer) = pointer {
            band.pointer = pointer;
        }
        // Held near the top or the bottom edge, the grid scrolls under it: by the
        // edge's speed times this frame's own duration, so as fast on a screen of 120
        // frames a second as on one of 60. A frame is asked for only while the grid can
        // move that way: at its top or its end the band rests, and whatever makes room
        // again - the wheel, a scan's longer grid - draws a frame that comes through
        // here.
        let speed = edge_scroll_speed(
            f64::from(band.pointer.y),
            f64::from(area.top()),
            f64::from(area.bottom()),
            BAND_EDGE,
            BAND_SCROLL_MAX,
        );
        let position = self.scroll.position();
        let room = if speed < 0.0 {
            position > 0.0
        } else {
            position < self.scroll.max()
        };
        if speed == 0.0 || !room {
            band.scrolled_at = None;
        } else {
            let now = ui.input(|input| input.time);
            let elapsed = band.scrolled_at.map_or(0.0, |before| now - before);
            band.scrolled_at = Some(now);
            self.scroll
                .scroll_by(speed * elapsed.clamp(0.0, BAND_FRAME_MAX));
            ui.ctx().request_repaint();
        }
        let covered = items_in_rect(
            &self.rows,
            span(&press, &band, area, self.scroll.position()),
            tile,
        );
        selection.band_to(index, &covered);
        self.band = Some(band);
        clicked
    }

    /// Lets go of a band without keeping it: what was selected before it is put back.
    fn abandon(&mut self, selection: &mut Selection) {
        if self.band.take().is_some() {
            selection.band_cancel();
        }
    }

    /// Draws the band over the photos, cut at the grid's edges.
    fn draw_band(&self, ui: &egui::Ui, area: Rect, top: f64) {
        let (Some(press), Some(band)) = (self.press, self.band) else {
            return;
        };
        let span = span(&press, &band, area, top);
        let at = |x: f64, y: f64| pos2(area.left() + x as f32, area.top() + (y - top) as f32);
        let rect = Rect::from_two_pos(at(span.x0, span.y0), at(span.x1, span.y1));
        let palette = palette(ui.ctx());
        let painter = ui.painter_at(area);
        painter.rect_filled(rect, R[0], color(palette.accent_soft));
        painter.rect_stroke(
            rect,
            R[0],
            Stroke::new(1.0, color(palette.accent)),
            StrokeKind::Outside,
        );
    }

    /// The thumbnails to ask for, most wanted first - what is in view, then what the scroll
    /// is heading for, nearest first, then what it has just left - and the photos in view.
    fn wanted(
        &self,
        index: &GridIndex,
        top: f64,
        viewport: f64,
        motion: Motion,
        (first, last): (usize, usize),
    ) -> (Vec<Want>, Vec<i64>) {
        let (from, to) = wanted_range(&self.rows, top, viewport, motion);
        let mut wanted = Vec::new();
        let take = |wanted: &mut Vec<Want>, row: &Row| {
            if row.kind == RowKind::Tiles {
                let entries = index.rows(row.first, row.count).iter();
                wanted.extend(entries.map(|entry| Want {
                    id: entry.id,
                    key: entry.thumb_key,
                }));
            }
        };
        for row in &self.rows[first..last] {
            take(&mut wanted, row);
        }
        let on_screen = wanted.len();
        let below = &self.rows[last.min(to)..to];
        let above = &self.rows[from..first.max(from)];
        let upward = matches!(
            motion,
            Motion::Scroll {
                direction: Direction::Up,
                ..
            }
        );
        if upward {
            above.iter().rev().for_each(|row| take(&mut wanted, row));
            below.iter().for_each(|row| take(&mut wanted, row));
        } else {
            below.iter().for_each(|row| take(&mut wanted, row));
            above.iter().rev().for_each(|row| take(&mut wanted, row));
        }
        let ids = wanted[..on_screen].iter().map(|want| want.id).collect();
        (wanted, ids)
    }

    /// Draws the rows in view. `true` when every tile drawn has its picture or its mark.
    fn draw(
        &self,
        ui: &mut egui::Ui,
        area: Rect,
        data: &GridData<'_>,
        thumbs: &mut Thumbs,
        selection: &Selection,
        (first, last): (usize, usize),
    ) -> (bool, usize) {
        let Some(laid) = self.laid_out else {
            return (true, 0);
        };
        let palette = palette(ui.ctx());
        let clipped = clipped_to(ui, area);
        let top = snapped(self.scroll.position(), f64::from(ui.pixels_per_point()));
        let (mut settled, mut marks) = (true, 0);
        for row in &self.rows[first..last] {
            let y = area.top() + (row.top - top) as f32;
            match row.kind {
                RowKind::Header => {
                    // The rows are built from the sections they index; one that is not
                    // there means an index was published without its layout generation
                    // moving, and a missing header is the least harm.
                    let Some(section) = data.index.sections().get(row.section) else {
                        continue;
                    };
                    let heading = header::heading(section, data.folders, data.zone);
                    let rect = Rect::from_min_size(
                        pos2(area.left(), y),
                        vec2(area.width(), HEADER as f32),
                    );
                    header::paint(&clipped, rect, &heading, false, palette);
                }
                RowKind::Tiles => {
                    let entries = data.index.rows(row.first, row.count);
                    for (column, entry) in entries.iter().enumerate() {
                        let x = area.left() + (GAP + column as f64 * tile_row(laid.tile)) as f32;
                        let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(laid.tile as f32));
                        // Its mark, for a picture that cannot be made and for one that
                        // is not there yet: either way the tile says so and is not a
                        // blank the eye waits on.
                        let marked =
                            thumbs.failed(entry.thumb_key) || thumbs.troubled(entry.thumb_key);
                        let texture = thumbs.texture(entry.thumb_key);
                        settled &= marked || texture.is_some();
                        marks += usize::from(marked);
                        let selected = selection.is_selected(entry.id);
                        tile::paint(&clipped, rect, entry, texture, marked, selected, palette);
                    }
                }
            }
        }
        (settled, marks)
    }

    fn draw_scrollbar(&self, ui: &egui::Ui, bar: Rect) {
        let Some((start, length)) = self.scroll.thumb(f64::from(bar.height())) else {
            return;
        };
        let thumb = Rect::from_min_size(
            pos2(bar.left() + 3.0, bar.top() + start as f32),
            vec2(bar.width() - 6.0, length as f32),
        );
        let palette = palette(ui.ctx());
        let tint = if self.grab.is_some() {
            palette.text_dim
        } else {
            palette.field_hover
        };
        ui.painter_at(bar)
            .rect_filled(thumb, thumb.width() / 2.0, color(tint));
    }
}

impl GridView {
    /// The strip: the years where they begin, a line where the grid is, and the year
    /// under the pointer, named beside the strip over the photos.
    fn draw_strip(&self, ui: &egui::Ui, strip: Rect, top: f64) {
        let palette = palette(ui.ctx());
        let painter = ui.painter_at(strip);
        let (total, height) = (total_height(&self.rows), f64::from(strip.height()));
        let down = |place: f64| strip.top() + strip_y(place, total, height) as f32;
        painter.vline(
            strip.left() + 0.5,
            strip.y_range(),
            (1.0, color(palette.line)),
        );
        for mark in labelled_marks(&self.marks, total, height, MIN_LABEL_GAP) {
            painter.text(
                pos2(strip.center().x, down(mark.top) + LABEL_LINE / 2.0),
                Align2::CENTER_CENTER,
                mark.year,
                fonts::regular(T[0]),
                color(palette.text_dim),
            );
        }
        let here = down(top);
        painter.rect_filled(
            Rect::from_min_max(
                pos2(strip.left() + 4.0, here - 1.0),
                pos2(strip.right() - 4.0, here + 1.0),
            ),
            1.0,
            color(palette.accent),
        );

        // While the strip is held the pointer may be anywhere, and is still on it.
        let pointer = ui
            .input(|input| input.pointer.latest_pos())
            .filter(|_| self.scrubbing || ui.rect_contains_pointer(strip));
        let Some(pointer) = pointer else {
            return;
        };
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let y = (pointer.y - strip.top()).clamp(0.0, strip.height());
        let Some(year) = year_at(&self.marks, f64::from(y) / height * total) else {
            return;
        };
        // Beside the strip and over the photos, so through the grid's own painter.
        let painter = ui.painter();
        let galley = painter.layout_no_wrap(
            year.to_string(),
            fonts::semibold(ui.ctx(), T[1]),
            color(palette.text),
        );
        let size = galley.size() + vec2(2.0 * S[1], 6.0);
        let bubble = Rect::from_min_size(
            pos2(strip.left() - 6.0 - size.x, strip.top() + y - size.y / 2.0),
            size,
        );
        painter.add(menu_shadow().as_shape(bubble, R[1]));
        painter.rect_filled(bubble, R[1], color(palette.raised));
        painter.rect_stroke(
            bubble,
            R[1],
            Stroke::new(1.0, color(palette.line)),
            StrokeKind::Outside,
        );
        painter.galley(
            bubble.center() - galley.size() / 2.0,
            galley,
            color(palette.text),
        );
    }
}

/// A child of `ui` that draws nothing outside `area`: a row half scrolled out is cut at the
/// grid's edge and not drawn over what lies beside the grid. The clip has to be set:
/// `new_child` gives the child the parent's, whatever its `max_rect`.
fn clipped_to(ui: &mut egui::Ui, area: Rect) -> egui::Ui {
    let mut clipped = ui.new_child(UiBuilder::new().max_rect(area));
    clipped.set_clip_rect(area.intersect(ui.clip_rect()));
    clipped
}

/// `position` on a whole device pixel at `scale` device pixels a point. Rows drawn at a
/// fraction of one have every picture and every letter resampled, differently on each
/// frame of a scroll.
pub(crate) fn snapped(position: f64, scale: f64) -> f64 {
    if scale > 0.0 {
        (position * scale).round() / scale
    } else {
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        grid::{
            layout::{layout_height, row_index_at, row_of_item},
            timeline::{scroll_top_for, strip_y},
        },
        thumbs::{
            loader::{Building, LoadError, Loader, ThumbSource},
            textures::Pixels,
        },
    };
    use eframe::egui::{
        Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchPhase,
    };
    use photon_core::{
        grid::{GridEntry, Layout},
        media::MediaKind,
    };
    use std::{sync::Arc, time::Instant};

    /// Every thumbnail is cached, a 2x1 picture.
    struct AllCached;

    impl ThumbSource for AllCached {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            Some(Pixels {
                width: 2,
                height: 1,
                rgba: vec![255; 8],
            })
        }
        fn build(&self, _want: Want) -> Building<'_> {
            Box::pin(async { Err(LoadError::Unavailable) })
        }
    }

    /// An index of folders holding `counts` photos each. A photo's id is its place plus
    /// one, and its thumbnail key its id.
    fn index(counts: &[usize]) -> GridIndex {
        index_laid_out(counts, Layout::Folders)
    }

    fn index_laid_out(counts: &[usize], layout: Layout) -> GridIndex {
        let mut entries = Vec::new();
        for (folder, count) in counts.iter().enumerate() {
            for _ in 0..*count {
                let id = entries.len() as i64 + 1;
                entries.push(GridEntry {
                    id,
                    folder_id: folder as i64 + 1,
                    taken_at: id,
                    aspect: 1.5,
                    kind: MediaKind::Image,
                    duration_ms: None,
                    starred: false,
                    has_copies: false,
                    thumb_key: id as u64,
                    size: 0,
                    mtime_ms: 0,
                });
            }
        }
        GridIndex::build(entries, layout)
    }

    struct Fixture {
        ctx: egui::Context,
        view: GridView,
        thumbs: Thumbs,
        index: GridIndex,
        layout_gen: u64,
        size: Vec2,
        time: f64,
        /// How long the last frame said it can wait for the next one.
        repaint_delay: Duration,
        /// Room left above the grid, as a bar over it would take.
        space_above: f32,
        /// A text field above the grid that has the focus, and what is typed in it.
        typing: Option<String>,
        /// A button above the grid, and whether it asks for the keyboard this frame.
        button: Option<bool>,
        tile: GridTile,
        selection: Selection,
        /// Whether the grid is drawn as All photos.
        library: bool,
        /// How long a frame is, in seconds.
        dt: f64,
        /// Whether the window has been left for another, and whether egui has been told.
        left: bool,
        told_left: bool,
        /// The modifier keys held in the frames to come, and those egui was last told of.
        held: Modifiers,
        told: Modifiers,
        /// Every rectangle the last frame outlined two points wide, inside it: the rings.
        rings: Vec<Rect>,
        /// Every filled rectangle the last frame drew.
        fills: Vec<(Rect, egui::Color32)>,
        /// What the last frame drew: each text with where it is and what it is clipped
        /// to, and the top of every clip rectangle anything was drawn in.
        texts: Vec<(String, Rect, Rect)>,
        clip_tops: Vec<f32>,
    }

    /// Every shape in `shape`, with the clip rectangle it is drawn in.
    fn each_shape(shape: &egui::Shape, clip: Rect, visit: &mut impl FnMut(&egui::Shape, Rect)) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    each_shape(shape, clip, visit);
                }
            }
            egui::Shape::Noop => {}
            other => visit(other, clip),
        }
    }

    /// No thumbnail can be made.
    struct NoneCanBeMade;

    impl ThumbSource for NoneCanBeMade {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            None
        }
        fn build(&self, _want: Want) -> Building<'_> {
            Box::pin(async { Err(LoadError::Failed("not a picture".to_owned())) })
        }
    }

    /// No thumbnail is there to be had yet: a video with no poster, a photo on a drive
    /// that is not plugged in.
    struct NoneYet;

    impl ThumbSource for NoneYet {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            None
        }
        fn build(&self, _want: Want) -> Building<'_> {
            Box::pin(async { Err(LoadError::Unavailable) })
        }
    }

    /// 800x600: the 12px bar and the two gutters leave a row of 772px, which is four medium
    /// tiles widened to 187px, in rows of 195px.
    fn fixture(counts: &[usize]) -> Fixture {
        fixture_from(counts, AllCached)
    }

    fn fixture_from(counts: &[usize], source: impl ThumbSource) -> Fixture {
        let loader = Loader::spawn(Arc::new(source), 1, Duration::from_secs(1), || {});
        Fixture {
            ctx: egui::Context::default(),
            view: GridView::default(),
            thumbs: Thumbs::new(loader),
            index: index(counts),
            layout_gen: 1,
            size: vec2(800.0, 600.0),
            time: 0.0,
            repaint_delay: Duration::MAX,
            space_above: 0.0,
            typing: None,
            button: None,
            tile: GridTile::Medium,
            selection: Selection::default(),
            library: true,
            dt: 1.0 / 60.0,
            left: false,
            told_left: false,
            held: Modifiers::NONE,
            told: Modifiers::NONE,
            rings: Vec::new(),
            fills: Vec::new(),
            texts: Vec::new(),
            clip_tops: Vec::new(),
        }
    }

    /// Noon on 1 July, so no time zone can move it into a neighbouring year.
    fn mid(year: i16) -> i64 {
        jiff::civil::date(year, 7, 1)
            .at(12, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    /// An index of folders, each of so many photos taken in one year.
    fn index_of_years(folders: &[(usize, i16)], layout: Layout) -> GridIndex {
        let mut entries = Vec::new();
        for (folder, (count, year)) in folders.iter().enumerate() {
            for _ in 0..*count {
                let id = entries.len() as i64 + 1;
                entries.push(GridEntry {
                    id,
                    folder_id: folder as i64 + 1,
                    taken_at: mid(*year) + id,
                    aspect: 1.5,
                    kind: MediaKind::Image,
                    duration_ms: None,
                    starred: false,
                    has_copies: false,
                    thumb_key: id as u64,
                    size: 0,
                    mtime_ms: 0,
                });
            }
        }
        GridIndex::build(entries, layout)
    }

    /// A grid of two years that scrolls: forty photos of 2024, then four hundred of 2019.
    fn two_years() -> Fixture {
        let mut f = fixture(&[]);
        f.index = index_of_years(&[(40, 2024), (400, 2019)], Layout::Folders);
        f
    }

    /// Where the strip is in the fixture's window, beside a scrollbar of twelve points.
    const STRIP_LEFT: f32 = 800.0 - TIMELINE_WIDTH as f32;

    impl Fixture {
        fn frame(&mut self, mut events: Vec<Event>) -> GridOutput {
            // So is the window being left, or come back to.
            if self.told_left != self.left {
                self.told_left = self.left;
                events.insert(0, Event::WindowFocused(!self.left));
            }
            // The keys held are told when they change, as a window tells them.
            if self.told != self.held {
                self.told = self.held;
                events.insert(0, Event::ModifiersChanged(self.held));
            }
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                focused: !self.left,
                ..Default::default()
            };
            self.time += self.dt;
            let folders = HashMap::new();
            let data = GridData {
                layout_gen: self.layout_gen,
                index: &self.index,
                folders: &folders,
                size: self.tile,
                zone: &TimeZone::UTC,
                library: self.library,
                keys: true,
            };
            let (view, thumbs, space) = (&mut self.view, &mut self.thumbs, self.space_above);
            let selection = &mut self.selection;
            let typing = &mut self.typing;
            let button = self.button;
            let mut output = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.add_space(space);
                        if let Some(typed) = typing {
                            ui.add(egui::TextEdit::singleline(typed)).request_focus();
                        }
                        if let Some(asks) = button {
                            let response = ui.button("Something else");
                            if asks {
                                response.request_focus();
                            }
                        }
                        output = Some(view.show(ui, &data, thumbs, selection));
                    });
            });
            full.textures_delta.clear();
            self.texts.clear();
            self.rings.clear();
            self.clip_tops.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, clipped.clip_rect, &mut |shape, clip| {
                    // The panel's own background, which is not the grid's doing.
                    if matches!(shape, egui::Shape::Rect(rect) if rect.rect.size() == self.size) {
                        return;
                    }
                    self.clip_tops.push(clip.top());
                    if let egui::Shape::Rect(rect) = shape {
                        self.fills.push((rect.rect, rect.fill));
                        // Inside the tile: a ring around it is cut at the grid's top
                        // edge, where a row lies flush.
                        if rect.stroke.width == 2.0 && rect.stroke_kind == StrokeKind::Inside {
                            self.rings.push(rect.rect);
                        }
                    }
                    if let egui::Shape::Text(text) = shape {
                        let place = text.visual_bounding_rect();
                        self.texts
                            .push((text.galley.text().to_owned(), place, clip));
                    }
                });
            }
            self.repaint_delay = full
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(Duration::MAX, |viewport| viewport.repaint_delay);
            output.unwrap()
        }

        fn key(&mut self, key: Key) -> GridOutput {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }])
        }

        /// The first photo of the row at the top of the grid.
        fn top_photo(&self) -> usize {
            let rows = &self.view.rows;
            rows[row_index_at(rows, self.view.scroll.position())].first
        }

        /// The tile the rows are laid out with.
        fn tile(&self) -> f64 {
            self.view.laid_out.unwrap().tile
        }

        /// Where `year` is printed on the strip.
        fn label(&self, year: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(text, place, _)| text == year && place.left() >= STRIP_LEFT)
                .map(|(_, place, _)| *place)
        }

        /// The year named beside the strip, under the pointer, with where.
        fn named(&self) -> Option<(&str, Rect)> {
            (self.texts.iter())
                .find(|(text, place, _)| text.parse::<i64>().is_ok() && place.right() <= STRIP_LEFT)
                .map(|(text, place, _)| (text.as_str(), *place))
        }

        /// The primary button down or up at `at`, with what is held, as a window
        /// reports a button: the keys held are in the event.
        fn button(&mut self, at: Pos2, pressed: bool) -> GridOutput {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: self.held,
            }])
        }

        /// A click at `at` with `held` down: the pointer there, the button down, and up.
        fn click_at(&mut self, at: Pos2, held: Modifiers) -> GridOutput {
            self.held = held;
            self.frame(vec![Event::PointerMoved(at)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: held,
                }]);
            }
            self.held = Modifiers::NONE;
            self.frame(Vec::new())
        }

        /// A click in the middle of the photo at grid offset `offset`.
        fn click(&mut self, offset: usize, held: Modifiers) -> GridOutput {
            let at = self.view.rect_of(offset).expect("the photo is laid out");
            self.click_at(at.center(), held)
        }

        fn key_with(&mut self, key: Key, modifiers: Modifiers) -> GridOutput {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        }

        /// The button down at `from`, and the pointer taken to `to` with it held: far
        /// enough, and it is a drag. Each a frame.
        fn drag(&mut self, from: Pos2, to: Pos2) -> GridOutput {
            self.frame(vec![Event::PointerMoved(from)]);
            self.button(from, true);
            self.to(to)
        }

        /// The pointer taken to `at`, whatever is held.
        fn to(&mut self, at: Pos2) -> GridOutput {
            self.frame(vec![Event::PointerMoved(at)])
        }

        /// The middle of the photo at grid offset `offset`.
        fn middle(&self, offset: usize) -> Pos2 {
            self.view
                .rect_of(offset)
                .expect("the photo is laid out")
                .center()
        }

        /// The band's rectangle as the last frame drew it: the one filled in the soft
        /// accent.
        fn band(&self) -> Option<Rect> {
            let soft = color(palette(&self.ctx).accent_soft);
            (self.fills.iter())
                .find(|(_, fill)| *fill == soft)
                .map(|(rect, _)| *rect)
        }

        /// The ids selected, in the grid's order.
        fn picked(&self) -> Vec<i64> {
            self.selection.ids(&self.index)
        }

        /// The grid offset the lead is on.
        fn lead(&self) -> Option<usize> {
            self.selection.lead().map(|lead| lead.offset)
        }

        /// The grid offsets of the tiles the last frame drew a ring in.
        fn ringed(&self) -> Vec<usize> {
            let mut ringed: Vec<usize> = (0..self.index.len())
                .filter(|offset| {
                    let tile = self.view.rect_of(*offset).unwrap();
                    self.rings
                        .iter()
                        .any(|ring| (ring.min - tile.min).length() < 0.5)
                })
                .collect();
            ringed.sort_unstable();
            ringed
        }
    }

    #[test]
    fn the_strip_stands_beside_a_grid_of_several_years_that_scrolls() {
        let mut f = two_years();
        let frame = f.frame(Vec::new());
        assert!(frame.strip);
        // The tiles are laid out for the room beside it and the scrollbar.
        let row = row_width(800.0 - TIMELINE_WIDTH - BAR_WIDTH);
        assert_eq!(f.tile(), tile_for(row, tile_width(GridTile::Medium)));
        // Each year where its first header is in the layout, scaled to the strip.
        let total = total_height(&f.view.rows);
        let first = f.label("2024").expect("the first year is printed");
        let second = f.label("2019").expect("the second year is printed");
        let at = strip_y(header_of(&f.view, 1).top, total, 600.0) as f32;
        assert!(at > 20.0, "{at}");
        assert!((first.center().y - 7.0).abs() < 3.0, "{first:?}");
        assert!((second.center().y - (at + 7.0)).abs() < 3.0, "{second:?}");
        let middle = STRIP_LEFT + TIMELINE_WIDTH as f32 / 2.0;
        assert!((second.center().x - middle).abs() < 2.0, "{second:?}");
        // And no tile under it: the widest thing the grid draws ends at its scrollbar.
        assert!(
            f.fills
                .iter()
                .all(|(rect, _)| rect.left() >= STRIP_LEFT - BAR_WIDTH as f32
                    || rect.right() <= STRIP_LEFT - BAR_WIDTH as f32 + 0.5),
            "{:?}",
            f.fills
        );
    }

    #[test]
    fn there_is_no_strip_over_one_year_or_nothing_to_scroll_or_no_headers() {
        let alone = tile_for(row_width(800.0 - BAR_WIDTH), tile_width(GridTile::Medium));
        for (folders, layout) in [
            // One year, however long.
            (&[(400, 2024)][..], Layout::Folders),
            // Two years on one screen.
            (&[(4, 2024), (4, 2019)][..], Layout::Folders),
            // Sorted by name: every photo together, under no header.
            (&[(40, 2024), (400, 2019)][..], Layout::Flat),
        ] {
            let mut f = fixture(&[]);
            f.index = index_of_years(folders, layout);
            let frame = f.frame(Vec::new());
            assert!(!frame.strip, "{folders:?}");
            assert_eq!(f.tile(), alone, "{folders:?}");
            assert_eq!(f.label("2024"), None);
        }
    }

    // The strip takes width from the tiles, and tiles that fill the row are shorter for
    // it: there is a band of heights where the grid fits beside the strip and overflows
    // without it. Asked of the grid as it stands, the strip came and went on every frame.
    #[test]
    fn the_strip_does_not_come_and_go_where_it_would_make_the_grid_fit() {
        let mut f = fixture(&[]);
        f.index = index_of_years(&[(8, 2024), (8, 2019)], Layout::Folders);
        let nominal = tile_width(GridTile::Medium);
        let height = |outer: f64| {
            let row = row_width(outer);
            layout_height(
                f.index.sections(),
                columns_for(row, nominal),
                tile_for(row, nominal),
            )
        };
        let alone = height(800.0 - BAR_WIDTH);
        let beside = height(800.0 - BAR_WIDTH - TIMELINE_WIDTH);
        assert!(beside + 10.0 < alone, "{beside} and {alone}");
        f.size.y = ((alone + beside) / 2.0) as f32;
        let frames: Vec<(bool, bool)> = (0..6)
            .map(|_| {
                let frame = f.frame(Vec::new());
                (frame.strip, frame.rebuilt)
            })
            .collect();
        assert_eq!(frames[0], (false, true));
        assert_eq!(frames[1..], [(false, false); 5]);
        // A grid that scrolls by a few points, without a strip.
        assert!(f.view.max_position() > 0.0);
    }

    // A scan finds a folder of an older year, or loses the only one: the strip comes or
    // goes with the new index, the tiles change with it, and the row at the top of the
    // grid still holds the photo it held. (A folder that sorts before that photo moves
    // the grid by its rows, as it does when no strip comes with it: `lay_out`.)
    #[test]
    fn the_grid_stays_on_its_photo_when_the_strip_comes_and_when_it_goes() {
        let mut f = fixture(&[]);
        f.index = index_of_years(&[(400, 2024)], Layout::Folders);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        assert!(!f.frame(Vec::new()).strip);
        let photo = f.top_photo();
        assert!(photo > 0);

        f.index = index_of_years(&[(400, 2024), (8, 2019)], Layout::Folders);
        f.layout_gen = 2;
        let came = f.frame(Vec::new());
        assert!(came.strip && came.rebuilt);
        assert_eq!(f.top_photo(), photo);
        assert!(came.position < 5000.0, "narrower tiles make shorter rows");

        f.index = index_of_years(&[(400, 2024)], Layout::Folders);
        f.layout_gen = 3;
        let went = f.frame(Vec::new());
        assert!(!went.strip && went.rebuilt);
        assert_eq!(f.top_photo(), photo);
    }

    // The size control: other tiles, other rows, the same photo.
    #[test]
    fn another_tile_size_keeps_the_photo_at_the_top() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        f.frame(Vec::new());
        for size in [GridTile::Small, GridTile::Large, GridTile::Medium] {
            // The photo the top row begins with: rows of another length begin with
            // others, and the row that holds this one is the one at the top next.
            let photo = f.top_photo();
            assert!(photo > 0);
            f.tile = size;
            let after = f.frame(Vec::new());
            assert!(after.rebuilt);
            let rows = &f.view.rows;
            assert_eq!(
                row_of_item(rows, photo),
                Some(row_index_at(rows, after.position)),
                "{size:?}"
            );
        }
    }

    #[test]
    fn a_press_on_the_strip_goes_to_that_place_and_a_drag_goes_on_from_there() {
        let mut f = two_years();
        f.frame(Vec::new());
        let total = total_height(&f.view.rows);
        let header = header_of(&f.view, 1).top;
        // On the second year's label.
        let on = Pos2::new(STRIP_LEFT + 22.0, strip_y(header, total, 600.0) as f32);
        f.frame(vec![Event::PointerMoved(on)]);
        let pressed = f.button(on, true);
        let held = f.frame(Vec::new());
        assert_eq!(pressed.position, held.position);
        assert!((held.position - header).abs() < 0.5, "{}", held.position);

        // A drag that strays off the narrow strip keeps scrubbing.
        let to = Pos2::new(500.0, 300.0);
        let dragged = f.frame(vec![Event::PointerMoved(to)]);
        let there = scroll_top_for(300.0, 600.0, total, 600.0);
        assert!(
            (dragged.position - there).abs() < 0.5,
            "{}",
            dragged.position
        );
        assert!(there > header + 1000.0);
        // The year is still named, level with the pointer, though the pointer is off
        // the strip: it is the strip that is held.
        let (year, place) = f
            .named()
            .expect("the year is named while the strip is held");
        assert_eq!(year, "2019");
        assert!((place.center().y - 300.0).abs() < 3.0, "{place:?}");
        // Past the strip's end is the grid's end, and the year is named at the strip's.
        let below = f.frame(vec![Event::PointerMoved(Pos2::new(500.0, 900.0))]);
        assert_eq!(below.position, f.view.max_position());
        let (_, place) = f.named().expect("the year is named");
        assert!((place.center().y - 600.0).abs() < 3.0, "{place:?}");

        // Let go of, it follows the pointer no more.
        f.button(Pos2::new(500.0, 900.0), false);
        let after = f.frame(vec![Event::PointerMoved(Pos2::new(
            STRIP_LEFT + 22.0,
            100.0,
        ))]);
        assert_eq!(after.position, below.position);
    }

    // The button that opens a menu is not the one that moves the grid.
    #[test]
    fn the_other_button_on_the_strip_moves_nothing() {
        let mut f = two_years();
        f.frame(Vec::new());
        let on = Pos2::new(STRIP_LEFT + 22.0, 400.0);
        f.frame(vec![Event::PointerMoved(on)]);
        let pressed = f.frame(vec![Event::PointerButton {
            pos: on,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert_eq!(pressed.position, 0.0);
        assert_eq!(f.frame(Vec::new()).position, 0.0);
    }

    // A press that began on the photos and wandered onto the strip is not a press on it.
    #[test]
    fn a_press_begun_elsewhere_does_not_move_the_grid_from_the_strip() {
        let mut f = two_years();
        f.frame(Vec::new());
        let on_photos = Pos2::new(300.0, 300.0);
        f.frame(vec![Event::PointerMoved(on_photos)]);
        f.button(on_photos, true);
        let over = f.frame(vec![Event::PointerMoved(Pos2::new(
            STRIP_LEFT + 22.0,
            400.0,
        ))]);
        assert_eq!(over.position, 0.0);
        assert_eq!(f.frame(Vec::new()).position, 0.0);
    }

    #[test]
    fn the_strip_marks_where_the_grid_is_and_names_the_year_under_the_pointer() {
        let mut f = two_years();
        f.frame(Vec::new());
        f.view.scroll_to(3000.0);
        f.frame(Vec::new());
        let total = total_height(&f.view.rows);
        let accent = color(palette(&f.ctx).accent);
        let lines: Vec<Rect> = (f.fills.iter())
            .filter(|(rect, fill)| *fill == accent && rect.left() >= STRIP_LEFT)
            .map(|(rect, _)| *rect)
            .collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let at = strip_y(3000.0, total, 600.0) as f32;
        assert!((lines[0].center().y - at).abs() < 0.6, "{:?}", lines[0]);
        assert_eq!(lines[0].width(), TIMELINE_WIDTH as f32 - 8.0);
        // No year is named with the pointer elsewhere.
        assert_eq!(f.named(), None);

        // Deep in the second year's share of the strip.
        f.frame(vec![Event::PointerMoved(Pos2::new(
            STRIP_LEFT + 22.0,
            400.0,
        ))]);
        f.frame(Vec::new());
        let (year, place) = f.named().expect("the year under the pointer is named");
        assert_eq!(year, "2019");
        assert!((place.center().y - 400.0).abs() < 3.0, "{place:?}");
        // And at its top, the first.
        f.frame(vec![Event::PointerMoved(Pos2::new(STRIP_LEFT + 22.0, 3.0))]);
        f.frame(Vec::new());
        assert_eq!(f.named().map(|(year, _)| year), Some("2024"));
        // Off the strip, none again.
        f.frame(vec![Event::PointerMoved(Pos2::new(300.0, 300.0))]);
        f.frame(Vec::new());
        assert_eq!(f.named(), None);
    }

    // A mouse wheel reports notches, not points, and how far a notch goes is the
    // application's to say (`set_wheel_notch`).
    #[test]
    fn a_notch_of_the_wheel_moves_the_grid_as_far_as_it_is_set_to() {
        for notch in [40.0, 100.0] {
            let mut f = fixture(&[200]);
            set_wheel_notch(&f.ctx, notch);
            f.frame(vec![Event::PointerMoved(Pos2::new(300.0, 300.0))]);
            f.frame(vec![Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: vec2(0.0, -2.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
            let mut position = 0.0;
            for _ in 0..60 {
                position = f.frame(Vec::new()).position;
            }
            assert!(
                (position - 2.0 * notch).abs() < 0.5,
                "{notch}: moved {position}"
            );
        }
    }

    #[test]
    fn the_wheel_moves_the_grid_by_what_it_was_turned() {
        let mut f = fixture(&[200]);
        let over = Pos2::new(300.0, 300.0);
        f.frame(vec![Event::PointerMoved(over)]);
        f.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        // egui spreads a wheel step over a few frames.
        let mut position = 0.0;
        for _ in 0..60 {
            position = f.frame(Vec::new()).position;
        }
        assert!((position - 120.0).abs() < 0.5, "moved {position}");
    }

    #[test]
    fn end_shows_the_last_row_and_home_the_first() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let end = f.key(Key::End);
        assert_eq!(end.position, f.view.max_position());
        assert!(end.position > 0.0);
        assert_eq!(
            *end.on_screen.last().unwrap(),
            200,
            "the last photo is in view"
        );
        assert_eq!(f.key(Key::Home).position, 0.0);
    }

    // With nothing selected a page key scrolls, as it always has: moving the selection
    // would start from the first photo, the top of the library from wherever the wheel
    // had got to.
    #[test]
    fn a_page_key_scrolls_a_viewport_less_a_row_while_nothing_is_selected() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // Rows of 195px: a 600px viewport less a row is 405.
        assert_eq!(f.key(Key::PageDown).position, 405.0);
        assert_eq!(f.key(Key::PageDown).position, 810.0);
        assert_eq!(f.key(Key::PageUp).position, 405.0);
        assert_eq!(f.lead(), None);
        assert!(f.picked().is_empty());
    }

    // A viewport less a row is less than nothing there, and Page Down went up.
    #[test]
    fn a_page_key_still_scrolls_the_right_way_in_a_window_shorter_than_a_row() {
        let mut f = fixture(&[200]);
        f.size = vec2(800.0, 150.0);
        f.frame(Vec::new());
        assert_eq!(f.key(Key::PageDown).position, 40.0);
        assert_eq!(f.key(Key::PageDown).position, 80.0);
        assert_eq!(f.key(Key::PageUp).position, 40.0);
    }

    // Tiles fill the row, so a narrower window is other rows at other heights: the grid
    // must still be on the photo it was on.
    #[test]
    fn narrowing_the_window_keeps_the_photo_at_the_top() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        f.frame(Vec::new());
        let photo = f.top_photo();
        assert!(photo > 0);

        f.size = vec2(560.0, 600.0);
        let after = f.frame(Vec::new());
        assert!(after.rebuilt);
        let rows = &f.view.rows;
        assert_eq!(
            row_of_item(rows, photo),
            Some(row_index_at(rows, after.position)),
            "the row at the top holds the photo that was at the top"
        );
    }

    #[test]
    fn the_rows_are_built_again_for_a_new_layout_and_not_for_a_new_frame() {
        let mut f = fixture(&[50, 50]);
        assert!(f.frame(Vec::new()).rebuilt);
        assert!(!f.frame(Vec::new()).rebuilt);
        // A publish that moved a photo between sections.
        f.index = index(&[49, 51]);
        f.layout_gen = 2;
        assert!(f.frame(Vec::new()).rebuilt);
        assert_eq!(f.view.rows.iter().map(|row| row.count).sum::<usize>(), 100);
    }

    #[test]
    fn a_rebuilt_index_keeps_the_position_and_holds_it_to_the_new_end() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        assert_eq!(f.frame(Vec::new()).position, 5000.0);
        // The same place in a library that grew.
        f.index = index(&[800]);
        f.layout_gen = 2;
        assert_eq!(f.frame(Vec::new()).position, 5000.0);
        // And the end of one that shrank under it.
        f.index = index(&[8]);
        f.layout_gen = 3;
        let shrunk = f.frame(Vec::new());
        assert_eq!(shrunk.position, f.view.max_position());
        assert!(shrunk.position < 5000.0);
    }

    #[test]
    fn deep_in_a_folder_its_header_is_pinned_and_the_next_one_pushes_it_out() {
        // The second folder is long enough that the grid can scroll to its header.
        let mut f = fixture(&[8, 40]);
        assert_eq!(f.frame(Vec::new()).pinned, None);
        f.view.scroll_to(200.0);
        let deep = f.frame(Vec::new()).pinned.unwrap();
        assert_eq!((deep.section, deep.y), (0, 0.0));
        // Folder 1 is a header and two rows: 32 + 2 * 195 = 422, then the gap; folder 2's
        // header is at 446. Ten pixels short of it, the pinned header is pushed up by 22.
        f.view.scroll_to(436.0);
        let pushed = f.frame(Vec::new()).pinned.unwrap();
        assert_eq!((pushed.section, pushed.y), (0, -22.0));
    }

    #[test]
    fn a_press_on_the_scrollbars_track_takes_the_grid_there() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        // The bar is the last 12px of the width; its bottom is the end of the grid.
        let bottom = Pos2::new(794.0, 598.0);
        f.frame(vec![Event::PointerMoved(bottom)]);
        let pressed = f.frame(vec![Event::PointerButton {
            pos: bottom,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        let held = f.frame(Vec::new());
        assert_eq!(pressed.position.max(held.position), f.view.max_position());
    }

    #[test]
    fn what_is_in_view_is_reported_and_gets_its_pictures() {
        let mut f = fixture(&[40]);
        let first = f.frame(Vec::new());
        // Four columns; the header and three rows are in a 600px viewport, the fourth row
        // begins at 32 + 3 * 195 = 617.
        assert_eq!(first.on_screen, (1..=12).collect::<Vec<i64>>());
        assert!(!first.settled, "nothing has been read yet");
        assert!(first.loading);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the pictures never came");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.texture(1).is_some());
        // Settled is what is in view. The rows wanted beyond it may still be on their
        // way, and then they come too, and nothing is.
        while f.frame(Vec::new()).loading {
            assert!(
                Instant::now() < deadline,
                "the pictures never stopped coming"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.frame(Vec::new()).settled);
    }

    // A first run, and every launch until the engine's first build: the index is empty.
    #[test]
    fn an_empty_library_draws_nothing_and_still_takes_its_keys() {
        let mut f = fixture(&[]);
        let first = f.frame(Vec::new());
        assert!(first.on_screen.is_empty());
        assert_eq!(
            (first.position, first.pinned, first.settled),
            (0.0, None, true)
        );
        for key in [
            Key::End,
            Key::PageDown,
            Key::ArrowDown,
            Key::Home,
            Key::PageUp,
        ] {
            assert_eq!(f.key(key).position, 0.0, "{key:?}");
        }
        // And picks nothing: there is no photo for a key or a click to land on.
        f.key_with(Key::A, Modifiers::COMMAND);
        f.key_with(Key::End, Modifiers::SHIFT);
        f.click_at(Pos2::new(100.0, 100.0), Modifiers::NONE);
        assert!(f.picked().is_empty());
        assert_eq!(f.lead(), None);
    }

    // A window being dragged shut, or minimised: the grid is given no room, or less than a
    // tile, or less than the scrollbar is wide.
    #[test]
    fn a_window_with_no_room_is_drawn_without_a_panic() {
        let mut f = fixture(&[40]);
        f.frame(Vec::new());
        f.view.scroll_to(500.0);
        for size in [
            vec2(0.0, 0.0),
            vec2(5.0, 5.0),
            vec2(800.0, 3.0),
            vec2(8.0, 600.0),
        ] {
            f.size = size;
            f.frame(Vec::new());
            f.key(Key::End);
        }
        // And it is a grid again when the room comes back.
        f.size = vec2(800.0, 600.0);
        assert!(!f.frame(Vec::new()).on_screen.is_empty());
    }

    // The same window with a strip in it, the pointer on the strip and the button down:
    // a strip of no height has no place to name and none to go to.
    #[test]
    fn a_strip_with_no_room_is_drawn_and_pressed_without_a_panic() {
        let mut f = two_years();
        f.frame(Vec::new());
        for size in [
            vec2(0.0, 0.0),
            vec2(5.0, 5.0),
            vec2(800.0, 0.0),
            vec2(30.0, 600.0),
        ] {
            f.size = size;
            let on = Pos2::new(size.x - 2.0, 0.0);
            f.frame(vec![Event::PointerMoved(on)]);
            let pressed = f.button(on, true);
            assert!(pressed.position.is_finite(), "{size:?}");
            f.button(on, false);
        }
        f.size = vec2(800.0, 600.0);
        let back = f.frame(Vec::new());
        assert!(back.strip && !back.on_screen.is_empty());
    }

    // The rows are built for the sections of one index. An index swapped in without its
    // layout generation moving is the engine's contract broken, and must cost a wrong
    // frame, not the application.
    #[test]
    fn rows_that_outlive_their_index_are_drawn_as_far_as_it_goes() {
        let mut f = fixture(&[50, 50]);
        f.frame(Vec::new());
        // The second folder's header is at 2591: a header and thirteen rows of 195, and
        // the gap. In view at 2500, and pinned at the end.
        for position in [2500.0, f64::INFINITY] {
            f.index = index(&[50, 50]);
            f.view.scroll_to(position);
            f.frame(Vec::new());
            // One folder of three photos, under rows built for two folders of fifty.
            f.index = index(&[3]);
            let after = f.frame(Vec::new());
            assert!(!after.rebuilt);
            assert!(after.on_screen.iter().all(|id| (1..=3).contains(id)));
        }
    }

    // An unreadable file, or one that is no picture: the tile shows its mark, is not
    // asked for again, and counts as having what it will ever have.
    #[test]
    fn a_thumbnail_that_cannot_be_made_settles_as_its_mark() {
        let mut f = fixture_from(&[4], NoneCanBeMade);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the failures never came back");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.failed(1));
        assert!(f.thumbs.texture(1).is_none());
    }

    // A laptop at 150%, a 4K screen at 175%: a row drawn between two device pixels blurs
    // every picture in it.
    #[test]
    fn rows_are_drawn_on_whole_device_pixels_at_any_scale() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for position in [0.0, 100.3, 100.5, 33_554_431.7] {
                let device = snapped(position, scale) * scale;
                assert!(
                    (device - device.round()).abs() < 1e-6,
                    "{position} at {scale}"
                );
                assert!((snapped(position, scale) - position).abs() <= 0.5 / scale + 1e-9);
            }
        }
        assert_eq!(snapped(100.3, 1.0), 100.0);
        assert_eq!(snapped(100.3, 0.0), 100.3);
    }

    // The other half of "a thumbnail that can never be made": one that is not there *yet*.
    // Its tile is not a blank for ever - it has its mark, the view counts as settled, and
    // a frame is asked for when the thumbnail is due to be asked for again, since a still
    // grid would otherwise never ask.
    #[test]
    fn a_thumbnail_not_to_be_had_yet_shows_its_mark_and_is_asked_for_again() {
        let mut f = fixture_from(&[4], NoneYet);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the tiles never settled");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.troubled(1));
        assert!(!f.thumbs.failed(1));
        // All four tiles are marks, and the frame says how many: a grid that "shows its
        // pictures" by showing none is what the gate must be able to tell apart.
        assert_eq!(f.frame(Vec::new()).marked, 4);
        // Nothing else is going on, so what the frame asks to be woken for is the retry.
        f.frame(Vec::new());
        let retry = Duration::from_secs_f64(crate::thumbs::textures::RETRY_SECS);
        assert!(
            f.repaint_delay > Duration::from_secs(1) && f.repaint_delay <= retry,
            "the next frame is in {:?}, not at the retry",
            f.repaint_delay
        );
    }

    // The grid may share its panel: a bar above it, a strip beside it. A row half scrolled
    // out is cut at the grid's own edge and not drawn over what lies there.
    #[test]
    fn nothing_is_drawn_outside_the_grids_own_area() {
        let mut f = fixture(&[40]);
        f.space_above = 50.0;
        f.frame(Vec::new());
        // The first row of tiles starts 68px above the top of the grid now.
        f.view.scroll_to(100.0);
        f.frame(Vec::new());
        assert!(!f.clip_tops.is_empty());
        let highest = f.clip_tops.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            highest >= 50.0,
            "something is drawn from {highest}, above the grid"
        );
    }

    // As the next folder's header arrives it pushes the pinned one out, and the pinned one
    // slides: its words move up with it and are cut at the grid's top. Anchored to the
    // part of the header still in view instead, they were gone as soon as the push began.
    #[test]
    fn a_pinned_header_being_pushed_out_slides_with_its_words() {
        let mut f = fixture(&[8, 40]);
        f.frame(Vec::new());
        // Ten pixels short of the second folder's header: the pinned one is pushed up by 22.
        f.view.scroll_to(436.0);
        let pushed = f.frame(Vec::new()).pinned.unwrap();
        assert_eq!((pushed.section, pushed.y), (0, -22.0));
        // The first folder holds eight photos; no other header on screen says so.
        let summaries: Vec<&(String, Rect, Rect)> = f
            .texts
            .iter()
            .filter(|(text, _, _)| text.starts_with("8 photos"))
            .collect();
        assert_eq!(summaries.len(), 1, "{:?}", f.texts);
        let (_, place, clip) = summaries[0];
        // The header is 32 tall and 22 of it is above the grid: its line is centred 2.5px
        // above the top edge, so its lower part is in view.
        assert!(
            place.top() < 0.0 && place.bottom() > 0.0,
            "drawn at {place:?}"
        );
        assert!(
            clip.intersects(*place),
            "cut away altogether: {place:?} in {clip:?}"
        );
    }

    // The gate's programme moves the grid from outside, between two frames. Moved by
    // setting the position, the grid never knew it had moved: no lead was loaded ahead of
    // a scroll, nothing was held back in a sweep, and the hold after a jump that the
    // Svelte grid pays was not paid - so the two were not doing the same thing.
    #[test]
    fn a_move_asked_for_from_outside_is_a_scroll_like_any_other() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        assert_eq!(f.view.speed.motion(), Motion::Still);

        // Under a viewport a frame: a scroll.
        for step in 1..=4 {
            f.view.move_to(f64::from(step) * 300.0);
            let frame = f.frame(Vec::new());
            assert_eq!(frame.position, f64::from(step) * 300.0);
        }
        assert!(
            matches!(f.view.speed.motion(), Motion::Scroll { .. }),
            "{:?}",
            f.view.speed.motion()
        );

        // Many viewports a frame, frame after frame: a stream of jumps, with the
        // thumbnails held back and a frame asked for in which it will count as over.
        for step in 1..=3 {
            f.view.move_to(f64::from(step) * 20_000.0);
            f.frame(Vec::new());
        }
        assert_eq!(f.view.speed.motion(), Motion::Jump { stream: true });
        assert!(f.view.speed.settles_at().is_some());
        assert!(f.repaint_delay < Duration::from_secs(1));
    }

    // Another view, another search, another order: the photos are another list, and the
    // place held in the old one names nothing in this one.
    #[test]
    fn other_results_are_shown_from_their_top() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.view.move_to(5000.0);
        assert_eq!(f.frame(Vec::new()).position, 5000.0);
        // With a move still asked for and not yet taken: that was for the old list too.
        f.view.move_to(9000.0);
        f.view.to_top();
        assert_eq!(f.frame(Vec::new()).position, 0.0);
        assert_eq!(f.frame(Vec::new()).position, 0.0);
    }

    /// The row that is the header of section `section`.
    fn header_of(view: &GridView, section: usize) -> Row {
        (view.rows.iter().copied())
            .find(|row| row.kind == RowKind::Header && row.section == section)
            .expect("the section has a header")
    }

    // A click on a folder in the sidebar: its header at the top of the grid.
    #[test]
    fn a_photo_asked_for_is_at_the_start_of_the_next_frame_under_its_header() {
        let mut f = fixture(&[50, 50, 50]);
        assert_eq!(f.frame(Vec::new()).top_folder, Some(1));
        // The second folder's first photo.
        f.view.go_to(50, Align::Start);
        let frame = f.frame(Vec::new());
        assert_eq!(frame.position, header_of(&f.view, 1).top);
        assert_eq!(frame.top_folder, Some(2));
        // Once: the grid is the user's again afterwards.
        f.view.scroll_by(-100.0);
        assert_eq!(
            f.frame(Vec::new()).position,
            header_of(&f.view, 1).top - 100.0
        );
    }

    // From the middle of a section the row goes under the pinned header, not behind it.
    #[test]
    fn a_photo_in_the_middle_of_a_folder_is_put_under_the_pinned_header() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.view.go_to(100, Align::Start);
        let frame = f.frame(Vec::new());
        let row = f.view.rows[row_of_item(&f.view.rows, 100).unwrap()];
        assert_eq!(frame.position, row.top - HEADER);
        assert!(frame.pinned.is_some());
    }

    // Under no header there is nothing over the top of the grid to stay clear of.
    #[test]
    fn a_photo_asked_for_in_a_flat_view_is_flush_to_the_top_and_names_no_folder() {
        let mut f = fixture(&[200]);
        f.index = index_laid_out(&[200], Layout::Flat);
        assert_eq!(f.frame(Vec::new()).top_folder, None);
        f.view.go_to(100, Align::Start);
        let frame = f.frame(Vec::new());
        let row = f.view.rows[row_of_item(&f.view.rows, 100).unwrap()];
        assert_eq!(frame.position, row.top);
        assert_eq!(frame.top_folder, None);
    }

    // A place is not a move: the grid is put there, as it is put at its top, and does not
    // hold its thumbnails back as it does after a jump of the scrollbar.
    #[test]
    fn going_to_a_place_is_not_a_scroll() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.view.go_to(3000, Align::Start);
        let frame = f.frame(Vec::new());
        assert!(frame.position > 10_000.0);
        assert_eq!(f.view.speed.motion(), Motion::Still);
    }

    #[test]
    fn a_photo_is_brought_into_view_by_the_least_movement() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        let row_of = |f: &Fixture, offset| f.view.rows[row_of_item(&f.view.rows, offset).unwrap()];
        // In view already: nothing moves.
        f.view.go_to(5, Align::Nearest);
        assert_eq!(f.frame(Vec::new()).position, 0.0);
        // Below: up until its bottom edge shows.
        f.view.go_to(100, Align::Nearest);
        let below = row_of(&f, 100);
        assert_eq!(
            f.frame(Vec::new()).position,
            below.top + below.height - 600.0
        );
        // Above: down to just under the pinned header.
        f.view.go_to(40, Align::Nearest);
        assert_eq!(f.frame(Vec::new()).position, row_of(&f, 40).top - HEADER);
    }

    // Other results are shown from their top, and a place asked for in the old ones names
    // nothing in these; a place asked for in the new ones is where they are shown.
    #[test]
    fn the_top_and_a_place_asked_for_are_each_the_last_word() {
        let mut f = fixture(&[50, 50, 50]);
        f.frame(Vec::new());
        f.view.go_to(50, Align::Start);
        f.view.to_top();
        assert_eq!(f.frame(Vec::new()).position, 0.0);
        f.view.to_top();
        f.view.go_to(50, Align::Start);
        assert_eq!(f.frame(Vec::new()).position, header_of(&f.view, 1).top);
    }

    // The folder has left the view between the click and the grid.
    #[test]
    fn a_photo_the_grid_does_not_hold_moves_nothing() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.view.scroll_to(1000.0);
        f.view.go_to(200, Align::Start);
        assert_eq!(f.frame(Vec::new()).position, 1000.0);
        // And is not kept for a grid that does.
        f.index = index(&[400]);
        f.layout_gen += 1;
        assert_eq!(f.frame(Vec::new()).position, 1000.0);
    }

    // The keys are the text field's while it has them; the pointer is not. With the
    // caret in the search box the scrollbar did nothing, however often it was pressed:
    // egui gives up a field's focus on a click elsewhere, not on a press that is held.
    #[test]
    fn the_scrollbar_is_the_pointers_while_a_text_field_has_the_keys() {
        let mut f = fixture(&[4000]);
        f.typing = Some(String::new());
        f.space_above = 0.0;
        f.frame(Vec::new());
        f.frame(Vec::new());
        let on_track = Pos2::new(794.0, 400.0);
        f.frame(vec![Event::PointerMoved(on_track)]);
        let pressed = f.frame(vec![Event::PointerButton {
            pos: on_track,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(
            pressed.position > 1000.0,
            "the grid is at {}",
            pressed.position
        );
    }

    // With the search box focused, Home and End move its caret, and Page Down is not the
    // library's either: the grid has the keys only while no text field does.
    #[test]
    fn the_keys_are_a_text_fields_while_it_has_the_focus() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.typing = Some(String::new());
        // The field asks for the focus in one frame and has it in the next.
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.key(Key::End).position, 0.0);
        assert_eq!(f.key(Key::PageDown).position, 0.0);
        f.key(Key::ArrowRight);
        f.key_with(Key::A, Modifiers::COMMAND);
        assert!(f.picked().is_empty());
        assert_eq!(f.lead(), None);

        // The field gone, the keys are the grid's again.
        f.typing = None;
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(f.key(Key::PageDown).position > 0.0);
        assert_eq!(f.key(Key::End).position, f.view.max_position());
    }

    // ---- picking: the mouse ----

    #[test]
    fn a_click_selects_the_photo_under_it_and_its_tile_alone_is_ringed() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        assert_eq!(f.ringed(), [] as [usize; 0]);
        f.click(5, Modifiers::NONE);
        assert_eq!(f.picked(), [6]);
        assert_eq!(f.lead(), Some(5));
        assert_eq!(f.ringed(), [5]);
        // Another click is another photo, alone.
        f.click(2, Modifiers::NONE);
        assert_eq!(f.picked(), [3]);
        assert_eq!(f.ringed(), [2]);
    }

    #[test]
    fn ctrl_click_adds_a_photo_and_takes_it_out_and_shift_click_ranges() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(1, Modifiers::NONE);
        f.click(6, Modifiers::COMMAND);
        assert_eq!(f.picked(), [2, 7]);
        assert_eq!(f.ringed(), [1, 6]);
        f.click(1, Modifiers::COMMAND);
        assert_eq!(f.picked(), [7]);
        // From the anchor - the photo last clicked, in or out - to the photo.
        f.click(4, Modifiers::SHIFT);
        assert_eq!(f.picked(), [2, 3, 4, 5]);
        assert_eq!(f.ringed(), [1, 2, 3, 4]);
        assert_eq!(f.lead(), Some(4));
    }

    // The grid is not the window: a bar stands over it, and the sidebar beside it.
    #[test]
    fn a_click_is_measured_from_where_the_photos_are_drawn() {
        let mut f = fixture(&[200]);
        f.space_above = 100.0;
        f.frame(Vec::new());
        // The second row, a point above its bottom edge: a hundred points further down
        // is the row under it.
        let tile = f.view.rect_of(5).unwrap();
        assert_eq!(tile.top(), 100.0 + 32.0 + 195.0);
        f.click_at(tile.center_bottom() - vec2(0.0, 1.0), Modifiers::NONE);
        assert_eq!(f.picked(), [6]);
        // And the pinned header is where it is drawn, not a header's height from the
        // top of the window.
        f.view.scroll_to(430.0);
        f.frame(Vec::new());
        f.click_at(Pos2::new(100.0, 110.0), Modifiers::NONE);
        assert_eq!(f.picked(), [6]);
        f.click_at(Pos2::new(100.0, 140.0), Modifiers::NONE);
        assert_eq!(f.picked(), [9]);
    }

    // As in every file manager.
    #[test]
    fn shift_wins_when_ctrl_is_held_too() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(1, Modifiers::NONE);
        f.click(3, Modifiers::SHIFT | Modifiers::COMMAND);
        assert_eq!(f.picked(), [2, 3, 4]);
    }

    #[test]
    fn a_click_on_no_photo_selects_nothing_and_clears_nothing() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        // The gutter between the first two tiles of the first row, the gutter under
        // that row, and the folder's header.
        for at in [
            Pos2::new(199.0, 100.0),
            Pos2::new(100.0, 223.0),
            Pos2::new(100.0, 10.0),
        ] {
            f.click_at(at, Modifiers::NONE);
            assert_eq!(f.picked(), [6], "{at:?}");
            assert_eq!(f.lead(), Some(5), "{at:?}");
        }
        // And with nothing selected, nothing is.
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click_at(Pos2::new(199.0, 100.0), Modifiers::NONE);
        assert!(f.picked().is_empty());
    }

    // The pinned header lies over a row of photos: the photo under it is one nobody can
    // see, and a press there is the header's.
    #[test]
    fn a_click_on_the_pinned_header_is_not_a_click_on_the_photo_under_it() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // The third row's photos under the top edge: its top is at 422.
        f.view.scroll_to(430.0);
        let frame = f.frame(Vec::new());
        assert!(frame.pinned.is_some());
        f.click_at(Pos2::new(100.0, 10.0), Modifiers::NONE);
        assert!(f.picked().is_empty());
        // Just under the header the same photo is there to be clicked.
        f.click_at(Pos2::new(100.0, 40.0), Modifiers::NONE);
        assert_eq!(f.picked(), [9]);
    }

    // Where the grid has no headers nothing is pinned, and the photos begin at its top
    // edge: the first points of the grid are a photo's there.
    #[test]
    fn a_click_at_the_top_of_a_grid_with_no_headers_is_on_the_photo_there() {
        let mut f = fixture(&[]);
        f.index = index_laid_out(&[200], Layout::Flat);
        f.frame(Vec::new());
        f.click_at(Pos2::new(100.0, 10.0), Modifiers::NONE);
        assert_eq!(f.picked(), [1]);
        f.view.scroll_to(400.0);
        f.frame(Vec::new());
        f.click_at(Pos2::new(100.0, 10.0), Modifiers::NONE);
        assert_eq!(f.picked(), [9]);
    }

    #[test]
    fn a_press_on_the_scrollbar_or_the_strip_picks_nothing() {
        let mut f = two_years();
        f.frame(Vec::new());
        // The strip at the right edge, the scrollbar left of it.
        let strip = STRIP_LEFT + 20.0;
        let bar = STRIP_LEFT - BAR_WIDTH as f32 / 2.0;
        for x in [strip, bar] {
            f.click_at(Pos2::new(x, 100.0), Modifiers::NONE);
            assert!(f.picked().is_empty(), "{x}");
            assert_eq!(f.lead(), None, "{x}");
        }
    }

    // ---- picking: the keys ----

    #[test]
    fn the_arrows_home_and_end_move_the_lead_and_select_the_photo_it_lands_on() {
        // Two folders, in rows of four: eight photos and five.
        let mut f = fixture(&[8, 5]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        for (key, lands) in [
            (Key::ArrowRight, 6),
            (Key::ArrowUp, 2),
            (Key::ArrowLeft, 1),
            (Key::ArrowDown, 5),
            // Into the next folder, in the same column.
            (Key::ArrowDown, 9),
            (Key::End, 12),
            (Key::Home, 0),
        ] {
            f.key(key);
            assert_eq!(f.lead(), Some(lands), "{key:?}");
            assert_eq!(f.picked(), [lands as i64 + 1], "{key:?}");
            assert_eq!(f.ringed(), [lands], "{key:?}");
        }
    }

    // Each key answers for itself from nothing: End reaches the last photo.
    #[test]
    fn a_key_with_nothing_selected_starts_from_the_first_photo() {
        let mut f = fixture(&[8, 5]);
        f.frame(Vec::new());
        f.key(Key::End);
        assert_eq!(f.lead(), Some(12));
        f.selection.clear();
        f.key(Key::ArrowDown);
        assert_eq!(f.lead(), Some(4));
    }

    #[test]
    fn shift_with_a_key_selects_from_the_anchor_to_where_the_key_leads() {
        let mut f = fixture(&[8, 5]);
        f.frame(Vec::new());
        f.click(1, Modifiers::NONE);
        f.key_with(Key::ArrowRight, Modifiers::SHIFT);
        f.key_with(Key::ArrowDown, Modifiers::SHIFT);
        assert_eq!(f.picked(), [2, 3, 4, 5, 6, 7]);
        assert_eq!(f.lead(), Some(6));
        // Back across the anchor: the range is measured from it, not grown.
        f.key_with(Key::Home, Modifiers::SHIFT);
        assert_eq!(f.picked(), [1, 2]);
        f.key_with(Key::End, Modifiers::SHIFT);
        assert_eq!(f.picked().len(), 12);
        // The plain key is one photo again.
        f.key(Key::ArrowLeft);
        assert_eq!(f.picked(), [12]);
    }

    // With nothing selected there is no anchor to select from.
    #[test]
    fn shift_with_a_key_is_the_plain_key_while_nothing_is_selected() {
        let mut f = fixture(&[8, 5]);
        f.frame(Vec::new());
        f.key_with(Key::End, Modifiers::SHIFT);
        assert_eq!(f.picked(), [13]);
        assert_eq!(f.lead(), Some(12));
    }

    #[test]
    fn a_page_key_moves_the_lead_a_screenful_in_its_column() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(1, Modifiers::NONE);
        // Rows of 195 in a viewport of 600: three rows down, the fourth being past it.
        f.key(Key::PageDown);
        assert_eq!(f.lead(), Some(13));
        assert_eq!(f.picked(), [14]);
        f.key_with(Key::PageDown, Modifiers::SHIFT);
        assert_eq!(f.lead(), Some(25));
        assert_eq!(f.picked().len(), 13);
        f.key(Key::PageUp);
        assert_eq!(f.lead(), Some(13));
        assert_eq!(f.picked(), [14]);
    }

    #[test]
    fn the_grid_follows_the_lead_into_sight_and_stays_while_it_is_in_sight() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(0, Modifiers::NONE);
        // The second row is in sight: nothing moves.
        assert_eq!(f.key(Key::ArrowDown).position, 0.0);
        // The third row begins at 422 and ends, gutter and all, at 617, past the bottom
        // edge at 600: it is brought in by the least movement. So is the fourth.
        assert_eq!(f.key(Key::ArrowDown).position, 17.0);
        assert_eq!(f.key(Key::ArrowDown).position, 212.0);
        assert_eq!(f.lead(), Some(12));
        // Back up. The third row is in sight, below the pinned header: nothing moves.
        assert_eq!(f.key(Key::ArrowUp).position, 212.0);
        // The second begins at 227, under the pinned header's 32 points: it comes in
        // below the header, not flush to the edge under it.
        assert_eq!(f.key(Key::ArrowUp).position, 227.0 - HEADER);
        assert_eq!(f.key(Key::ArrowUp).position, 0.0);
        assert_eq!(f.lead(), Some(0));
    }

    #[test]
    fn ctrl_a_is_the_section_in_the_library_and_the_whole_view_elsewhere() {
        let mut f = fixture(&[8, 5]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        f.key_with(Key::A, Modifiers::COMMAND);
        assert_eq!(f.picked(), [9, 10, 11, 12, 13]);
        assert_eq!(f.lead(), Some(9));
        f.library = false;
        f.key_with(Key::A, Modifiers::COMMAND);
        assert_eq!(f.picked().len(), 13);
        // The letter alone is no key of the grid's, nor is it with Alt held too: Ctrl
        // and Alt together are AltGr, which types a letter.
        f.click(9, Modifiers::NONE);
        f.key(Key::A);
        f.key_with(Key::A, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(f.picked(), [10]);
    }

    // Escape in an empty search box hands the keys to the grid, and a key still held
    // from there must not take the selection with it.
    #[test]
    fn escape_clears_the_selection_and_its_repeat_does_not() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // The key goes down with nothing selected, and stays down. egui works out for
        // itself whether a press is a repeat: it is one while the key has not come up.
        let escape = |pressed| Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![escape(true)]);
        f.click(5, Modifiers::NONE);
        f.frame(vec![escape(true)]);
        assert_eq!(f.picked(), [6]);
        // Let go of and pressed, it is the grid's key.
        f.frame(vec![escape(false)]);
        f.frame(vec![escape(true)]);
        assert!(f.picked().is_empty());
        assert_eq!(f.lead(), None);
        assert_eq!(f.ringed(), [] as [usize; 0]);
    }

    // A chord is the system's, or another part of the window's: Alt+Left is "back" on
    // some desktops, and Ctrl+End is nobody's here.
    #[test]
    fn a_key_with_ctrl_or_alt_held_is_not_the_grids() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        for held in [Modifiers::ALT, Modifiers::COMMAND, Modifiers::CTRL] {
            for key in [Key::ArrowRight, Key::End, Key::PageDown, Key::Escape] {
                let frame = f.key_with(key, held);
                assert_eq!(f.picked(), [6], "{held:?} {key:?}");
                assert_eq!(frame.position, 0.0, "{held:?} {key:?}");
            }
        }
    }

    #[test]
    fn a_held_arrow_moves_at_each_repeat() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(0, Modifiers::NONE);
        let repeat = |repeat| Event::Key {
            key: Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        // Two in one frame, as a slow frame gets them.
        f.frame(vec![repeat(false), repeat(true)]);
        f.frame(vec![repeat(true)]);
        assert_eq!(f.lead(), Some(3));
    }

    #[test]
    fn a_selection_draws_no_frame_by_itself() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.repaint_delay, Duration::MAX);
    }

    // ---- from the review ----

    // The keys are whoever's has the keyboard. A splitter that answers the arrows, a
    // control with its list open, a row the Tab key reached: each key they answer would
    // otherwise be answered here as well, by a grid drawn later in the same frame.
    #[test]
    fn the_keys_are_not_the_grids_while_anything_else_has_the_keyboard() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        // From the frame it takes the keyboard in: a key in that frame is not the grid's
        // because nothing had the keyboard a frame ago.
        f.button = Some(true);
        f.key(Key::ArrowRight);
        assert_eq!(f.picked(), [6]);
        f.frame(Vec::new());
        f.button = Some(false);
        for (key, held) in [
            (Key::ArrowRight, Modifiers::NONE),
            (Key::End, Modifiers::NONE),
            (Key::PageDown, Modifiers::SHIFT),
            (Key::A, Modifiers::COMMAND),
        ] {
            let frame = f.key_with(key, held);
            assert_eq!(f.picked(), [6], "{key:?}");
            assert_eq!(frame.position, 0.0, "{key:?}");
        }
    }

    // egui takes the keyboard off a widget at Escape before anything is drawn: by the
    // time the grid is, nothing has it. The key was that widget's all the same.
    #[test]
    fn the_escape_that_takes_the_keyboard_off_something_else_is_not_the_grids() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        f.button = Some(true);
        f.frame(Vec::new());
        f.frame(Vec::new());
        f.button = Some(false);
        let escape = |pressed| Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![escape(true)]);
        assert!(f.ctx.memory(|memory| memory.focused().is_none()));
        assert_eq!(f.picked(), [6]);
        // The next one is the grid's.
        f.frame(vec![escape(false)]);
        f.frame(vec![escape(true)]);
        assert!(f.picked().is_empty());
    }

    // The Shift held when the button came up is the click's, whatever becomes of Shift
    // later in the same frame: a frame is sixteen milliseconds when nothing is slow.
    #[test]
    fn a_shift_click_is_one_when_shift_comes_up_in_the_frame_of_the_click() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(1, Modifiers::NONE);
        let at = f.view.rect_of(4).unwrap().center();
        f.held = Modifiers::SHIFT;
        f.frame(vec![Event::PointerMoved(at)]);
        let button = |pressed, modifiers| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        f.frame(vec![button(true, Modifiers::SHIFT)]);
        // The button up with Shift held, then Shift up, in one frame.
        f.held = Modifiers::NONE;
        f.told = Modifiers::NONE;
        f.frame(vec![
            button(false, Modifiers::SHIFT),
            Event::ModifiersChanged(Modifiers::NONE),
        ]);
        assert_eq!(f.picked(), [2, 3, 4, 5]);
        // And the other way round: Shift down in the frame of a plain click's release.
        f.frame(vec![button(true, Modifiers::NONE)]);
        f.held = Modifiers::SHIFT;
        f.told = Modifiers::SHIFT;
        f.frame(vec![
            button(false, Modifiers::NONE),
            Event::ModifiersChanged(Modifiers::SHIFT),
        ]);
        assert_eq!(f.picked(), [5]);
    }

    // The photos sense a click, so a press on them is theirs however near the scrollbar
    // it lands: egui hands a near miss to a neighbouring widget only where nothing is hit.
    #[test]
    fn a_press_on_the_photos_beside_the_scrollbar_is_not_the_scrollbars() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        // The bar is the last twelve points; this is three points left of it.
        let frame = f.click_at(Pos2::new(785.0, 400.0), Modifiers::NONE);
        assert_eq!(frame.position, 0.0);
        // And on the bar itself the grid goes there, as before.
        let frame = f.click_at(Pos2::new(794.0, 400.0), Modifiers::NONE);
        assert!(frame.position > 1000.0, "{}", frame.position);
    }

    // ---- the rubber band ----

    #[test]
    fn a_drag_across_the_photos_selects_what_its_rectangle_touches() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // From the second photo of the first row to the third of the second.
        let (from, to) = (f.middle(1), f.middle(6));
        let frame = f.drag(from, to);
        assert!(frame.banding);
        assert_eq!(f.picked(), [2, 3, 6, 7]);
        assert_eq!(f.ringed(), [1, 2, 5, 6]);
        // The rectangle is drawn from where the button went down to the pointer.
        let band = f.band().expect("the band is drawn");
        assert!((band.min - from).length() < 1.0, "{band:?}");
        assert!((band.max - to).length() < 1.0, "{band:?}");
        // The lead is not moved by a band being drawn.
        assert_eq!(f.lead(), None);

        let frame = f.button(to, false);
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
        assert_eq!(f.picked(), [2, 3, 6, 7]);
        // Let go of, the lead is on the band's first photo.
        assert_eq!(f.lead(), Some(1));
    }

    #[test]
    fn a_band_dragged_back_takes_photos_off_again() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(10));
        assert_eq!(f.picked(), [2, 3, 6, 7, 10, 11]);
        f.to(f.middle(2));
        assert_eq!(f.picked(), [2, 3]);
        // Up and to the left of where it began.
        f.to(f.middle(0));
        assert_eq!(f.picked(), [1, 2]);
        f.button(f.middle(0), false);
        assert_eq!(f.picked(), [1, 2]);
        assert_eq!(f.lead(), Some(0));
    }

    #[test]
    fn a_band_replaces_the_selection_and_with_ctrl_or_shift_adds_to_it() {
        for (held, picked) in [
            (Modifiers::NONE, vec![2, 3]),
            (Modifiers::COMMAND, vec![2, 3, 10]),
            (Modifiers::SHIFT, vec![2, 3, 10]),
        ] {
            let mut f = fixture(&[200]);
            f.frame(Vec::new());
            f.click(9, Modifiers::NONE);
            f.held = held;
            let to = f.middle(2);
            f.drag(f.middle(1), to);
            assert_eq!(f.picked(), picked, "{held:?}");
            // Whatever is held when the button comes up: it is how the band began.
            f.held = Modifiers::NONE;
            f.button(to, false);
            assert_eq!(f.picked(), picked, "{held:?}");
        }
    }

    // Dragging over empty room is how a selection is cleared with the mouse.
    #[test]
    fn a_band_over_no_photo_clears_the_selection() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(5, Modifiers::NONE);
        // Down the gutter between the first two columns.
        let (from, to) = (Pos2::new(199.0, 50.0), Pos2::new(199.0, 400.0));
        f.drag(from, to);
        assert!(f.picked().is_empty());
        f.button(to, false);
        assert!(f.picked().is_empty());
        assert_eq!(f.lead(), None);
    }

    #[test]
    fn a_press_that_does_not_move_is_still_a_click() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let at = f.middle(5);
        f.frame(vec![Event::PointerMoved(at)]);
        f.button(at, true);
        // A few points, as a hand does: not a drag.
        let frame = f.to(at + vec2(3.0, 2.0));
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
        f.button(at + vec2(3.0, 2.0), false);
        assert_eq!(f.picked(), [6]);
        assert_eq!(f.lead(), Some(5));
    }

    #[test]
    fn escape_abandons_a_band_and_the_rest_of_that_press_selects_nothing() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        f.drag(f.middle(1), f.middle(6));
        assert_eq!(f.picked(), [2, 3, 6, 7]);
        let frame = f.key(Key::Escape);
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
        // What was selected before the band is back, not cleared by the key.
        assert_eq!(f.picked(), [10]);
        assert_eq!(f.lead(), Some(9));
        // The button is still down and the pointer goes on: nothing follows it.
        let frame = f.to(f.middle(11));
        assert!(!frame.banding);
        assert_eq!(f.picked(), [10]);
        f.button(f.middle(11), false);
        assert_eq!(f.picked(), [10]);
        // And the next press is a band again.
        f.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        f.drag(f.middle(1), f.middle(2));
        assert_eq!(f.picked(), [2, 3]);
    }

    // The pinned header lies over photos nobody can see; the scrollbar and the strip
    // are their own.
    #[test]
    fn a_drag_begun_on_the_pinned_header_the_scrollbar_or_the_strip_is_no_band() {
        let mut f = two_years();
        f.frame(Vec::new());
        f.view.scroll_to(430.0);
        f.frame(Vec::new());
        let strip = STRIP_LEFT + 20.0;
        let bar = STRIP_LEFT - BAR_WIDTH as f32 / 2.0;
        for from in [
            Pos2::new(100.0, 10.0),
            Pos2::new(bar, 200.0),
            Pos2::new(strip, 200.0),
        ] {
            let frame = f.drag(from, Pos2::new(300.0, 300.0));
            assert!(!frame.banding, "{from:?}");
            assert!(f.picked().is_empty(), "{from:?}");
            f.button(Pos2::new(300.0, 300.0), false);
            assert!(f.picked().is_empty(), "{from:?}");
        }
    }

    #[test]
    fn a_band_keeps_its_grip_on_the_photos_it_began_over_while_the_wheel_turns() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let (from, to) = (f.middle(1), f.middle(6));
        f.drag(from, to);
        // The grid goes down two rows under a pointer that holds still. egui spreads a
        // turn of the wheel over a few frames.
        f.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -390.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        for _ in 0..60 {
            f.frame(Vec::new());
        }
        assert!((f.view.scroll.position() - 390.0).abs() < 0.5);
        // The origin is still on the second photo; the pointer is two rows further on.
        assert_eq!(f.picked(), [2, 3, 6, 7, 10, 11, 14, 15]);
        let band = f.band().unwrap();
        assert!((band.top() - (from.y - 390.0)).abs() < 1.0, "{band:?}");
        assert!((band.bottom() - to.y).abs() < 1.0, "{band:?}");
    }

    /// A band held in the bottom margin for `frames` frames of `dt` seconds: how far
    /// the grid went.
    fn held_at_the_bottom(dt: f64, frames: usize) -> (Fixture, f64) {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.dt = dt;
        // Twelve points from the bottom edge: three quarters of the way into the margin.
        f.drag(f.middle(1), Pos2::new(300.0, 588.0));
        let mut position = 0.0;
        for _ in 0..frames {
            position = f.frame(Vec::new()).position;
        }
        (f, position)
    }

    #[test]
    fn a_band_held_near_the_bottom_edge_scrolls_the_grid_and_grows() {
        let (mut f, position) = held_at_the_bottom(1.0 / 60.0, 60);
        // Three quarters of 1,400 points a second, for a second.
        assert!((position - 1050.0).abs() < 25.0, "{position}");
        // The band has grown with it: from the photo it began on to the one under the
        // pointer now, rows further down.
        // It is one column wide - the pointer is straight under where it began - so
        // one photo of each row it has passed, down to the row under the pointer.
        let picked = f.picked();
        assert_eq!(picked[0], 2);
        assert!(picked.len() >= 8, "{picked:?}");
        assert!(picked.iter().all(|id| id % 4 == 2), "{picked:?}");
        // And each frame of it was asked for.
        assert_eq!(f.repaint_delay, Duration::ZERO);
        // In the top margin it goes back up.
        f.to(Pos2::new(300.0, 12.0));
        let up = f.frame(Vec::new()).position;
        assert!(up < position, "{up}");
    }

    // In the frame the grid moves, the band covers what it is drawn over: worked out
    // from where the grid was, the photo under the pointer was a frame late in each row.
    #[test]
    fn a_scrolling_band_selects_what_is_under_the_pointer_in_the_same_frame() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        let pointer = Pos2::new(300.0, 590.0);
        f.drag(f.middle(1), pointer);
        let mut rows_met = std::collections::HashSet::new();
        for _ in 0..90 {
            f.frame(Vec::new());
            let under = (0..400).find(|offset| f.view.rect_of(*offset).unwrap().contains(pointer));
            if let Some(under) = under {
                rows_met.insert(under);
                assert!(f.picked().contains(&(under as i64 + 1)), "photo {under}");
            }
        }
        assert!(rows_met.len() > 5, "{rows_met:?}");
    }

    // The speed is points a second: a faster screen draws more frames of less each.
    #[test]
    fn a_band_scrolls_as_far_in_a_second_whatever_the_frame_rate() {
        let (_, at_60) = held_at_the_bottom(1.0 / 60.0, 60);
        let (_, at_120) = held_at_the_bottom(1.0 / 120.0, 120);
        assert!((at_60 - at_120).abs() < 25.0, "{at_60} {at_120}");
    }

    // A frame that took half a second - the window dragged, the machine asleep - is
    // not half a second of scroll.
    #[test]
    fn a_long_frame_scrolls_no_further_than_fifty_milliseconds_does() {
        let (_, position) = held_at_the_bottom(0.5, 4);
        assert!(position <= 4.0 * 0.05 * 1400.0, "{position}");
        assert!(position > 0.0);
    }

    #[test]
    fn a_band_held_still_away_from_the_edges_asks_for_no_frame() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(6));
        f.frame(Vec::new());
        let frame = f.frame(Vec::new());
        assert_eq!(frame.position, 0.0);
        assert_eq!(f.repaint_delay, Duration::MAX);
    }

    #[test]
    fn the_keys_do_nothing_while_a_band_is_held() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(6));
        for (key, held) in [
            (Key::ArrowRight, Modifiers::NONE),
            (Key::End, Modifiers::NONE),
            (Key::PageDown, Modifiers::NONE),
            (Key::A, Modifiers::COMMAND),
        ] {
            let frame = f.key_with(key, held);
            assert!(frame.banding, "{key:?}");
            assert_eq!(f.picked(), [2, 3, 6, 7], "{key:?}");
            assert_eq!(frame.position, 0.0, "{key:?}");
        }
    }

    // A drag on the photos gives them the keys, as a click on one does: the Escape
    // that abandons the band must not be a text field's.
    #[test]
    fn a_band_takes_the_keyboard_from_whatever_had_it() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        f.button = Some(true);
        f.frame(Vec::new());
        f.frame(Vec::new());
        f.button = Some(false);
        assert!(f.ctx.memory(|memory| memory.focused().is_some()));
        f.drag(f.middle(1), f.middle(6));
        assert!(f.ctx.memory(|memory| memory.focused().is_none()));
        f.key(Key::Escape);
        assert_eq!(f.picked(), [10]);
    }

    // The selection was cleared under the band - a step's grid arrived: the band began
    // in the grid before, and nothing of it goes on.
    #[test]
    fn a_band_ends_when_the_selection_is_cleared_under_it() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(6));
        f.selection.clear();
        let frame = f.to(f.middle(10));
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
        assert!(f.picked().is_empty());
        f.button(f.middle(10), false);
        assert!(f.picked().is_empty());
        // The next press is a band of its own.
        f.drag(f.middle(1), f.middle(2));
        assert_eq!(f.picked(), [2, 3]);
    }

    // The origin is a place in the layout, not in the window: deep in a library the
    // two are thousands of points apart.
    #[test]
    fn a_band_begun_deep_in_the_grid_is_over_the_photos_under_the_pointer() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.view.scroll_to(4000.0);
        f.frame(Vec::new());
        let (first, last) = (f.top_photo() + 5, f.top_photo() + 10);
        f.drag(f.middle(first), f.middle(last));
        let ids = |offsets: [usize; 4]| offsets.map(|offset| offset as i64 + 1);
        assert_eq!(f.picked(), ids([first, first + 1, last - 1, last]));
        f.button(f.middle(last), false);
        assert_eq!(f.lead(), Some(first));
    }

    #[test]
    fn a_drag_with_the_other_button_is_no_band() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        let (from, to) = (f.middle(1), f.middle(6));
        f.frame(vec![Event::PointerMoved(from)]);
        let other = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![other(from, true)]);
        let frame = f.to(to);
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
        f.frame(vec![other(to, false)]);
        assert_eq!(f.picked(), [10]);
    }

    // A button let go of over another window may never be heard of: egui goes on
    // calling the photos dragged, and a pointer last seen in a margin would scroll to
    // the end of the library, selecting all the way.
    #[test]
    fn a_band_is_abandoned_when_the_window_is_left_with_the_button_down() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        f.drag(f.middle(1), Pos2::new(300.0, 590.0));
        f.frame(Vec::new());
        assert!(f.frame(Vec::new()).position > 0.0);
        f.left = true;
        let frame = f.frame(Vec::new());
        assert!(!frame.banding);
        assert_eq!(f.picked(), [10]);
        // Nothing scrolls while it is away, and once the scroll that was has counted as
        // over nothing is asked for; nor when the window is come back to.
        let at = frame.position;
        for _ in 0..30 {
            assert_eq!(f.frame(Vec::new()).position, at);
        }
        assert_eq!(f.repaint_delay, Duration::MAX);
        f.left = false;
        assert_eq!(f.frame(Vec::new()).position, at);
        assert_eq!(f.picked(), [10]);
        // The button comes up at last, and the next press is a band of its own.
        f.button(Pos2::new(300.0, 590.0), false);
        assert_eq!(f.picked(), [10]);
        let (from, to) = (f.top_photo() + 5, f.top_photo() + 6);
        f.drag(f.middle(from), f.middle(to));
        assert_eq!(f.picked(), [from as i64 + 1, to as i64 + 1]);
    }

    // ---- from the review of the band ----

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        }
    }

    // An adding band is begun with Ctrl held, and Escape is pressed with it still down.
    // egui ends a drag at Escape whatever is held; read as a plain key only, the Escape
    // was not seen, and "no longer dragged" was taken for the button coming up.
    #[test]
    fn escape_with_ctrl_still_held_abandons_an_adding_band() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        f.held = ctrl();
        f.drag(f.middle(1), f.middle(6));
        assert_eq!(f.picked(), [2, 3, 6, 7, 10]);
        let frame = f.key_with(Key::Escape, ctrl());
        assert!(!frame.banding);
        assert_eq!(f.picked(), [10]);
        assert_eq!(f.lead(), Some(9));
    }

    // The release that is never heard of: the band is abandoned as the window is left,
    // the window is come back to, and no button-up was ever delivered.
    #[test]
    fn after_a_release_never_heard_of_the_next_drag_is_a_band() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(6));
        f.left = true;
        assert!(!f.frame(Vec::new()).banding);
        f.left = false;
        f.frame(Vec::new());
        let frame = f.drag(f.middle(1), f.middle(2));
        assert!(frame.banding);
        assert_eq!(f.picked(), [2, 3]);
    }

    // Another button coming up says nothing about this drag.
    #[test]
    fn another_button_let_go_of_does_not_end_the_band() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.drag(f.middle(1), f.middle(2));
        assert_eq!(f.picked(), [2, 3]);
        let other = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![other(f.middle(2), true)]);
        let frame = f.frame(vec![other(f.middle(2), false)]);
        assert!(frame.banding);
        // The first button is still down: the band follows the pointer.
        f.to(f.middle(6));
        assert_eq!(f.picked(), [2, 3, 6, 7]);
        f.button(f.middle(6), false);
        assert_eq!(f.picked(), [2, 3, 6, 7]);
        assert_eq!(f.lead(), Some(1));
    }

    fn touch(phase: TouchPhase, pos: Pos2) -> Event {
        Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos,
            force: None,
        }
    }

    // The system takes a touch for a gesture of its own: a cancel and the pointer gone,
    // with no release. The Svelte band's old bug: a loop left running to the end of the
    // library, selecting all the way.
    #[test]
    fn a_touch_the_system_takes_ends_the_band_and_nothing_scrolls() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.click(9, Modifiers::NONE);
        let (from, to) = (f.middle(1), Pos2::new(300.0, 590.0));
        // A touch as a window reports one: the touch, and the pointer it stands for.
        f.frame(vec![
            touch(TouchPhase::Start, from),
            Event::PointerMoved(from),
            Event::PointerButton {
                pos: from,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        let frame = f.frame(vec![touch(TouchPhase::Move, to), Event::PointerMoved(to)]);
        assert!(frame.banding);
        f.frame(Vec::new());
        assert!(f.frame(Vec::new()).position > 0.0);
        let frame = f.frame(vec![touch(TouchPhase::Cancel, to), Event::PointerGone]);
        assert!(!frame.banding);
        assert_eq!(f.picked(), [10]);
        let at = frame.position;
        for _ in 0..120 {
            assert_eq!(f.frame(Vec::new()).position, at);
        }
        // The next touch is a band of its own.
        f.frame(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        let (first, last) = (f.top_photo() + 5, f.top_photo() + 6);
        f.drag(f.middle(first), f.middle(last));
        assert_eq!(f.picked(), [first as i64 + 1, last as i64 + 1]);
    }

    // The origin is where the button went down: the wheel turned under a button that is
    // down and still must not move it to other photos.
    #[test]
    fn the_origin_is_where_the_button_went_down_whatever_the_wheel_did_before_the_drag() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let from = f.middle(1);
        f.frame(vec![Event::PointerMoved(from)]);
        f.button(from, true);
        f.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -390.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        for _ in 0..30 {
            f.frame(Vec::new());
        }
        assert!(f.view.scroll.position() > 380.0);
        assert!(!f.frame(Vec::new()).banding);
        // Now the pointer moves, a column to the right.
        let frame = f.to(from + vec2(195.0, 0.0));
        assert!(frame.banding);
        // From the photo the button went down on, now two rows above the pointer.
        assert_eq!(f.picked(), [2, 3, 6, 7, 10, 11]);
    }

    // At its top the grid cannot go further up: a band resting in the top margin
    // there asks for no frame. Whatever makes room again draws one.
    #[test]
    fn a_band_resting_in_a_margin_where_the_grid_cannot_move_asks_for_no_frame() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.drag(f.middle(5), Pos2::new(300.0, 10.0));
        // egui draws a frame of its own after one with input in it.
        f.frame(Vec::new());
        f.frame(Vec::new());
        for _ in 0..20 {
            let frame = f.frame(Vec::new());
            assert_eq!(frame.position, 0.0);
            assert_ne!(f.repaint_delay, Duration::ZERO);
        }
        // The wheel takes the grid down, and the band takes it back up.
        f.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -2000.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        let mut seen = Vec::new();
        for _ in 0..240 {
            seen.push(f.frame(Vec::new()).position);
        }
        let deepest = seen.iter().copied().fold(0.0, f64::max);
        assert!(deepest > 500.0, "{deepest}");
        assert_eq!(seen.last(), Some(&0.0));
        // And at the end of the library the same, in the bottom margin.
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(f64::INFINITY);
        f.frame(Vec::new());
        f.drag(Pos2::new(300.0, 300.0), Pos2::new(300.0, 590.0));
        f.frame(Vec::new());
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_ne!(f.repaint_delay, Duration::ZERO);
    }

    // Where a library has no photos the shell draws what it says in the grid's room.
    #[test]
    fn no_band_is_drawn_in_a_grid_with_no_photos() {
        let mut f = fixture(&[]);
        f.frame(Vec::new());
        let frame = f.drag(Pos2::new(100.0, 100.0), Pos2::new(400.0, 400.0));
        assert!(!frame.banding);
        assert_eq!(f.band(), None);
    }

    // The window narrowed under a held band - a tiling window manager, a sidebar shown:
    // the rows are built again with another tile, and the band still begins on its photo.
    #[test]
    fn a_band_keeps_the_photo_it_began_on_when_the_rows_are_built_again_under_it() {
        let mut f = fixture(&[4000]);
        f.frame(Vec::new());
        f.view.scroll_to(40_000.0);
        f.frame(Vec::new());
        let first = f.top_photo() + 5;
        let from = f.middle(first);
        f.drag(from, from + vec2(20.0, 20.0));
        assert_eq!(f.picked(), [first as i64 + 1]);
        f.size = vec2(620.0, 600.0);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(f.picked().contains(&(first as i64 + 1)), "{:?}", f.picked());
        // From its photo to the pointer, and not from a place a hundred rows off.
        assert!(f.picked().len() < 12, "{:?}", f.picked());
    }

    // A window that has the pointer and not the keyboard: it is the focus being lost
    // that takes a gesture away, not the focus being elsewhere.
    #[test]
    fn a_band_can_be_drawn_in_a_window_that_does_not_have_the_keyboard() {
        let mut f = fixture(&[200]);
        f.left = true;
        f.frame(Vec::new());
        let frame = f.drag(f.middle(1), f.middle(6));
        assert!(frame.banding);
        assert_eq!(f.picked(), [2, 3, 6, 7]);
    }

    // egui's click has a time limit. A press held still is a click however long it
    // lasts, as in a browser: the slow Ctrl+click still takes a photo out.
    #[test]
    fn a_press_held_still_is_a_click_however_long_it_is_held() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.click(2, Modifiers::NONE);
        let at = f.middle(5);
        f.frame(vec![Event::PointerMoved(at)]);
        f.held = ctrl();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: ctrl(),
        };
        f.frame(vec![button(true)]);
        f.dt = 1.0;
        let held = f.frame(Vec::new());
        assert!(!held.banding);
        f.frame(Vec::new());
        f.dt = 1.0 / 60.0;
        f.frame(vec![button(false)]);
        assert_eq!(f.picked(), [3, 6]);
        // On the room between photos it selects nothing and clears nothing.
        f.held = Modifiers::NONE;
        let gutter = Pos2::new(199.0, 100.0);
        f.frame(vec![Event::PointerMoved(gutter)]);
        f.button(gutter, true);
        f.dt = 1.0;
        f.frame(Vec::new());
        f.dt = 1.0 / 60.0;
        f.button(gutter, false);
        assert_eq!(f.picked(), [3, 6]);
    }

    // A tap of a touchpad: the button down and up before a frame is drawn.
    #[test]
    fn a_press_and_its_release_in_one_frame_are_a_click() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let at = f.middle(5);
        f.frame(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![button(true), button(false)]);
        assert_eq!(f.picked(), [6]);
    }

    // The window is left with the button down on a photo and the pointer still: the
    // release is never heard of. Come back to, the pointer moves with no button down -
    // and that is no band.
    #[test]
    fn a_press_left_behind_with_the_window_is_no_band_when_the_pointer_comes_back() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let at = f.middle(1);
        f.frame(vec![Event::PointerMoved(at)]);
        f.button(at, true);
        f.left = true;
        f.frame(Vec::new());
        f.left = false;
        f.frame(Vec::new());
        let frame = f.to(f.middle(6));
        assert!(!frame.banding);
        assert!(f.picked().is_empty());
    }

    // Escape is the key going down, not the key being down: one held from before a
    // band began does not abandon it.
    #[test]
    fn an_escape_held_since_before_a_band_began_does_not_abandon_it() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        f.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        let frame = f.drag(f.middle(1), f.middle(6));
        assert!(frame.banding);
        let frame = f.to(f.middle(10));
        assert!(frame.banding);
        assert_eq!(f.picked(), [2, 3, 6, 7, 10, 11]);
    }
}
