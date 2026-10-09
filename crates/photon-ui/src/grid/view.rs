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
    /// How many tiles in view show that mark.
    pub marked: usize,
    /// Whether a picture the grid wants, in view or just outside it, is still on its way:
    /// being read, or read and not yet uploaded.
    pub loading: bool,
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
        let viewport = f64::from(area.height()).max(0.0);

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
        let loading = thumbs.frame(ui.ctx(), &wanted, defers_thumbs(motion, viewport));

        let (settled, marked) = self.draw(ui, area, data, thumbs, (first, last));
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

        GridOutput {
            on_screen,
            pinned,
            rebuilt,
            position: top,
            settled,
            marked,
            loading,
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
                        tile::paint(&clipped, rect, entry, texture, marked, palette);
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
        fn build(&self, _want: Want) -> Building<'_> {
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
        /// How long the last frame said it can wait for the next one.
        repaint_delay: Duration,
        /// Room left above the grid, as a bar over it would take.
        space_above: f32,
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
            texts: Vec::new(),
            clip_tops: Vec::new(),
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
            let (view, thumbs, space) = (&mut self.view, &mut self.thumbs, self.space_above);
            let mut output = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.add_space(space);
                        output = Some(view.show(ui, &data, thumbs));
                    });
            });
            full.textures_delta.clear();
            self.texts.clear();
            self.clip_tops.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, clipped.clip_rect, &mut |shape, clip| {
                    // The panel's own background, which is not the grid's doing.
                    if matches!(shape, egui::Shape::Rect(rect) if rect.rect.size() == self.size) {
                        return;
                    }
                    self.clip_tops.push(clip.top());
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
}
