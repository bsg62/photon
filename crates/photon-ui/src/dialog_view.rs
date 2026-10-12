//! Draws the dialog that is open, over the whole window, and answers what was done in
//! it: the question asked before something is deleted, the keyword picker, and the
//! export's dialog, whose body is `export_view.rs`'s.
//!
//! It is egui's `Modal`: an area in front of everything with a ground over the whole
//! window. From the frame after it is first shown nothing under it is pressed, hovered
//! or given the keyboard, so Tab stays inside it - and wherever egui is asked which layer
//! is on top, it is the dialog's, which is what the grid and the sidebar ask of a press.
//!
//! What `Modal` does not keep from what lies behind is the keys: the grid, the search
//! box and the shell read them from the frame's events and not through a widget. So the
//! application calls this **before anything else is drawn in the frame**, as it does a
//! menu, and when the dialog has read its keys every key press and everything typed is
//! taken out of the frame's input.

use crate::{
    dialogs::{CANCEL, Confirm, DELETE, Dialog},
    export_view,
    icons::Icon,
    tag_picker::{LIST, NO_KEYWORDS, NO_MATCH, TagPicker},
    text::paint_line,
    theme::{
        apply::{color, dialog_shadow, palette},
        fonts,
        tokens::{Palette, R, S, SCRIM, T},
    },
};
use eframe::egui::{
    self, Event, Id, Key, Modifiers, Rect, Response, Sense, Stroke, StrokeKind, UiBuilder, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};

/// A dialog is at most this wide, and the keyword picker at most this tall. The export's
/// is the Svelte dialog's own width, which is what its five sizes need in a row.
const WIDTH: f32 = 420.0;
const WIDTH_EXPORT: f32 = 440.0;
const HEIGHT: f32 = 480.0;
/// The room between the dialog's edge and what is in it, and between its parts.
pub(crate) const PAD: f32 = S[3];
pub(crate) const GAP: f32 = S[2];
/// A button, a field and a row of the list: their line and six points over and under.
pub(crate) const CONTROL: f32 = 30.0;
/// The square a dialog's close button is.
pub(crate) const CLOSE: f32 = 28.0;

/// What a frame of an open dialog came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// It is still open.
    Open,
    /// It was closed and nothing is to be done: Escape, "Cancel", a press on the scrim.
    Closed,
    /// The question was answered with yes.
    Confirmed,
    /// What the keyword picker's field holds now.
    Typed(String),
    /// A keyword is to be written: `tag`, a row of the list, or - none - what is typed.
    /// With what the field holds now, when that changed in this same frame: a letter
    /// and the Enter after it arrive together in a frame that was long, and the keyword
    /// written is the one with the letter in it.
    Submit {
        tag: Option<String>,
        typed: Option<String>,
    },
    /// What the export's field holds now: a folder's path, to be asked about.
    Path(String),
    /// The export's checkbox was ticked, or unticked.
    Tick(bool),
    /// One of the export's sizes was chosen: the longest edge a copy may have, or none
    /// for the photo's own.
    Size(Option<u32>),
    /// The export is to run, to the destination that has passed.
    Export,
}

#[derive(Default)]
pub struct DialogView {
    /// Whether the dialog on screen has been given the keyboard: it is asked for once,
    /// in the frame the dialog is first drawn.
    opened: bool,
    /// When the dialog was first drawn, on egui's clock.
    opened_at: Option<f64>,
    /// A button went down so soon after that it is the second press of a double click
    /// on whatever opened the dialog: until it comes up, nothing is done in the dialog.
    deaf: bool,
}

fn field_id() -> Id {
    Id::new("dialog-keyword")
}

/// How a button of a dialog looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// The one that does what the dialog is for.
    Primary,
    /// The one that deletes.
    Danger,
    Plain,
    /// A primary one with nothing to do yet.
    Off,
    /// A row of the list: no ground until the pointer is on it.
    Row,
}

/// A button of a dialog, at `rect`. It takes the keyboard like any button: Tab walks to
/// it, and Enter and Space on it are a click.
pub(crate) fn button(ui: &mut egui::Ui, rect: Rect, id: Id, label: &str, look: Look) -> Response {
    let palette = palette(ui.ctx());
    let works = look != Look::Off;
    let sense = if works {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, works, label));
    let (ground, words) = match look {
        Look::Primary => (Some(palette.accent), palette.on_accent),
        Look::Danger => (Some(palette.danger), palette.on_accent),
        Look::Plain if response.hovered() => (Some(palette.field_hover), palette.text),
        Look::Plain => (Some(palette.field), palette.text),
        Look::Off => (Some(palette.field), palette.text_dim),
        Look::Row if response.hovered() => (Some(palette.hover), palette.text),
        Look::Row => (None, palette.text),
    };
    let painter = ui.painter().clone();
    if let Some(ground) = ground {
        painter.rect_filled(rect, R[1], color(ground));
    }
    // Which button the keys are on has to be seen: Enter presses it.
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            R[1] + 2.0,
            Stroke::new(2.0, color(palette.accent)),
            StrokeKind::Inside,
        );
    }
    let font = fonts::regular(T[2]);
    if look == Look::Row {
        let left = rect.left() + S[1];
        let room = rect.right() - S[1] - left;
        paint_line(
            ui,
            &painter,
            (left, rect.center().y),
            room,
            label,
            font,
            color(words),
        );
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            color(words),
        );
    }
    response
}

/// The cross at a dialog's top right corner, at `rect`: whether it was pressed. Made
/// last, although it is drawn at the top: it is the last thing one wants to tab to.
pub(crate) fn close(ui: &mut egui::Ui, rect: Rect, palette: &Palette) -> bool {
    let closed = ui.interact(rect, Id::new("dialog-close"), Sense::click());
    closed.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, CANCEL));
    if closed.hovered() {
        ui.painter().rect_filled(rect, R[1], color(palette.hover));
    }
    // Which button the keys are on has to be seen, here as on every other: with no
    // keyword listed Tab goes from the picker's field straight to this one, and Enter
    // closes.
    if closed.has_focus() {
        ui.painter().rect_stroke(
            rect,
            R[1],
            Stroke::new(2.0, color(palette.accent)),
            StrokeKind::Inside,
        );
    }
    let tint = if closed.hovered() {
        palette.text
    } else {
        palette.text_dim
    };
    Icon::X.paint(ui, rect, 16.0, false, color(tint));
    closed.clicked()
}

/// How wide a button with `label` on it is.
pub(crate) fn button_width(ui: &egui::Ui, label: &str) -> f32 {
    let words =
        ui.painter()
            .layout_no_wrap(label.to_owned(), fonts::regular(T[2]), egui::Color32::WHITE);
    words.size().x.ceil() + 2.0 * S[2]
}

