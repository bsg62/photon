//! The sidebar, drawn: the entries of `list.rs` that are in view, at a position this view
//! keeps, and which of them was clicked.
//!
//! Its position is a number held here and not egui's: the sidebar that is hidden is not
//! drawn at all, and comes back where it was because nothing forgot the number. In the
//! Svelte UI it has to stay laid out, unseen, to keep its scroll position.

use super::{
    list::{ABOVE, BELOW, Count, Detail, Entry, Group, List, Stack, What, height, into_view},
    rows::{Fixed, ROW},
};
use crate::{
    grid::{labels::grouped, scroll::Scroll, view::snapped},
    icons::Icon,
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Align2, Color32, Painter, Rect, Response, Sense, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

/// What a row is inset by from the panel's left edge.
const INSET: f32 = 6.0;
/// The room at the right edge, which is the scrollbar's when there is somewhere to scroll.
/// Always taken: a note is wrapped to the width that is left, and a bar that came and went
/// with the overflow would change the height of what overflows.
const BAR: f32 = 8.0;
const THUMB: f32 = 4.0;
const ICON: f32 = 14.0;
const SMALL_ICON: f32 = 12.0;
/// Where a row under a heading starts its name.
const INDENT: f32 = 28.0;
/// A note's words: thirty-four points in from the left, fourteen from the right, two under
/// the row above and six over the one below.
const NOTE_LEFT: f32 = 34.0;
const NOTE_RIGHT: f32 = 14.0;
const NOTE_ABOVE: f32 = 2.0;
const NOTE_BELOW: f32 = 6.0;
/// Where a year's heading starts, and where its line's middle is under the room above it.
const YEAR_LEFT: f32 = 14.0;
const YEAR_MIDDLE: f32 = 20.0;

pub struct SidebarData<'a> {
    pub list: &'a List,
    /// The folder at the top of the grid, whose entry is marked and kept in sight.
    pub here: Option<i64>,
}

#[derive(Default)]
pub struct SidebarView {
    scroll: Scroll,
    /// Where each entry is, stacked again every frame the sidebar is drawn: a sum over
    /// the entries, and nothing to keep in step with the list, the width and the scale.
    stack: Stack,
    /// The folder the list last followed the grid to, or was kept from following to.
    followed: Option<i64>,
    /// Where on the thumb the pointer took hold, while the scrollbar is held.
    grab: Option<f64>,
}

fn icon(what: Fixed) -> Option<Icon> {
    Some(match what {
        Fixed::All => Icon::LayoutGrid,
        Fixed::Starred => Icon::Star,
        Fixed::Recent => Icon::Clock,
        Fixed::OnThisDay => Icon::Calendar,
        Fixed::Videos => Icon::Play,
        Fixed::Duplicates => Icon::Copy,
        Fixed::Hidden => Icon::EyeOff,
        Fixed::CopiesOf => return None,
    })
}

fn group_icon(group: Group) -> Icon {
    match group {
        Group::Albums => Icon::Folder,
        Group::Searches => Icon::Bookmark,
        Group::People => Icon::User,
        Group::Tags => Icon::Tag,
    }
}

fn count_text(count: Count) -> String {
    match count {
        Count::Of(number) => grouped(number),
        Count::ToName(number) => format!("{} to name", grouped(number)),
    }
}

fn note_font() -> egui::FontId {
    fonts::regular(T[1])
}

/// What draws an entry: the panel it is in, and where the grid is.
struct Frame<'a> {
    painter: &'a Painter,
    /// The sidebar's own area, which no row takes a press outside of.
    panel: Rect,
    palette: &'a Palette,
    here: Option<i64>,
}

impl SidebarView {
    /// Where the list is.
    pub fn position(&self) -> f64 {
        self.scroll.position()
    }

