//! The grid: lays the published index out, moves through it, and draws what is in view.
//!
//! One function does all three in that order, every frame. That order is why several rules
//! of the Svelte grid have no successor here (`placeIn`, `restoredTo`, `viewTop`): they
//! bridged the render in which the tiles changed and the effect that moved the viewport
//! after it, and here the place is found again before anything is drawn.

use super::{
    header,
    layout::{
        GAP, HEADER, PinnedHeader, Row, RowKind, build_rows, columns_for, defers_thumbs,
        header_rows, pin_at, pin_top, pinned_header, row_width, tile_for, tile_row, tile_width,
        total_height, visible_range, wanted_range,
    },
    motion::{Direction, Motion, ScrollSpeed},
    scroll::{BAR_WIDTH, Scroll},
    tile,
};
use crate::{
    theme::apply::{color, palette},
    thumbs::{loader::Want, shown::Thumbs},
};
use eframe::egui::{self, Key, Rect, Sense, UiBuilder, Vec2, pos2, vec2};
use jiff::tz::TimeZone;
use photon_core::{
    grid::GridIndex,
    library::{Folder, GridTile},
};
use std::{collections::HashMap, time::Duration};

/// How far an arrow key moves the grid.
const ARROW_STEP: f64 = 40.0;

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
}

impl GridView {
    /// Moves the grid to `position`, as the probe's programme does.
    pub fn scroll_to(&mut self, position: f64) {
        self.scroll.set(position);
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll.scroll_by(delta);
    }

    pub fn max_position(&self) -> f64 {
        self.scroll.max()
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &GridData<'_>,
        thumbs: &mut Thumbs,
    ) -> GridOutput {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let bar = Rect::from_min_max(pos2(rect.right() - BAR_WIDTH as f32, rect.top()), rect.max);
        let area = Rect::from_min_max(rect.min, pos2(bar.left(), rect.bottom()));
        let viewport = f64::from(area.height());

        let rebuilt = self.lay_out(data, f64::from(area.width()), viewport);

        // Measured after the layout: a place restored across a resize is not a scroll.
        let before = self.scroll.position();
        self.take_input(ui, rect, bar, viewport);
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
        thumbs.frame(ui.ctx(), &wanted, defers_thumbs(motion, viewport));

        let settled = self.draw(ui, area, data, thumbs, (first, last));
        let pinned = pinned_header(&self.rows, &self.headers, top);
        if let Some(pinned) = pinned
            && let Some(section) = data.index.sections().get(pinned.section)
        {
            let heading = header::heading(section, data.folders, data.zone);
            let at = pos2(area.left(), area.top() + pinned.y as f32);
            let rect = Rect::from_min_size(at, vec2(area.width(), HEADER as f32));
            let clipped = ui.new_child(UiBuilder::new().max_rect(area));
            header::paint(
                &clipped,
                rect.intersect(area),
                &heading,
                true,
                palette(ui.ctx()),
            );
        }
        self.draw_scrollbar(ui, bar);

        GridOutput {
            on_screen,
            pinned,
            rebuilt,
            position: top,
            settled,
        }
    }