impl DialogView {
    /// Forgets that a dialog was given the keyboard: for one that has just been opened.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Draws `dialog` over the window and answers what the frame came to. `keywords`
    /// are the library's, in its order, for the picker's list.
    pub fn show(&mut self, ui: &egui::Ui, dialog: &Dialog, keywords: &[String]) -> Done {
        let ctx = ui.ctx();
        let palette = palette(ctx);
        // Escape closes it, whatever in it has the keyboard, and is nobody's after -
        // and neither is a key that came in the same frame, which was typed at a
        // dialog: left in the frame, an `h` after the Escape hid a photo.
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            take_keys(ctx);
            return Done::Closed;
        }
        // A key held when the dialog opens presses nothing in it: egui presses the
        // button the keyboard is on at every Enter and Space, repeats among them, and
        // the Enter that picked "Delete…" in a menu closed the question it had asked.
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(
                    event,
                    Event::Key {
                        key: Key::Enter | Key::Space,
                        pressed: true,
                        repeat: true,
                        ..
                    }
                )
            });
        });
        // A double click on a menu's item opens the dialog at its first click, and its
        // second lands on whatever the dialog has put under the pointer: a row of the
        // keyword list, which writes at a click, or "Delete". A button that goes down
        // within the time egui takes two clicks for one double click is that second
        // press, and nothing is done in the dialog until it has come up again.
        let (now, pressed, released) = ctx.input(|input| {
            (
                input.time,
                input.pointer.any_pressed(),
                input.pointer.any_released(),
            )
        });
        let double = ctx.options(|options| options.input_options.max_double_click_delay);
        let since = now - *self.opened_at.get_or_insert(now);
        if pressed && since < double {
            self.deaf = true;
        }
        let deaf = self.deaf;
        if released {
            self.deaf = false;
        }
        let window = ctx.content_rect();
        let widest = match dialog {
            Dialog::Export(_) => WIDTH_EXPORT,
            Dialog::Confirm(_) | Dialog::Tags(_) => WIDTH,
        };
        let width = widest.min(window.width() - 2.0 * PAD).max(0.0);
        let inner = width - 2.0 * PAD;
        let height = match dialog {
            Dialog::Confirm(confirm) => confirm_height(ui, confirm, inner),
            Dialog::Tags(picker) => {
                picker_height(picker.suggestions(keywords).len(), window.height())
            }
            Dialog::Export(export) => export_view::height(ui, export, inner, palette),
        };
        let opened = !std::mem::replace(&mut self.opened, true);
        let mut done = Done::Open;
        let mut ground = Rect::NOTHING;
        let id = Id::new("dialog");
        // In front of everything, as `Modal`'s own area is, but not anchored as that
        // one is: in the middle of the window by the height it has in this frame. An
        // anchored area is placed by the size it had in the frame before - and the
        // export's dialog changes height when what is said under its field needs more
        // lines or fewer. In that frame it was drawn with its top where it had been, a
        // press on "Export" where the button had stood was a press beside the dialog,
        // which closes it, and it was put in the middle a frame later.
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(window.center() - vec2(width, height) / 2.0)
            // There at once, as a dialog is: egui fades a new area in over a few
            // frames, each of them asked for.
            .fade_in(false);
        egui::Modal::new(id)
            .area(area)
            .backdrop_color(color(SCRIM))
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                ground = rect;
                let painter = ui.painter().clone();
                painter.add(dialog_shadow().as_shape(rect, R[3]));
                painter.rect_filled(rect, R[3], color(palette.surface));
                painter.rect_stroke(
                    rect,
                    R[3],
                    Stroke::new(1.0, color(palette.line)),
                    StrokeKind::Outside,
                );
                let room = rect.shrink(PAD);
                done = match dialog {
                    Dialog::Confirm(confirm) => self::confirm(ui, room, confirm, palette, opened),
                    Dialog::Tags(picker) => tags(ui, room, picker, keywords, palette, opened),
                    Dialog::Export(export) => export_view::show(ui, room, export, palette, opened),
                };
            });
        // A press on the scrim closes it, at the press and not at the click, as a menu
        // closes: a press that becomes a drag is never a click.
        let pressed = ctx.input(|input| {
            (input.pointer.any_pressed())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if done == Done::Open && pressed.is_some_and(|at| !ground.contains(at)) {
            done = Done::Closed;
        }
        // The second press of a double click does nothing; what was typed stands.
        if deaf {
            done = match done {
                Done::Typed(text) => Done::Typed(text),
                Done::Submit {
                    typed: Some(text), ..
                } => Done::Typed(text),
                Done::Path(text) => Done::Path(text),
                _ => Done::Open,
            };
        }
        take_keys(ctx);
        done
    }
}

/// The keys were the dialog's, and it has read them: nothing drawn after it - the
/// search box, the sidebar, the grid - reads one, or anything typed. Nor a button going
/// down: the grid and the sidebar's menus read that from the frame's events, and in the
/// dialog's first frame nothing else keeps a press from them - a right press there
/// opened a photo's menu over the dialog, and its "Hide photo" hid a photo behind it.
fn take_keys(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                Event::Key { pressed: true, .. }
                    | Event::PointerButton { pressed: true, .. }
                    | Event::Text(_)
                    | Event::Copy
                    | Event::Cut
                    | Event::Paste(_)
            )
        });
    });
}

/// The question's words, wrapped to the dialog.
fn question(
    ui: &egui::Ui,
    confirm: &Confirm,
    width: f32,
    palette: &Palette,
) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout(
        confirm.question(),
        fonts::regular(T[2]),
        color(palette.text),
        width,
    )
}

fn title_height(ui: &egui::Ui) -> f32 {
    let font = fonts::semibold(ui.ctx(), T[3]);
    ui.ctx().fonts_mut(|fonts| fonts.row_height(&font)).ceil()
}

fn confirm_height(ui: &egui::Ui, confirm: &Confirm, inner: f32) -> f32 {
    let palette = palette(ui.ctx());
    let words = question(ui, confirm, inner, palette).size().y.ceil();
    2.0 * PAD + title_height(ui) + GAP + words + PAD + CONTROL
}

