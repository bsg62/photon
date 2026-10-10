//! The window around the grid: the top bar, the sidebar and its splitter, the status bar,
//! and the messages over them. `App.svelte`'s frame, drawn in the four areas of
//! `window_layout.rs`.
//!
//! A view: it draws what it is given and answers what the user did. What that changes is
//! the application's business.

use crate::{
    controls_bar::{self, ControlAction, ControlsBar, ControlsData},
    empty::{Panel, PanelButton},
    empty_panel,
    icons::Icon,
    search_bar::{SearchAction, SearchBar, SearchBarData},
    sidebar::{
        list::{List, What},
        view::{SidebarData, SidebarView},
    },
    status::{Bar, Line},
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, SHADOW_MENU, T},
    },
    toasts::{Kind, Toast},
    window_layout::{Area, Layout, SIDEBAR_STEP, areas, clamp_sidebar_width},
};
use eframe::egui::{
    self, CursorIcon, Event, EventFilter, Id, Key, Order, Rect, Sense, Stroke, UiBuilder,
    WidgetInfo, WidgetType, epaint::Shadow, pos2, vec2,
};
use photon_core::{library::GridTile, sort::Sort};

/// A button of the top bar: thirty points square, its icon eighteen.
const BUTTON: f32 = 30.0;
const BUTTON_ICON: f32 = 18.0;
/// The widest a message is drawn, its edge and its button included.
const TOAST_WIDTH: f32 = 420.0;
const TOAST_BUTTON: f32 = 24.0;
/// The coloured edge of a message, on its left.
const TOAST_EDGE: f32 = 3.0;

pub struct ShellData<'a> {
    pub layout: &'a Layout,
    /// The sidebar's entries, and the folder at the top of the grid, which it marks.
    pub list: &'a List,
    pub here: Option<i64>,
    /// What the search box needs to know of the search it holds.
    pub search: SearchBarData<'a>,
    /// The sort the user is going to, and the size of the tiles: what the controls at the
    /// right of the top bar say.
    pub sort: Sort,
    pub size: GridTile,
    /// The status bar's photo count, when there is one to give.
    pub count: Option<&'a str>,
    /// What photon is doing in the background, for the status bar's left.
    pub lines: &'a [Line],
    /// What an empty library says where its photos would be.
    pub panel: Option<&'a Panel>,
    /// The line shown in the middle of the content in place of photos.
    pub notice: Option<&'a str>,
    pub toasts: &'a [Toast],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The toggle in the top bar, or its key.
    ToggleSidebar,
    /// An entry of the sidebar: a view, a folder, a heading to fold.
    Row(What),
    /// Something done in the search box.
    Search(SearchAction),
    /// The sort wanted: the one the controls showed, with one field of it changed.
    Sort(Sort),
    /// The size of the tiles wanted.
    Size(GridTile),
    /// A button of the empty library's panel.
    Panel(PanelButton),
    /// The splitter moved to `width`. `store` when the user has let go of it or moved it
    /// by a key: a width is stored when it was chosen, not at every point on the way.
    Width {
        width: f32,
        store: bool,
    },
    Dismiss(u64),
}

#[derive(Default)]
pub struct Shell {
    /// How far right of the sidebar's edge the pointer took hold of the splitter, while it
    /// is held.
    grab: Option<f32>,
    /// Whether the splitter has the focus because it was pressed, not tabbed to.
    pointer_focus: bool,
    /// The sidebar's list, which keeps its place while the sidebar is hidden.
    sidebar: SidebarView,
    search: SearchBar,
    controls: ControlsBar,
}

fn rect_of(area: Area, window: Rect) -> Rect {
    Rect::from_min_max(
        window.min + vec2(area.left, area.top),
        window.min + vec2(area.right, area.bottom),
    )
}

impl Shell {
    /// Where the sidebar's list is.
    pub fn sidebar_position(&self) -> f64 {
        self.sidebar.position()
    }

    /// Whether the splitter is being dragged.
    pub fn resizing(&self) -> bool {
        self.grab.is_some()
    }

    /// Whether the panel that lists what the search box understands is open.
    pub fn search_help_open(&self) -> bool {
        self.search.help_open()
    }