    /// Draws the sidebar in `rect` and answers the entry that was clicked.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, data: &SidebarData<'_>) -> Option<What> {
        let palette = palette(ui.ctx());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color(palette.chrome));

        let list = data.list;
        self.measure(ui, list, rect.width());
        let viewport = f64::from(rect.height()).max(0.0);
        self.scroll.set_extent(self.stack.total(), viewport);
        let over = ui.rect_contains_pointer(rect);
        self.follow(list, data.here, over, viewport);
        let bar = Rect::from_min_max(pos2(rect.right() - BAR, rect.top()), rect.max);
        self.take_input(ui, bar, over);

        let top = snapped(self.scroll.position(), f64::from(ui.pixels_per_point()));
        let frame = Frame {
            painter: &painter,
            panel: rect,
            palette,
            here: data.here,
        };
        let mut clicked = None;
        for index in self.stack.range(top, top + viewport) {
            let entry = &list.entries[index];
            let band = Rect::from_min_max(
                pos2(
                    rect.left(),
                    rect.top() + (self.stack.top(index) - top) as f32,
                ),
                pos2(
                    rect.right() - BAR,
                    rect.top() + (self.stack.bottom(index) - top) as f32,
                ),
            );
            if draw(ui, &frame, band, entry) {
                clicked = Some(entry.what.clone());
            }
        }
        self.draw_scrollbar(&painter, bar, palette);
        clicked
    }

    /// Stacks the entries: each as tall as its kind, and a note as tall as its words are
    /// when wrapped to the list.
    fn measure(&mut self, ui: &egui::Ui, list: &List, width: f32) {
        let room = (width - NOTE_LEFT - NOTE_RIGHT).max(0.0);
        let heights = list.entries.iter().map(|entry| {
            height(&entry.what).unwrap_or_else(|| {
                let words =
                    ui.painter()
                        .layout(entry.label.clone(), note_font(), Color32::WHITE, room);
                f64::from(NOTE_ABOVE + words.size().y + NOTE_BELOW)
            })
        });
        self.stack = Stack::new(heights, ABOVE, BELOW);
    }

    /// The list follows the grid: when the folder at the top of the grid becomes another,
    /// its entry is brought into view by the least movement.
    ///
    /// Not while the pointer is over the list - then it is the user's, and a list that
    /// moved under the pointer would put another row under a click already on its way.
    /// Not "while the list has the focus": a clicked folder keeps it, and the list would
    /// then stop following the very scroll that click began. Asked once for each folder,
    /// as the Svelte effect runs once for each: a list the user has scrolled away is not
    /// pulled back until the grid reaches another folder.
    fn follow(&mut self, list: &List, here: Option<i64>, over: bool, viewport: f64) {
        if here == self.followed {
            return;
        }
        self.followed = here;
        if over {
            return;
        }
        let Some(index) = here.and_then(|folder| list.folder(folder)) else {
            return;
        };
        let (top, bottom) = (self.stack.top(index), self.stack.bottom(index));
        if let Some(to) = into_view(top, bottom, self.scroll.position(), viewport) {
            self.scroll.set(to);
        }
    }

    fn take_input(&mut self, ui: &egui::Ui, bar: Rect, over: bool) {
        if over {
            // Positive moves the content down, which is towards the top of the list.
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.scroll.scroll_by(-f64::from(delta));
            }
        }
        let track = f64::from(bar.height());
        let Some((start, length)) = self.scroll.thumb(track) else {
            self.grab = None;
            return;
        };
        // As the grid's: a press on the thumb holds it where it was taken, a press on the
        // track brings the thumb's middle under the pointer.
        let response = ui.interact(
            bar,
            ui.id().with("sidebar-scrollbar"),
            Sense::click_and_drag(),
        );
        let held = response
            .is_pointer_button_down_on()
            .then(|| response.interact_pointer_pos())
            .flatten();
        match held {
            Some(pointer) => {
                let y = f64::from(pointer.y - bar.top());
                let on_thumb = (start..=start + length).contains(&y);
                let grab =
                    *self
                        .grab
                        .get_or_insert(if on_thumb { y - start } else { length / 2.0 });
                self.scroll
                    .set(self.scroll.position_for_thumb(track, y - grab));
            }
            None => self.grab = None,
        }
    }

    fn draw_scrollbar(&self, painter: &Painter, bar: Rect, palette: &Palette) {
        let Some((start, length)) = self.scroll.thumb(f64::from(bar.height())) else {
            return;
        };
        let thumb = Rect::from_min_size(
            pos2(bar.center().x - THUMB / 2.0, bar.top() + start as f32),
            vec2(THUMB, length as f32),
        );
        let tint = if self.grab.is_some() {
            palette.text_dim
        } else {
            palette.field_hover
        };
        painter.rect_filled(thumb, THUMB / 2.0, color(tint));
    }
}

/// Something of the list that takes a press: `rect` of it, under the entry's own name.
/// `current` is whether it is where the user is, for an entry that can be: it is said to
/// whoever reads the window without seeing its fill.
fn button(
    ui: &egui::Ui,
    frame: &Frame<'_>,
    rect: Rect,
    entry: &Entry,
    label: &str,
    current: Option<bool>,
) -> Response {
    let response = ui.interact(
        rect.intersect(frame.panel),
        ui.id().with(("sidebar", &entry.what)),
        Sense::click(),
    );
    response.widget_info(|| match current {
        Some(current) => WidgetInfo::selected(WidgetType::Button, true, current, label),
        None => WidgetInfo::labeled(WidgetType::Button, true, label),
    });
    response
}

fn hinted(response: Response, hint: &str) -> Response {
    if hint.is_empty() {
        response
    } else {
        response.on_hover_text(hint)
    }
}

/// The number at the right of `row`, and where what is left of the row ends.
fn draw_count(frame: &Frame<'_>, row: Rect, entry: &Entry) -> f32 {
    let right = row.right() - S[1];
    let Some(count) = entry.count else {
        return right;
    };
    // Dimmed, except over the fill of the entry that is shown, where the dimmed colour
    // does not have the contrast.
    let tint = if entry.active {
        frame.palette.text
    } else {
        frame.palette.text_dim
    };
    let drawn = frame.painter.text(
        pos2(right, row.center().y),
        Align2::RIGHT_CENTER,
        count_text(count),
        fonts::regular(T[0]),
        color(tint),
    );
    drawn.left() - S[1]
}