/// The question: its title, its words, and the two buttons at the trailing edge, the one
/// that does nothing first in reading order and with the keyboard on it.
fn confirm(
    ui: &mut egui::Ui,
    room: Rect,
    confirm: &Confirm,
    palette: &Palette,
    opened: bool,
) -> Done {
    let painter = ui.painter().clone();
    let title = title_height(ui);
    paint_line(
        ui,
        &painter,
        (room.left(), room.top() + title / 2.0),
        room.width(),
        confirm.title(),
        fonts::semibold(ui.ctx(), T[3]),
        color(palette.text),
    );
    let words = question(ui, confirm, room.width(), palette);
    painter.galley(
        pos2(room.left(), room.top() + title + GAP),
        words,
        color(palette.text),
    );
    ui.interact(room, Id::new("dialog-question"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, confirm.question()));

    let delete = button_width(ui, DELETE);
    let cancel = button_width(ui, CANCEL);
    let row = Rect::from_min_max(pos2(room.left(), room.bottom() - CONTROL), room.max);
    let delete_at = Rect::from_min_max(pos2(row.right() - delete, row.top()), row.max);
    let cancel_at = Rect::from_min_max(
        pos2(delete_at.left() - S[1] - cancel, row.top()),
        pos2(delete_at.left() - S[1], row.bottom()),
    );
    // Cancel first: Tab goes from it to Delete, and it has the keyboard as the dialog
    // opens, so an Enter that came with the dialog deletes nothing.
    let cancelled = button(ui, cancel_at, Id::new("dialog-cancel"), CANCEL, Look::Plain);
    if opened {
        cancelled.request_focus();
    }
    let deleted = button(
        ui,
        delete_at,
        Id::new("dialog-delete"),
        DELETE,
        Look::Danger,
    );
    if deleted.clicked() {
        Done::Confirmed
    } else if cancelled.clicked() {
        Done::Closed
    } else {
        Done::Open
    }
}

/// How tall the picker is with `rows` keywords listed: as tall as they need, the line
/// that says there are none being one row, and no taller than the window lets it be.
fn picker_height(rows: usize, window: f32) -> f32 {
    let list = rows.max(1) as f32 * CONTROL;
    let whole = 2.0 * PAD + CLOSE + GAP + CONTROL + GAP + list;
    whole.min(HEIGHT).min(window - 2.0 * PAD)
}