    /// Draws the shell in the whole of `ui`, and `content` - the grid - in the area left
    /// for it. `search` is the search box's text, which the box edits in place. Answers
    /// what the user did, in the order they did it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &ShellData<'_>,
        search: &mut String,
        content: impl FnOnce(&mut egui::Ui),
    ) -> Vec<Action> {
        let window = ui.max_rect();
        let found = areas(window.width(), window.height(), data.layout);
        let palette = palette(ui.ctx());
        let mut actions = Vec::new();

        // Ctrl+B, or Command+B, and nothing beside it: with Shift or Alt it is another
        // chord. Not on a key held down, which would flap the sidebar, and
        // not while the splitter is held, which would take away what is being dragged.
        let pressed = ui.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key { key: Key::B, pressed: true, repeat: false, modifiers, .. }
                        if modifiers.command && !modifiers.shift && !modifiers.alt
                )
            })
        });
        if pressed && !self.resizing() {
            actions.push(Action::ToggleSidebar);
        }

        self.top_bar(
            ui,
            rect_of(found.top_bar, window),
            data,
            search,
            palette,
            &mut actions,
        );
        let sidebar = SidebarData {
            list: data.list,
            here: data.here,
        };
        if let Some(area) = found.sidebar
            && let Some(entry) = self.sidebar.show(ui, rect_of(area, window), &sidebar)
        {
            actions.push(Action::Row(entry));
        }
        if let Some(area) = found.splitter {
            self.splitter(
                ui,
                rect_of(area, window),
                window,
                data.layout,
                palette,
                &mut actions,
            );
        }
        status_bar(ui, rect_of(found.status_bar, window), data, palette);

        let place = rect_of(found.content, window);
        ui.painter_at(place)
            .rect_filled(place, 0.0, color(palette.surface));
        ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
            ui.set_clip_rect(place.intersect(ui.clip_rect()));
            content(ui);
        });
        if let Some(notice) = data.notice {
            self::notice(ui, place, notice, palette);
        }
        if let Some(panel) = data.panel
            && let Some(button) = empty_panel::show(ui, place, panel)
        {
            actions.push(Action::Panel(button));
        }
        if let Some(id) = toasts(ui, window, data.toasts, palette) {
            actions.push(Action::Dismiss(id));
        }
        actions
    }

    fn top_bar(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        data: &ShellData<'_>,
        search: &mut String,
        palette: &Palette,
        actions: &mut Vec<Action>,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color(palette.chrome));
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
        let middle = rect.center().y - 0.5;
        let at = |left: f32| {
            Rect::from_min_size(pos2(left, middle - BUTTON / 2.0), vec2(BUTTON, BUTTON))
        };

        let label = if data.layout.sidebar_hidden {
            "Show sidebar"
        } else {
            "Hide sidebar"
        };
        // "Hide sidebar (Ctrl+B)", as the Svelte button's title says it.
        let chord = if cfg!(target_os = "macos") {
            "⌘+B"
        } else {
            "Ctrl+B"
        };
        let toggle = Button {
            icon: Icon::PanelLeft,
            label,
            hint: Some(format!("{label} ({chord})")),
            enabled: true,
        };
        if bar_button(
            ui,
            at(rect.left() + S[1]),
            "toggle-sidebar",
            &toggle,
            palette,
        ) {
            actions.push(Action::ToggleSidebar);
        }
        // Settings come with a later part of the native interface: the gear is drawn where
        // it will be, so nothing moves when it starts to work, and takes no press yet.
        let gear = Button {
            icon: Icon::Settings,
            label: "Settings",
            hint: None,
            enabled: false,
        };
        let gear_at = at(rect.right() - S[1] - BUTTON);
        bar_button(ui, gear_at, "settings", &gear, palette);
        // The grid's own controls, left of the gear, as wide as what they offer.
        let controls_left = gear_at.left() - S[1] - controls_bar::width(ui);

        // The search box, between the toggle and the controls: the one thing in the bar
        // that gives way to a narrow window.
        let room = Rect::from_min_max(
            pos2(rect.left() + S[1] + BUTTON + S[1], rect.top()),
            pos2(controls_left - S[1], rect.bottom() - 1.0),
        );
        let done = self.search.show(ui, room, search, &data.search);
        actions.extend(done.into_iter().map(Action::Search));

        // After the search box: a key a list of theirs used is taken out of the frame's
        // input, and the box has read its own by then.
        let controls = ControlsData {
            sort: data.sort,
            size: data.size,
        };
        let done = self.controls.show(ui, controls_left, middle, &controls);
        actions.extend(done.into_iter().map(|action| match action {
            ControlAction::Sort(sort) => Action::Sort(sort),
            ControlAction::Size(size) => Action::Size(size),
        }));
    }

    fn splitter(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        window: Rect,
        layout: &Layout,
        palette: &Palette,
        actions: &mut Vec<Action>,
    ) {
        let id = ui.id().with("splitter");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Resize sidebar"));
        // A press gives it the focus, as a press on anything that takes the keyboard does,
        // so the arrows move it afterwards. That focus is not shown: see `lit`, below.
        if response.clicked() || response.drag_started() {
            response.request_focus();
            self.pointer_focus = true;
        }
        if !response.has_focus() {
            self.pointer_focus = false;
        }
        let shown = layout.shown_width(window.width());
        let pointer = ui.input(|input| input.pointer.interact_pos());
        // Where the button went down, not where the pointer is when the press has become
        // a drag: by then it has moved, and the edge would be left that far behind it.
        if response.drag_started()
            && let Some(pressed) = ui.input(|input| input.pointer.press_origin())
        {
            self.grab = Some(pressed.x - window.left() - shown);
        }
        // Where the pointer has the edge now. Worked out in the frame the button is let
        // go in too: the last move and the release can arrive together, and by then the
        // press is no longer a drag.
        let held = match (self.grab, pointer) {
            (Some(grab), Some(pointer)) => {
                clamp_sidebar_width(pointer.x - window.left() - grab, window.width())
            }
            _ => shown,
        };
        if self.grab.is_some() && response.dragged() && held != shown {
            actions.push(Action::Width {
                width: held,
                store: false,
            });
        }
        // The end of a drag as egui tells it: the button let go, or Escape. A release the
        // window is never told of leaves the splitter held until the next press, as it
        // leaves every drag in egui.
        if self.grab.is_some() && !response.dragged() {
            self.grab = None;
            actions.push(Action::Width {
                width: held,
                store: true,
            });
        }
        if response.has_focus() {
            // The arrows are the splitter's while it has the focus, not egui's for moving
            // the focus on.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    EventFilter {
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let step = ui.input(|input| {
                f32::from(i8::from(input.key_pressed(Key::ArrowRight)))
                    - f32::from(i8::from(input.key_pressed(Key::ArrowLeft)))
            });
            if step != 0.0 {
                let width = clamp_sidebar_width(shown + step * SIDEBAR_STEP, window.width());
                actions.push(Action::Width { width, store: true });
            }
        }

        // Lit under the pointer, while it is held, and when the keyboard has brought the
        // focus here - not while it merely still has the focus a press gave it: the Svelte
        // splitter is lit on `:focus-visible`, and one left lit after every drag reads as
        // something still going on.
        let by_keyboard = response.has_focus() && !self.pointer_focus;
        let lit = response.hovered() || response.dragged() || by_keyboard;
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let painter = ui.painter_at(rect);
        // Its own focus treatment rather than a ring: a bar five points wide cannot hold one.
        let fill = if lit { palette.accent } else { palette.chrome };
        painter.rect_filled(rect, 0.0, color(fill));
        if !lit {
            painter.vline(
                rect.left() + 0.5,
                rect.y_range(),
                (1.0, color(palette.line)),
            );
        }
    }
}

