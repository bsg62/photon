//! The body of the export's dialog, drawn by the dialog layer (`dialog_view.rs`): the
//! title, the field a folder's path is typed in, what is said of a path photon will not
//! export to, the checkbox, the five sizes, the note and the two buttons.
//!
//! Where the Svelte dialog has a button that opens the system's folder picker, this has
//! a field, with the picker's button beside it drawn and taking no press until a later
//! sub-project brings the picker; and where that has a dropdown for the size, this has
//! the five sizes in a row - a list over a dialog is a second thing open at once, and
//! five choices fit a row.

use crate::{
    dialog_view::{CLOSE, CONTROL, Done, GAP, Look, PAD, button, button_width, close},
    export_dialog::{
        APPLY_EDITS, CANCEL, CHOOSE, EXPORT, ExportDialog, FIELD, LONGEST_EDGE, NOTE, PLACEHOLDER,
        SIZES,
    },
    icons::Icon,
    text::paint_line,
    theme::{
        apply::color,
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, UiBuilder, WidgetInfo, WidgetType,
    pos2, vec2,
};
use std::sync::Arc;

/// The checkbox's square.
const BOX: f32 = 16.0;

pub(crate) fn field_id() -> Id {
    Id::new("dialog-export-path")
}

/// Small words wrapped to `width`: what is said of a refused path, and the note.
fn small(ui: &egui::Ui, text: &str, tint: egui::Color32, width: f32) -> Arc<egui::Galley> {
    ui.painter()
        .layout(text.to_owned(), fonts::regular(T[1]), tint, width)
}

/// The height of one line of the small words.
fn small_line(ui: &egui::Ui) -> f32 {
    let font = fonts::regular(T[1]);
    ui.ctx().fonts_mut(|fonts| fonts.row_height(&font)).ceil()
}

/// How tall the dialog is, `inner` wide inside its padding: what is said of a refused
/// path and the note are wrapped, and take the lines they need.
///
/// The room of one line is kept under the field whether or not anything is said there.
/// A path is asked about at every letter, and the short reasons - no folder there, no
/// whole path - come and go as it is typed: a dialog only as tall as what it says, in
/// the middle of the window, moved up and down with them at every part of a path.
pub(crate) fn height(ui: &egui::Ui, dialog: &ExportDialog, inner: f32, palette: &Palette) -> f32 {
    let line = small_line(ui);
    let said = S[0]
        + dialog.problem().map_or(line, |why| {
            let words = small(ui, why, color(palette.danger), inner);
            words.size().y.ceil().max(line)
        });
    let note = small(ui, NOTE, color(palette.text_dim), inner)
        .size()
        .y
        .ceil();
    let sizes = small_line(ui) + S[0] + CONTROL;
    2.0 * PAD
        + CLOSE
        + GAP
        + CONTROL
        + said
        + GAP
        + CONTROL
        + GAP
        + sizes
        + GAP
        + note
        + PAD
        + CONTROL
}

/// One of the five sizes: a button that says whether it is the one chosen.
fn size(ui: &mut egui::Ui, rect: Rect, label: &str, chosen: bool, palette: &Palette) -> bool {
    let response = ui.interact(rect, Id::new(("dialog-export-size", label)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, chosen, label));
    let (ground, words) = if chosen {
        (palette.accent, palette.on_accent)
    } else if response.hovered() {
        (palette.field_hover, palette.text)
    } else {
        (palette.field, palette.text)
    };
    let painter = ui.painter().clone();
    painter.rect_filled(rect, R[1], color(ground));
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            R[1] + 2.0,
            Stroke::new(2.0, color(palette.accent)),
            StrokeKind::Inside,
        );
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        fonts::regular(T[2]),
        color(words),
    );
    response.clicked()
}