/// The keyword picker: its title and the button that closes it, the field with the
/// button that writes what is typed, and the library's keywords narrowed by it.
fn tags(
    ui: &mut egui::Ui,
    room: Rect,
    picker: &TagPicker,
    keywords: &[String],
    palette: &Palette,
    opened: bool,
) -> Done {
    let painter = ui.painter().clone();
    let mode = picker.mode();

    // Enter in the field writes what is typed. Read before the field is drawn and taken
    // out of the frame: egui's field gives the keyboard up at Enter, and a name that is
    // blank writes nothing and leaves the dialog as it was, the caret still in it.
    let id = field_id();
    let in_field = ui.memory(|memory| memory.has_focus(id));
    let mut submit = None;
    if in_field && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
        submit = Some(None);
    }

    let header = Rect::from_min_max(room.min, pos2(room.right(), room.top() + CLOSE));
    let close_at = Rect::from_min_max(pos2(header.right() - CLOSE, header.top()), header.max);
    paint_line(
        ui,
        &painter,
        (header.left(), header.center().y),
        close_at.left() - S[2] - header.left(),
        &mode.title(picker.count()),
        fonts::semibold(ui.ctx(), T[3]),
        color(palette.text),
    );

    let form = Rect::from_min_size(
        pos2(room.left(), header.bottom() + GAP),
        vec2(room.width(), CONTROL),
    );
    let write = button_width(ui, mode.button());
    let write_at = Rect::from_min_max(pos2(form.right() - write, form.top()), form.max);
    let field = Rect::from_min_max(form.min, pos2(write_at.left() - S[1], form.bottom()));
    painter.rect_filled(field, R[1], color(palette.field));
    let font = fonts::regular(T[2]);
    let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let edit = Rect::from_min_max(
        pos2(field.left() + S[1], field.center().y - line / 2.0),
        pos2(field.right() - S[1], field.center().y + line / 2.0),
    );
    let mut text = picker.draft().to_owned();
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(edit), |ui| {
            let room = Rect::from_x_y_ranges(edit.x_range(), field.y_range());
            ui.set_clip_rect(room.intersect(ui.clip_rect()));
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(font)
                .text_color(color(palette.text))
                .hint_text(egui::RichText::new(mode.placeholder()).color(color(palette.text_dim)))
                .desired_width(edit.width())
                .show(ui)
        })
        .inner;
    // Named, and not only hinted at: egui hands a field's hint to a screen reader as its
    // placeholder, which is what an empty field already shows.
    output.response.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, "", picker.draft(), mode.placeholder());
        info.label = Some(mode.field().to_owned());
        info
    });
    if opened {
        // The keyboard in the field as the dialog opens: it is what the dialog is for.
        // And a history of its own: the field has one id for every picker that is
        // opened, egui keeps a field's undo by its id, and Ctrl+Z in a new picker
        // brought back the keyword typed in the one before.
        output.response.request_focus();
        egui::text_edit::TextEditState::default().store(ui.ctx(), id);
    }
    let typed = output.response.changed();

    // After the field, which Tab leaves for it; and before the close button, which is
    // drawn at the top but is the last thing one wants to tab to.
    let blank = picker.draft().trim().is_empty();
    let look = if blank { Look::Off } else { Look::Primary };
    if button(ui, write_at, Id::new("dialog-write"), mode.button(), look).clicked() {
        submit = Some(None);
    }

    // The library's own keywords, narrowed as it is typed: how a second "Beach" beside
    // "beach" is avoided.
    let list = Rect::from_min_max(pos2(room.left(), form.bottom() + GAP), room.max);
    let suggestions = picker.suggestions(keywords);
    if suggestions.is_empty() {
        let none = if keywords.is_empty() {
            NO_KEYWORDS
        } else {
            NO_MATCH
        };
        painter.text(
            pos2(list.left() + S[1], list.top() + CONTROL / 2.0),
            egui::Align2::LEFT_CENTER,
            none,
            fonts::regular(T[1]),
            color(palette.text_dim),
        );
        ui.interact(list, Id::new("dialog-none"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, none));
    } else {
        let mut picked = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ui.set_clip_rect(list.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical()
                .id_salt(LIST)
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for tag in &suggestions {
                        let (row, _) = ui.allocate_exact_size(
                            vec2(ui.available_width(), CONTROL),
                            Sense::hover(),
                        );
                        // Named by its words: the list is narrowed as it is typed, and a
                        // press on a row that is another by the time the button comes up
                        // is no click of the new one.
                        let id = Id::new(("dialog-row", *tag));
                        if button(ui, row, id, tag, Look::Row).clicked() {
                            picked = Some((*tag).to_owned());
                        }
                    }
                });
        });
        if let Some(tag) = picked {
            submit = Some(Some(tag));
        }
    }

    if close(ui, close_at, palette) {
        return Done::Closed;
    }
    let typed = typed.then_some(text);
    match (submit, typed) {
        (Some(tag), typed) => Done::Submit { tag, typed },
        (None, Some(text)) => Done::Typed(text),
        (None, None) => Done::Open,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tag_picker::Mode;
    use eframe::egui::{Color32, PointerButton, Pos2, RawInput};

    pub(crate) const WINDOW: Vec2 = vec2(1000.0, 700.0);

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    /// A dialog over a window that has a button and a text field in it.
    pub(crate) struct Fixture {
        pub(crate) ctx: egui::Context,
        pub(crate) view: DialogView,
        pub(crate) dialog: Dialog,
        pub(crate) keywords: Vec<String>,
        /// The window's size.
        pub(crate) size: Vec2,
        pub(crate) time: f64,
        /// What is typed in the field behind the dialog, and whether its button was
        /// clicked in the last frame.
        pub(crate) behind: String,
        pub(crate) behind_clicked: bool,
        /// Whether the window behind asks for the keyboard this frame.
        pub(crate) behind_asks: bool,
        /// The keys the frame's input still held after the dialog had read it, and
        /// whether anything typed was left.
        pub(crate) left_over: Vec<Key>,
        pub(crate) typed_left_over: bool,
        /// How many buttons going down the frame's input still held after it.
        pub(crate) presses_left_over: usize,
        pub(crate) texts: Vec<(String, Rect)>,
        pub(crate) fills: Vec<(Rect, Color32)>,
        pub(crate) repaint: std::time::Duration,
    }

    fn confirm_album() -> Dialog {
        Dialog::Confirm(Confirm::Album {
            id: 4,
            name: "Lisbon".to_owned(),
        })
    }

    fn picker(keywords: &[&str]) -> Fixture {
        let mut f = Fixture::new(Dialog::Tags(TagPicker::new(Mode::Add, vec![1, 2, 3])));
        f.keywords = keywords.iter().map(|tag| (*tag).to_owned()).collect();
        f
    }

    impl Fixture {
        pub(crate) fn new(dialog: Dialog) -> Self {
            Self {
                ctx: egui::Context::default(),
                view: DialogView::default(),
                dialog,
                keywords: Vec::new(),
                size: WINDOW,
                time: 0.0,
                behind: String::new(),
                behind_clicked: false,
                behind_asks: false,
                left_over: Vec::new(),
                typed_left_over: false,
                presses_left_over: 0,
                texts: Vec::new(),
                fills: Vec::new(),
                repaint: std::time::Duration::MAX,
            }
        }

        pub(crate) fn frame(&mut self, events: Vec<Event>) -> Done {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let (view, dialog, keywords) = (&mut self.view, &self.dialog, &self.keywords);
            let behind = &mut self.behind;
            let asks = self.behind_asks;
            let mut done = Done::Open;
            let mut clicked = false;
            let (mut left_over, mut typed, mut presses) = (Vec::new(), false, 0);
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                done = view.show(ui, dialog, keywords);
                egui::CentralPanel::default().show(ui, |ui| {
                    let at = Rect::from_min_size(pos2(40.0, 40.0), vec2(160.0, 40.0));
                    clicked = ui.put(at, egui::Button::new("behind")).clicked();
                    let at = Rect::from_min_size(pos2(40.0, 100.0), vec2(160.0, 24.0));
                    let field = ui.put(at, egui::TextEdit::singleline(behind));
                    if asks {
                        field.request_focus();
                    }
                    ui.input(|input| {
                        for event in &input.events {
                            match event {
                                Event::Key {
                                    key, pressed: true, ..
                                } => left_over.push(*key),
                                Event::Text(_) | Event::Paste(_) | Event::Copy | Event::Cut => {
                                    typed = true;
                                }
                                Event::PointerButton { pressed: true, .. } => presses += 1,
                                _ => {}
                            }
                        }
                    });
                });
            });
            full.textures_delta.clear();
            // What the picker's field holds is the dialog's from now on, as the
            // application keeps it.
            let holds = match &done {
                Done::Typed(text) => Some(text),
                Done::Submit { typed, .. } => typed.as_ref(),
                _ => None,
            };
            if let (Some(text), Dialog::Tags(picker)) = (holds, &mut self.dialog) {
                picker.type_in(text.clone());
            }
            // And the export's dialog is told what was done in it, as the application
            // tells it. The path is asked about by the test, which is the engine here.
            if let Dialog::Export(export) = &mut self.dialog {
                match &done {
                    Done::Path(text) => {
                        export.type_in(text.clone());
                    }
                    Done::Tick(apply) => export.tick(*apply),
                    Done::Size(max_edge) => export.size(*max_edge),
                    _ => {}
                }
            }
            self.behind_clicked = clicked;
            self.left_over = left_over;
            self.typed_left_over = typed;
            self.presses_left_over = presses;
            self.texts.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => self
                        .texts
                        .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                    egui::Shape::Rect(rect) => self.fills.push((rect.rect, rect.fill)),
                    _ => {}
                });
            }
            self.repaint = full
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay);
            done
        }

        /// The dialog's first frames: an area is measured in one frame and shown in
        /// the next, and what lies under it is kept from the pointer from the one after.
        /// Draws the dialog until it stands, and lets the time pass in which a press
        /// would be the second of a double click on whatever opened it.
        pub(crate) fn open(&mut self) {
            for _ in 0..3 {
                assert_eq!(self.frame(Vec::new()), Done::Open);
            }
            self.time += 1.0;
        }

        pub(crate) fn key_with(&mut self, key: Key, modifiers: Modifiers) -> Done {
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            };
            let done = self.frame(vec![event(true)]);
            let (left_over, typed) = (self.left_over.clone(), self.typed_left_over);
            if done == Done::Open || matches!(done, Done::Typed(_)) {
                self.frame(vec![event(false)]);
            }
            self.left_over = left_over;
            self.typed_left_over = typed;
            done
        }

        pub(crate) fn key(&mut self, key: Key) -> Done {
            self.key_with(key, Modifiers::NONE)
        }

        pub(crate) fn typed(&mut self, text: &str) -> Done {
            self.frame(vec![Event::Text(text.to_owned())])
        }

        pub(crate) fn button(&mut self, at: Pos2, button: PointerButton, pressed: bool) -> Done {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        /// A click at `at`: what the frame of the press came to, or the release's when
        /// the press did nothing.
        pub(crate) fn click(&mut self, at: Pos2) -> Done {
            self.frame(vec![Event::PointerMoved(at)]);
            match self.button(at, PointerButton::Primary, true) {
                Done::Open => self.button(at, PointerButton::Primary, false),
                done => done,
            }
        }

        pub(crate) fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }

        pub(crate) fn at(&self, text: &str) -> Pos2 {
            self.drew(text)
                .unwrap_or_else(|| panic!("{text} is drawn"))
                .center()
        }

        pub(crate) fn tint(&self, of: impl Fn(&Palette) -> crate::theme::tokens::Rgba) -> Color32 {
            color(of(palette(&self.ctx)))
        }

        /// The dialog's own rectangle: the one filled in the surface's colour.
        pub(crate) fn ground(&self) -> Rect {
            let surface = self.tint(|palette| palette.surface);
            (self.fills.iter())
                .filter(|(rect, fill)| *fill == surface && rect.width() > 300.0)
                .map(|(rect, _)| *rect)
                .next()
                .expect("the dialog is drawn")
        }

        pub(crate) fn focused(&self) -> Option<Id> {
            self.ctx.memory(|memory| memory.focused())
        }
    }

    #[test]
    fn a_dialog_stands_in_the_middle_of_the_window_over_a_scrim() {
        let mut f = Fixture::new(confirm_album());
        f.open();
        let whole = Rect::from_min_size(Pos2::ZERO, WINDOW);
        assert!(f.fills.contains(&(whole, color(SCRIM))));
        let ground = f.ground();
        assert_eq!(ground.width(), WIDTH);
        assert!(
            (ground.center() - whole.center()).length() < 1.0,
            "{ground:?}"
        );
        // Its title over its question, and the two buttons at the trailing edge under.
        let title = f.drew("Delete album").unwrap();
        let question = f
            .drew("Delete the album “Lisbon”? The photos stay in your library.")
            .unwrap();
        let (cancel, delete) = (f.drew(CANCEL).unwrap(), f.drew(DELETE).unwrap());
        assert!(ground.contains_rect(title) && ground.contains_rect(question));
        assert!(title.bottom() < question.top() && question.bottom() < cancel.top());
        assert!(cancel.right() < delete.left());
        assert!(delete.right() < ground.right() && delete.right() > ground.right() - 40.0);
        assert!((cancel.center().y - delete.center().y).abs() < 1.0);
    }

    // A long name wraps, and the dialog is as tall as its words need.
    #[test]
    fn a_long_question_is_wrapped_inside_the_dialog() {
        let mut f = Fixture::new(Dialog::Confirm(Confirm::Search {
            id: 1,
            name: "everything taken near the lake in the summer of 2024 ".repeat(4),
        }));
        f.open();
        let ground = f.ground();
        let question = (f.texts.iter())
            .find(|(text, _)| text.starts_with("Delete the saved search"))
            .map(|(_, place)| *place)
            .expect("the question is drawn");
        assert!(ground.contains_rect(question), "{question:?} in {ground:?}");
        assert!(question.height() > 40.0);
        let short = {
            let mut f = Fixture::new(confirm_album());
            f.open();
            f.ground().height()
        };
        assert!(ground.height() > short + 30.0);
    }

    // The keyboard is on "Cancel" as it opens: an Enter that came with the dialog - the
    // one that picked "Delete…" in the menu, still on its way down - deletes nothing.
    #[test]
    fn the_question_opens_with_the_keyboard_on_cancel() {
        let mut f = Fixture::new(confirm_album());
        f.open();
        assert_eq!(f.focused(), Some(Id::new("dialog-cancel")));
        assert_eq!(f.key(Key::Enter), Done::Closed);
        let mut f = Fixture::new(confirm_album());
        f.open();
        assert_eq!(f.key(Key::Space), Done::Closed);
        // Tab goes to "Delete", where Enter deletes; and on from there back to
        // "Cancel": it does not leave the dialog for the window behind.
        let mut f = Fixture::new(confirm_album());
        f.open();
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-delete")));
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-cancel")));
        f.key(Key::Tab);
        assert_eq!(f.key(Key::Enter), Done::Confirmed);
    }

    #[test]
    fn a_click_on_each_button_answers_the_question() {
        let mut f = Fixture::new(confirm_album());
        f.open();
        let at = f.at(DELETE);
        assert_eq!(f.click(at), Done::Confirmed);
        let mut f = Fixture::new(confirm_album());
        f.open();
        let at = f.at(CANCEL);
        assert_eq!(f.click(at), Done::Closed);
        // On the dialog's own ground, beside its words: nothing, and it stays.
        let mut f = Fixture::new(confirm_album());
        f.open();
        let at = f.ground().min + vec2(6.0, 6.0);
        assert_eq!(f.click(at), Done::Open);
    }

    // At the press, as a menu closes: a press that becomes a drag is never a click.
    #[test]
    fn escape_and_a_press_on_the_scrim_close_it() {
        let mut f = Fixture::new(confirm_album());
        f.open();
        assert_eq!(f.key(Key::Escape), Done::Closed);
        assert_eq!(f.left_over, []);
        for button in [PointerButton::Primary, PointerButton::Secondary] {
            let mut f = Fixture::new(confirm_album());
            f.open();
            let outside = pos2(900.0, 600.0);
            f.frame(vec![Event::PointerMoved(outside)]);
            assert_eq!(f.button(outside, button, true), Done::Closed, "{button:?}");
        }
        // The picker's, with the caret in its field.
        let mut f = picker(&["beach"]);
        f.open();
        assert_eq!(f.key(Key::Escape), Done::Closed);
        assert_eq!(f.left_over, []);
    }

    // A frame can be long, and the Escape that closes the dialog and the key typed
    // after it in it together: the key was typed at a dialog, and is not the grid's,
    // where `h` hides a photo.
    #[test]
    fn a_key_in_the_frame_of_the_escape_that_closes_it_reaches_nothing_behind() {
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut f = Fixture::new(confirm_album());
        f.open();
        let done = f.frame(vec![
            key(Key::Escape),
            key(Key::H),
            Event::Text("h".to_owned()),
        ]);
        assert_eq!(done, Done::Closed);
        assert_eq!(f.left_over, []);
        assert!(!f.typed_left_over);
    }

    // A key held when the dialog opens is not a press of the button the dialog puts the
    // keyboard on: egui presses a focused button at every Enter and Space, repeats
    // among them, and the Enter that picked "Delete…" closed the question it had asked.
    #[test]
    fn a_held_enter_or_space_presses_no_button() {
        for held in [Key::Enter, Key::Space] {
            let key = |pressed| Event::Key {
                key: held,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            };
            let mut f = Fixture::new(confirm_album());
            // Down before the dialog is there, and not let go of: egui decides for
            // itself that what follows is a repeat.
            f.frame(vec![key(true)]);
            f.open();
            for _ in 0..3 {
                assert_eq!(f.frame(vec![key(true)]), Done::Open, "{held:?}");
            }
            // Let go of and pressed, it presses "Cancel", which has the keyboard.
            f.frame(vec![key(false)]);
            assert_eq!(f.frame(vec![key(true)]), Done::Closed, "{held:?}");
        }
    }

    // Nothing under a dialog takes a press or a key: not a button, not a text field.
    #[test]
    fn nothing_behind_a_dialog_is_pressed_typed_in_or_given_the_keyboard() {
        let mut f = Fixture::new(confirm_album());
        f.behind_asks = true;
        f.frame(Vec::new());
        f.behind_asks = false;
        f.open();
        // A press on the button behind is a press on the scrim: the dialog would close,
        // and the button is not clicked.
        let behind = pos2(100.0, 60.0);
        f.frame(vec![Event::PointerMoved(behind)]);
        f.button(behind, PointerButton::Primary, true);
        f.button(behind, PointerButton::Primary, false);
        assert!(!f.behind_clicked);
        // Every key and everything typed is the dialog's, and is nobody's after it.
        for (key, held) in [
            (Key::H, Modifiers::NONE),
            (Key::Period, Modifiers::NONE),
            (Key::ArrowRight, Modifiers::NONE),
            (Key::A, Modifiers::COMMAND),
            (Key::F, Modifiers::COMMAND),
            (Key::B, Modifiers::COMMAND),
            (Key::F11, Modifiers::NONE),
        ] {
            f.key_with(key, held);
            assert_eq!(f.left_over, [], "{key:?}");
        }
        f.typed("h");
        assert!(!f.typed_left_over);
        f.frame(vec![Event::Paste("lisbon".to_owned())]);
        assert!(!f.typed_left_over);
        assert_eq!(f.behind, "");
        // Tab walks the dialog's own buttons and nothing behind.
        for _ in 0..5 {
            f.key(Key::Tab);
            let on = f.focused();
            assert!(
                on == Some(Id::new("dialog-cancel")) || on == Some(Id::new("dialog-delete")),
                "{on:?}"
            );
        }
    }

    #[test]
    fn the_picker_says_what_it_writes_to_and_opens_with_the_caret_in_its_field() {
        let mut f = picker(&["beach", "Lisbon", "sunset"]);
        f.open();
        assert_eq!(f.focused(), Some(field_id()));
        let ground = f.ground();
        let title = f.drew("Add a keyword to 3 photos").unwrap();
        let hint = f.drew("New or existing keyword").unwrap();
        let add = f.drew("Add").unwrap();
        assert!(ground.contains_rect(title));
        assert!(title.bottom() < hint.top());
        assert!((hint.center().y - add.center().y).abs() < 2.0);
        assert!(hint.right() < add.left());
        // The library's keywords under it, in its order, each a row.
        let rows: Vec<f32> = ["beach", "Lisbon", "sunset"]
            .iter()
            .map(|tag| f.drew(tag).unwrap().center().y)
            .collect();
        assert!(rows[0] > hint.bottom());
        // (To a point and a half: a word's place is measured by its own letters.)
        assert!((rows[1] - rows[0] - CONTROL).abs() < 1.5, "{rows:?}");
        assert!((rows[2] - rows[1] - CONTROL).abs() < 1.5, "{rows:?}");
        // With nothing typed there is nothing to add yet.
        let off = (f.fills.iter())
            .find(|(rect, _)| rect.contains(add.center()) && rect.width() < 120.0)
            .map(|(_, fill)| *fill);
        assert_eq!(off, Some(f.tint(|palette| palette.field)));
        // Removing says so.
        let mut f = Fixture::new(Dialog::Tags(TagPicker::new(Mode::Remove, vec![7])));
        f.open();
        assert!(f.drew("Remove a keyword from 1 photo").is_some());
        assert!(f.drew("Keyword to remove").is_some());
        assert!(f.drew("Remove").is_some());
    }

    // One field under one name for every picker that is opened, and egui keeps a
    // field's history by that name: Ctrl+Z in a picker just opened must not bring back
    // the keyword typed in the one before.
    #[test]
    fn undo_in_a_new_picker_does_not_bring_back_the_last_keyword() {
        let mut f = picker(&[]);
        f.open();
        // Typed with pauses, as a person types: egui notes a point to go back to when
        // the text has rested.
        for letter in ["p", "o", "r", "t", "o"] {
            f.typed(letter);
            for _ in 0..90 {
                f.frame(Vec::new());
            }
        }
        // Written, and the dialog closed; another is opened.
        f.dialog = Dialog::Tags(TagPicker::new(Mode::Remove, vec![1]));
        f.view.reset();
        f.open();
        for _ in 0..90 {
            f.frame(Vec::new());
        }
        assert_eq!(f.key_with(Key::Z, Modifiers::COMMAND), Done::Open);
    }

    #[test]
    fn what_is_typed_narrows_the_list_and_enter_writes_it() {
        let mut f = picker(&["beach", "Lisbon", "sunset"]);
        f.open();
        assert_eq!(f.typed("li"), Done::Typed("li".to_owned()));
        assert_eq!(f.typed("s"), Done::Typed("lis".to_owned()));
        f.frame(Vec::new());
        assert!(f.drew("Lisbon").is_some());
        assert!(f.drew("beach").is_none() && f.drew("sunset").is_none());
        // Now there is something to add, and its button says so.
        let add = f.drew("Add").unwrap();
        let ground = (f.fills.iter())
            .find(|(rect, _)| rect.contains(add.center()) && rect.width() < 120.0)
            .map(|(_, fill)| *fill);
        assert_eq!(ground, Some(f.tint(|palette| palette.accent)));
        assert_eq!(
            f.key(Key::Enter),
            Done::Submit {
                tag: None,
                typed: None,
            }
        );
        assert_eq!(f.left_over, []);
        // What matches nothing says so.
        let mut f = picker(&["beach"]);
        f.open();
        f.typed("zz");
        f.frame(Vec::new());
        assert!(f.drew(NO_MATCH).is_some());
        // And a library with no keywords says that.
        let mut f = picker(&[]);
        f.open();
        assert!(f.drew(NO_KEYWORDS).is_some());
        f.typed("zz");
        f.frame(Vec::new());
        assert!(f.drew(NO_KEYWORDS).is_some());
    }

    // A frame can be long, and a letter and the Enter after it in it together: the
    // keyword written is the one with the letter.
    #[test]
    fn a_letter_and_enter_in_one_frame_write_the_keyword_with_the_letter() {
        let mut f = picker(&["beach"]);
        f.open();
        f.typed("lisbo");
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let done = f.frame(vec![Event::Text("n".to_owned()), enter]);
        assert_eq!(
            done,
            Done::Submit {
                tag: None,
                typed: Some("lisbon".to_owned()),
            }
        );
    }

    // A blank name writes nothing: the application leaves the dialog as it is, and the
    // caret is still in its field.
    #[test]
    fn enter_on_a_blank_name_leaves_the_caret_in_the_field() {
        let mut f = picker(&["beach"]);
        f.open();
        assert_eq!(
            f.key(Key::Enter),
            Done::Submit {
                tag: None,
                typed: None,
            }
        );
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(field_id()));
        // Its button takes no press while there is nothing to write.
        let at = f.at("Add");
        assert_eq!(f.click(at), Done::Open);
    }

    #[test]
    fn a_click_on_a_row_writes_that_keyword_and_the_buttons_do_what_they_say() {
        let mut f = picker(&["beach", "Lisbon"]);
        f.open();
        f.typed("be");
        f.frame(Vec::new());
        let at = f.at("beach");
        assert_eq!(
            f.click(at),
            Done::Submit {
                tag: Some("beach".to_owned()),
                typed: None,
            }
        );
        // The button beside the field writes what is typed.
        let mut f = picker(&["beach", "Lisbon"]);
        f.open();
        f.typed("be");
        f.frame(Vec::new());
        let at = f.at("Add");
        assert_eq!(
            f.click(at),
            Done::Submit {
                tag: None,
                typed: None,
            }
        );
        // And the one in the corner closes the dialog.
        let mut f = picker(&["beach"]);
        f.open();
        let ground = f.ground();
        let corner = pos2(
            ground.right() - PAD - CLOSE / 2.0,
            ground.top() + PAD + CLOSE / 2.0,
        );
        assert_eq!(f.click(corner), Done::Closed);
    }

    // More keywords than fit: the dialog is no taller than it may be, and the wheel
    // over the list moves the list.
    #[test]
    fn a_long_list_is_held_to_the_dialogs_height_and_scrolls() {
        let names: Vec<String> = (0..60).map(|n| format!("keyword {n:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut f = picker(&names);
        f.open();
        let ground = f.ground();
        assert_eq!(ground.height(), HEIGHT);
        let inside = |f: &Fixture, text: &str| {
            let ground = f.ground();
            f.drew(text)
                .is_some_and(|place| ground.contains_rect(place))
        };
        assert!(inside(&f, "keyword 00"));
        assert!(!inside(&f, "keyword 59"));
        f.frame(vec![Event::PointerMoved(ground.center() + vec2(0.0, 80.0))]);
        for _ in 0..40 {
            f.frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -300.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
        }
        for _ in 0..60 {
            f.frame(Vec::new());
        }
        assert!(inside(&f, "keyword 59"));
        assert!(!inside(&f, "keyword 00"));
    }

    // But for the caret in the picker's field, which blinks as in any application: it
    // is switched off here, as the harness of the whole application switches it off.
    #[test]
    fn a_dialog_that_stands_asks_for_no_frame() {
        for mut f in [Fixture::new(confirm_album()), picker(&["beach"])] {
            f.ctx
                .all_styles_mut(|style| style.visuals.text_cursor.blink = false);
            // From its third frame on, with no time let go by: egui measures a new
            // area in one frame and shows it in the next - and would fade it in over
            // several more, each of them asked for.
            for _ in 0..3 {
                f.frame(Vec::new());
            }
            for _ in 0..4 {
                f.frame(Vec::new());
                assert_eq!(f.repaint, std::time::Duration::MAX);
            }
        }
    }

    // A dialog opened after another was closed is given the keyboard again.
    #[test]
    fn a_dialog_opened_again_starts_over() {
        let mut f = picker(&["beach"]);
        f.open();
        f.key(Key::Tab);
        assert_ne!(f.focused(), Some(field_id()));
        f.view.reset();
        f.dialog = Dialog::Tags(TagPicker::new(Mode::Remove, vec![1]));
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(field_id()));
    }

    // What a review of the dialogs found, each with the test that showed it.

    fn down(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    // A double click on a menu's item opens the dialog at its first click, and its
    // second lands on whatever the dialog has put under the pointer: a row of the
    // list, which writes at a click, or "Delete".
    #[test]
    fn the_second_press_of_a_double_click_does_nothing_in_a_dialog() {
        // The dialog just drawn, and no time gone by.
        let just_opened = |dialog: Fixture| {
            let mut f = dialog;
            for _ in 0..3 {
                f.frame(Vec::new());
            }
            f
        };
        let mut f = just_opened(Fixture::new(confirm_album()));
        let (delete, cancel) = (f.at(DELETE), f.at(CANCEL));
        assert_eq!(f.click(delete), Done::Open);
        assert_eq!(f.click(cancel), Done::Open);
        // Nor does it close it from beside the dialog.
        assert_eq!(f.click(pos2(900.0, 600.0)), Done::Open);
        // A click of its own, once a double click's time is over, is a click.
        f.time += 1.0;
        assert_eq!(f.click(delete), Done::Confirmed);

        // A row of the list, which writes: pressed in that time and let go of after it,
        // it is still that second press.
        let mut f = just_opened(picker(&["beach", "Lisbon"]));
        let row = f.at("beach");
        f.frame(vec![Event::PointerMoved(row)]);
        assert_eq!(f.button(row, PointerButton::Primary, true), Done::Open);
        f.time += 1.0;
        assert_eq!(f.button(row, PointerButton::Primary, false), Done::Open);
        // What is typed meanwhile is typed.
        let mut f = just_opened(picker(&["beach"]));
        f.frame(vec![Event::PointerMoved(row)]);
        let done = f.frame(vec![
            Event::PointerButton {
                pos: row,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::Text("b".to_owned()),
        ]);
        assert_eq!(done, Done::Typed("b".to_owned()));
        // With the Enter after it in the same frame, too: typed, and not written.
        let mut f = just_opened(picker(&["beach"]));
        f.frame(vec![Event::PointerMoved(row)]);
        let done = f.frame(vec![
            Event::PointerButton {
                pos: row,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::Text("b".to_owned()),
            down(Key::Enter),
        ]);
        assert_eq!(done, Done::Typed("b".to_owned()));
        // And the next click on the row is its own.
        let mut f = just_opened(picker(&["beach", "Lisbon"]));
        f.click(row);
        f.time += 1.0;
        assert_eq!(
            f.click(row),
            Done::Submit {
                tag: Some("beach".to_owned()),
                typed: None
            }
        );
    }

    // The grid and the sidebar's menus read a button going down from the frame's
    // events, and in the dialog's first frame nothing else keeps it from them.
    #[test]
    fn a_button_going_down_in_a_dialogs_frame_reaches_nothing_behind() {
        for button in [PointerButton::Primary, PointerButton::Secondary] {
            let mut f = Fixture::new(confirm_album());
            let behind = pos2(100.0, 60.0);
            // Its first frame, and one in which it stands.
            for _ in 0..2 {
                f.frame(vec![Event::PointerButton {
                    pos: behind,
                    button,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }]);
                assert_eq!(f.presses_left_over, 0, "{button:?}");
                f.frame(vec![Event::PointerButton {
                    pos: behind,
                    button,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }]);
            }
        }
    }

    // An answer and a press beside the dialog in one frame: the answer stands.
    #[test]
    fn an_answer_in_the_frame_of_a_press_on_the_scrim_stands() {
        let mut f = picker(&["beach"]);
        f.open();
        f.typed("lisbon");
        let outside = pos2(900.0, 600.0);
        let done = f.frame(vec![
            down(Key::Enter),
            Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        assert_eq!(
            done,
            Done::Submit {
                tag: None,
                typed: None
            }
        );
    }

    // Enter with the keyboard on a row of the list writes that row, and not what is
    // typed in the field the keyboard has left.
    #[test]
    fn enter_on_a_row_the_tab_key_reached_writes_that_row() {
        let mut f = picker(&["beach", "Lisbon"]);
        f.open();
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new(("dialog-row", "beach"))));
        assert_eq!(
            f.key(Key::Enter),
            Done::Submit {
                tag: Some("beach".to_owned()),
                typed: None
            }
        );
        let mut f = picker(&["beach", "Lisbon"]);
        f.open();
        f.typed("bea");
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-write")));
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new(("dialog-row", "beach"))));
        assert_eq!(
            f.key(Key::Enter),
            Done::Submit {
                tag: Some("beach".to_owned()),
                typed: None
            }
        );
    }

    // A row pressed, the list narrowed under the button that is down, and the button
    // let go of over the row that has come to stand there: no click of that one.
    #[test]
    fn a_press_on_a_row_that_is_another_when_the_button_comes_up_writes_nothing() {
        // More keywords than the picker has room for, before the list is narrowed and
        // after: it is as tall as it may be both times, and stands where it stood. (A
        // picker made lower by a shorter list is put in the middle of the window
        // again, and its rows move.)
        let mut f = picker(&[
            "beach",
            "Lima",
            "Linz",
            "Lisbon",
            "Lille",
            "Liège",
            "Limoges",
            "Livorno",
            "Limerick",
            "Lincoln",
            "Lienz",
            "Lindau",
            "Limassol",
            "Lichfield",
            "Linköping",
            "Liberec",
            "Liverpool",
        ]);
        f.open();
        assert_eq!(f.ground().height(), HEIGHT);
        let at = f.at("beach");
        f.frame(vec![Event::PointerMoved(at)]);
        assert_eq!(f.button(at, PointerButton::Primary, true), Done::Open);
        // The field still has the keyboard: egui gives it up at a click, not a press.
        assert_eq!(f.typed("li"), Done::Typed("li".to_owned()));
        f.frame(Vec::new());
        assert_eq!(f.ground().height(), HEIGHT);
        assert!(f.drew("beach").is_none());
        let now = f.drew("Lima").unwrap();
        assert!(now.expand(8.0).contains(at), "Lima stands where beach did");
        assert_eq!(f.button(at, PointerButton::Primary, false), Done::Open);
    }

    // A dialog is in front of everything that floats over the window, as `Modal`'s
    // own area is: what was there before it opened - the search box's help, a list -
    // is under its scrim, and a click on it is a press beside the dialog.
    #[test]
    fn what_floats_over_the_window_is_under_a_dialog() {
        let ctx = egui::Context::default();
        let mut view = DialogView::default();
        let dialog = confirm_album();
        let at = Rect::from_min_size(pos2(40.0, 40.0), vec2(160.0, 40.0));
        let mut time = 0.0;
        let mut frame = |events: Vec<Event>, open: bool| {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)),
                time: Some(time),
                events,
                ..Default::default()
            };
            time += 0.25;
            let (mut clicked, mut done) = (false, Done::Open);
            let mut full = ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                if open {
                    done = view.show(ui, &dialog, &[]);
                }
                egui::Area::new(Id::new("floating"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(at.min)
                    .show(ui.ctx(), |ui| {
                        clicked = ui
                            .add_sized(at.size(), egui::Button::new("floating"))
                            .clicked();
                    });
            });
            full.textures_delta.clear();
            (clicked, done)
        };
        let press = |pressed| Event::PointerButton {
            pos: at.center(),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        // What floats is pressed like anything, while no dialog is open.
        for _ in 0..3 {
            frame(Vec::new(), false);
        }
        frame(vec![Event::PointerMoved(at.center())], false);
        frame(vec![press(true)], false);
        assert!(frame(vec![press(false)], false).0);
        // Under a dialog the same click is a press on the scrim, which closes the
        // dialog, and no click of what floats - nor when the button comes up over it
        // with the dialog gone.
        for _ in 0..3 {
            assert_eq!(frame(Vec::new(), true), (false, Done::Open));
        }
        assert_eq!(frame(vec![press(true)], true), (false, Done::Closed));
        assert!(!frame(vec![press(false)], false).0);
    }

    // A copy and a cut are taken out of the frame with the keys: they are what egui
    // makes of their chords, and no key at all.
    #[test]
    fn a_copy_and_a_cut_reach_nothing_behind() {
        let mut f = Fixture::new(confirm_album());
        f.open();
        for event in [Event::Copy, Event::Cut] {
            f.frame(vec![event]);
            assert!(!f.typed_left_over);
        }
    }

    // Which button the keys are on has to be seen: Enter presses it. The cross in the
    // picker's corner had no ring, and with no keyword listed and nothing typed Tab
    // goes from the field straight to it.
    #[test]
    fn the_keyboard_is_seen_on_whatever_it_is_on() {
        fn drawn(f: &Fixture) -> std::collections::BTreeSet<String> {
            (f.fills.iter())
                .map(|(rect, fill)| format!("{rect:?} {fill:?}"))
                .collect()
        }
        // (A ring is a rectangle with a stroke and no fill: the fixture keeps every
        // rectangle drawn.)
        let mut f = picker(&["beach"]);
        f.open();
        let before = drawn(&f);
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new(("dialog-row", "beach"))));
        f.frame(Vec::new());
        assert!(drawn(&f).difference(&before).count() > 0, "a row");
        let mut f = picker(&[]);
        f.open();
        let before = drawn(&f);
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-close")));
        f.frame(Vec::new());
        assert!(drawn(&f).difference(&before).count() > 0, "the cross");
    }

    // The smallest window photon has is 500 points high, and a library has more
    // keywords than fit in that: the picker stops short of the window's edges.
    #[test]
    fn the_picker_fits_a_window_lower_than_its_list() {
        let many: Vec<String> = (0..40).map(|n| format!("keyword {n:02}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut f = picker(&many);
        f.size = vec2(800.0, 400.0);
        f.open();
        let ground = f.ground();
        assert!(
            ground.top() >= PAD && ground.bottom() <= 400.0 - PAD,
            "{ground:?}"
        );
        assert!(ground.height() < HEIGHT);
        // And in one high enough it is as tall as a picker gets.
        let mut f = picker(&many);
        f.open();
        assert_eq!(f.ground().height(), HEIGHT);
    }

    // A window narrower than a dialog: the dialog is as wide as the window lets it be.
    #[test]
    fn a_dialog_is_no_wider_than_its_window_lets_it_be() {
        let mut f = Fixture::new(confirm_album());
        f.size = vec2(380.0, 600.0);
        f.open();
        let ground = f.ground();
        assert_eq!(ground.width(), 380.0 - 2.0 * PAD);
        assert!(ground.left() >= PAD && ground.right() <= 380.0 - PAD);
    }
}