/// A button of the top bar.
struct Button<'a> {
    icon: Icon,
    /// What it is called, which changes with what it would do.
    label: &'a str,
    /// What it says when the pointer rests on it.
    hint: Option<String>,
    enabled: bool,
}

/// One button of the top bar, under `name`: the same button whatever it is called at the
/// moment, so that it keeps the focus when its label flips. Answers whether it was
/// pressed, which one that is not enabled never is.
fn bar_button(
    ui: &mut egui::Ui,
    rect: Rect,
    name: &str,
    button: &Button<'_>,
    palette: &Palette,
) -> bool {
    let sense = if button.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let mut response = ui.interact(rect, ui.id().with(("bar", name)), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, button.enabled, button.label));
    if let Some(hint) = &button.hint {
        response = response.on_hover_text(hint);
    }
    let hovered = button.enabled && response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, R[2], color(palette.hover));
    }
    let tint = if hovered {
        color(palette.text)
    } else if button.enabled {
        color(palette.text_dim)
    } else {
        // What cannot be pressed yet is there, and fainter than what can.
        color(palette.text_dim).gamma_multiply(0.5)
    };
    button.icon.paint(ui, rect, BUTTON_ICON, false, tint);
    button.enabled && response.clicked()
}

/// The bar beside a status line: a hundred and twenty points of track, six tall.
const STATUS_TRACK: f32 = 120.0;
/// Between two lines of the status bar.
const STATUS_GAP: f32 = 16.0;
/// The least of a line's words worth drawing: with less room the line is left out.
const STATUS_LEAST: f32 = 80.0;
/// How many places the bar of a first scan has along its track: one on for each report
/// of the scan, and round again.
const UNKNOWN_PLACES: u64 = 4;

fn status_bar(ui: &egui::Ui, rect: Rect, data: &ShellData<'_>, palette: &Palette) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, color(palette.chrome));
    painter.hline(rect.x_range(), rect.top() + 0.5, (1.0, color(palette.line)));
    let (font, dim) = (fonts::regular(T[1]), color(palette.text_dim));
    let middle = rect.center().y + 0.5;
    // The count first: it is the one thing here that is always wanted, and the lines
    // have what is left of it.
    let mut right = rect.right() - S[2];
    if let Some(count) = data.count {
        let drawn = painter.text(
            pos2(right, middle),
            egui::Align2::RIGHT_CENTER,
            count,
            font.clone(),
            dim,
        );
        right = drawn.left() - STATUS_GAP;
    }
    let mut left = rect.left() + S[2];
    for (index, line) in data.lines.iter().enumerate() {
        let bar = line.bar.map_or(0.0, |_| S[1] + STATUS_TRACK);
        let room = right - left - bar;
        if room < STATUS_LEAST {
            break;
        }
        let wide = painter
            .layout_no_wrap(line.label.clone(), font.clone(), dim)
            .size()
            .x;
        paint_line(
            ui,
            &painter,
            (left, middle),
            room,
            &line.label,
            font.clone(),
            dim,
        );
        let mut end = left + wide.min(room);
        if let Some(bar) = line.bar {
            let track = Rect::from_min_size(
                pos2(end + S[1], (middle - 3.0).round()),
                vec2(STATUS_TRACK, 6.0),
            );
            painter.rect_filled(track, 3.0, color(palette.line));
            let (from, width) = match bar {
                Bar::Share(share) => (0.0, STATUS_TRACK * share.clamp(0.0, 1.0) as f32),
                // No clock: it stands where the scan's last report put it.
                Bar::Unknown { beat } => {
                    let place = beat % UNKNOWN_PLACES;
                    let width = STATUS_TRACK / UNKNOWN_PLACES as f32;
                    (place as f32 * width, width)
                }
            };
            let filled =
                Rect::from_min_size(pos2(track.left() + from, track.top()), vec2(width, 6.0));
            painter.rect_filled(filled, 3.0, color(palette.accent));
            end = track.right();
        }
        // Said to whoever reads the window without seeing it, as the footer's
        // `role="status"` says it.
        let said = Rect::from_min_max(pos2(left, rect.top()), pos2(end, rect.bottom()));
        ui.interact(said, ui.id().with(("status-line", index)), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, &line.label));
        left = end + STATUS_GAP;
    }
}

/// The line an empty view shows, in the middle of where its photos would be.
fn notice(ui: &egui::Ui, place: Rect, text: &str, palette: &Palette) {
    let painter = ui.painter_at(place);
    let font = fonts::regular(T[2]);
    let dim = color(palette.text_dim);
    // Measured as egui lays it out, drawn as `text.rs` orders it: a search for a name in
    // another script is quoted in this line.
    let width = painter
        .layout_no_wrap(text.to_owned(), font.clone(), dim)
        .size()
        .x
        .min(place.width() - 2.0 * S[3]);
    let left = place.center().x - width / 2.0;
    paint_line(
        ui,
        &painter,
        (left, place.center().y),
        place.width() - 2.0 * S[3],
        text,
        font,
        dim,
    );
}