/// The export's dialog in `room`. `opened` in the frame it is first drawn.
pub(crate) fn show(
    ui: &mut egui::Ui,
    room: Rect,
    dialog: &ExportDialog,
    palette: &Palette,
    opened: bool,
) -> Done {
    let painter = ui.painter().clone();
    let works = dialog.job().is_some();

    // Enter in the field exports. Read before the field is drawn and taken out of the
    // frame: egui's field gives the keyboard up at Enter, and before a destination has
    // passed the key does nothing and the caret stays where it is.
    let id = field_id();
    let in_field = ui.memory(|memory| memory.has_focus(id));
    let enter = in_field && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));

    let header = Rect::from_min_max(room.min, pos2(room.right(), room.top() + CLOSE));
    let close_at = Rect::from_min_max(pos2(header.right() - CLOSE, header.top()), header.max);
    paint_line(
        ui,
        &painter,
        (header.left(), header.center().y),
        close_at.left() - S[1] - header.left(),
        &dialog.title(),
        fonts::semibold(ui.ctx(), T[3]),
        color(palette.text),
    );

    // The folder's path, and beside it the button that opens the system's picker.
    let form = Rect::from_min_size(
        pos2(room.left(), header.bottom() + GAP),
        vec2(room.width(), CONTROL),
    );
    let choose = button_width(ui, CHOOSE);
    let choose_at = Rect::from_min_max(pos2(form.right() - choose, form.top()), form.max);
    let field = Rect::from_min_max(form.min, pos2(choose_at.left() - S[0], form.bottom()));
    let refused = dialog.problem();
    painter.rect_filled(field, R[1], color(palette.field));
    if refused.is_some() {
        painter.rect_stroke(
            field,
            R[1],
            Stroke::new(1.0, color(palette.danger)),
            StrokeKind::Inside,
        );
    }
    let font = fonts::regular(T[2]);
    let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let edit = Rect::from_min_max(
        pos2(field.left() + S[0], field.center().y - line / 2.0),
        pos2(field.right() - S[0], field.center().y + line / 2.0),
    );
    let mut text = dialog.path().to_owned();
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
                .hint_text(egui::RichText::new(PLACEHOLDER).color(color(palette.text_dim)))
                .desired_width(edit.width())
                .show(ui)
        })
        .inner;
    // Named, and not only hinted at: egui hands a field's hint to a screen reader as its
    // placeholder, which is what an empty field already shows.
    output.response.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, "", dialog.path(), PLACEHOLDER);
        info.label = Some(FIELD.to_owned());
        info
    });
    if opened {
        // The keyboard in the field as the dialog opens: a destination is the one thing
        // that has to be there before anything else can happen. And a history of its
        // own, as the keyword picker's field has, and for its reason.
        output.response.request_focus();
        egui::text_edit::TextEditState::default().store(ui.ctx(), id);
    }
    let typed = output.response.changed();
    // The system's picker, one at a time - dimmed while it is open, a button that senses
    // no press: the folder chosen is put in the field and asked about as a typed one is.
    let look = if dialog.choosing {
        Look::Off
    } else {
        Look::Plain
    };
    let chose = button(ui, choose_at, Id::new("dialog-choose"), CHOOSE, look).clicked();

    // Against the field, while the dialog is still open: a folder photon will not write
    // into is worth saying before the export and not after it.
    // (In the room of one line that is kept for it, or in as many as it needs.)
    let said_at = form.bottom() + S[0];
    let mut top = said_at + small_line(ui);
    if let Some(why) = refused {
        let words = small(ui, why, color(palette.danger), room.width());
        let place = Rect::from_min_size(pos2(room.left(), said_at), words.size());
        painter.galley(place.min, words, color(palette.danger));
        ui.interact(place, Id::new("dialog-export-problem"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, why));
        top = top.max(place.top() + place.height().ceil());
    }

    // The checkbox, with its words: a press on either ticks it.
    let row = Rect::from_min_size(pos2(room.left(), top + GAP), vec2(room.width(), CONTROL));
    let ticked = dialog.apply_edits();
    let label_width = (painter.layout_no_wrap(
        APPLY_EDITS.to_owned(),
        fonts::regular(T[2]),
        color(palette.text),
    ))
    .size()
    .x
    .ceil();
    let tick_at = Rect::from_min_size(row.min, vec2(BOX + S[1] + label_width, row.height()));
    let tick = ui.interact(tick_at, Id::new("dialog-export-edits"), Sense::click());
    tick.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, ticked, APPLY_EDITS));
    let square =
        Rect::from_center_size(pos2(row.left() + BOX / 2.0, row.center().y), vec2(BOX, BOX));
    if ticked {
        painter.rect_filled(square, R[0], color(palette.accent));
        Icon::Check.paint(ui, square, 12.0, false, color(palette.on_accent));
    } else {
        painter.rect_filled(square, R[0], color(palette.field));
        painter.rect_stroke(
            square,
            R[0],
            Stroke::new(1.0, color(palette.text_dim)),
            StrokeKind::Inside,
        );
    }
    if tick.has_focus() {
        painter.rect_stroke(
            square.expand(2.0),
            R[0] + 2.0,
            Stroke::new(2.0, color(palette.accent)),
            StrokeKind::Inside,
        );
    }
    paint_line(
        ui,
        &painter,
        (square.right() + S[1], row.center().y),
        label_width + 1.0,
        APPLY_EDITS,
        fonts::regular(T[2]),
        color(palette.text),
    );

    // How large a copy may be: the five sizes, the one chosen marked.
    let small_line = small_line(ui);
    let edge = pos2(room.left(), row.bottom() + GAP);
    painter.text(
        pos2(edge.x, edge.y + small_line / 2.0),
        egui::Align2::LEFT_CENTER,
        LONGEST_EDGE,
        fonts::regular(T[1]),
        color(palette.text_dim),
    );
    let sizes = Rect::from_min_size(
        pos2(room.left(), edge.y + small_line + S[0]),
        vec2(room.width(), CONTROL),
    );
    // As wide as their words need, and what is left over shared out between them.
    let natural: Vec<f32> = (SIZES.iter())
        .map(|(_, label)| button_width(ui, label))
        .collect();
    let gaps = S[0] * (SIZES.len() - 1) as f32;
    // In a window too narrow for the dialog's own width there is none, and they give
    // up some of the room beside their words instead.
    let spare = (sizes.width() - gaps - natural.iter().sum::<f32>()) / SIZES.len() as f32;
    let mut left = sizes.left();
    let mut sized = None;
    for ((max_edge, label), natural) in SIZES.iter().zip(&natural) {
        let at = Rect::from_min_size(pos2(left, sizes.top()), vec2(natural + spare, CONTROL));
        if size(ui, at, label, dialog.max_edge() == *max_edge, palette) {
            sized = Some(*max_edge);
        }
        left = at.right() + S[0];
    }

    let note = small(ui, NOTE, color(palette.text_dim), room.width());
    let note_at = Rect::from_min_size(pos2(room.left(), sizes.bottom() + GAP), note.size());
    painter.galley(note_at.min, note, color(palette.text_dim));
    ui.interact(note_at, Id::new("dialog-export-note"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, NOTE));

    // Cancel first, then Export: Tab goes from the one to the other.
    let actions = Rect::from_min_max(pos2(room.left(), room.bottom() - CONTROL), room.max);
    let export = button_width(ui, EXPORT);
    let cancel = button_width(ui, CANCEL);
    let export_at = Rect::from_min_max(pos2(actions.right() - export, actions.top()), actions.max);
    let cancel_at = Rect::from_min_max(
        pos2(export_at.left() - S[0] - cancel, actions.top()),
        pos2(export_at.left() - S[0], actions.bottom()),
    );
    let cancelled = button(
        ui,
        cancel_at,
        Id::new("dialog-export-cancel"),
        CANCEL,
        Look::Plain,
    )
    .clicked();
    let look = if works { Look::Primary } else { Look::Off };
    let exported = button(ui, export_at, Id::new("dialog-export-go"), EXPORT, look).clicked();
    let closed = close(ui, close_at, palette);

    if closed || cancelled {
        return Done::Closed;
    }
    // What was typed in this frame comes first: the Enter of the same frame would export
    // to the path as it was before the letter, which is not the one in the field.
    if typed {
        return Done::Path(text);
    }
    if exported || (enter && works) {
        return Done::Export;
    }
    if chose {
        return Done::Choose;
    }
    if tick.clicked() {
        return Done::Tick(!ticked);
    }
    match sized {
        Some(max_edge) => Done::Size(max_edge),
        None => Done::Open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dialog_view::tests::{Fixture, WINDOW},
        dialogs::Dialog,
        export_dialog::{Dest, NO_FOLDER},
    };
    use eframe::egui::{Event, PointerButton};

    const FOLDER: &str = "/exports";
    const INSIDE: &str = "that folder is inside the watched folder /home/ada/Pictures - \
photon would index the copies as new photos";

    fn export(ids: &[i64]) -> Fixture {
        Fixture::new(Dialog::Export(ExportDialog::new(ids.to_vec())))
    }

    fn dialog(f: &Fixture) -> &ExportDialog {
        match &f.dialog {
            Dialog::Export(export) => export,
            other => panic!("{other:?}"),
        }
    }

    /// The engine's answer about the path the field holds.
    fn answer(f: &mut Fixture, answer: Result<&str, &str>) {
        let asked = dialog(f).path().to_owned();
        if let Dialog::Export(export) = &mut f.dialog {
            export.checked(&asked, answer.map(str::to_owned).map_err(str::to_owned));
        }
        f.frame(Vec::new());
    }

    /// The dialog with `FOLDER` typed, and passed.
    fn passed() -> Fixture {
        let mut f = export(&[1, 2, 3]);
        f.open();
        f.typed(FOLDER);
        answer(&mut f, Ok(FOLDER));
        f
    }

    /// The colour the button or the size called `label` stands on.
    fn ground_of(f: &Fixture, label: &str) -> Color32 {
        let at = f.at(label);
        (f.fills.iter().rev())
            .find(|(rect, _)| rect.contains(at) && rect.width() < 200.0)
            .map(|(_, fill)| *fill)
            .unwrap_or_else(|| panic!("{label} stands on nothing"))
    }

    use eframe::egui::Color32;

    #[test]
    fn it_says_how_many_photos_and_opens_with_the_caret_in_its_field() {
        let mut f = export(&[1, 2, 3]);
        f.open();
        assert_eq!(f.focused(), Some(field_id()));
        let ground = f.ground();
        assert_eq!(ground.width(), 440.0);
        // Everything it says is inside it, from the top down.
        let mut last = ground.top();
        for words in [
            "Export 3 photos",
            PLACEHOLDER,
            APPLY_EDITS,
            LONGEST_EDGE,
            SIZES[0].1,
            NOTE,
            EXPORT,
        ] {
            let place = f.drew(words).unwrap_or_else(|| panic!("{words} is drawn"));
            assert!(
                ground.contains_rect(place),
                "{words}: {place:?} in {ground:?}"
            );
            assert!(
                place.top() >= last,
                "{words} is under what is said before it"
            );
            last = place.top();
        }
        // The picker's button beside the field, and "Cancel" beside "Export".
        let (hint, choose) = (f.drew(PLACEHOLDER).unwrap(), f.drew(CHOOSE).unwrap());
        assert!((hint.center().y - choose.center().y).abs() < 2.0);
        assert!(hint.right() < choose.left() && ground.contains_rect(choose));
        let (cancel, go) = (f.drew(CANCEL).unwrap(), f.drew(EXPORT).unwrap());
        assert!((cancel.center().y - go.center().y).abs() < 2.0);
        assert!(cancel.right() < go.left());
        // The five sizes in one row, in their order, none over another.
        let sizes: Vec<Rect> = SIZES
            .iter()
            .map(|(_, label)| f.drew(label).unwrap())
            .collect();
        for pair in sizes.windows(2) {
            assert!((pair[0].center().y - pair[1].center().y).abs() < 2.0);
            assert!(pair[0].right() < pair[1].left(), "{pair:?}");
        }
        assert!(ground.contains_rect(*sizes.last().unwrap()));
        // One photo is counted singly.
        let mut f = export(&[7]);
        f.open();
        assert!(f.drew("Export 1 photo").is_some());
    }

    #[test]
    fn what_is_typed_is_the_path_to_ask_about() {
        let mut f = export(&[1]);
        f.open();
        assert_eq!(f.typed("/ex"), Done::Path("/ex".to_owned()));
        assert_eq!(dialog(&f).dest(), &Dest::Checking);
        assert_eq!(f.typed("ports"), Done::Path(FOLDER.to_owned()));
        assert!(f.drew(FOLDER).is_some());
        assert_eq!(f.drew(PLACEHOLDER), None);
    }

    // "Export" waits for a destination; "Choose…" opens the system's picker, once.
    #[test]
    fn export_takes_no_press_and_enter_does_nothing_until_a_destination_has_passed() {
        let mut f = export(&[1, 2]);
        f.open();
        let field = f.tint(|palette| palette.field);
        assert_eq!(ground_of(&f, EXPORT), field);
        let (go, choose) = (f.at(EXPORT), f.at(CHOOSE));
        f.typed(FOLDER);
        // Asked about, and not yet answered.
        assert_eq!(f.key(Key::Enter), Done::Open);
        assert_eq!(
            f.focused(),
            Some(field_id()),
            "the caret stays in the field"
        );
        assert_eq!(f.left_over, [], "and the key goes no further");
        assert_eq!(f.click(go), Done::Open);
        assert_eq!(f.click(choose), Done::Choose);
        // (A click elsewhere takes the caret out of the field, as egui has it.)
        let field_at = f.at(FOLDER);
        f.click(field_at);

        answer(&mut f, Ok(FOLDER));
        assert_eq!(ground_of(&f, EXPORT), f.tint(|palette| palette.accent));
        assert_eq!(f.key(Key::Enter), Done::Export);
        let mut f = passed();
        let go = f.at(EXPORT);
        assert_eq!(f.click(go), Done::Export);
        // While the picker is open, "Choose…" takes no second press.
        let mut f = passed();
        let choose = f.at(CHOOSE);
        if let Dialog::Export(export) = &mut f.dialog {
            export.choosing = true;
        }
        assert_eq!(f.click(choose), Done::Open);
    }

    // Against the field, while the dialog is open: the words are wrapped to it, and it
    // is as much taller as they need.
    #[test]
    fn a_refused_path_is_said_under_the_field() {
        let mut f = export(&[1]);
        f.open();
        let before = f.ground();
        f.typed("/home/ada/Pictures/out");
        assert_eq!(f.ground().height(), before.height(), "nothing is said yet");
        answer(&mut f, Err(INSIDE));
        let said = f.drew(INSIDE).expect("why it was refused");
        let ground = f.ground();
        assert!(ground.contains_rect(said), "{said:?} in {ground:?}");
        assert!(said.top() > f.drew("/home/ada/Pictures/out").unwrap().bottom());
        assert!(said.bottom() < f.drew(APPLY_EDITS).unwrap().top());
        assert!(said.height() > 20.0, "on more than one line: {said:?}");
        // Taller by the lines beyond the one whose room was there already.
        assert!(ground.height() > before.height() + 10.0);
        assert_eq!(ground_of(&f, EXPORT), f.tint(|palette| palette.field));
        let go = f.at(EXPORT);
        assert_eq!(f.click(go), Done::Open);
        // Typed over, it is asked about again, and what was said stands until the
        // answer: taken away at every letter and put back at every answer, the line
        // came and went as a path was typed, and the dialog, which stands in the
        // middle of the window, moved up and down with it.
        let refused = f.ground();
        let field_at = f.at("/home/ada/Pictures/out");
        f.click(field_at);
        f.typed("2");
        f.frame(Vec::new());
        assert_eq!(dialog(&f).dest(), &Dest::Checking);
        assert!(f.drew(INSIDE).is_some());
        assert_eq!(f.ground(), refused);
        // What stands under the words is as far under two lines of them as under one.
        let under = f.drew(APPLY_EDITS).unwrap().top() - f.drew(INSIDE).unwrap().bottom();
        answer(&mut f, Err(NO_FOLDER));
        f.frame(Vec::new());
        let short = f.drew(NO_FOLDER).expect("the other reason");
        let under_one = f.drew(APPLY_EDITS).unwrap().top() - short.bottom();
        assert!((under - under_one).abs() < 2.0, "{under} and {under_one}");
        // The answer that it passes takes the line away.
        f.typed("3");
        answer(&mut f, Ok("/home/ada/Pictures/out23"));
        f.frame(Vec::new());
        assert_eq!(f.drew(NO_FOLDER), None);
        assert_eq!(f.ground().height(), before.height());
    }

    #[test]
    fn the_checkbox_is_ticked_by_a_press_on_it_or_on_its_words_and_by_space() {
        let mut f = export(&[1]);
        f.open();
        let words = f.drew(APPLY_EDITS).unwrap();
        assert_eq!(f.click(words.center()), Done::Tick(false));
        assert!(!dialog(&f).apply_edits());
        // The square itself, left of the words.
        let square = pos2(words.left() - S[1] - BOX / 2.0, words.center().y);
        assert_eq!(f.click(square), Done::Tick(true));
        // Beside the words it is no press on the checkbox.
        assert_eq!(
            f.click(pos2(words.right() + 30.0, words.center().y)),
            Done::Open
        );
        // Tab leaves the field for it, by way of "Choose…", and Space ticks it.
        let field = f.at(PLACEHOLDER);
        f.click(field);
        f.key(Key::Tab);
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-export-edits")));
        assert_eq!(f.key(Key::Space), Done::Tick(false));
    }

    #[test]
    fn a_size_is_chosen_by_a_press_and_the_one_chosen_is_marked() {
        let mut f = export(&[1]);
        f.open();
        let (accent, field) = (
            f.tint(|palette| palette.accent),
            f.tint(|palette| palette.field),
        );
        assert_eq!(ground_of(&f, "Original size"), accent);
        assert_eq!(ground_of(&f, "1920 px"), field);
        let at = f.at("1920 px");
        assert_eq!(f.click(at), Done::Size(Some(1920)));
        f.frame(vec![Event::PointerMoved(pos2(5.0, 5.0))]);
        assert_eq!(ground_of(&f, "1920 px"), accent);
        assert_eq!(ground_of(&f, "Original size"), field);
        let at = f.at("Original size");
        assert_eq!(f.click(at), Done::Size(None));
        assert_eq!(dialog(&f).max_edge(), None);
    }

    #[test]
    fn escape_cancel_the_cross_and_a_press_on_the_scrim_close_it() {
        let mut f = passed();
        assert_eq!(f.key(Key::Escape), Done::Closed);
        let mut f = passed();
        let cancel = f.at(CANCEL);
        assert_eq!(f.click(cancel), Done::Closed);
        // Enter with the keyboard on "Cancel" is a press of that button: the Enter that
        // exports is the field's.
        let mut f = passed();
        for _ in 0..8 {
            f.key(Key::Tab);
        }
        assert_eq!(f.focused(), Some(Id::new("dialog-export-cancel")));
        assert_eq!(f.key(Key::Enter), Done::Closed);
        let mut f = passed();
        let ground = f.ground();
        let cross = pos2(
            ground.right() - PAD - CLOSE / 2.0,
            ground.top() + PAD + CLOSE / 2.0,
        );
        assert_eq!(f.click(cross), Done::Closed);
        let mut f = passed();
        let outside = pos2(WINDOW.x - 20.0, WINDOW.y - 20.0);
        f.frame(vec![Event::PointerMoved(outside)]);
        assert_eq!(
            f.button(outside, PointerButton::Primary, true),
            Done::Closed
        );
        // A press on the dialog itself does not.
        let mut f = passed();
        let note = f.at(NOTE);
        assert_eq!(f.click(note), Done::Open);
    }

    // A frame can be long, and a letter and the Enter after it in it together: the
    // path with the letter has not been asked about, and is not exported to under the
    // answer for the path without it.
    #[test]
    fn what_is_typed_in_the_frame_of_enter_is_asked_about_and_not_exported_to() {
        let mut f = passed();
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let done = f.frame(vec![Event::Text("2".to_owned()), enter]);
        assert_eq!(done, Done::Path(format!("{FOLDER}2")));
        assert_eq!(dialog(&f).dest(), &Dest::Checking);
        assert_eq!(dialog(&f).job(), None);
    }

    // Tab stays in the dialog and goes through it in the order it reads: the field, the
    // checkbox, the sizes, Cancel, Export, the cross.
    #[test]
    fn tab_walks_the_dialog_in_its_order_and_stays_in_it() {
        let mut f = passed();
        let mut walked = Vec::new();
        for _ in 0..12 {
            f.key(Key::Tab);
            walked.push(
                f.focused()
                    .expect("something in the dialog has the keyboard"),
            );
        }
        let mut expected = vec![Id::new("dialog-choose"), Id::new("dialog-export-edits")];
        expected.extend(SIZES.map(|(_, label)| Id::new(("dialog-export-size", label))));
        expected.extend([
            Id::new("dialog-export-cancel"),
            Id::new("dialog-export-go"),
            Id::new("dialog-close"),
            field_id(),
            Id::new("dialog-choose"),
        ]);
        assert_eq!(walked, expected);
        // With no destination "Export" takes no keyboard either.
        let mut f = export(&[1]);
        f.open();
        for _ in 0..8 {
            f.key(Key::Tab);
        }
        assert_eq!(f.focused(), Some(Id::new("dialog-export-cancel")));
        f.key(Key::Tab);
        assert_eq!(f.focused(), Some(Id::new("dialog-close")));
    }

    // One field under one name for every export: Ctrl+Z in a dialog just opened must
    // not bring back the path typed in the one before.
    #[test]
    fn undo_in_a_new_dialog_does_not_bring_back_the_last_path() {
        let mut f = export(&[1]);
        f.open();
        for letter in ["/", "a", "b"] {
            f.typed(letter);
            for _ in 0..90 {
                f.frame(Vec::new());
            }
        }
        f.dialog = Dialog::Export(ExportDialog::new(vec![2]));
        f.view.reset();
        f.open();
        for _ in 0..90 {
            f.frame(Vec::new());
        }
        assert_eq!(f.key_with(Key::Z, Modifiers::COMMAND), Done::Open);
    }

    // But for the caret, which is switched off here as the harness of the whole
    // application switches it off.
    #[test]
    fn the_dialog_standing_asks_for_no_frame() {
        let mut f = export(&[1, 2]);
        f.ctx
            .all_styles_mut(|style| style.visuals.text_cursor.blink = false);
        f.open();
        f.typed(FOLDER);
        answer(&mut f, Err(INSIDE));
        for _ in 0..4 {
            f.frame(Vec::new());
        }
        assert_eq!(f.repaint, std::time::Duration::MAX);
    }

    // In a window narrower than the dialog the sizes still stand in it, side by side.
    #[test]
    fn the_sizes_stay_inside_a_dialog_made_narrow_by_its_window() {
        let mut f = export(&[1]);
        f.size = vec2(400.0, 600.0);
        f.open();
        let ground = f.ground();
        assert!(ground.width() < 440.0);
        let sizes: Vec<Rect> = SIZES
            .iter()
            .map(|(_, label)| f.drew(label).unwrap())
            .collect();
        assert!(ground.contains_rect(sizes[0]) && ground.contains_rect(sizes[4]));
        for pair in sizes.windows(2) {
            assert!(pair[0].right() <= pair[1].left(), "{pair:?}");
        }
    }

    // The reasons of one line come and go as a path is typed - there is no folder at
    // "/home/ad", and there is one at "/home/ada" - and the dialog does not move with
    // them: the room of one line is under the field whether or not anything is said.
    #[test]
    fn a_reason_of_one_line_does_not_move_the_dialog() {
        let mut f = export(&[1]);
        f.open();
        let empty = f.ground();
        let checkbox = f.drew(APPLY_EDITS).unwrap();
        f.typed("/home/ad");
        answer(&mut f, Err(NO_FOLDER));
        f.frame(Vec::new());
        let said = f.drew(NO_FOLDER).expect("why not");
        assert!(said.top() > f.drew("/home/ad").unwrap().bottom());
        assert!(
            said.bottom() <= checkbox.top(),
            "{said:?} over {checkbox:?}"
        );
        assert_eq!(f.ground(), empty);
        assert_eq!(f.drew(APPLY_EDITS), Some(checkbox));
        f.typed("a");
        answer(&mut f, Ok("/home/ada"));
        f.frame(Vec::new());
        assert_eq!(f.drew(NO_FOLDER), None);
        assert_eq!(f.ground(), empty);
        assert_eq!(f.drew(APPLY_EDITS), Some(checkbox));
    }

    // A button that is down within the double-click time of the dialog's first frame
    // does nothing in it - and what is typed or pasted before it comes up is kept, as
    // the picker keeps it: the path was lost, and the field showed it for one frame.
    #[test]
    fn what_is_typed_while_a_double_clicks_second_press_is_down_is_kept() {
        let mut f = export(&[1]);
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        let at = f.at(NOTE);
        f.frame(vec![Event::PointerMoved(at)]);
        assert_eq!(f.button(at, PointerButton::Primary, true), Done::Open);
        assert_eq!(f.focused(), Some(field_id()));
        let done = f.frame(vec![Event::Paste(FOLDER.to_owned())]);
        f.button(at, PointerButton::Primary, false);
        f.frame(Vec::new());
        assert_eq!(done, Done::Path(FOLDER.to_owned()));
        assert_eq!(dialog(&f).path(), FOLDER);
    }

    // The dialog is as tall as what it says, and stands in the middle of the window in
    // the frame its height changes, not a frame later: placed by the height of the
    // frame before, it was drawn with its top where it had been in the frame the line
    // under the field went, and a press on "Export" where the button had stood was a
    // press beside the dialog - which closes it.
    #[test]
    fn a_press_on_export_in_the_frame_the_line_goes_does_not_close_the_dialog() {
        let mut f = export(&[1, 2, 3]);
        f.open();
        f.typed("/home/ada/Pictures/out");
        answer(&mut f, Err(INSIDE));
        f.frame(Vec::new());
        f.frame(Vec::new());
        let refused = f.ground();
        let middle = f.size.to_pos2() / 2.0;
        assert!((refused.center() - middle).length() < 1.0, "{refused:?}");
        let at = f.at(EXPORT);
        f.frame(vec![Event::PointerMoved(at)]);
        f.typed("2");
        // The answer, in the frame of the press.
        let asked = dialog(&f).path().to_owned();
        if let Dialog::Export(export) = &mut f.dialog {
            export.checked(&asked, Ok(asked.clone()));
        }
        let done = f.button(at, PointerButton::Primary, true);
        let moving = f.ground();
        assert!(moving.height() < refused.height() - 10.0, "the line went");
        assert_ne!(done, Done::Closed);
        assert!((moving.center() - middle).length() < 1.0, "{moving:?}");
        f.frame(Vec::new());
        assert_eq!(f.ground(), moving, "and it stays where it was put");
    }
}
