//! The sidebar, drawn: the entries of `list.rs` that are in view, at a position this view
//! keeps, and which of them was clicked, which was asked for its menu, and what was
//! typed in the one field a name is given in (`rename.rs`).
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
    menu_view::{asks_for_menu, menu_press},
    rename::Of,
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Align2, Color32, Event, EventFilter, Id, Key, Painter, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2,
    text::{CCursor, CCursorRange},
    vec2,
};

/// What a row is inset by from the panel's left edge.
const INSET: f32 = 6.0;
/// The field a name is typed in: what it is set in by from the panel's left edge, and
/// from the row's right.
const FIELD_LEFT: f32 = 26.0;
const FIELD_RIGHT: f32 = 6.0;
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
/// The words that say no folder is watched are no group's note: they start where a
/// year's heading would, the folders' own place, with air over them. At a note's indent,
/// right under the last group's heading, they read as that group's - "Tags: No folders
/// yet." - which a picture showed.
const NO_FOLDERS_ABOVE: f32 = 12.0;
/// The button under them, to Settings' folders: as tall as this, in a row.
const ADD_FOLDER: f32 = 24.0;
/// Where a year's heading starts, and where its line's middle is under the room above it.
const YEAR_LEFT: f32 = 14.0;
const YEAR_MIDDLE: f32 = 20.0;

pub struct SidebarData<'a> {
    pub list: &'a List,
    /// The folder at the top of the grid, whose entry is marked and kept in sight.
    pub here: Option<i64>,
    /// The name being typed, drawn in the place of the entry it is the name of.
    pub naming: Option<Naming<'a>>,
}

/// The one field of the sidebar, as `rename.rs` holds it.
#[derive(Clone, Copy, Debug)]
pub struct Naming<'a> {
    pub of: &'a Of,
    pub text: &'a str,
    /// Whether the name is being stored: the field takes no typing then, and keeps the
    /// keyboard.
    pub busy: bool,
}

/// What was done to the field in a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// What it holds now.
    Typed(String),
    /// The name is to be stored: Enter, a press somewhere else, the keyboard walked on,
    /// its row scrolled out of sight.
    Commit,
    /// It is to close and store nothing: Escape, or its entry gone from the list.
    Cancel,
}

/// What a frame of the sidebar came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// The entry that was clicked.
    pub clicked: Option<What>,
    /// The entry the other button went down on, where: its menu is asked for.
    pub menu: Option<(What, Pos2)>,
    pub named: Vec<Named>,
}

/// The placeholder of a new album's field, and what the fields say they are to whoever
/// reads the window without seeing it: `FolderTree.svelte`'s.
const NEW_NAME: &str = "Album name";

fn field_label(of: &Of) -> &'static str {
    match of {
        Of::NewAlbum => "New album name",
        Of::Album(_) => "Album name",
        Of::Search(_) => "Saved search name",
        Of::Folder(_) => "Folder name in photon",
    }
}

fn field_id() -> Id {
    Id::new("sidebar-name")
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
    /// The entry the list is held by when its entries change.
    kept: Option<Kept>,
    /// What the field stood in for when it was last drawn, in the frame before this
    /// one: a field that was not drawn then - another row's, or this one's while its row
    /// was out of sight or the sidebar hidden - is opened now, and takes the keyboard.
    field: Option<Of>,
    /// The field whose name was sent because its row is out of sight: once, not in
    /// every frame it stays there. A name the library refuses leaves the field open,
    /// unseen, and sent again each frame it was refused again each frame.
    unseen: Option<Of>,
    /// Whether the button that is down, or was last, asked for a menu: its click is
    /// not a row's as well.
    menu_button: bool,
    /// An entry to bring into sight when the sidebar is next drawn.
    reveal: Option<What>,
}

/// An entry and where it was when the list was last drawn. When the entries are built
/// again - the albums read a moment after the folders, a scan that found a folder, a group
/// folded - or a note wraps to another height, the list is put where this entry is where
/// it was. Without it rows slid under a resting pointer, and at launch the folder the
/// list had just followed the grid to was pushed out under the bottom edge by the albums
/// arriving above it, with nothing to bring it back until the grid reached another folder.
struct Kept {
    what: What,
    index: usize,
    /// Its top in the list, not on the screen.
    top: f64,
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

/// Where a note's words begin in its band: how far in, and how far down.
fn note_place(what: &What) -> (f32, f32) {
    match what {
        What::NoFolders => (YEAR_LEFT, NO_FOLDERS_ABOVE),
        _ => (NOTE_LEFT, NOTE_ABOVE),
    }
}

fn note_font() -> egui::FontId {
    fonts::regular(T[1])
}

/// What draws an entry: the panel it is in, and where the grid is.
/// Whether the last button to go down in this frame asked for a menu, if one went down:
/// with Control held on a Mac the first button does, the row's menu is opened at that
/// press, and the click the same button ends in is not the row's as well - whenever
/// Control is let go of.
fn menu_button(ui: &egui::Ui) -> Option<bool> {
    ui.input(|input| {
        input.events.iter().rev().find_map(|event| match event {
            Event::PointerButton {
                button,
                pressed: true,
                modifiers,
                ..
            } => Some(asks_for_menu(
                *button,
                *modifiers,
                cfg!(target_os = "macos"),
            )),
            _ => None,
        })
    })
}

struct Frame<'a> {
    painter: &'a Painter,
    /// The sidebar's own area.
    panel: Rect,
    palette: &'a Palette,
    here: Option<i64>,
}

impl SidebarView {
    /// Where the list is.
    pub fn position(&self) -> f64 {
        self.scroll.position()
    }

    /// Has `what` brought into sight when the sidebar is next drawn: the row a name is
    /// about to be typed in, asked for from a header in the grid.
    pub fn reveal(&mut self, what: What) {
        self.reveal = Some(what);
    }

    /// The sidebar is not drawn in this frame: it is hidden. A field left open under
    /// it - a name the library refused - is opened again when it is next drawn, and
    /// takes the keyboard then: drawn without it, what was typed "into" it was the
    /// grid's, where `h` hides a photo.
    pub fn not_drawn(&mut self) {
        self.field = None;
    }