/// The messages, the newest lowest, over the bottom right of the window. Answers the one
/// whose button was pressed.
fn toasts(ui: &egui::Ui, window: Rect, list: &[Toast], palette: &Palette) -> Option<u64> {
    let mut dismissed = None;
    let mut bottom = window.bottom() - 40.0;
    let room = TOAST_WIDTH - TOAST_EDGE - 3.0 * S[2] - TOAST_BUTTON;
    for toast in list.iter().rev() {
        let galley = ui.painter().layout(
            toast.message.clone(),
            fonts::regular(T[2]),
            color(palette.text),
            room,
        );
        let size = vec2(
            TOAST_EDGE + S[2] + galley.size().x + S[2] + TOAST_BUTTON + S[2],
            galley.size().y.max(TOAST_BUTTON) + 20.0,
        );
        let place =
            Rect::from_min_size(pos2(window.right() - S[3] - size.x, bottom - size.y), size);
        bottom = place.top() - S[1];
        // Over everything, and taking its own presses: a message lies on the grid.
        egui::Area::new(Id::new(("toast", toast.id)))
            .order(Order::Foreground)
            .fixed_pos(place.min)
            .show(ui.ctx(), |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter();
                painter.add(shadow().as_shape(rect, R[1]));
                // The edge is the whole shape in its colour, with the message's own
                // surface laid over all of it but the left three points.
                let edge = match toast.kind {
                    Kind::Error => palette.danger,
                    Kind::Done => palette.accent,
                };
                painter.rect_filled(rect, R[1], color(edge));
                let face = Rect::from_min_max(pos2(rect.left() + TOAST_EDGE, rect.top()), rect.max);
                painter.rect_filled(
                    face,
                    egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: R[1] as u8,
                        se: R[1] as u8,
                    },
                    color(palette.raised),
                );
                painter.rect_stroke(
                    rect,
                    R[1],
                    Stroke::new(1.0, color(palette.line)),
                    egui::StrokeKind::Outside,
                );
                painter.galley(
                    pos2(face.left() + S[2], rect.center().y - galley.size().y / 2.0),
                    galley.clone(),
                    color(palette.text),
                );
                let button = Rect::from_center_size(
                    pos2(rect.right() - S[2] - TOAST_BUTTON / 2.0, rect.center().y),
                    vec2(TOAST_BUTTON, TOAST_BUTTON),
                );
                let response = ui.interact(button, ui.id().with("dismiss"), Sense::click());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Dismiss"));
                let tint = if response.hovered() {
                    ui.painter().rect_filled(button, R[1], color(palette.hover));
                    palette.text
                } else {
                    palette.text_dim
                };
                Icon::X.paint(ui, button, 14.0, false, color(tint));
                if response.clicked() {
                    dismissed = Some(toast.id);
                }
            });
    }
    dismissed
}