    /// Builds the rows again when the sections, the column count or the tile changed.
    ///
    /// Tiles fill the row, so every row's height moves with every pixel of a resize and a
    /// position kept as a number would name another photo: across a change of the tile or
    /// the columns the place is read from the rows as they were (`pin_at`) and found in the
    /// rows as they are (`pin_top`). Across a rebuilt index it is kept as the number it
    /// was, held to the new end, as the Svelte grid keeps it.
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
        if rebuilt {
            let resized = self
                .laid_out
                .is_some_and(|was| was.columns != now.columns || was.tile != now.tile);
            if resized {
                pin = pin_at(&self.rows, self.scroll.position());
            }
            self.rows = build_rows(data.index.sections(), now.columns, now.tile);
            self.headers = header_rows(&self.rows);
            self.laid_out = Some(now);
        }
        self.scroll.set_extent(total_height(&self.rows), viewport);
        if let Some(top) = pin.and_then(|pin| pin_top(&self.rows, pin)) {
            self.scroll.set(top);
        }
        rebuilt
    }

    fn take_input(&mut self, ui: &egui::Ui, rect: Rect, bar: Rect, viewport: f64) {
        if ui.rect_contains_pointer(rect) {
            // Positive moves the content down, which is towards the top of the grid.
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.scroll.scroll_by(-f64::from(delta));
            }
        }

        let row = self.laid_out.map_or(0.0, |laid| tile_row(laid.tile));
        let page = (viewport - row).max(ARROW_STEP);
        let scroll = &mut self.scroll;
        ui.input(|input| {
            if input.key_pressed(Key::Home) {
                scroll.set(0.0);
            }
            if input.key_pressed(Key::End) {
                scroll.set(f64::INFINITY);
            }
            if input.key_pressed(Key::PageDown) {
                scroll.scroll_by(page);
            }
            if input.key_pressed(Key::PageUp) {
                scroll.scroll_by(-page);
            }
            if input.key_pressed(Key::ArrowDown) {
                scroll.scroll_by(ARROW_STEP);
            }
            if input.key_pressed(Key::ArrowUp) {
                scroll.scroll_by(-ARROW_STEP);
            }
        });

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
        (first, last): (usize, usize),
    ) -> bool {
        let Some(laid) = self.laid_out else {
            return true;
        };
        let palette = palette(ui.ctx());
        // A child whose clip is the grid's own area: a row half scrolled out is cut at the
        // edge and not drawn over what lies beside the grid.
        let clipped = ui.new_child(UiBuilder::new().max_rect(area));
        let top = snapped(self.scroll.position(), f64::from(ui.pixels_per_point()));
        let mut settled = true;
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
                    header::paint(&clipped, rect.intersect(area), &heading, false, palette);
                }
                RowKind::Tiles => {
                    let entries = data.index.rows(row.first, row.count);
                    for (column, entry) in entries.iter().enumerate() {
                        let x = area.left() + (GAP + column as f64 * tile_row(laid.tile)) as f32;
                        let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(laid.tile as f32));
                        let failed = thumbs.failed(entry.thumb_key);
                        let texture = thumbs.texture(entry.thumb_key);
                        settled &= failed || texture.is_some();
                        tile::paint(&clipped, rect, entry, texture, failed, palette);
                    }
                }
            }
        }
        settled
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

/// `position` on a whole device pixel at `scale` device pixels a point. Rows drawn at a
/// fraction of one have every picture and every letter resampled, differently on each
/// frame of a scroll.
fn snapped(position: f64, scale: f64) -> f64 {
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
        grid::layout::{row_index_at, row_of_item},
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
        fn build(&self, _id: i64) -> Building<'_> {
            Box::pin(async { Err(LoadError::Unavailable) })
        }
    }

    /// An index of folders holding `counts` photos each. A photo's id is its place plus
    /// one, and its thumbnail key its id.
    fn index(counts: &[usize]) -> GridIndex {
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
        GridIndex::build(entries, Layout::Folders)
    }

    struct Fixture {
        ctx: egui::Context,
        view: GridView,
        thumbs: Thumbs,
        index: GridIndex,
        layout_gen: u64,
        size: Vec2,
        time: f64,
    }

    /// No thumbnail can be made.
    struct NoneCanBeMade;

    impl ThumbSource for NoneCanBeMade {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            None
        }
        fn build(&self, _id: i64) -> Building<'_> {
            Box::pin(async { Err(LoadError::Failed("not a picture".to_owned())) })
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
        }
    }

    impl Fixture {
        fn frame(&mut self, events: Vec<Event>) -> GridOutput {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let folders = HashMap::new();
            let data = GridData {
                layout_gen: self.layout_gen,
                index: &self.index,
                folders: &folders,
                size: GridTile::Medium,
                zone: &TimeZone::UTC,
            };
            let (view, thumbs) = (&mut self.view, &mut self.thumbs);
            let mut output = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| output = Some(view.show(ui, &data, thumbs)));
            });
            full.textures_delta.clear();
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

    #[test]
    fn a_page_key_moves_a_viewport_less_a_row_and_an_arrow_forty_pixels() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // Rows of 195px: a 600px viewport less a row is 405.
        assert_eq!(f.key(Key::PageDown).position, 405.0);
        assert_eq!(f.key(Key::ArrowDown).position, 445.0);
        assert_eq!(f.key(Key::ArrowUp).position, 405.0);
        assert_eq!(f.key(Key::PageUp).position, 0.0);
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the pictures never came");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.texture(1).is_some());
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
}