    /// Draws the sidebar in `rect` and answers what was done in it.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, data: &SidebarData<'_>) -> Shown {
        let palette = palette(ui.ctx());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color(palette.chrome));

        let list = data.list;
        self.measure(ui, list, rect.width());
        let viewport = f64::from(rect.height()).max(0.0);
        let held = self.held_place(list);
        self.scroll.set_extent(self.stack.total(), viewport);
        if let Some(place) = held {
            self.scroll.set(place);
        }
        // The entry a name is being typed in the place of.
        let naming = data.naming.and_then(|naming| {
            let at = (list.entries.iter())
                .position(|entry| entry.what.named().as_ref() == Some(naming.of))?;
            Some((at, naming))
        });
        if let Some(what) = self.reveal.take()
            && let Some(index) = list.entries.iter().position(|entry| entry.what == what)
        {
            let (top, bottom) = (self.stack.top(index), self.stack.bottom(index));
            if let Some(to) = into_view(top, bottom, self.scroll.position(), viewport) {
                self.scroll.set(to);
            }
        }
        let over = ui.rect_contains_pointer(rect);
        // The list does not follow the grid while a name is being typed in it: it is
        // the user's then, and the row under the caret must not move.
        if data.naming.is_none() {
            self.follow(list, data.here, over, viewport);
        }
        let bar = Rect::from_min_max(pos2(rect.right() - BAR, rect.top()), rect.max);
        self.take_input(ui, bar, over);

        let top = snapped(self.scroll.position(), f64::from(ui.pixels_per_point()));
        let frame = Frame {
            painter: &painter,
            panel: rect,
            palette,
            here: data.here,
        };
        // A child that draws nothing and takes no press outside the sidebar: a row half
        // scrolled out is cut at the edge, its icon with it, and the bar over that edge
        // keeps its own presses. The clip has to be set: `new_child` gives the child the
        // parent's, whatever its `max_rect`.
        let mut inner = ui.new_child(UiBuilder::new().max_rect(rect));
        inner.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut inner;
        let mut shown = Shown::default();
        let mut drawn = None;
        let in_view = self.stack.range(top, top + viewport);
        for index in in_view.clone() {
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
            match naming {
                Some((at, naming)) if at == index => {
                    let opened = self.field.as_ref() != Some(naming.of);
                    shown.named = field(ui, &frame, band, &naming, opened);
                    drawn = Some(naming.of.clone());
                }
                _ => {
                    if let Some(clicked) = draw(ui, &frame, band, entry) {
                        shown.clicked = Some(clicked);
                    }
                }
            }
        }
        self.draw_scrollbar(&painter, bar, palette);
        // A field whose row is not drawn has nowhere to be typed in, and egui has taken
        // the keyboard off a widget that is gone: left open, the next name typed would
        // go to the grid, where `h` hides a photo. Scrolled out of sight, its name is
        // stored, as leaving the field stores it; with no entry left in the list - the
        // album deleted from elsewhere, the folder no longer in the view - it closes.
        // Sent once: a name the library refuses leaves the field open, unseen, and
        // sent in every frame it was refused in every frame, a message each. One that
        // is being stored already is sent all the same, and the application, which
        // stores a name once, drops it.
        match (data.naming, naming) {
            (Some(_), None) => shown.named.push(Named::Cancel),
            (Some(naming), Some((at, _)))
                if !in_view.contains(&at) && self.unseen.as_ref() != Some(naming.of) =>
            {
                shown.named.push(Named::Commit);
                self.unseen = Some(naming.of.clone());
            }
            _ => {}
        }
        if drawn.is_some() || data.naming.is_none() {
            self.unseen = None;
        }
        self.field = drawn;
        if let Some(asked) = menu_button(ui) {
            self.menu_button = asked;
        }
        if self.menu_button {
            shown.clicked = None;
        }

        // The other button on an entry that has a menu asks for it, where it goes
        // down - as on a photo - and not through anything lying over the list.
        if let Some(at) = menu_press(ui)
            && rect.contains(at)
            && at.x < bar.left()
            && ui.ctx().layer_id_at(at) == Some(ui.layer_id())
        {
            let y = top + f64::from(at.y - rect.top());
            // Not the row a name is being typed in: the other button in a text field is
            // the field's own, and a menu opened there took the keyboard from the
            // field, which stored the name half typed.
            let entry = (in_view.clone())
                .find(|&index| (self.stack.top(index)..self.stack.bottom(index)).contains(&y))
                .filter(|&index| naming.is_none_or(|(at, _)| at != index))
                .map(|index| &list.entries[index]);
            if let Some(entry) = entry.filter(|entry| entry.has_menu()) {
                shown.menu = Some((entry.what.clone(), at));
            }
        }

        // What the list is held by, should its entries change before the next frame: the
        // row under the pointer, else the marked folder's if it is in sight, else the
        // first in view.
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let under = pointer.filter(|_| over).and_then(|pointer| {
            let y = top + f64::from(pointer.y - rect.top());
            (in_view.clone())
                .find(|&index| (self.stack.top(index)..self.stack.bottom(index)).contains(&y))
        });
        let marked = (data.here)
            .and_then(|folder| list.folder(folder))
            .filter(|index| in_view.contains(index));
        let first = (!in_view.is_empty()).then_some(in_view.start);
        // The row a name is being typed in comes first: rows arriving above it - the
        // lists read again after the name before it was stored - must not move it.
        let typed_in = naming.map(|(at, _)| at).filter(|at| in_view.contains(at));
        self.keep(list, typed_in.or(under).or(marked).or(first));
        shown
    }

    /// Where the list is to be put so that the entry it is held by is where it was, when
    /// that entry has moved in the list.
    fn held_place(&self, list: &List) -> Option<f64> {
        let kept = self.kept.as_ref()?;
        let same =
            |index: usize| (list.entries.get(index)).is_some_and(|entry| entry.what == kept.what);
        let index = if same(kept.index) {
            kept.index
        } else {
            // The entries are another list: look for it, once.
            (0..list.entries.len()).find(|&index| same(index))?
        };
        let moved = self.stack.top(index) - kept.top;
        (moved != 0.0).then(|| self.scroll.position() + moved)
    }

    fn keep(&mut self, list: &List, index: Option<usize>) {
        let Some(index) = index else {
            self.kept = None;
            return;
        };
        let top = self.stack.top(index);
        match &mut self.kept {
            // The same entry as last frame, which is every frame but a few: no copy of
            // its name is made.
            Some(kept) if kept.index == index && list.entries[index].what == kept.what => {
                kept.top = top;
            }
            kept => {
                *kept = Some(Kept {
                    what: list.entries[index].what.clone(),
                    index,
                    top,
                });
            }
        }
    }

    /// Stacks the entries: each as tall as its kind, and a note as tall as its words are
    /// when wrapped to the list.
    fn measure(&mut self, ui: &egui::Ui, list: &List, width: f32) {
        let heights = list.entries.iter().map(|entry| {
            height(&entry.what).unwrap_or_else(|| {
                let (left, above) = note_place(&entry.what);
                let room = (width - left - NOTE_RIGHT).max(0.0);
                let words =
                    ui.painter()
                        .layout(entry.label.clone(), note_font(), Color32::WHITE, room);
                f64::from(above + words.size().y + NOTE_BELOW)
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
    rect: Rect,
    entry: &Entry,
    label: &str,
    current: Option<bool>,
) -> Response {
    let response = ui.interact(rect, ui.id().with(("sidebar", &entry.what)), Sense::click());
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
/// Draws the field a name is typed in, in the place of the row in `band`, and answers
/// what was done to it. egui's own text field, with what the search box learnt of it.
///
/// - **Enter and Escape are read before the field is drawn**, and taken out of the
///   frame: egui's field gives the keyboard up at Enter, and a field whose name is being
///   stored has to keep it; and an Escape left in the frame is the grid's next, which
///   clears the selection. Escape is claimed for the field (`event_filter`), or egui
///   takes the keyboard away before anything is drawn and the field cannot tell it had
///   had it.
/// - **A press anywhere else stores the name**, at the press: egui lets go of a field at
///   a click elsewhere, which is the button coming up, and never at a press that becomes
///   a drag.
/// - **While the name is stored nothing typed is taken.** A field stored by Enter still
///   has the keyboard, so what is typed next does not reach the grid, where `h` hides a
///   photo. One stored by being left has given the keyboard to what the user went to,
///   and does not ask for it back: asked back, Ctrl+F out of a name left the search box
///   without the keyboard, and when the write landed nothing had it - the search typed
///   next was the grid's.
fn field(
    ui: &mut egui::Ui,
    frame: &Frame<'_>,
    band: Rect,
    naming: &Naming<'_>,
    opened: bool,
) -> Vec<Named> {
    let palette = frame.palette;
    let id = field_id();
    let row = Rect::from_min_max(pos2(band.left() + INSET, band.bottom() - ROW), band.max);
    // The Svelte field's margins: where a row's name starts, less its own padding.
    let ground = Rect::from_min_max(
        pos2(band.left() + FIELD_LEFT, row.top()),
        pos2(row.right() - FIELD_RIGHT, row.bottom()),
    );
    let mut named = Vec::new();
    let had_keys = ui.memory(|memory| memory.has_focus(id));
    if had_keys {
        // Every press of the key is taken out of the frame, and only one that is no
        // repeat is answered: the Enter that picked "Rename…" in the menu is still down
        // when the field opens, and its repeat stored and closed the field before
        // anything was typed in it.
        let pressed = |key: Key| {
            ui.input_mut(|input| {
                let mut fresh = false;
                input.events.retain(|event| match event {
                    Event::Key {
                        key: at,
                        pressed: true,
                        repeat,
                        ..
                    } if *at == key => {
                        fresh |= !repeat;
                        false
                    }
                    _ => true,
                });
                fresh
            })
        };
        if pressed(Key::Enter) {
            named.push(Named::Commit);
        } else if pressed(Key::Escape) {
            named.push(Named::Cancel);
        }
        // While the name is stored nothing typed is taken by the field, which would
        // show it for a frame and then forget it.
        if naming.busy {
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, Event::Text(_) | Event::Paste(_) | Event::Cut)
                });
            });
        }
    }

    frame
        .painter
        .rect_filled(ground, R[1], color(palette.surface));
    frame.painter.rect_stroke(
        ground,
        R[1],
        Stroke::new(2.0, color(palette.accent)),
        StrokeKind::Inside,
    );
    let font = fonts::regular(T[2]);
    let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let edit = Rect::from_min_max(
        pos2(ground.left() + S[1], ground.center().y - line / 2.0),
        pos2(ground.right() - S[1], ground.center().y + line / 2.0),
    );
    let mut text = naming.text.to_owned();
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(edit), |ui| {
            let room = Rect::from_x_y_ranges(edit.x_range(), ground.y_range());
            ui.set_clip_rect(room.intersect(ui.clip_rect()));
            let hint = match naming.of {
                Of::NewAlbum => NEW_NAME,
                _ => "",
            };
            egui::TextEdit::singleline(&mut text)
                .id(id)
                // Escape is the field's own key, and so are the arrows, which move the
                // caret: a filter built from the default leaves them to egui, which
                // walks the keyboard to a neighbouring row with them.
                .event_filter(EventFilter {
                    escape: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    tab: false,
                })
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(font)
                .text_color(color(palette.text))
                .hint_text(egui::RichText::new(hint).color(color(palette.text_dim)))
                .desired_width(edit.width())
                .show(ui)
        })
        .inner;
    let label = field_label(naming.of);
    output
        .response
        .widget_info(|| WidgetInfo::text_edit(true, "", naming.text, label));
    if opened {
        // With the name in it, all of it selected: typing replaces it, and an arrow
        // keeps it to correct. And with a history of its own: one field serves every
        // row, egui keeps a field's undo by its id, and Ctrl+Z in a folder's field
        // brought back the name typed for an album a moment before.
        output.response.request_focus();
        let mut state = egui::text_edit::TextEditState::default();
        let end = CCursor::new(naming.text.chars().count());
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
        state.store(ui.ctx(), id);
    }
    // What was typed in this frame is said before the Enter of the same frame: a frame
    // can be long, and a name's last letter and its Enter in it together - stored
    // without the letter, a rename stored the name as it was. Not before Escape, which
    // keeps nothing.
    let typed = (output.response.changed()).then_some(Named::Typed(text));
    match named.first() {
        Some(Named::Commit) => {
            named.splice(0..0, typed);
        }
        Some(_) => return named,
        None => named.extend(typed),
    }
    if !named.contains(&Named::Commit) {
        let elsewhere = ui.input(|input| {
            input.pointer.any_pressed()
                && (input.pointer.interact_pos()).is_some_and(|at| !ground.contains(at))
        });
        // The keyboard walked on with Tab, too: the field has been left.
        if elsewhere || output.response.lost_focus() {
            named.push(Named::Commit);
        }
    }
    named
}