/// `--shadow-menu`, as egui draws a shadow.
fn shadow() -> Shadow {
    Shadow {
        offset: [SHADOW_MENU.x as i8, SHADOW_MENU.y as i8],
        blur: SHADOW_MENU.blur as u8,
        spread: 0,
        color: color(SHADOW_MENU.ink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nav::Place,
        sidebar::{
            list::{Held, Sources},
            rows::{Counts, Fixed, Today},
        },
        status::Bar,
        toasts::Toasts,
        window_layout::{SIDEBAR_DEFAULT, SIDEBAR_MIN, SPLITTER, STATUS_BAR, TOP_BAR},
    };
    use eframe::egui::{Modifiers, PointerButton, Pos2, RawInput};
    use photon_core::grid::GridView;

    const TODAY: Today = Today { month: 7, day: 4 };

    /// The sidebar of a library with `counts` and nothing else in it, for a user at `at`.
    fn list(counts: &Counts, at: &Place) -> List {
        let mut list = List::default();
        list.follow(&Sources {
            counts,
            at,
            today: TODAY,
            open: Default::default(),
            sort: Default::default(),
            held: &Held::default(),
            tallies: &[],
            layout_gen: 0,
            zone: &jiff::tz::TimeZone::UTC,
            no_folders: false,
        });
        list
    }

    fn row(what: Fixed) -> Action {
        Action::Row(What::Fixed(what))
    }

    /// Every shape in `shape`.
    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// The shell in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        shell: Shell,
        layout: Layout,
        list: List,
        toasts: Toasts,
        notice: Option<String>,
        lines: Vec<Line>,
        panel: Option<Panel>,
        sort: Sort,
        tile: GridTile,
        size: egui::Vec2,
        time: f64,
        /// Every text the last frame drew, with where.
        texts: Vec<(String, Rect)>,
        /// Every filled rectangle the last frame drew.
        fills: Vec<(Rect, egui::Color32)>,
        /// The area the last frame gave the content, and what it was clipped to.
        content: Rect,
        clip: Rect,
    }

    impl Fixture {
        fn new() -> Self {
            let counts = Counts {
                starred: 12,
                hidden: 3,
                ..Counts::default()
            };
            Self {
                ctx: egui::Context::default(),
                shell: Shell::default(),
                layout: Layout::default(),
                list: list(&counts, &Place::of(GridView::All)),
                toasts: Toasts::default(),
                notice: None,
                lines: Vec::new(),
                panel: None,
                sort: Sort::default(),
                tile: GridTile::Medium,
                size: vec2(1280.0, 800.0),
                time: 0.0,
                texts: Vec::new(),
                fills: Vec::new(),
                content: Rect::NOTHING,
                clip: Rect::NOTHING,
            }
        }

        /// One frame with `events`. What the user did is applied as the application
        /// applies it, so that a drag is followed from frame to frame.
        fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = ShellData {
                layout: &self.layout,
                list: &self.list,
                here: None,
                search: SearchBarData {
                    saved_as: None,
                    can_save: false,
                    in_search: false,
                },
                sort: self.sort,
                size: self.tile,
                count: Some("1,234 photos"),
                lines: &self.lines,
                panel: self.panel.as_ref(),
                notice: self.notice.as_deref(),
                toasts: self.toasts.held(),
            };
            let (shell, mut actions) = (&mut self.shell, Vec::new());
            let mut search = String::new();
            let (mut content, mut clip) = (Rect::NOTHING, Rect::NOTHING);
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        actions = shell.show(ui, &data, &mut search, |ui| {
                            content = ui.max_rect();
                            clip = ui.clip_rect();
                        });
                    });
            });
            // The font atlas and the icons: nothing here draws to a screen.
            full.textures_delta.clear();
            self.content = content;
            self.clip = clip;
            self.texts.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                    }
                    egui::Shape::Rect(rect) => self.fills.push((rect.rect, rect.fill)),
                    _ => {}
                });
            }
            for action in &actions {
                match action {
                    Action::ToggleSidebar => {
                        self.layout.sidebar_hidden = !self.layout.sidebar_hidden
                    }
                    Action::Width { width, .. } => self.layout.sidebar_width = *width,
                    Action::Dismiss(id) => self.toasts.dismiss(*id),
                    Action::Sort(sort) => self.sort = *sort,
                    Action::Size(size) => self.tile = *size,
                    Action::Row(_) | Action::Search(_) | Action::Panel(_) => {}
                }
            }
            actions
        }

        fn pointer(&mut self, at: Pos2) -> Vec<Action> {
            self.frame(vec![Event::PointerMoved(at)])
        }

        fn button(&mut self, at: Pos2, pressed: bool) -> Vec<Action> {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        /// A press and a release at `at`, and everything it did.
        fn click(&mut self, at: Pos2) -> Vec<Action> {
            let mut actions = self.pointer(at);
            actions.extend(self.button(at, true));
            actions.extend(self.button(at, false));
            actions
        }

        /// `key` going down. egui works out for itself whether that is a repeat: it is
        /// one when the key has not come up since it last went down.
        fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Action> {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        }

        fn key_up(&mut self, key: Key, modifiers: Modifiers) -> Vec<Action> {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            }])
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            self.texts
                .iter()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }

        /// Whether the splitter is drawn in the accent colour.
        fn splitter_lit(&self) -> bool {
            let accent = color(palette(&self.ctx).accent);
            self.fills
                .iter()
                .any(|(rect, fill)| rect.width() == SPLITTER && *fill == accent)
        }

        /// The middle of the sidebar row `index` from the top.
        fn row(&self, index: usize) -> Pos2 {
            pos2(
                100.0,
                TOP_BAR + S[1] + (index as f32 + 0.5) * crate::sidebar::rows::ROW,
            )
        }
    }

    #[test]
    fn the_content_is_given_what_the_bars_and_the_sidebar_leave() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(
            f.content,
            Rect::from_min_max(
                pos2(SIDEBAR_DEFAULT + SPLITTER, TOP_BAR),
                pos2(1280.0, 800.0 - STATUS_BAR)
            )
        );
        // The rows are in the sidebar, the count at the right of the status bar.
        let starred = f.drew("Starred").expect("the row is drawn");
        assert!(starred.right() < SIDEBAR_DEFAULT && starred.top() > TOP_BAR);
        let twelve = f.drew("12").expect("its count is drawn");
        assert!(twelve.right() <= SIDEBAR_DEFAULT - 6.0 && twelve.left() > starred.right());
        let count = f.drew("1,234 photos").expect("the count is drawn");
        assert!(count.top() > 800.0 - STATUS_BAR && count.right() > 1200.0);
    }

    #[test]
    fn a_click_on_a_row_asks_for_it() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.click(f.row(1)), [row(Fixed::Starred)]);
        assert_eq!(f.click(f.row(0)), [row(Fixed::All)]);
        // The fifth row of this library is Hidden: Videos and Duplicates hold nothing.
        assert_eq!(f.click(f.row(4)), [row(Fixed::Hidden)]);
        // Below the last entry there is nothing to click.
        assert_eq!(f.click(pos2(100.0, 700.0)), []);
    }

    // It names the photo whose copies are shown and does nothing: the view is open.
    #[test]
    fn the_row_that_is_not_a_button_takes_no_click() {
        let mut f = Fixture::new();
        let at = Place {
            view: GridView::Copies,
            arg: "42".to_owned(),
        };
        f.list = list(&Counts::default(), &at);
        let copies = (f.list.entries.iter())
            .position(|entry| entry.what == What::Fixed(Fixed::CopiesOf))
            .unwrap();
        f.frame(Vec::new());
        assert!(f.drew("Copies of a photo").is_some());
        assert_eq!(f.click(f.row(copies)), []);
        assert_eq!(f.click(f.row(copies - 1)), [row(Fixed::Duplicates)]);
    }

    #[test]
    fn the_toggle_and_its_key_hide_the_sidebar_and_bring_it_back() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let toggle = pos2(S[1] + BUTTON / 2.0, TOP_BAR / 2.0);
        assert_eq!(f.click(toggle), [Action::ToggleSidebar]);
        // Hidden: the content begins at the window's edge, and no row is drawn.
        f.frame(Vec::new());
        assert_eq!(f.content.left(), 0.0);
        assert_eq!(f.drew("Starred"), None);
        assert_eq!(
            f.click(f.row(1)),
            [],
            "a row that is not there takes no click"
        );

        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        f.frame(Vec::new());
        assert_eq!(f.content.left(), SIDEBAR_DEFAULT + SPLITTER);
        assert!(f.drew("Starred").is_some());
    }

    #[test]
    fn the_key_is_not_answered_held_down_or_without_its_modifier() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        // Held down, the key goes down again and again: answered each time, the sidebar
        // would flap.
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        f.key_up(Key::B, Modifiers::COMMAND);
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        f.key_up(Key::B, Modifiers::COMMAND);
        assert_eq!(f.key(Key::B, Modifiers::NONE), []);
        assert_eq!(f.key(Key::N, Modifiers::COMMAND), []);
    }

    fn line(label: &str, bar: Option<Bar>) -> Line {
        Line {
            label: label.to_owned(),
            bar,
        }
    }

    /// The bars the last frame drew in the status bar: each track, and the part of it
    /// that is filled.
    fn bars(f: &Fixture) -> Vec<(Rect, Rect)> {
        let (track, value) = (color(palette(&f.ctx).line), color(palette(&f.ctx).accent));
        let in_bar = |rect: &Rect| rect.top() > f.size.y - STATUS_BAR && rect.height() == 6.0;
        let tracks = (f.fills.iter()).filter(|(rect, fill)| in_bar(rect) && *fill == track);
        tracks
            .map(|(track, _)| {
                let filled = (f.fills.iter())
                    .find(|(rect, fill)| {
                        in_bar(rect) && *fill == value && track.contains_rect(*rect)
                    })
                    .map_or(Rect::NOTHING, |(rect, _)| *rect);
                (*track, filled)
            })
            .collect()
    }

    #[test]
    fn the_status_bar_says_what_photon_is_doing_at_its_left_each_with_its_bar() {
        let mut f = Fixture::new();
        f.lines = vec![
            line("Live updates limited.", None),
            line(
                "Scanning Pictures… 1,250 of ~5,000 files (25%)",
                Some(Bar::Share(0.25)),
            ),
            line("Finding faces: 5 of 10", Some(Bar::Share(0.5))),
        ];
        f.frame(Vec::new());
        let bar_top = 800.0 - STATUS_BAR;
        let places: Vec<Rect> = (f.lines.iter())
            .map(|line| f.drew(&line.label).expect("the line is drawn"))
            .collect();
        // In order from the left edge, all in the bar, none over the count.
        let count = f.drew("1,234 photos").unwrap();
        assert!(places[0].left() >= S[2] - 1.0 && places[0].left() < S[2] + 4.0);
        for pair in places.windows(2) {
            assert!(pair[0].right() < pair[1].left(), "{pair:?}");
        }
        // Sixteen points between one line and the next: after the words of a line that
        // has no bar, after the bar of one that has.
        assert!(places[1].left() >= places[0].right() + 15.0, "{places:?}");
        for place in &places {
            assert!(place.top() > bar_top && place.right() < count.left());
        }
        // A bar after each line that has one, and none after the notice: a hundred and
        // twenty points of track, filled as far as the line has got.
        let drawn = bars(&f);
        assert_eq!(drawn.len(), 2);
        for ((track, filled), (at, share)) in drawn.iter().zip([(1, 0.25), (2, 0.5)]) {
            assert_eq!(track.width(), 120.0);
            assert!(track.left() > places[at].right() && track.left() < places[at].right() + 16.0);
            assert_eq!(filled.left(), track.left());
            assert!((filled.width() - 120.0 * share).abs() < 0.01, "{filled:?}");
        }
        assert!(
            places[2].left() >= drawn[0].0.right() + 15.0,
            "the next line is a gap after the bar"
        );
    }

    // A first scan has nothing to be measured against, and its bar no clock of its own:
    // a part of the track that moves when the scan reports, and stands still between.
    #[test]
    fn the_bar_of_a_first_scan_moves_with_the_scans_reports_and_not_by_itself() {
        let mut f = Fixture::new();
        let scanning = |beat| {
            vec![line(
                "Scanning Pictures… files",
                Some(Bar::Unknown { beat }),
            )]
        };
        let mut seen = Vec::new();
        for beat in [1, 1, 2, 3, 4, 5] {
            f.lines = scanning(beat);
            f.frame(Vec::new());
            let (track, part) = bars(&f)[0];
            assert!(track.contains_rect(part), "{part:?} in {track:?}");
            assert!(part.width() > 20.0 && part.width() < track.width() / 2.0);
            seen.push(part.left() - track.left());
        }
        assert_eq!(seen[0], seen[1], "still while nothing is reported");
        assert_ne!(seen[1], seen[2]);
        assert_ne!(seen[2], seen[3]);
        assert_ne!(seen[3], seen[4]);
        assert_eq!(seen[5], seen[1], "and round again");
        // One place on at each report, never back but to begin again.
        assert!(seen[1] < seen[2] && seen[2] < seen[3], "{seen:?}");
    }

    // The bar is one row. What does not fit is cut short or left out; nothing is drawn
    // over the photo count, which is the one thing here that is always wanted.
    #[test]
    fn lines_too_long_for_the_bar_stop_short_of_the_count() {
        let mut f = Fixture::new();
        f.size = vec2(800.0, 500.0);
        f.lines = vec![
            line(
                "Scanning Pictures… 15,200 of ~15,000 files (100%), 1,200 new or changed",
                Some(Bar::Share(1.0)),
            ),
            line(
                "Scanning Holidays… 15,200 of ~15,000 files (100%), 1,200 new or changed",
                Some(Bar::Share(0.5)),
            ),
            line("Finding faces: 12,400 of 98,000", Some(Bar::Share(0.1))),
        ];
        f.frame(Vec::new());
        let count = f.drew("1,234 photos").expect("the count is drawn");
        assert!(count.right() > 700.0);
        let in_bar = |rect: &Rect| rect.top() > 500.0 - STATUS_BAR;
        let lines: Vec<&(String, Rect)> = (f.texts.iter())
            .filter(|(text, rect)| in_bar(rect) && text != "1,234 photos")
            .collect();
        assert!(!lines.is_empty());
        for (text, rect) in &lines {
            assert!(rect.right() < count.left() - 4.0, "{text} at {rect:?}");
        }
        for (track, _) in bars(&f) {
            assert!(track.right() < count.left() - 4.0, "{track:?}");
        }
        // The first is whole, with its bar. What has no room for its words is left out
        // with its bar: a bar beside nothing says nothing.
        assert!(f.drew(&f.lines[0].label).is_some());
        let said = lines.iter().filter(|(text, _)| !text.is_empty()).count();
        assert_eq!(bars(&f).len(), said);
        assert!((1..3).contains(&said), "{said}");
    }

    // The empty library's panel stands where the photos would be, and its one working
    // button is answered.
    #[test]
    fn the_empty_librarys_panel_is_in_the_content_and_its_button_is_answered() {
        let mut f = Fixture::new();
        f.panel = Some(Panel {
            title: "Every photo is hidden",
            text: "The library has no photo to show here.".to_owned(),
            buttons: &[PanelButton::ShowHidden],
        });
        f.frame(Vec::new());
        let title = f.drew("Every photo is hidden").expect("the title is drawn");
        assert!(f.content.contains_rect(title));
        assert!((title.center().x - f.content.center().x).abs() < 2.0);
        let button = f.drew("Show hidden photos").expect("the button is drawn");
        assert_eq!(
            f.click(button.center()),
            [Action::Panel(PanelButton::ShowHidden)]
        );
    }

    // The grid's own controls: left of the gear, and what they answer is the shell's
    // answer. The search box ends before them.
    #[test]
    fn the_controls_stand_left_of_the_gear_and_their_choices_are_answered() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let gear_left = 1280.0 - S[1] - BUTTON;
        let large = f.drew("Large").expect("the size control is drawn");
        let by_folder = f.drew("By folder").expect("the grouping is drawn");
        let date = f.drew("Date taken").expect("the sort is drawn");
        assert!(large.right() < gear_left && large.left() > by_folder.right());
        assert!(by_folder.left() > date.right());
        // The last of them ends a gap short of the gear.
        let field = color(palette(&f.ctx).field);
        let last = (f.fills.iter())
            .filter(|(rect, fill)| *fill == field && rect.top() < TOP_BAR)
            .map(|(rect, _)| rect.right())
            .fold(0.0, f32::max);
        assert_eq!(last, gear_left - S[1]);
        assert!(date.top() > 0.0 && date.bottom() < TOP_BAR);
        // The search field's own fill ends short of them.
        let search = (f.fills.iter())
            .filter(|(rect, fill)| *fill == field && rect.left() < 100.0)
            .map(|(rect, _)| *rect)
            .next()
            .expect("the search field is drawn");
        assert!(
            search.right() < date.left() - S[1],
            "{search:?} and {date:?}"
        );

        assert_eq!(f.click(large.center()), [Action::Size(GridTile::Large)]);
        // The sort's list opens under the bar, over the content, and an option of it is
        // the sort wanted.
        assert_eq!(f.click(date.center()), []);
        f.frame(Vec::new());
        let name = (f.texts.iter())
            .find(|(text, place)| text == "Name" && place.top() > TOP_BAR)
            .map(|(_, place)| place.center())
            .expect("the list is open");
        let by_name = Sort {
            key: photon_core::sort::SortKey::Name,
            ..Sort::default()
        };
        assert_eq!(f.click(name), [Action::Sort(by_name)]);
    }

    // In a window at its narrowest the controls keep their width and the search box has
    // what is left: nothing stands on anything else.
    #[test]
    fn in_a_narrow_window_the_search_box_gives_way_to_the_controls() {
        let mut f = Fixture::new();
        f.size = vec2(800.0, 500.0);
        f.frame(Vec::new());
        let date = f.drew("Date taken").expect("the sort is drawn");
        let field = color(palette(&f.ctx).field);
        let search = (f.fills.iter())
            .filter(|(rect, fill)| *fill == field && rect.left() < 100.0)
            .map(|(rect, _)| *rect)
            .next()
            .expect("the search field is drawn");
        assert!(search.left() >= S[1] + BUTTON + S[1]);
        assert!(
            search.right() < date.left() - S[1],
            "{search:?} and {date:?}"
        );
        assert!(search.width() > 120.0, "{search:?}");
        let large = f.drew("Large").expect("the size control is drawn");
        assert!(large.right() < 800.0 - S[1] - BUTTON);
    }

    // The gear is where it will be and takes no press: Settings are a later part.
    #[test]
    fn the_gear_is_drawn_and_does_nothing_yet() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let gear = pos2(1280.0 - S[1] - BUTTON / 2.0, TOP_BAR / 2.0);
        assert_eq!(f.click(gear), []);
    }

    #[test]
    fn dragging_the_splitter_moves_the_sidebar_and_stores_the_width_when_it_is_let_go() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        assert_eq!(f.button(hold, true), []);
        // Followed while it is held, and not stored at every point on the way.
        let moved = f.pointer(pos2(hold.x + 60.0, 400.0));
        assert_eq!(
            moved,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 60.0,
                store: false
            }]
        );
        assert!(f.shell.resizing());
        let moved = f.pointer(pos2(hold.x + 100.0, 380.0));
        assert_eq!(
            moved,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 100.0,
                store: false
            }]
        );
        // The sidebar's key is not answered while its edge is held.
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        let let_go = f.button(pos2(hold.x + 100.0, 380.0), false);
        assert_eq!(
            let_go,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 100.0,
                store: true
            }]
        );
        assert!(!f.shell.resizing());
        f.frame(Vec::new());
        assert_eq!(f.content.left(), SIDEBAR_DEFAULT + 100.0 + SPLITTER);
        // And the next frames, with nothing held, say nothing more.
        assert_eq!(f.pointer(pos2(700.0, 400.0)), []);
    }

    #[test]
    fn a_drag_stops_at_the_minimum_and_at_half_the_window() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        f.button(hold, true);
        let far_left = f.pointer(pos2(20.0, 400.0));
        assert_eq!(
            far_left,
            [Action::Width {
                width: SIDEBAR_MIN,
                store: false
            }]
        );
        let far_right = f.pointer(pos2(1200.0, 400.0));
        assert_eq!(
            far_right,
            [Action::Width {
                width: 640.0,
                store: false
            }]
        );
    }

    #[test]
    fn the_focused_splitter_is_moved_by_the_arrows_and_each_move_is_stored() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        // A click gives it the focus, and moves nothing.
        let on = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        let clicked = f.click(on);
        assert!(
            clicked.iter().all(|action| matches!(
                action,
                Action::Width { width, .. } if *width == SIDEBAR_DEFAULT
            )),
            "{clicked:?}"
        );
        f.frame(Vec::new());
        assert_eq!(
            f.key(Key::ArrowRight, Modifiers::NONE),
            [Action::Width {
                width: SIDEBAR_DEFAULT + 16.0,
                store: true
            }]
        );
        assert_eq!(
            f.key(Key::ArrowLeft, Modifiers::NONE),
            [Action::Width {
                width: SIDEBAR_DEFAULT,
                store: true
            }]
        );
    }

    // Without the focus the arrows are the grid's, or nobody's: not the splitter's.
    #[test]
    fn the_arrows_move_nothing_while_the_splitter_has_no_focus() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::ArrowRight, Modifiers::NONE), []);
    }

    #[test]
    fn the_line_of_an_empty_view_is_in_the_middle_of_the_content() {
        let mut f = Fixture::new();
        f.notice = Some("No starred photos. Star one in the viewer, or in Picasa.".to_owned());
        f.frame(Vec::new());
        let line = f
            .drew("No starred photos. Star one in the viewer, or in Picasa.")
            .expect("the line is drawn");
        assert!(
            (line.center().x - f.content.center().x).abs() < 2.0,
            "{line:?}"
        );
        assert!(
            (line.center().y - f.content.center().y).abs() < 8.0,
            "{line:?}"
        );
    }

    #[test]
    fn a_message_is_drawn_over_the_corner_and_its_button_dismisses_it() {
        let mut f = Fixture::new();
        f.toasts.error("could not read the library", 0.0);
        f.toasts.done("3 photos starred", 0.0);
        // egui measures a new area in a frame it does not show, and shows it in the next.
        f.frame(Vec::new());
        f.frame(Vec::new());
        let error = f.drew("could not read the library").expect("drawn");
        let done = f.drew("3 photos starred").expect("drawn");
        // The newest lowest, both in the bottom right, clear of the status bar.
        assert!(done.top() > error.bottom());
        assert!(done.bottom() < 800.0 - STATUS_BAR && error.right() < 1280.0 - S[3]);
        assert!(error.left() > 800.0);

        // The button is at the message's right end, level with its text.
        let first = f.toasts.held()[0].id;
        let button = pos2(1280.0 - S[3] - S[2] - TOAST_BUTTON / 2.0, error.center().y);
        assert_eq!(f.click(button), [Action::Dismiss(first)]);
        f.frame(Vec::new());
        assert_eq!(f.drew("could not read the library"), None);
        assert!(f.drew("3 photos starred").is_some());
    }

    // What the content draws stays in its area: a grid that overran it would paint over
    // the bars, which are drawn first.
    #[test]
    fn the_content_is_clipped_to_its_area() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert!(
            f.content.contains_rect(f.clip),
            "{:?} in {:?}",
            f.clip,
            f.content
        );
        assert!(f.clip.width() > 0.0);
    }

    // Ctrl+Shift+B and Ctrl+Alt+B are other chords, as they are in the Svelte UI.
    #[test]
    fn the_key_with_another_modifier_beside_it_is_another_chord() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        for more in [Modifiers::SHIFT, Modifiers::ALT] {
            assert_eq!(f.key(Key::B, Modifiers::COMMAND | more), []);
            f.key_up(Key::B, Modifiers::COMMAND | more);
        }
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
    }

    // The last move and the release can come in one frame, and by then the press is no
    // longer a drag: the edge has to go where the button was let go, not stay where the
    // frame before left it.
    #[test]
    fn the_splitter_is_left_where_the_button_was_let_go() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        f.button(hold, true);
        f.pointer(pos2(hold.x + 60.0, 400.0));
        let to = pos2(hold.x + 100.0, 400.0);
        let let_go = f.frame(vec![
            Event::PointerMoved(to),
            Event::PointerButton {
                pos: to,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]);
        assert_eq!(
            let_go,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 100.0,
                store: true
            }]
        );
        f.frame(Vec::new());
        assert_eq!(f.content.left(), SIDEBAR_DEFAULT + 100.0 + SPLITTER);
    }

    // The arrows are the splitter's for as long as it has the focus. Without that, egui
    // takes an arrow to move the focus to the next thing in that direction - leftwards, a
    // row of the sidebar - and the second press moves nothing.
    #[test]
    fn the_focused_splitter_keeps_the_arrows_press_after_press() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        f.click(pos2(SIDEBAR_DEFAULT + 2.0, 400.0));
        f.frame(Vec::new());
        for step in 1..=3 {
            assert_eq!(
                f.key(Key::ArrowLeft, Modifiers::NONE),
                [Action::Width {
                    width: SIDEBAR_DEFAULT - step as f32 * 16.0,
                    store: true
                }]
            );
            f.key_up(Key::ArrowLeft, Modifiers::NONE);
        }
    }

    // Lit under the pointer and while it is held, and when the keyboard has put the focus
    // on it - not for ever after a mouse let go of it, which is when it still has the
    // focus and nobody is looking for where the focus is.
    #[test]
    fn the_splitter_is_lit_under_the_pointer_and_for_the_keyboard_not_after_a_drag() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert!(!f.splitter_lit());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        f.frame(Vec::new());
        assert!(f.splitter_lit(), "under the pointer");
        f.button(hold, true);
        let to = pos2(hold.x + 60.0, 400.0);
        f.pointer(to);
        assert!(f.splitter_lit(), "while it is held");
        f.button(to, false);
        // The pointer goes off over the grid. The splitter still has the focus - the
        // arrows still move it - and is not lit.
        f.pointer(pos2(900.0, 400.0));
        f.frame(Vec::new());
        assert!(!f.splitter_lit(), "after the drag");
        assert!(matches!(
            f.key(Key::ArrowRight, Modifiers::NONE).as_slice(),
            [Action::Width { store: true, .. }]
        ));
        f.key_up(Key::ArrowRight, Modifiers::NONE);

        // The focus taken away and brought back by the keyboard: Tab until it is here.
        let mut g = Fixture::new();
        g.frame(Vec::new());
        let mut found = false;
        for _ in 0..24 {
            g.key(Key::Tab, Modifiers::NONE);
            g.key_up(Key::Tab, Modifiers::NONE);
            g.frame(Vec::new());
            if g.splitter_lit() {
                found = true;
                break;
            }
        }
        assert!(found, "Tab never lit the splitter");
    }
}