/// Draws one entry in `band` - the list's whole width, less the scrollbar's room - and
/// answers whether it was clicked.
fn draw(ui: &mut egui::Ui, frame: &Frame<'_>, band: Rect, entry: &Entry) -> bool {
    let palette = frame.palette;
    let painter = frame.painter;
    let text = color(palette.text);
    let dim = color(palette.text_dim);
    // A row is inset from the panel's edge and as tall as a row, at the bottom of its band.
    let row = Rect::from_min_max(pos2(band.left() + INSET, band.bottom() - ROW), band.max);
    let middle = row.center().y;
    let square = |left: f32, size: f32| {
        Rect::from_min_size(pos2(left, middle - size / 2.0), Vec2::splat(size))
    };

    match &entry.what {
        What::Fixed(fixed) => {
            // The entry that names the photo whose copies are shown does nothing on a
            // click - the view is already open - so it must not offer one.
            let pressable = *fixed != Fixed::CopiesOf;
            let response = if pressable {
                button(ui, frame, row, entry, &entry.label, Some(entry.active))
            } else {
                let rect = row.intersect(frame.panel);
                let response =
                    ui.interact(rect, ui.id().with(("sidebar", &entry.what)), Sense::hover());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &entry.label));
                response
            };
            let response = hinted(response, &entry.hint);
            if entry.active {
                painter.rect_filled(row, R[2], color(palette.accent_soft));
            } else if pressable && response.hovered() {
                painter.rect_filled(row, R[2], color(palette.hover));
            }
            let left = match icon(*fixed) {
                Some(icon) => {
                    icon.paint(ui, square(row.left() + S[1], ICON), ICON, false, text);
                    row.left() + S[1] + ICON + S[1]
                }
                // No icon: the name starts where a row under a heading starts.
                None => row.left() + INDENT,
            };
            let right = draw_count(frame, row, entry);
            paint_line(
                ui,
                painter,
                (left, middle),
                right - left,
                &entry.label,
                fonts::regular(T[2]),
                text,
            );
            pressable && response.clicked()
        }
        What::Group(group) => {
            let open = entry.detail == Detail::Open(true);
            // People's heading is two things, the fold and the page, laid out to sit
            // exactly where a one-button heading puts its chevron and its icon. The page
            // comes with a later part of the native interface: its label is drawn where it
            // will be, and takes no press yet. The other headings fold where they are
            // clicked, anywhere along the row.
            let people = *group == Group::People;
            let fold_end = row.left() + S[1] + SMALL_ICON + S[0];
            let pressed = if people {
                Rect::from_min_max(row.min, pos2(fold_end, row.bottom()))
            } else {
                row
            };
            let name = match (people, open) {
                (true, true) => "Hide people",
                (true, false) => "Show people",
                (false, _) => entry.label.as_str(),
            };
            let response = button(ui, frame, pressed, entry, name, None);
            let response = if people {
                hinted(response, name)
            } else {
                response
            };
            if response.hovered() {
                painter.rect_filled(pressed, R[2], color(palette.hover));
            }
            let chevron = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            let mut left = row.left() + S[1];
            chevron.paint(ui, square(left, SMALL_ICON), SMALL_ICON, false, dim);
            left += SMALL_ICON + S[1];
            group_icon(*group).paint(ui, square(left, ICON), ICON, false, dim);
            left += ICON + S[1];
            let right = draw_count(frame, row, entry);
            let font = fonts::semibold(ui.ctx(), T[0]);
            paint_line(
                ui,
                painter,
                (left, middle),
                right - left,
                &entry.label,
                font,
                dim,
            );
            response.clicked()
        }
        What::Album(_) | What::Search { .. } | What::Person(_) | What::Tag(_) | What::Folder(_) => {
            let response = hinted(
                button(ui, frame, row, entry, &entry.label, Some(entry.active)),
                &entry.hint,
            );
            if entry.active {
                painter.rect_filled(row, R[2], color(palette.accent_soft));
            } else if response.hovered() {
                painter.rect_filled(row, R[2], color(palette.hover));
            }
            // Where the grid is: a bar in the row's indent, beside the name. Not the fill,
            // which says which view the grid shows - both are on screen at once.
            if matches!(entry.what, What::Folder(id) if frame.here == Some(id)) {
                let bar = Rect::from_min_max(
                    pos2(row.left() + 12.0, row.top() + 7.0),
                    pos2(row.left() + 15.0, row.bottom() - 7.0),
                );
                painter.rect_filled(bar, 2.0, color(palette.accent));
            }
            let left = row.left() + INDENT;
            let mut right = draw_count(frame, row, entry);
            let picasa = entry.detail == Detail::Picasa;
            if picasa {
                // The mark stands after the name, and the name is cut short for it.
                right -= S[1] + S[0] + SMALL_ICON;
            }
            let used = paint_line(
                ui,
                painter,
                (left, middle),
                right - left,
                &entry.label,
                fonts::regular(T[2]),
                text,
            );
            if picasa {
                let at = square(left + used + S[1] + S[0], SMALL_ICON);
                Icon::Images.paint(ui, at, SMALL_ICON, false, dim);
            }
            response.clicked()
        }
        What::Year(_) => {
            let font = fonts::semibold(ui.ctx(), T[0]);
            let at = (band.left() + YEAR_LEFT, band.top() + YEAR_MIDDLE);
            let room = band.right() - band.left() - 2.0 * YEAR_LEFT;
            paint_line(ui, painter, at, room, &entry.label, font, dim);
            false
        }
        What::Note(_) => {
            let room = (frame.panel.width() - NOTE_LEFT - NOTE_RIGHT).max(0.0);
            let words = painter.layout(entry.label.clone(), note_font(), dim, room);
            painter.galley(
                pos2(band.left() + NOTE_LEFT, band.top() + NOTE_ABOVE),
                words,
                dim,
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nav::Place,
        sidebar::{
            list::{Collections, Held, Sources},
            rows::{Counts, Today},
        },
        window_layout::OpenGroups,
    };
    use eframe::egui::{
        Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchPhase,
    };
    use jiff::tz::TimeZone;
    use photon_core::{
        grid::{FolderTally, GridView},
        library::{AlbumSummary, Folder, Person, SavedSearch, TagCount},
        sort::Sort,
    };

    const TODAY: Today = Today { month: 7, day: 4 };
    /// Noon UTC on 2024-06-01.
    const IN_2024: i64 = 1_717_243_200;
    const WIDTH: f32 = 260.0;

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// What a list is built from, owned.
    struct World {
        at: Place,
        open: OpenGroups,
        collections: Collections,
        folders: Vec<Folder>,
        tallies: Vec<FolderTally>,
    }

    impl World {
        /// A library of `folders` folders, each a day older than the one before it, with
        /// an album of its own, one of Picasa's, a saved search, a person and a keyword.
        fn new(folders: usize) -> Self {
            let ids = 1..=folders as i64;
            Self {
                at: Place::of(GridView::All),
                open: OpenGroups {
                    albums: true,
                    searches: true,
                    people: true,
                    tags: true,
                },
                collections: Collections {
                    albums: vec![
                        AlbumSummary {
                            id: 4,
                            name: "Best of".to_owned(),
                            count: 1200,
                            picasa: false,
                        },
                        AlbumSummary {
                            id: 9,
                            name: "Scans".to_owned(),
                            count: 3,
                            picasa: true,
                        },
                    ],
                    searches: vec![SavedSearch {
                        id: 2,
                        name: "Lakes".to_owned(),
                        query: "lake 2024".to_owned(),
                        created_ms: 0,
                    }],
                    people: vec![Person {
                        key: "p:7".to_owned(),
                        name: "Anna".to_owned(),
                        count: 31,
                    }],
                    to_name: 0,
                    tags: vec![TagCount {
                        tag: "coast".to_owned(),
                        count: 5,
                        total: 5,
                    }],
                },
                folders: ids
                    .clone()
                    .map(|id| Folder {
                        id,
                        watched_id: 1,
                        parent_id: None,
                        path: format!("/photos/folder-{id:04}"),
                        name: format!("folder-{id:04}"),
                        hidden: false,
                        alias: None,
                    })
                    .collect(),
                tallies: ids
                    .map(|id| FolderTally {
                        folder_id: id,
                        count: 10,
                        taken_at_min: IN_2024 - id * 86_400,
                        bytes: 0,
                        modified_ms: 0,
                    })
                    .collect(),
            }
        }

        fn list(&self) -> List {
            let mut held = Held::default();
            held.set_collections(self.collections.clone());
            held.set_folders(self.folders.clone());
            let mut list = List::default();
            list.follow(&Sources {
                counts: &Counts::default(),
                at: &self.at,
                today: TODAY,
                open: self.open,
                sort: Sort::default(),
                held: &held,
                tallies: &self.tallies,
                layout_gen: 1,
                zone: &TimeZone::UTC,
            });
            list
        }
    }

    /// The sidebar in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        view: SidebarView,
        world: World,
        list: List,
        here: Option<i64>,
        size: Vec2,
        time: f64,
        /// Room left above the sidebar and to its right, as the bars and the grid take.
        above: f32,
        beside: f32,
        /// Every text the last frame drew, with where and in which colour.
        texts: Vec<(String, Rect, Color32)>,
        /// Every filled rectangle the last frame drew, and every picture - an icon is
        /// one - with the texture it is drawn from.
        fills: Vec<(Rect, Color32)>,
        icons: Vec<(Rect, egui::TextureId)>,
        /// The top of every clip rectangle anything was drawn in.
        clip_tops: Vec<f32>,
    }

    impl Fixture {
        fn new(folders: usize) -> Self {
            let world = World::new(folders);
            Self {
                ctx: egui::Context::default(),
                view: SidebarView::default(),
                list: world.list(),
                world,
                here: None,
                size: vec2(WIDTH, 600.0),
                time: 0.0,
                above: 0.0,
                beside: 0.0,
                texts: Vec::new(),
                fills: Vec::new(),
                icons: Vec::new(),
                clip_tops: Vec::new(),
            }
        }

        /// Builds the list again from the world as the test has changed it.
        fn rebuild(&mut self) {
            self.list = self.world.list();
        }

        fn frame(&mut self, events: Vec<Event>) -> Option<What> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = SidebarData {
                list: &self.list,
                here: self.here,
            };
            let (view, above, beside) = (&mut self.view, self.above, self.beside);
            let mut clicked = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let window = ui.max_rect();
                        let rect = Rect::from_min_max(
                            pos2(window.left(), window.top() + above),
                            pos2(window.right() - beside, window.bottom()),
                        );
                        clicked = view.show(ui, rect, &data);
                    });
            });
            full.textures_delta.clear();
            self.texts.clear();
            self.fills.clear();
            self.icons.clear();
            self.clip_tops.clear();
            for clipped in &full.shapes {
                let clip_top = clipped.clip_rect.top();
                each_shape(&clipped.shape, &mut |shape| {
                    // The panel's own background, which is not the sidebar's doing.
                    if matches!(shape, egui::Shape::Rect(rect) if rect.rect.size() == self.size) {
                        return;
                    }
                    self.clip_tops.push(clip_top);
                    match shape {
                        egui::Shape::Text(text) => {
                            let tint = (text.galley.job.sections.first())
                                .map_or(Color32::PLACEHOLDER, |section| section.format.color);
                            let place = text.visual_bounding_rect();
                            self.texts
                                .push((text.galley.text().to_owned(), place, tint));
                        }
                        egui::Shape::Rect(rect) if rect.brush.is_some() => {
                            let texture = rect.brush.as_ref().unwrap().fill_texture_id;
                            self.icons.push((rect.rect, texture));
                        }
                        egui::Shape::Rect(rect) => self.fills.push((rect.rect, rect.fill)),
                        _ => {}
                    }
                });
            }
            clicked
        }

        fn pointer(&mut self, at: Pos2) -> Option<What> {
            self.frame(vec![Event::PointerMoved(at)])
        }

        fn button(&mut self, at: Pos2, pressed: bool) -> Option<What> {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        fn click(&mut self, at: Pos2) -> Option<What> {
            self.pointer(at);
            self.button(at, true);
            self.button(at, false)
        }

        /// Turns the wheel by `delta` over `at` and draws the frames egui spreads it over.
        fn wheel(&mut self, at: Pos2, delta: f32) {
            self.pointer(at);
            self.frame(vec![Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, delta),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
            for _ in 0..60 {
                self.frame(Vec::new());
            }
        }

        fn index(&self, what: &What) -> usize {
            (self.list.entries.iter())
                .position(|entry| entry.what == *what)
                .unwrap_or_else(|| panic!("no entry {what:?}"))
        }

        /// Where the entry is drawn: its band, in the window.
        fn band(&self, what: &What) -> Rect {
            let index = self.index(what);
            let top = self.view.position() - f64::from(self.above);
            let right = self.size.x - self.beside - BAR;
            Rect::from_min_max(
                pos2(0.0, (self.view.stack.top(index) - top) as f32),
                pos2(right, (self.view.stack.bottom(index) - top) as f32),
            )
        }

        /// The middle of the entry's row.
        fn middle(&self, what: &What) -> Pos2 {
            let band = self.band(what);
            pos2(100.0, band.bottom() - ROW / 2.0)
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, ..)| drawn == text)
                .map(|(_, place, _)| *place)
        }

        fn tint_of(&self, text: &str) -> Color32 {
            (self.texts.iter())
                .find(|(drawn, ..)| drawn == text)
                .map(|(.., tint)| *tint)
                .unwrap_or_else(|| panic!("{text} is not drawn"))
        }

        /// The pictures drawn on the entry's row.
        fn icons_on(&self, what: &What) -> Vec<(Rect, egui::TextureId)> {
            let row = self.middle(what).y;
            (self.icons.iter().copied())
                .filter(|(icon, _)| (icon.center().y - row).abs() < 2.0)
                .collect()
        }

        fn filled(&self, tint: Color32) -> Vec<Rect> {
            (self.fills.iter())
                .filter(|(_, fill)| *fill == tint)
                .map(|(rect, _)| *rect)
                .collect()
        }

        fn tint(&self, of: impl Fn(&Palette) -> crate::theme::tokens::Rgba) -> Color32 {
            color(of(palette(&self.ctx)))
        }
    }

    fn folder(id: i64) -> What {
        What::Folder(id)
    }

    fn search() -> What {
        What::Search {
            id: 2,
            query: "lake 2024".to_owned(),
        }
    }

    #[test]
    fn every_entry_is_drawn_where_its_height_puts_it() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        // Four rows of 28 under 8, then a heading with its own 8 above it.
        let all = f.drew("All photos").expect("drawn");
        assert!((all.center().y - 22.0).abs() < 2.0, "{all:?}");
        let albums = f.drew("Albums").expect("drawn");
        assert!(
            (albums.center().y - (8.0 + 4.0 * 28.0 + 8.0 + 14.0)).abs() < 2.0,
            "{albums:?}"
        );
        for (label, what) in [
            ("Best of", What::Album(4)),
            ("Lakes", search()),
            ("Anna", What::Person("p:7".to_owned())),
            ("coast", What::Tag("coast".to_owned())),
            ("folder-0002", folder(2)),
        ] {
            let drawn = f
                .drew(label)
                .unwrap_or_else(|| panic!("{label} is not drawn"));
            let row = f.middle(&what);
            assert!(
                (drawn.center().y - row.y).abs() < 2.0,
                "{label}: {drawn:?} at {row:?}"
            );
            // Under its heading: the name starts at the indent.
            assert!(
                (drawn.left() - (INSET + INDENT)).abs() < 2.0,
                "{label}: {drawn:?}"
            );
        }
        // A year stands over its folders, further left than they do.
        let year = f.drew("2024").expect("the year is drawn");
        let band = f.band(&What::Year(2024));
        assert!(year.top() > band.top() + 10.0 && year.bottom() <= band.bottom());
        assert!((year.left() - YEAR_LEFT).abs() < 2.0);
    }

    #[test]
    fn a_count_is_at_the_right_and_faces_to_name_are_said_in_words() {
        let mut f = Fixture::new(1);
        f.frame(Vec::new());
        let count = f.drew("1,200").expect("the album's count");
        assert!(
            (count.right() - (WIDTH - BAR - S[1])).abs() < 2.0,
            "{count:?}"
        );
        assert!((count.center().y - f.middle(&What::Album(4)).y).abs() < 2.0);
        // People's own count is the number of people, until there are faces to name.
        assert!(f.drew("1 to name").is_none());
        f.world.collections.to_name = 1234;
        f.rebuild();
        f.frame(Vec::new());
        let waiting = f.drew("1,234 to name").expect("the faces waiting");
        assert!((waiting.center().y - f.middle(&What::Group(Group::People)).y).abs() < 2.0);
    }

    #[test]
    fn a_click_on_an_entry_answers_it() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        for what in [
            What::Fixed(Fixed::Starred),
            What::Album(9),
            search(),
            What::Person("p:7".to_owned()),
            What::Tag("coast".to_owned()),
            folder(3),
            What::Group(Group::Albums),
            What::Group(Group::Tags),
        ] {
            let at = f.middle(&what);
            assert_eq!(f.click(at), Some(what.clone()), "{what:?}");
        }
    }

    // A year is a heading and a note is words. (The few points between two rows are not
    // tried: egui gives a press that lands beside a button to the nearest one in reach.)
    #[test]
    fn what_is_not_a_button_answers_no_click() {
        let mut f = Fixture::new(3);
        f.world.collections.people.clear();
        f.rebuild();
        f.frame(Vec::new());
        let year = f.band(&What::Year(2024));
        assert_eq!(f.click(pos2(40.0, year.top() + 14.0)), None);
        let note = f.band(&What::Note(Group::People));
        assert_eq!(f.click(note.center()), None);
    }

    // Its chevron folds the list; its label is the People page's, which is not built yet.
    #[test]
    fn people_fold_at_their_chevron_and_their_label_takes_no_press() {
        let mut f = Fixture::new(1);
        f.frame(Vec::new());
        let people = What::Group(Group::People);
        let row = f.middle(&people);
        assert_eq!(
            f.click(pos2(INSET + S[1] + 6.0, row.y)),
            Some(people.clone())
        );
        assert_eq!(f.click(pos2(120.0, row.y)), None);
        // Another heading folds wherever it is clicked.
        let tags = What::Group(Group::Tags);
        let row = f.middle(&tags);
        assert_eq!(f.click(pos2(120.0, row.y)), Some(tags));
    }

    #[test]
    fn the_entry_of_the_view_shown_is_filled_and_its_count_is_not_dimmed() {
        let mut f = Fixture::new(1);
        f.world.at = Place {
            view: GridView::Album,
            arg: "4".to_owned(),
        };
        f.rebuild();
        f.frame(Vec::new());
        let soft = f.tint(|palette| palette.accent_soft);
        let filled = f.filled(soft);
        assert_eq!(filled.len(), 1, "one entry is where the user is");
        let row = f.middle(&What::Album(4));
        assert!(filled[0].contains(row), "{:?} is not at {row:?}", filled[0]);
        // Inset from the panel's edge, and clear of the scrollbar's room.
        assert_eq!((filled[0].left(), filled[0].right()), (INSET, WIDTH - BAR));
        // Over that fill the dimmed colour does not have the contrast.
        assert_eq!(f.tint_of("1,200"), f.tint(|palette| palette.text));
        assert_eq!(f.tint_of("3"), f.tint(|palette| palette.text_dim));
    }

    #[test]
    fn the_row_under_the_pointer_is_lit_and_the_one_shown_keeps_its_own_fill() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let hover = f.tint(|palette| palette.hover);
        assert_eq!(f.filled(hover), []);
        for what in [
            What::Fixed(Fixed::Starred),
            What::Album(9),
            folder(2),
            What::Group(Group::Tags),
        ] {
            let at = f.middle(&what);
            f.pointer(at);
            f.frame(Vec::new());
            let lit = f.filled(hover);
            assert_eq!(lit.len(), 1, "{what:?}");
            assert!(lit[0].contains(at), "{what:?}");
        }
        // All photos is where the user is: filled as that, and not lit over it.
        let all = f.middle(&What::Fixed(Fixed::All));
        f.pointer(all);
        f.frame(Vec::new());
        assert_eq!(f.filled(hover), []);
        // People's heading is lit at its chevron alone: its label takes no press.
        let people = f.middle(&What::Group(Group::People));
        f.pointer(pos2(INSET + S[1] + 6.0, people.y));
        f.frame(Vec::new());
        let lit = f.filled(hover);
        assert_eq!(lit.len(), 1);
        assert!(lit[0].width() < 40.0, "{:?}", lit[0]);
        f.pointer(pos2(120.0, people.y));
        f.frame(Vec::new());
        assert_eq!(f.filled(hover), []);
    }

    #[test]
    fn the_folder_the_grid_is_in_is_marked_with_a_bar_and_not_filled() {
        let mut f = Fixture::new(3);
        f.here = Some(2);
        f.frame(Vec::new());
        let accent = f.tint(|palette| palette.accent);
        let bars = f.filled(accent);
        assert_eq!(bars.len(), 1);
        let row = f.middle(&folder(2));
        assert!((bars[0].center().y - row.y).abs() < 1.0);
        assert_eq!(bars[0].width(), 3.0);
        assert!(
            bars[0].right() < INSET + INDENT,
            "in the indent, beside the name"
        );
        // The fill is the view's: All photos, at the top.
        let soft = f.filled(f.tint(|palette| palette.accent_soft));
        assert_eq!(soft.len(), 1);
        assert!(soft[0].contains(f.middle(&What::Fixed(Fixed::All))));
        // Nowhere, when the grid is in no folder.
        f.here = None;
        f.frame(Vec::new());
        assert_eq!(f.filled(accent), []);
    }

    #[test]
    fn a_heading_has_its_chevron_and_its_icon_and_picasas_album_its_mark() {
        let mut f = Fixture::new(1);
        f.frame(Vec::new());
        let albums = What::Group(Group::Albums);
        let heading = f.icons_on(&albums);
        assert_eq!(heading.len(), 2, "a chevron and the group's icon");
        assert_eq!(f.icons_on(&What::Album(4)).len(), 0);
        let marked = f.icons_on(&What::Album(9));
        assert_eq!(marked.len(), 1);
        // After the name, not at the row's edge.
        let name = f.drew("Scans").unwrap();
        let mark = marked[0].0;
        assert!(mark.left() > name.right() && mark.left() < name.right() + 20.0);
        assert_eq!(f.icons_on(&What::Fixed(Fixed::Starred)).len(), 1);

        // Folded, the chevron is another picture, and the group's own icon the same one.
        let open: Vec<_> = heading.iter().map(|(_, texture)| *texture).collect();
        f.world.open.albums = false;
        f.rebuild();
        f.frame(Vec::new());
        let folded: Vec<_> = (f.icons_on(&albums).iter())
            .map(|(_, texture)| *texture)
            .collect();
        assert_ne!(folded[0], open[0], "the chevron points another way");
        assert_eq!(folded[1], open[1]);
    }

    #[test]
    fn only_the_entries_in_view_are_drawn() {
        let mut f = Fixture::new(5000);
        f.frame(Vec::new());
        assert!(f.texts.len() < 60, "{} texts drawn", f.texts.len());
        assert!(f.drew("folder-0001").is_some());
        assert!(f.drew("folder-0100").is_none());
        // And in the middle of the list, the ones that are there.
        f.view.scroll.set(f.view.stack.top(f.index(&folder(2500))));
        f.frame(Vec::new());
        assert!(f.texts.len() < 60, "{} texts drawn", f.texts.len());
        assert!(f.drew("folder-2500").is_some());
        assert!(f.drew("folder-0001").is_none());
        // Its name is under the top edge by half a row.
        let name = f.drew("folder-2500").unwrap();
        assert!((name.center().y - ROW / 2.0).abs() < 2.0, "{name:?}");
        // And a heading deep in the list is under the room above it, where the list is.
        f.view
            .scroll
            .set(f.view.stack.top(f.index(&What::Year(2020))));
        f.frame(Vec::new());
        let year = f.drew("2020").expect("the year is drawn");
        assert!((year.center().y - YEAR_MIDDLE).abs() < 2.0, "{year:?}");
    }

    #[test]
    fn the_wheel_over_the_list_moves_it_and_not_past_its_end() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        let over = pos2(100.0, 300.0);
        f.wheel(over, -120.0);
        assert!(
            (f.view.position() - 120.0).abs() < 0.5,
            "at {}",
            f.view.position()
        );
        f.wheel(over, -1_000_000.0);
        let end = f.view.stack.total() - 600.0;
        assert_eq!(f.view.position(), end);
        // The last folder is in view, and the room under it.
        let last = f.drew("folder-0200").expect("the last folder is drawn");
        assert!(last.bottom() < 600.0 - BELOW as f32 + 1.0);
        f.wheel(over, 1_000_000.0);
        assert_eq!(f.view.position(), 0.0);
    }

    // A list that fits has nowhere to go.
    #[test]
    fn a_list_that_fits_does_not_move_and_has_no_thumb() {
        let mut f = Fixture::new(2);
        f.frame(Vec::new());
        let thumb = f.tint(|palette| palette.field_hover);
        assert_eq!(f.filled(thumb), []);
        f.wheel(pos2(100.0, 300.0), -500.0);
        assert_eq!(f.view.position(), 0.0);
    }

    #[test]
    fn the_thumb_is_drawn_when_there_is_somewhere_to_scroll_and_takes_the_list_with_it() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        let tint = f.tint(|palette| palette.field_hover);
        let thumbs = f.filled(tint);
        assert_eq!(thumbs.len(), 1);
        assert_eq!(thumbs[0].top(), 0.0);
        assert!(thumbs[0].left() >= WIDTH - BAR && thumbs[0].right() <= WIDTH);
        // A press on the track brings the thumb's middle under the pointer.
        let track = pos2(WIDTH - BAR / 2.0, 300.0);
        f.pointer(track);
        f.button(track, true);
        let half = f.view.position();
        let max = f.view.stack.total() - 600.0;
        assert!((half - max / 2.0).abs() < max * 0.02, "at {half} of {max}");
        // And a drag from there takes the list along, to its end.
        f.pointer(pos2(WIDTH - BAR / 2.0, 5000.0));
        assert_eq!(f.view.position(), max);
        f.button(pos2(WIDTH - BAR / 2.0, 5000.0), false);
        f.pointer(pos2(100.0, 5000.0));
        assert_eq!(f.view.position(), max, "let go, it stays");
    }

    // As the grid scrolls through folders the list has scrolled past, the marked row is
    // kept in view, by the least movement.
    #[test]
    fn the_list_follows_the_folder_the_grid_is_in() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        // In view already: nothing moves.
        f.here = Some(3);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
        // Below the view: up until its bottom edge is at the view's.
        f.here = Some(150);
        f.frame(Vec::new());
        let bottom = f.view.stack.bottom(f.index(&folder(150)));
        assert_eq!(f.view.position(), bottom - 600.0);
        // Above it: down until its top edge is at the view's.
        f.here = Some(20);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), f.view.stack.top(f.index(&folder(20))));
        // A folder the list does not hold moves nothing.
        f.here = Some(9999);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), f.view.stack.top(f.index(&folder(20))));
    }

    // Then it is the user's: a list that moved under the pointer would put another row
    // under a click already on its way.
    #[test]
    fn the_list_does_not_follow_while_the_pointer_is_over_it() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.pointer(pos2(100.0, 300.0));
        f.here = Some(150);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
        // Nor when the pointer has left, for that folder: the user may have scrolled the
        // list away on purpose. The next folder the grid reaches is followed again.
        f.frame(vec![Event::PointerGone]);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
        f.here = Some(151);
        f.frame(Vec::new());
        assert!(f.view.position() > 0.0);
    }

    // The words of an empty group are wrapped to the list, and what is under them starts
    // where they end, at any width.
    #[test]
    fn a_note_is_wrapped_to_the_list_and_pushes_what_is_under_it_down() {
        let mut f = Fixture::new(1);
        f.world.collections.people.clear();
        f.rebuild();
        f.frame(Vec::new());
        let words = |f: &Fixture| {
            (f.texts.iter())
                .find(|(text, ..)| text.starts_with("No named people yet."))
                .map(|(_, place, _)| *place)
                .expect("the note is drawn")
        };
        let wide = words(&f);
        assert!(wide.left() >= NOTE_LEFT - 1.0 && wide.right() <= WIDTH - NOTE_RIGHT + 1.0);
        assert!(wide.height() > 30.0, "more than two lines: {wide:?}");
        let tags = f.drew("Tags").unwrap();
        assert!(
            tags.top() > wide.bottom(),
            "the next heading is under the note"
        );
        let band = f.band(&What::Note(Group::People));
        assert!(wide.top() >= band.top() && wide.bottom() <= band.bottom());

        // Narrower, the same words are taller, and the heading under them further down.
        f.size = vec2(180.0, 600.0);
        f.frame(Vec::new());
        let narrow = words(&f);
        assert!(narrow.right() <= 180.0 - NOTE_RIGHT + 1.0);
        assert!(narrow.height() > wide.height());
        assert!(f.drew("Tags").unwrap().top() > narrow.bottom());
    }

    // A fold, a new album: the list is another, and is stacked again.
    #[test]
    fn a_list_built_again_is_measured_again() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let before = f.drew("folder-0001").unwrap().top();
        f.world.open.albums = false;
        f.rebuild();
        f.frame(Vec::new());
        assert!(f.drew("Best of").is_none());
        // Two albums fewer above it.
        assert_eq!(f.drew("folder-0001").unwrap().top(), before - 2.0 * ROW);
    }

    // The grid is beside the list, and the wheel over the grid is the grid's.
    #[test]
    fn the_wheel_beside_the_list_is_not_the_lists() {
        let mut f = Fixture::new(200);
        f.size = vec2(800.0, 600.0);
        f.beside = 800.0 - WIDTH;
        f.frame(Vec::new());
        f.wheel(pos2(500.0, 300.0), -120.0);
        assert_eq!(f.view.position(), 0.0);
        f.wheel(pos2(100.0, 300.0), -120.0);
        assert!(f.view.position() > 100.0);
    }

    // A bar lies over the sidebar's top edge. A row scrolled half under it takes no press
    // there: the press is the bar's.
    #[test]
    fn a_row_takes_no_press_outside_the_sidebar() {
        let mut f = Fixture::new(200);
        f.above = 46.0;
        f.frame(Vec::new());
        // Half of the third folder's row above the sidebar's top edge.
        let third = f.view.stack.top(f.index(&folder(3)));
        f.view.scroll.set(third + 14.0);
        f.frame(Vec::new());
        let row = f.band(&folder(3));
        assert!(row.top() < 46.0 && row.bottom() > 46.0, "{row:?}");
        assert_eq!(f.click(pos2(100.0, 46.0 - 8.0)), None, "above the edge");
        assert_eq!(
            f.click(pos2(100.0, 46.0 + 7.0)),
            Some(folder(3)),
            "below it"
        );
        // And nothing of it is drawn there: everything is cut at the sidebar's edge.
        assert!(
            f.clip_tops.iter().all(|top| *top >= 46.0),
            "{:?}",
            f.clip_tops
        );
    }

    // A press on the thumb takes hold of it where it is pressed: the list does not jump
    // to put the thumb's middle under the pointer, as a press on the track does.
    #[test]
    fn a_press_on_the_thumb_holds_it_where_it_was_taken() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        let x = WIDTH - BAR / 2.0;
        let on_thumb = pos2(x, 4.0);
        f.pointer(on_thumb);
        f.button(on_thumb, true);
        assert_eq!(f.view.position(), 0.0);
        // Dragged down by a hundred points, the thumb's top is a hundred points down.
        f.pointer(pos2(x, 104.0));
        let tint = f.tint(|palette| palette.text_dim);
        let thumbs = f.filled(tint);
        assert_eq!(thumbs.len(), 1, "the thumb, held");
        assert!((thumbs[0].top() - 100.0).abs() < 1.0, "{:?}", thumbs[0]);
        f.button(pos2(x, 104.0), false);
    }

    // Rows drawn at a fraction of a pixel have every letter resampled, differently on
    // each frame of a scroll.
    #[test]
    fn the_list_is_drawn_on_whole_pixels() {
        let mut f = Fixture::new(200);
        f.world.at = Place::of(GridView::Starred);
        f.rebuild();
        f.frame(Vec::new());
        f.view.scroll.set(10.3);
        f.frame(Vec::new());
        let soft = f.filled(f.tint(|palette| palette.accent_soft));
        assert_eq!(soft.len(), 1);
        assert_eq!(soft[0].top(), 8.0 + 28.0 - 10.0);
    }

    // The entry that names the photo whose copies are shown does nothing: the view is open.
    #[test]
    fn the_entry_that_is_not_a_button_takes_no_click() {
        let mut f = Fixture::new(1);
        f.world.at = Place {
            view: GridView::Copies,
            arg: "42".to_owned(),
        };
        f.rebuild();
        f.frame(Vec::new());
        let copies = What::Fixed(Fixed::CopiesOf);
        let at = f.middle(&copies);
        assert_eq!(f.click(at), None);
        // Drawn all the same, under Duplicates and indented like a row under a heading.
        let name = f.drew("Copies of a photo").expect("drawn");
        assert!((name.left() - (INSET + INDENT)).abs() < 2.0);
    }
}