/// Draws `entry` in `band`. Answers what a click on it asks for: the entry itself, or the
/// People page for the label half of People's heading.
fn draw(ui: &mut egui::Ui, frame: &Frame<'_>, band: Rect, entry: &Entry) -> Option<What> {
    let clicked = |yes: bool| yes.then(|| entry.what.clone());
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
                button(ui, row, entry, &entry.label, Some(entry.active))
            } else {
                let response =
                    ui.interact(row, ui.id().with(("sidebar", &entry.what)), Sense::hover());
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
            clicked(pressable && response.clicked())
        }
        What::Group(group) => {
            let open = entry.detail == Detail::Open(true);
            // People's heading is two things, the fold and the page, laid out to sit
            // exactly where a one-button heading puts its chevron and its icon: the
            // chevron folds, the rest of the row - the label and its count - opens the
            // page, and is where the user is while the page is open. The other headings
            // fold where they are clicked, anywhere along the row.
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
            let response = button(ui, pressed, entry, name, None);
            let response = if people {
                hinted(response, name)
            } else {
                response
            };
            if response.hovered() {
                painter.rect_filled(pressed, R[2], color(palette.hover));
            }
            let mut page = false;
            if people {
                let label = Rect::from_min_max(pos2(fold_end, row.top()), row.max);
                let shown = ui.interact(
                    label,
                    ui.id().with(("sidebar", &What::PeoplePage)),
                    Sense::click(),
                );
                shown.widget_info(|| {
                    WidgetInfo::selected(WidgetType::Button, true, entry.active, &entry.label)
                });
                if entry.active {
                    painter.rect_filled(label, R[2], color(palette.accent_soft));
                } else if shown.hovered() {
                    painter.rect_filled(label, R[2], color(palette.hover));
                }
                page = shown.clicked();
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
            if page {
                return Some(What::PeoplePage);
            }
            clicked(response.clicked())
        }
        What::Album(_) | What::Search { .. } | What::Person(_) | What::Tag(_) | What::Folder(_) => {
            // A folder is a place in the grid, never the view shown: it says neither.
            let current = (!matches!(entry.what, What::Folder(_))).then_some(entry.active);
            let response = hinted(button(ui, row, entry, &entry.label, current), &entry.hint);
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
            clicked(response.clicked())
        }
        What::NewAlbum => {
            // Fainter than a row that names something, until the pointer is on it.
            let response = button(ui, row, entry, &entry.label, None);
            let tint = if response.hovered() { text } else { dim };
            let left = row.left() + INDENT;
            paint_line(
                ui,
                painter,
                (left, middle),
                row.right() - S[1] - left,
                &entry.label,
                fonts::regular(T[2]),
                tint,
            );
            clicked(response.clicked())
        }
        What::Year(_) => {
            let font = fonts::semibold(ui.ctx(), T[0]);
            let at = (band.left() + YEAR_LEFT, band.top() + YEAR_MIDDLE);
            let room = band.right() - band.left() - 2.0 * YEAR_LEFT;
            paint_line(ui, painter, at, room, &entry.label, font, dim);
            None
        }
        What::AddFolder => {
            // Settings, at their list of folders. The words are laid out in their own
            // colour: a galley is painted in the one it was laid out with, whatever it is
            // handed afterwards.
            let words = painter.layout_no_wrap(entry.label.clone(), fonts::regular(T[1]), text);
            let button = Rect::from_min_size(
                pos2(band.left() + YEAR_LEFT, middle - ADD_FOLDER / 2.0),
                vec2(words.size().x + 2.0 * S[1], ADD_FOLDER),
            );
            let response = ui.interact(
                button,
                ui.id().with(("sidebar", &entry.what)),
                Sense::click(),
            );
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &entry.label));
            let ground = if response.hovered() {
                palette.field_hover
            } else {
                palette.field
            };
            painter.rect_filled(button, R[2], color(ground));
            if response.has_focus() {
                painter.rect_stroke(
                    button.expand(2.0),
                    R[2] + 2.0,
                    egui::Stroke::new(2.0, color(palette.accent)),
                    egui::StrokeKind::Inside,
                );
            }
            painter.galley(
                pos2(button.left() + S[1], middle - words.size().y / 2.0),
                words,
                text,
            );
            clicked(response.clicked())
        }
        What::Note(_) | What::NoFolders => {
            let (left, above) = note_place(&entry.what);
            let room = (frame.panel.width() - left - NOTE_RIGHT).max(0.0);
            let words = painter.layout(entry.label.clone(), note_font(), dim, room);
            painter.galley(pos2(band.left() + left, band.top() + above), words, dim);
            // Words, and said as words to whoever reads the window without seeing it.
            ui.interact(band, ui.id().with(("sidebar", &entry.what)), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &entry.label));
            None
        }
        // No entry of its own: what a click on the label half of People's heading answers.
        What::PeoplePage => None,
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
        /// Whether the library is known to watch no folder.
        no_folders: bool,
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
                no_folders: false,
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
                no_folders: self.no_folders,
                people: false,
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
        /// The name being typed: what for, what it holds, whether it is being stored.
        naming: Option<(Of, String, bool)>,
        /// What the last frame came to.
        last: Shown,
        /// Something lying over the list, in a layer of its own.
        over: Option<Rect>,
        /// The keys the frame's input still held after the sidebar had read it.
        left_over: Vec<Key>,
        /// What was typed that the frame's input still held after the sidebar read it.
        left_typed: Vec<String>,
        /// How often the field has said its name is to be stored, in every frame so far.
        stores: usize,
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
                naming: None,
                last: Shown::default(),
                over: None,
                left_over: Vec::new(),
                left_typed: Vec::new(),
                stores: 0,
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
                naming: (self.naming.as_ref()).map(|(of, text, busy)| Naming {
                    of,
                    text,
                    busy: *busy,
                }),
            };
            let (view, above, beside) = (&mut self.view, self.above, self.beside);
            let over = self.over;
            let mut shown = Shown::default();
            let mut left_over = Vec::new();
            let mut left_typed = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                if let Some(over) = over {
                    egui::Area::new(Id::new("over"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(over.min)
                        .show(ui.ctx(), |ui| {
                            ui.allocate_exact_size(over.size(), Sense::hover());
                        });
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let window = ui.max_rect();
                        let rect = Rect::from_min_max(
                            pos2(window.left(), window.top() + above),
                            pos2(window.right() - beside, window.bottom()),
                        );
                        shown = view.show(ui, rect, &data);
                        ui.input(|input| {
                            for event in &input.events {
                                match event {
                                    Event::Key {
                                        key, pressed: true, ..
                                    } => left_over.push(*key),
                                    Event::Text(text) => left_typed.push(text.clone()),
                                    _ => {}
                                }
                            }
                        });
                    });
            });
            self.left_over = left_over;
            self.left_typed = left_typed;
            self.stores += (shown.named.iter())
                .filter(|named| **named == Named::Commit)
                .count();
            // What was typed is the field's from now on, as the application keeps it.
            for named in &shown.named {
                if let (Named::Typed(text), Some(naming)) = (named, self.naming.as_mut()) {
                    naming.1.clone_from(text);
                }
            }
            let clicked = shown.clicked.clone();
            self.last = shown;
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

        /// The other button down at `at`: the menu that frame asked for.
        fn other_down(&mut self, at: Pos2) -> Option<(What, Pos2)> {
            self.pointer(at);
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Secondary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }]);
            let menu = self.last.menu.clone();
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Secondary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }]);
            assert_eq!(self.last, Shown::default(), "its release asks nothing");
            menu
        }

        /// Opens the field for `of` with `name` in it, and draws it until it stands.
        fn name(&mut self, of: Of, name: &str) {
            self.naming = Some((of, name.to_owned(), false));
            self.frame(Vec::new());
            self.frame(Vec::new());
        }

        /// A key pressed and let go of: what the frame of the press did to the field.
        fn key(&mut self, key: Key) -> Vec<Named> {
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![event(true)]);
            let named = self.last.named.clone();
            let left_over = self.left_over.clone();
            self.frame(vec![event(false)]);
            self.left_over = left_over;
            named
        }

        fn typed(&mut self, text: &str) -> Vec<Named> {
            self.frame(vec![Event::Text(text.to_owned())]);
            self.last.named.clone()
        }

        fn in_field(&self) -> bool {
            self.ctx.memory(|memory| memory.has_focus(field_id()))
        }

        /// The characters selected in the field, as `(from, to)`.
        fn selected(&self) -> Option<(usize, usize)> {
            let state = egui::TextEdit::load_state(&self.ctx, field_id())?;
            let [from, to] = state.cursor.char_range()?.sorted_cursors();
            Some((from.index.0, to.index.0))
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

    // A library that watches nothing: said where its folders would be listed - at their
    // inset, with air over the words, not as the note of the group above - and under it
    // the button that leads to where one is added.
    #[test]
    fn no_folders_is_said_in_the_folders_place_over_a_button_to_settings() {
        let mut f = Fixture::new(0);
        f.world.no_folders = true;
        f.world.tallies.clear();
        f.rebuild();
        f.frame(Vec::new());
        let words = f.drew("No folders yet.").expect("the words are drawn");
        let band = f.band(&What::NoFolders);
        assert!((words.left() - YEAR_LEFT).abs() < 2.0, "{words:?}");
        assert!(
            words.top() >= band.top() + NO_FOLDERS_ABOVE - 1.0,
            "{words:?} in {band:?}"
        );
        assert!(words.bottom() <= band.bottom());

        let button = f
            .drew("Add a folder in Settings…")
            .expect("the button is drawn");
        assert!(
            button.top() >= band.bottom() - 1.0,
            "{button:?} under {band:?}"
        );
        assert!(button.left() > YEAR_LEFT);
        assert_eq!(f.click(button.center()), Some(What::AddFolder));
        assert_eq!(f.click(band.center()), None);
        // In the colour of what can be pressed, its words as well as its ground.
        let full = color(palette(&f.ctx).text);
        assert_eq!(f.tint_of("Add a folder in Settings…"), full);
    }

    // Its chevron folds the list; its label is the People page's, which is not built yet.
    #[test]
    fn people_fold_at_their_chevron_and_their_label_opens_the_page() {
        let mut f = Fixture::new(1);
        f.frame(Vec::new());
        let people = What::Group(Group::People);
        let row = f.middle(&people);
        assert_eq!(
            f.click(pos2(INSET + S[1] + 6.0, row.y)),
            Some(people.clone())
        );
        assert_eq!(f.click(pos2(120.0, row.y)), Some(What::PeoplePage));
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
        // People's heading is two buttons, each lit alone: the fold at its chevron, the
        // page along the rest.
        let people = f.middle(&What::Group(Group::People));
        f.pointer(pos2(INSET + S[1] + 6.0, people.y));
        f.frame(Vec::new());
        let lit = f.filled(hover);
        assert_eq!(lit.len(), 1);
        assert!(lit[0].width() < 40.0, "{:?}", lit[0]);
        let fold = lit[0];
        f.pointer(pos2(120.0, people.y));
        f.frame(Vec::new());
        let lit = f.filled(hover);
        assert_eq!(lit.len(), 1);
        assert!(
            lit[0].left() >= fold.right() - 0.5,
            "{:?} beside {fold:?}",
            lit[0]
        );
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
        // Two albums fewer above it, and the row a new one is made from.
        assert_eq!(f.drew("folder-0001").unwrap().top(), before - 3.0 * ROW);
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

    // A bar lies over the sidebar's top edge, and an icon is a picture like any other
    // thing drawn: painted through the window's own clip it stood on the bar, over the
    // edge its row's words were cut at.
    #[test]
    fn an_icon_is_cut_at_the_sidebars_edge_as_its_row_is() {
        let mut f = Fixture::new(200);
        f.above = 46.0;
        f.frame(Vec::new());
        for what in [
            What::Fixed(Fixed::Starred),
            What::Group(Group::Albums),
            What::Album(9),
        ] {
            // Half of the row above the sidebar's top edge.
            let bottom = f.view.stack.bottom(f.index(&what));
            f.view.scroll.set(bottom - 14.0);
            f.frame(Vec::new());
            let reaching = (f.icons.iter())
                .filter(|(rect, _)| rect.top() < 46.0 && rect.bottom() > 46.0)
                .count();
            assert!(reaching > 0, "{what:?}: no icon is at the edge");
            assert!(
                f.clip_tops.iter().all(|top| *top >= 46.0),
                "{what:?}: {:?}",
                f.clip_tops
            );
        }
    }

    /// Six albums more, as a library's collections arriving after its folders.
    fn more_albums(f: &mut Fixture) {
        for id in 20..26 {
            f.world.collections.albums.push(AlbumSummary {
                id,
                name: format!("Album {id}"),
                count: 1,
                picasa: false,
            });
        }
        f.rebuild();
    }

    // At launch the grid goes back to the folder it was left in and the list follows, its
    // row flush at the bottom edge. The albums are read a moment later and their rows go in
    // above: the row must stay where it was, not be pushed out under the edge - nothing
    // would bring it back until the grid reached another folder. Ten folders: the first
    // row in view is above the albums, and holding the list by that one would not do.
    #[test]
    fn rows_arriving_above_the_marked_folder_leave_it_where_it_is() {
        let mut f = Fixture::new(10);
        f.here = Some(10);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let before = f.band(&folder(10));
        assert_eq!(before.bottom(), 600.0, "followed to the bottom edge");
        let position = f.view.position();
        assert!(position > 0.0 && position < f.view.stack.top(f.index(&What::Album(4))));

        more_albums(&mut f);
        f.frame(Vec::new());
        assert_eq!(f.band(&folder(10)), before);
        assert_eq!(f.view.position(), position + 6.0 * f64::from(ROW));
        assert!(f.drew("folder-0010").is_some());
    }

    // The same of the row under a resting pointer, wherever the grid is: a list that
    // moved under it would put another row under a click already on its way. A scan
    // finds a folder that belongs between the one the grid is in and the one under the
    // pointer: it is the pointer's row that stays, and the marked one that gives way.
    #[test]
    fn a_row_arriving_above_does_not_move_the_row_under_the_pointer() {
        let mut f = Fixture::new(200);
        f.here = Some(1);
        f.frame(Vec::new());
        let at = pos2(100.0, 590.0);
        f.pointer(at);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
        let under = |f: &Fixture| {
            (f.list.entries.iter())
                .map(|entry| entry.what.clone())
                .find(|what| f.band(what).contains(at))
                .expect("a row is under the pointer")
        };
        let row = under(&f);
        assert!(matches!(row, What::Folder(id) if id > 3), "{row:?}");
        let (band, marked) = (f.band(&row), f.band(&folder(1)));

        // A day and a bit older than the second folder: listed after it.
        f.world.folders.push(Folder {
            id: 500,
            watched_id: 1,
            parent_id: None,
            path: "/photos/found".to_owned(),
            name: "found".to_owned(),
            hidden: false,
            alias: None,
        });
        f.world.tallies.push(FolderTally {
            folder_id: 500,
            count: 1,
            taken_at_min: IN_2024 - 2 * 86_400 - 60,
            bytes: 0,
            modified_ms: 0,
        });
        f.rebuild();
        f.frame(Vec::new());
        assert!(f.drew("found").is_some());
        assert_eq!(under(&f), row);
        assert_eq!(f.band(&row), band);
        assert_eq!(f.band(&folder(1)).top(), marked.top() - ROW);
    }

    // With neither a pointer nor a marked row in sight, the first row in view is what
    // stays: a list at its top stays at its top, and one in its middle does not slide.
    #[test]
    fn a_list_built_again_keeps_the_first_row_in_view() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        more_albums(&mut f);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);

        f.view.scroll.set(f.view.stack.top(f.index(&folder(60))));
        f.frame(Vec::new());
        let band = f.band(&folder(60));
        f.world.collections.albums.clear();
        f.rebuild();
        f.frame(Vec::new());
        assert_eq!(f.band(&folder(60)), band);

        // The marked folder holds the list only while it is in sight. The user has
        // scrolled back to the top since the list followed the grid far down: rows
        // arriving in between do not move a list whose marked row nobody is looking at.
        f.here = Some(150);
        f.frame(Vec::new());
        assert!(f.view.position() > 1000.0);
        f.view.scroll.set(0.0);
        f.frame(Vec::new());
        more_albums(&mut f);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
    }

    // The row the list was held by is gone - the folder under the pointer emptied, an
    // album deleted: the list stays where it is, and is not thrown to its first row.
    #[test]
    fn a_list_whose_held_row_is_gone_stays_where_it_is() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.view.scroll.set(1000.0);
        let at = pos2(100.0, 300.0);
        f.pointer(at);
        f.frame(Vec::new());
        let held = (f.list.entries.iter())
            .map(|entry| entry.what.clone())
            .find(|what| f.band(what).contains(at))
            .expect("a row is under the pointer");
        let What::Folder(gone) = held else {
            panic!("{held:?} is under the pointer");
        };
        f.world.tallies.retain(|tally| tally.folder_id != gone);
        f.rebuild();
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 1000.0);
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

    // ---- the menus ----

    #[test]
    fn the_other_button_on_an_album_a_search_and_a_folder_asks_for_its_menu() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        for what in [What::Album(4), search(), folder(2)] {
            let at = f.middle(&what);
            assert_eq!(f.other_down(at), Some((what, at)));
        }
    }

    // Picasa's album is changed in Picasa; a person and a keyword have no menu in the
    // Svelte sidebar either; and what is no row has none.
    #[test]
    fn nothing_else_in_the_list_has_a_menu() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        for what in [
            What::Album(9),
            What::NewAlbum,
            What::Fixed(Fixed::Starred),
            What::Group(Group::Albums),
            What::Person("p:7".to_owned()),
            What::Tag("coast".to_owned()),
            What::Year(2024),
        ] {
            let at = f.middle(&what);
            assert_eq!(f.other_down(at), None, "{what:?}");
        }
        // The scrollbar beside a row that has one, and the room under the last row.
        let row = f.middle(&folder(2));
        assert_eq!(f.other_down(pos2(WIDTH - 3.0, row.y)), None);
        assert_eq!(f.other_down(pos2(100.0, 590.0)), None);
    }

    // A bar lies over the sidebar's top edge, and a row with a menu is scrolled half
    // under it: the other button on the bar there is the bar's, not that row's.
    #[test]
    fn the_other_button_outside_the_sidebar_asks_for_no_rows_menu() {
        let mut f = Fixture::new(200);
        f.above = 46.0;
        f.frame(Vec::new());
        let third = f.view.stack.top(f.index(&folder(3)));
        f.view.scroll.set(third + 14.0);
        f.frame(Vec::new());
        let row = f.band(&folder(3));
        assert!(row.top() < 46.0 && row.bottom() > 46.0, "{row:?}");
        assert_eq!(
            f.other_down(pos2(100.0, 46.0 - 8.0)),
            None,
            "above the edge"
        );
        let below = pos2(100.0, 46.0 + 7.0);
        assert_eq!(f.other_down(below), Some((folder(3), below)), "below it");
    }

    // On a Mac the first button with Control held asks for the row's menu, and the click
    // that button ends in is not the row's as well - whether or not Control is still
    // held when the button comes up. Elsewhere it is a click like any other. (What a
    // Mac does is seen on a Mac: the system is not a thing a test can set.)
    #[test]
    fn the_click_of_a_press_that_asked_for_a_menu_is_not_the_rows() {
        let mac = cfg!(target_os = "macos");
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let at = f.middle(&folder(2));
        f.pointer(at);
        let button = |pressed, modifiers| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        f.frame(vec![button(true, Modifiers::CTRL)]);
        assert_eq!(f.last.menu.is_some(), mac);
        // Control let go of first, as often as not.
        let clicked = f.frame(vec![button(false, Modifiers::NONE)]);
        assert_eq!(clicked.is_none(), mac);
        // The next press is judged for itself.
        assert_eq!(f.click(at), Some(folder(2)));
    }

    // A menu already open lies over the row it was opened on, its corner at the pointer.
    #[test]
    fn the_other_button_asks_nothing_through_what_lies_over_the_list() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let at = f.middle(&folder(2)).round();
        f.over = Some(Rect::from_min_size(at, vec2(120.0, 80.0)));
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.other_down(at), None);
        assert_eq!(f.other_down(at + vec2(3.0, 3.0)), None);
        assert_eq!(f.click(at + vec2(3.0, 3.0)), None);
        // Beside it the row is pressed as ever.
        let beside = at - vec2(4.0, 4.0);
        assert_eq!(f.other_down(beside), Some((folder(2), beside)));
    }

    // ---- the field ----

    #[test]
    fn a_name_is_typed_in_the_place_of_its_row_with_the_name_in_it_all_selected() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let row = f.band(&What::Album(4));
        f.name(Of::Album(4), "Best of");
        assert!(f.in_field());
        assert_eq!(f.selected(), Some((0, 7)));
        // The name, in a field on the row's own place; the row's count is not drawn.
        let name = f.drew("Best of").expect("the name is in the field");
        assert!(row.contains_rect(name), "{name:?} in {row:?}");
        assert!(f.drew("1,200").is_none());
        let ground = (f.filled(f.tint(|palette| palette.surface)).into_iter())
            .find(|ground| row.contains(ground.center()))
            .expect("the field's ground");
        assert_eq!(ground.left(), FIELD_LEFT);
        assert_eq!(ground.right(), WIDTH - BAR - FIELD_RIGHT);
        assert_eq!(ground.height(), ROW);
        // The rows beside it are rows as ever.
        assert!(f.drew("Scans").is_some());
        // Typing replaces what is selected.
        assert_eq!(f.typed("Lis"), [Named::Typed("Lis".to_owned())]);
        assert_eq!(f.typed("bon"), [Named::Typed("Lisbon".to_owned())]);
        assert!(f.drew("Lisbon").is_some());
    }

    // The same field for each of the things a name is given to, by what its entry is.
    #[test]
    fn the_field_stands_in_for_a_search_a_folder_and_a_new_album() {
        for (of, what, name) in [
            (Of::Search(2), search(), "Lakes"),
            (Of::Folder(2), folder(2), "folder-0002"),
            (Of::NewAlbum, What::NewAlbum, ""),
        ] {
            let mut f = Fixture::new(3);
            f.frame(Vec::new());
            let row = f.band(&what);
            f.name(of.clone(), name);
            assert!(f.in_field(), "{of:?}");
            f.typed("x");
            // (An empty field draws its placeholder in the frame it is first typed in.)
            f.frame(Vec::new());
            let typed = f.drew("x").expect("what was typed is drawn");
            assert!(row.contains_rect(typed), "{of:?}: {typed:?} in {row:?}");
        }
        // A new album's empty field says what it is for; "New album…" itself is gone.
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        assert_eq!(f.tint_of("New album…"), f.tint(|palette| palette.text_dim));
        f.name(Of::NewAlbum, "");
        assert!(f.drew("New album…").is_none());
        assert!(f.drew(NEW_NAME).is_some());
        // Album 2 is nobody's entry: the same number in another list opens no field.
        let mut f = Fixture::new(3);
        f.name(Of::Album(2), "Lakes");
        assert!(!f.in_field());
        assert_eq!(f.last.named, [Named::Cancel]);
    }

    #[test]
    fn a_click_on_new_album_answers_it() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        let at = f.middle(&What::NewAlbum);
        assert_eq!(f.click(at), Some(What::NewAlbum));
    }

    // Enter stores and Escape closes, and neither key is anyone else's after: an Escape
    // left in the frame is the grid's next, which clears the selection.
    #[test]
    fn enter_stores_the_name_and_escape_closes_the_field() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        assert_eq!(f.key(Key::Enter), [Named::Commit]);
        assert_eq!(f.left_over, []);
        // The field has the keyboard still: its name is being stored.
        assert!(f.in_field());
        assert_eq!(f.key(Key::Escape), [Named::Cancel]);
        assert_eq!(f.left_over, []);
        assert!(f.in_field());
        // A key that is neither is typed, or nobody's, and stays in the frame.
        assert_eq!(f.key(Key::ArrowLeft), []);
        assert_eq!(f.left_over, [Key::ArrowLeft]);
    }

    // A frame can be long, and a letter and the Enter after it in it together: the name
    // stored is the one with the letter, which is said before the Enter is.
    #[test]
    fn a_letter_and_enter_in_one_frame_store_the_name_with_the_letter() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        f.typed("Lisbo");
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![Event::Text("n".to_owned()), enter]);
        assert_eq!(
            f.last.named,
            [Named::Typed("Lisbon".to_owned()), Named::Commit]
        );
        // Escape with it: what was typed is not kept, and nothing is stored.
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        let escape = Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![Event::Text("x".to_owned()), escape]);
        assert_eq!(f.last.named, [Named::Cancel]);
    }

    // At the press, not at the click: egui lets go of a field when a button comes up
    // somewhere else, and a press that becomes a drag never does.
    #[test]
    fn a_press_anywhere_else_stores_the_name() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        let elsewhere = f.middle(&folder(2));
        f.pointer(elsewhere);
        assert_eq!(f.last.named, []);
        f.button(elsewhere, true);
        assert_eq!(f.last.named, [Named::Commit]);
        // The press is that row's all the same.
        assert_eq!(f.button(elsewhere, false), Some(folder(2)));
        // A press in the field itself places the caret and stores nothing.
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        let inside = f.drew("Best of").unwrap().center();
        f.pointer(inside);
        f.button(inside, true);
        assert_eq!(f.last.named, []);
        f.button(inside, false);
        assert_eq!(f.last.named, []);
        assert!(f.in_field());
    }

    // The keyboard walked on: the field has been left, as by a press elsewhere.
    #[test]
    fn tab_out_of_the_field_stores_the_name() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        assert_eq!(f.key(Key::Tab), [Named::Commit]);
        assert!(!f.in_field());
    }

    // What is typed while the name is on its way must not reach the grid, where `h`
    // hides a photo: a field stored by Enter still has the keyboard, and takes none of
    // it.
    #[test]
    fn while_its_name_is_stored_the_field_takes_no_typing() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        assert_eq!(f.key(Key::Enter), [Named::Commit]);
        assert!(f.in_field());
        f.naming.as_mut().unwrap().2 = true;
        f.frame(Vec::new());
        assert_eq!(f.typed("h"), []);
        assert!(f.drew("Best of").is_some());
        assert!(f.in_field());
    }

    // The keyboard is where the user took it: asked back, Ctrl+F out of a name left the
    // search box without it, and when the write landed nothing had it - the search typed
    // next was the grid's.
    #[test]
    fn a_field_stored_by_being_left_does_not_take_the_keyboard_back() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        let elsewhere = pos2(100.0, 590.0);
        f.pointer(elsewhere);
        f.button(elsewhere, true);
        assert_eq!(f.last.named, [Named::Commit]);
        f.naming.as_mut().unwrap().2 = true;
        f.button(elsewhere, false);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(!f.in_field());
        // And what is typed then is not the field's to take out of the frame: it is
        // whose the keyboard is now.
        f.frame(vec![Event::Text("h".to_owned())]);
        assert_eq!(f.left_typed, ["h"]);
    }

    // The Enter that picked "Rename…" in the menu is still down when the field opens:
    // its repeat is not a second Enter, and it goes no further than the field.
    #[test]
    fn a_held_enter_stores_nothing_and_reaches_nothing_behind_the_field() {
        let enter = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        // egui decides for itself what a repeat is: the key went down before the field
        // was there, and has not come up.
        let mut f = Fixture::new(3);
        f.frame(vec![enter(true)]);
        f.name(Of::Album(4), "Best of");
        f.frame(vec![enter(true)]);
        assert_eq!(f.last.named, []);
        assert_eq!(f.left_over, []);
        assert!(f.in_field());
        assert_eq!(f.stores, 0);
        // Let go of and pressed again, it stores.
        f.frame(vec![enter(false)]);
        f.frame(vec![enter(true)]);
        assert_eq!(f.last.named, [Named::Commit]);
    }

    // One field under one name for every row, and egui keeps a field's history by that
    // name: Ctrl+Z in a field just opened must not bring back what was typed for the
    // row before.
    #[test]
    fn undo_in_a_new_field_does_not_bring_back_another_rows_name() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        // Typed with pauses, as a person types: egui notes a point to go back to when
        // the text has rested.
        for letter in ["P", "o", "r", "t", "o"] {
            f.typed(letter);
            for _ in 0..90 {
                f.frame(Vec::new());
            }
        }
        f.naming = None;
        f.frame(Vec::new());
        f.frame(Vec::new());
        f.name(Of::Folder(2), "folder-0002");
        for _ in 0..90 {
            f.frame(Vec::new());
        }
        let undo = Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        };
        f.frame(vec![undo]);
        assert_eq!(f.last.named, []);
        assert!(f.drew("folder-0002").is_some());
    }

    // The other button in a text field is the field's own: a menu of the row under it
    // took the keyboard, which stored the name half typed.
    #[test]
    fn the_other_button_in_the_field_asks_for_no_menu() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        let inside = f.drew("Best of").unwrap().center();
        assert_eq!(f.other_down(inside), None);
        assert_eq!(f.stores, 0);
        assert!(f.in_field());
        // On another row it does, as ever.
        let other = f.band(&folder(2)).center();
        f.naming = None;
        f.frame(Vec::new());
        assert!(f.other_down(other).is_some());
    }

    // A field opened in another row's place while one is open is another field: it
    // takes the keyboard, with its own name selected.
    #[test]
    fn a_field_opened_in_another_rows_place_starts_over() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        // The arrows are the field's: they move the caret, and the keyboard stays.
        f.key(Key::ArrowRight);
        assert_eq!(f.selected(), Some((7, 7)));
        f.key(Key::ArrowLeft);
        f.key(Key::ArrowDown);
        f.key(Key::ArrowUp);
        assert!(f.in_field());
        f.key(Key::Home);
        assert_eq!(f.typed("!"), [Named::Typed("!Best of".to_owned())]);
        f.key(Key::End);
        assert_eq!(f.selected(), Some((8, 8)));
        f.name(Of::Folder(2), "folder-0002");
        assert!(f.in_field());
        assert_eq!(f.selected(), Some((0, 11)));
        // The album's row is a row again, under its own name: nothing was stored.
        assert!(f.drew("Best of").is_some());
    }

    // It is the user's then: the row under the caret must not move.
    #[test]
    fn the_list_does_not_follow_the_grid_while_a_name_is_typed() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        f.here = Some(150);
        f.frame(Vec::new());
        assert_eq!(f.view.position(), 0.0);
        // With the field closed it goes where the grid is.
        f.naming = None;
        f.frame(Vec::new());
        let bottom = f.view.stack.bottom(f.index(&folder(150)));
        assert_eq!(f.view.position(), bottom - 600.0);
    }

    // The lists are read again when a name is stored, and a row made a moment ago is in
    // them: the field of the next name must stay under the caret.
    #[test]
    fn rows_arriving_above_an_open_field_leave_it_where_it_is() {
        let mut f = Fixture::new(40);
        f.frame(Vec::new());
        // Far from the first row in view, and with the pointer nowhere near.
        f.wheel(pos2(100.0, 300.0), -200.0);
        f.pointer(pos2(700.0, 300.0));
        f.name(Of::Folder(12), "folder-0012");
        let before = f.band(&folder(12)).top();
        more_albums(&mut f);
        f.frame(Vec::new());
        assert_eq!(f.band(&folder(12)).top(), before);
        assert!(f.in_field());
    }

    // A field whose row is not drawn cannot be typed in, and egui has taken the keyboard
    // off a widget that is gone: its name is stored, as leaving it stores it. And with no
    // entry left to stand in for, it closes.
    #[test]
    fn a_field_scrolled_out_of_sight_is_stored_and_one_whose_entry_is_gone_closes() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        f.wheel(pos2(100.0, 300.0), -2000.0);
        assert_eq!(f.stores, 1);

        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        f.world.collections.albums.remove(0);
        f.rebuild();
        f.frame(Vec::new());
        assert_eq!(f.last.named, [Named::Cancel]);
    }

    // A name the library refuses leaves the field open (`Rename::refused`), and its row
    // is still out of sight: sent again in every frame, it was refused again in every
    // frame, a write and a message each. Back in sight it is a field again, with the
    // keyboard - without it, what was typed "into" it was the grid's.
    #[test]
    fn a_name_out_of_sight_is_sent_once_and_its_field_is_opened_again_in_sight() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        f.wheel(pos2(100.0, 300.0), -2000.0);
        assert_eq!(f.stores, 1);
        assert!(!f.in_field());
        // Being stored, and then refused: not being stored any more, and still open.
        f.naming.as_mut().unwrap().2 = true;
        f.frame(Vec::new());
        f.naming.as_mut().unwrap().2 = false;
        for _ in 0..5 {
            f.frame(Vec::new());
        }
        assert_eq!(f.stores, 1);
        // Brought back into sight, it has the keyboard and its name selected.
        f.wheel(pos2(100.0, 300.0), 2000.0);
        assert!(f.drew("Best of").is_some());
        assert!(f.in_field());
        assert_eq!(f.selected(), Some((0, 7)));
        assert_eq!(f.stores, 1);
        // And out of sight once more, it is sent once more.
        f.wheel(pos2(100.0, 300.0), -2000.0);
        assert_eq!(f.stores, 2);
        // Closed there, and opened anew for the same row before it was ever drawn
        // again: another field, which is sent too, and not left open where nobody
        // sees it.
        f.naming = None;
        f.frame(Vec::new());
        f.naming = Some((Of::Album(4), "Best of".to_owned(), false));
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.stores, 3);
    }

    // A sidebar that is hidden is not drawn, and says so: the field of a name the
    // library refused under it is opened again when the sidebar is back.
    #[test]
    fn a_field_under_a_hidden_sidebar_takes_the_keyboard_when_it_is_shown_again() {
        let mut f = Fixture::new(3);
        f.frame(Vec::new());
        f.name(Of::Album(4), "Best of");
        // Hidden: nothing of it is drawn, and egui takes the keyboard off a widget
        // that is gone.
        f.view.not_drawn();
        f.ctx
            .memory_mut(|memory| memory.surrender_focus(field_id()));
        f.frame(Vec::new());
        assert!(f.in_field());
        assert_eq!(f.selected(), Some((0, 7)));
    }

    // A name asked for from a header in the grid: its row is far down the list.
    #[test]
    fn an_entry_asked_for_is_brought_into_sight_once() {
        let mut f = Fixture::new(200);
        f.frame(Vec::new());
        f.view.reveal(folder(150));
        f.frame(Vec::new());
        let bottom = f.view.stack.bottom(f.index(&folder(150)));
        assert_eq!(f.view.position(), bottom - 600.0);
        // Once: the list is the user's again after.
        f.wheel(pos2(100.0, 300.0), 400.0);
        assert!(f.view.position() < bottom - 600.0);
        // One the list does not hold moves nothing.
        let before = f.view.position();
        f.view.reveal(folder(9999));
        f.frame(Vec::new());
        assert_eq!(f.view.position(), before);
    }
}
