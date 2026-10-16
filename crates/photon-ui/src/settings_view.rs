//! The body of Settings, drawn by the dialog layer (`dialog_view.rs`): a header with the
//! title and the cross, the list of sections down the left, and the section chosen beside
//! it - `Settings.svelte`'s markup. What it holds is `settings.rs`'s; what is pressed in
//! it is answered as `Asked`, and the application does it.
//!
//! The section is one column painted by hand and scrolled by a number of its own, as the
//! viewer's info panel is, and by its rules (`viewer_info_view.rs`): through a child cut
//! off at the section's edge, so that what is scrolled out is neither drawn nor pressed;
//! with nothing out of sight handed to egui at all, or AccessKit is told of things that are
//! not on screen - but for what may hold the keyboard, the interval's field and the
//! question's buttons, which egui would let go of if they were not drawn; forty points a notch of the wheel, whatever the system's, since egui's
//! own speed is the grid's and at a Windows set to scroll no lines a notch the section
//! could not be brought to its foot.

use crate::controls::SIZES;
use crate::{
    dialog_view::{CONTROL, Done, Look, button, button_width, close_labelled},
    icons::Icon,
    settings::{
        APPEARANCE_HINT, Asked, CANCEL, CLOSE, DISTANCES, FIND_FACES, FIND_FACES_BOX,
        FIND_FACES_HINT, FIND_FACES_PRIVATE, FOLDERS_HINT, INTERVAL_AFTER, INTERVAL_BEFORE,
        LIBRARY, LICENCE, LOOK_ALIKES, LOOK_ALIKES_HINT, MEMORY, PHOTO_SIZE, PHOTO_SIZE_HINT,
        Question, REVEAL, SECTIONS, SHUFFLE, SLIDESHOW_HINT, Section, Settings, TAGS_HINT, THEMES,
        TITLE, VERSION, distance_hint, memory_amount, memory_scope,
    },
    tag_manager::{self, CHANGES, FILTER, NO_TAGS, RENAME, RESTORE, filter_tags, rule_label},
    text::paint_line,
    theme::{
        apply::color,
        fonts,
        tokens::{Palette, R, S, T},
    },
    watched_list::{self, ADD, ADD_FOLDER, CHOOSE, FolderRow, Kind, NO_FOLDERS, RESCAN},
};
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind,
    UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use photon_core::library::TagCount;
use std::sync::Arc;

/// The header: the title and the cross, ten points over and under them.
const HEADER: f32 = 48.0;
pub(crate) const HEADER_HEIGHT: f32 = HEADER;
/// The section list's width, and a row of it.
const NAV: f32 = 150.0;
const NAV_ROW: f32 = 28.0;
const NAV_GAP: f32 = 2.0;
/// The cross.
const CROSS: f32 = 28.0;
/// A segment of a segmented control: its words and this much room either side of them,
/// in a tray two points larger all round, with two points between segments - the top
/// bar's size control, which Appearance has again.
const SEGMENT_PAD: f32 = 14.0;
const SEGMENT: f32 = 26.0;
const TRAY: f32 = 2.0;
/// A checkbox's square.
const BOX: f32 = 16.0;
/// The interval's field.
const FIELD: f32 = 64.0;
/// A row of Statistics: its height, its label's room at most, and its count's.
const STAT_ROW: f32 = 26.0;
const STAT_LABEL: f32 = 176.0;
const STAT_COUNT: f32 = 72.0;

/// The interval's field, to egui.
pub(crate) fn interval_id() -> Id {
    Id::new("settings-interval")
}

/// A checkbox, to egui, by its words.
fn box_id(label: &str) -> Id {
    Id::new(("settings-box", label))
}

/// A question's "Cancel", to egui: the keyboard is put on it as the question is asked.
pub(crate) fn no_id() -> Id {
    Id::new("settings-question-no")
}

/// The keyword's field, the tags' filter and the field a folder to add is typed in.
pub(crate) fn rename_id() -> Id {
    Id::new("settings-tag-rename")
}

pub(crate) fn filter_id() -> Id {
    Id::new("settings-tag-filter")
}

pub(crate) fn add_id() -> Id {
    Id::new("settings-add-folder")
}

/// A folder's "Remove…" and a keyword's: what the keyboard comes back to when the
/// question it asked is answered.
fn remove_folder_id(id: i64) -> Id {
    Id::new(("settings-folder-remove", id))
}

fn remove_tag_id(tag: &str) -> Id {
    Id::new(("settings-tag-remove", tag))
}

impl SettingsView {
    /// What an Escape is in Settings when it is not the dialog's: Cancel for a question a
    /// row asks - one on screen: drawn in the last frame, and not one a filter hides - the
    /// field's own for a keyword being renamed, a filter that holds something and the
    /// field a folder is typed in. Asked before the dialog takes Escape as its close.
    pub(crate) fn claims_escape(&self, ctx: &egui::Context, settings: &Settings) -> bool {
        let focused = ctx.memory(|memory| memory.focused());
        self.asking.is_some()
            || (settings.renamer.editing().is_some() && focused == Some(rename_id()))
            || (!settings.filter.is_empty() && focused == Some(filter_id()))
            || (settings.adding.is_some() && focused == Some(add_id()))
    }
}

/// What the view keeps from frame to frame.
#[derive(Default)]
pub struct SettingsView {
    /// The section, scrolled by a number of its own.
    column: Column,
    /// The section last drawn: another begins at its top.
    of: Option<Section>,
    /// The question a row asked in the last frame, and in this one: the keyboard is put
    /// on its Cancel in the frame it is first asked.
    asked: Option<Question>,
    asking: Option<Question>,
    /// What had the keyboard when the question was asked - the box, the field, the
    /// button that asked - which it comes back to when the question is answered; and,
    /// for the frame after, the widget that is to take it back where it is drawn.
    back_to: Option<Id>,
    returning: Option<Id>,
    /// The keyword whose field was open in the last frame, and whether the field a folder
    /// is typed in was: a field opened is given the keyboard, and a history of its own.
    renaming: Option<String>,
    adding: bool,
}

/// A column painted by hand and scrolled by a number of its own: how far it is scrolled,
/// how tall it came to when last drawn, how far it is to be scrolled in the next frame to
/// bring something into sight, and its scrollbar (`scroll_bar`). Settings' section, the
/// shortcut sheet's body and the People page.
#[derive(Default)]
pub(crate) struct Column {
    pub(crate) scroll: f32,
    content: f32,
    pub(crate) reveal: f32,
    bar: crate::scroll_bar::Bar,
}

impl Column {
    /// The column in `inner`, within `place`, the room the wheel turns it in and whose
    /// right edge its bar stands at: the page to fill, which `end` is given back once it
    /// has been. `id` names the bar, which is a widget.
    pub(crate) fn begin(&mut self, ui: &egui::Ui, id: egui::Id, place: Rect, inner: Rect) -> Page {
        if ui.rect_contains_pointer(place) {
            self.scroll -= crate::scroll_bar::turned(ui, inner.height());
        }
        self.scroll += std::mem::take(&mut self.reveal);
        // Held to what there is to scroll, as it was when last drawn.
        let most = (self.content - inner.height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, most);
        let track = crate::scroll_bar::track(place);
        self.scroll = (self.bar).hold(ui, id, track, self.scroll, self.content, inner.height());
        Page {
            painter: ui.painter().clone(),
            clip: inner,
            left: inner.left(),
            width: inner.width(),
            y: inner.top() - self.scroll,
            after: None,
            asked: Vec::new(),
        }
    }

    /// The column filled: how tall it came to, and its bar at `place`'s right edge.
    pub(crate) fn end(
        &mut self,
        ui: &egui::Ui,
        page: &Page,
        place: Rect,
        inner: Rect,
        palette: &Palette,
    ) {
        self.content = page.y + self.scroll - inner.top();
        // Shorter than it was and scrolled past its new end: the frame that puts it
        // right is asked for.
        if self.scroll > (self.content - inner.height()).max(0.0) {
            ui.ctx().request_repaint();
        }
        (self.bar).paint(
            ui,
            crate::scroll_bar::track(place),
            self.scroll,
            self.content,
            inner.height(),
            color(palette.field_hover),
            color(palette.text_dim),
        );
    }

    /// How far it is scrolled, and whether its bar is held: for a test.
    pub fn scrolled(&self) -> (f32, bool) {
        (self.scroll, self.bar.held())
    }
}

/// A dialog's header on the chrome, its top corners the dialog's: `title`, and room for
/// the cross at its right, whose rectangle is answered. Settings' and the shortcut sheet's.
pub(crate) fn header(ui: &egui::Ui, rect: Rect, title: &str, palette: &Palette) -> (Rect, Rect) {
    let painter = ui.painter();
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    let top = CornerRadius {
        nw: R[3] as u8,
        ne: R[3] as u8,
        sw: 0,
        se: 0,
    };
    painter.rect_filled(header, top, color(palette.chrome));
    painter.hline(
        header.x_range(),
        header.bottom() - 0.5,
        (1.0, color(palette.line)),
    );
    let cross = Rect::from_center_size(
        pos2(header.right() - S[2] - CROSS / 2.0, header.center().y),
        vec2(CROSS, CROSS),
    );
    paint_line(
        ui,
        painter,
        (header.left() + S[3], header.center().y),
        cross.left() - S[1] - header.left() - S[3],
        title,
        fonts::semibold(ui.ctx(), T[4]),
        color(palette.text),
    );
    ui.interact(
        header.shrink(1.0),
        Id::new(("dialog-title", title)),
        Sense::hover(),
    )
    .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, title));
    (header, cross)
}

/// A text field of Settings at `rect`: its ground, its words, its placeholder. The field
/// is drawn in sight or out of it - egui lets go of a field that is not drawn - and the
/// rest only in sight.
pub(crate) struct Field<'a> {
    pub(crate) id: Id,
    pub(crate) text: &'a str,
    pub(crate) placeholder: &'a str,
    pub(crate) label: &'a str,
    pub(crate) refused: bool,
}

pub(crate) fn text_field(
    ui: &mut egui::Ui,
    page: &Page,
    rect: Rect,
    field: Field<'_>,
    palette: &Palette,
) -> (egui::text_edit::TextEditOutput, String) {
    if page.sees(rect) {
        page.painter.rect_filled(rect, R[1], color(palette.field));
        if field.refused {
            page.painter.rect_stroke(
                rect,
                R[1],
                Stroke::new(1.0, color(palette.danger)),
                StrokeKind::Inside,
            );
        }
    }
    let font = fonts::regular(T[2]);
    let line = line_height(ui, &font);
    let edit = Rect::from_min_max(
        pos2(rect.left() + S[1], rect.center().y - line / 2.0),
        pos2(rect.right() - S[1], rect.center().y + line / 2.0),
    );
    let mut text = field.text.to_owned();
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(edit), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            egui::TextEdit::singleline(&mut text)
                .id(field.id)
                // Escape claimed for the field: left to egui it takes the keyboard away
                // at the start of the frame, before `claims_escape` can see whose it
                // was, and the field's Escape closed Settings. All four said: the
                // default's arrows are egui's to walk the keyboard away with.
                .event_filter(egui::EventFilter {
                    escape: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    tab: false,
                })
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(font)
                .text_color(color(palette.text))
                .hint_text(egui::RichText::new(field.placeholder).color(color(palette.text_dim)))
                .desired_width(edit.width())
                .show(ui)
        })
        .inner;
    output.response.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, "", field.text, field.placeholder);
        info.label = Some(field.label.to_owned());
        info
    });
    (output, text)
}

/// A field opened in this frame takes the keyboard, with a history of its own - one
/// field under one id serves every row, egui keeps a field's undo by id - and its words
/// chosen, so that what is typed replaces them.
pub(crate) fn opened_field(ui: &egui::Ui, id: Id, text: &str) {
    let mut state = egui::text_edit::TextEditState::default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        )));
    state.store(ui.ctx(), id);
    ui.memory_mut(|memory| memory.request_focus(id));
}

/// Where the next thing goes in the section, and what is in sight of it.
pub(crate) struct Page {
    pub(crate) painter: egui::Painter,
    pub(crate) clip: Rect,
    pub(crate) left: f32,
    pub(crate) width: f32,
    /// The top of the next thing, on screen: above the section for what is scrolled
    /// past.
    pub(crate) y: f32,
    /// The room already left under what was put on it last, or none while nothing has
    /// been: a heading after something stands clear of it by its own room, which takes
    /// in what is already there as one CSS margin collapses into the next.
    after: Option<f32>,
    asked: Vec<Asked>,
}

impl Page {
    pub(crate) fn sees(&self, rect: Rect) -> bool {
        rect.intersects(self.clip)
    }

    /// Room under what was put on it last.
    pub(crate) fn gap(&mut self, by: f32) {
        self.y += by;
        self.after = Some(self.after.unwrap_or(0.0) + by);
    }

    /// Something put on it, ending at `bottom`.
    pub(crate) fn put(&mut self, bottom: f32) {
        self.y = bottom;
        self.after = Some(0.0);
    }

    pub(crate) fn row(&self, height: f32) -> Rect {
        Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, height))
    }

    /// Words wrapped to the column, at `self.y`, which they move on past. They tell
    /// AccessKit what they say, which is how a test of the whole application reads them.
    pub(crate) fn words(
        &mut self,
        ui: &egui::Ui,
        words: &str,
        font: FontId,
        tint: Color32,
        id: Id,
    ) -> Rect {
        let galley = wrap(ui, words, font, tint, self.width);
        let rect = Rect::from_min_size(pos2(self.left, self.y), galley.size());
        self.put(self.y + galley.size().y.ceil());
        if self.sees(rect) {
            self.painter.galley(rect.min, galley, tint);
            ui.interact(rect, id, Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
        }
        rect
    }

    /// A heading: a second one starts a new group, and stands clear of what is above it.
    pub(crate) fn heading(&mut self, ui: &egui::Ui, words: &str, palette: &Palette) {
        if let Some(after) = self.after {
            self.y += (S[4] - after).max(0.0);
        }
        let font = fonts::semibold(ui.ctx(), T[3]);
        self.words(
            ui,
            words,
            font,
            color(palette.text),
            Id::new(("settings-heading", words)),
        );
        self.gap(S[0]);
    }

    /// A line of dim words under a heading, and the room after it.
    pub(crate) fn hint(&mut self, ui: &egui::Ui, words: &str, palette: &Palette) {
        self.words(
            ui,
            words,
            fonts::regular(T[2]),
            color(palette.text_dim),
            Id::new(("settings-hint", words)),
        );
        self.gap(S[2]);
    }
}

pub(crate) fn wrap(
    ui: &egui::Ui,
    words: &str,
    font: FontId,
    tint: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    ui.painter().layout(words.to_owned(), font, tint, width)
}

pub(crate) fn line_height(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font)).ceil()
}

/// The ring round what has the keyboard: Enter or Space presses it. Drawn by every
/// button a Tab stops at, here and in the top bar and the empty library's panel.
pub(crate) fn ring(painter: &egui::Painter, rect: Rect, radius: f32, palette: &Palette) {
    painter.rect_stroke(
        rect.expand(2.0),
        radius + 2.0,
        Stroke::new(2.0, color(palette.accent)),
        StrokeKind::Inside,
    );
}

/// A row of the section list: whether it was pressed.
fn nav_row(
    ui: &mut egui::Ui,
    rect: Rect,
    section: Section,
    current: bool,
    palette: &Palette,
) -> bool {
    let label = section.label();
    let response = ui.interact(rect, Id::new(("settings-section", label)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, current, label));
    let painter = ui.painter();
    if current {
        painter.rect_filled(rect, R[1], color(palette.accent_soft));
    } else if response.hovered() {
        painter.rect_filled(rect, R[1], color(palette.hover));
    }
    if response.has_focus() {
        ring(painter, rect, R[1], palette);
    }
    paint_line(
        ui,
        painter,
        (rect.left() + S[1], rect.center().y),
        rect.width() - 2.0 * S[1],
        label,
        fonts::regular(T[2]),
        color(palette.text),
    );
    response.clicked()
}

/// A segmented control at the page's place: a tray with a segment for each label, the
/// one chosen filled. Answers the index of a segment pressed. With `works` false - what
/// it holds is not read yet - it takes no press.
fn segmented(
    ui: &mut egui::Ui,
    page: &mut Page,
    id: &str,
    labels: &[&str],
    chosen: Option<usize>,
    works: bool,
    palette: &Palette,
) -> Option<usize> {
    let font = fonts::regular(T[2]);
    let widths: Vec<f32> = (labels.iter())
        .map(|label| {
            let galley =
                page.painter
                    .layout_no_wrap((*label).to_owned(), font.clone(), Color32::WHITE);
            galley.size().x.ceil() + 2.0 * SEGMENT_PAD
        })
        .collect();
    let tray = Rect::from_min_size(
        pos2(page.left, page.y),
        vec2(
            widths.iter().sum::<f32>() + TRAY * (labels.len() + 1) as f32,
            SEGMENT + 2.0 * TRAY,
        ),
    );
    page.put(tray.bottom());
    page.gap(S[2]);
    if !page.sees(tray) {
        return None;
    }
    let painter = page.painter.clone();
    painter.rect_filled(tray, R[2], color(palette.field));
    let mut left = tray.left() + TRAY;
    let mut pressed = None;
    for (index, (label, width)) in labels.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(pos2(left, tray.top() + TRAY), vec2(*width, SEGMENT));
        left = rect.right() + TRAY;
        let sense = if works {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, Id::new(("settings-segment", id, *label)), sense);
        let on = chosen == Some(index);
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, works, on, *label));
        let (fill, tint) = if on {
            (Some(palette.accent), palette.on_accent)
        } else if works && response.hovered() {
            (Some(palette.hover), palette.text)
        } else if works {
            (None, palette.text)
        } else {
            (None, palette.text_dim)
        };
        if let Some(fill) = fill {
            painter.rect_filled(rect, R[1], color(fill));
        }
        if response.has_focus() {
            ring(&painter, rect, R[1], palette);
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            color(tint),
        );
        if response.clicked() {
            pressed = Some(index);
        }
    }
    pressed
}

/// A checkbox with its words at the page's place: a press on either ticks it. Answers
/// whether it was pressed.
fn checkbox(
    ui: &mut egui::Ui,
    page: &mut Page,
    label: &str,
    ticked: bool,
    works: bool,
    palette: &Palette,
    back: Option<Id>,
) -> bool {
    let row = page.row(CONTROL);
    page.put(row.bottom());
    if !page.sees(row) {
        return false;
    }
    let font = fonts::regular(T[2]);
    let words = (page.painter)
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
        .ceil();
    let at = Rect::from_min_size(
        row.min,
        vec2((BOX + S[1] + words).min(row.width()), row.height()),
    );
    let sense = if works {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(at, box_id(label), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, works, ticked, label));
    if back == Some(response.id) {
        response.request_focus();
    }
    let painter = page.painter.clone();
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
    if response.has_focus() {
        ring(&painter, square, R[0], palette);
    }
    let tint = if works {
        palette.text
    } else {
        palette.text_dim
    };
    paint_line(
        ui,
        &painter,
        (square.right() + S[1], row.center().y),
        row.right() - square.right() - S[1],
        label,
        font,
        color(tint),
    );
    response.clicked()
}

impl SettingsView {
    /// Settings in `rect`, the dialog's whole ground. `opened` in the frame it is first
    /// drawn.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        settings: &Settings,
        tags: &[TagCount],
        palette: &Palette,
        opened: bool,
    ) -> Done {
        // The interval's field opens with a history of its own: it is one field under one
        // id for every Settings opened, egui keeps a field's undo by id, and Ctrl+Z in a
        // Settings opened again brought back what was typed in the last one.
        if opened {
            egui::text_edit::TextEditState::default().store(ui.ctx(), interval_id());
            egui::text_edit::TextEditState::default().store(ui.ctx(), filter_id());
        }
        let painter = ui.painter().clone();
        let mut asked = Vec::new();
        // An Escape that is not the dialog's (`claims_escape`, which the dialog asked
        // before it took the key as its close).
        if self.claims_escape(ui.ctx(), settings)
            && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            let focused = ui.ctx().memory(|memory| memory.focused());
            asked.push(if self.asking.is_some() {
                Asked::No
            } else if focused == Some(rename_id()) {
                Asked::RenameCancel
            } else if focused == Some(filter_id()) {
                Asked::Filter(String::new())
            } else {
                Asked::AddClose
            });
        }

        let (header, cross) = self::header(ui, rect, TITLE, palette);

        // The section list, on the chrome down the left.
        let body = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
        let nav = Rect::from_min_max(body.min, pos2(body.left() + NAV, body.bottom()));
        let left = CornerRadius {
            nw: 0,
            ne: 0,
            sw: R[3] as u8,
            se: 0,
        };
        painter.rect_filled(nav, left, color(palette.chrome));
        painter.vline(nav.right() - 0.5, nav.y_range(), (1.0, color(palette.line)));
        let mut y = nav.top() + S[1];
        for section in SECTIONS {
            let row =
                Rect::from_min_size(pos2(nav.left() + 6.0, y), vec2(nav.width() - 12.0, NAV_ROW));
            y = row.bottom() + NAV_GAP;
            if nav_row(ui, row, section, section == settings.section, palette)
                && section != settings.section
            {
                asked.push(Asked::Section(section));
            }
        }

        // The section, scrolled by a number of its own.
        let place = Rect::from_min_max(pos2(nav.right(), body.top()), body.max);
        let inner = Rect::from_min_max(
            pos2(place.left() + S[3], place.top() + S[2]),
            pos2(place.right() - S[3], place.bottom() - S[2]),
        );
        if self.of != Some(settings.section) {
            self.of = Some(settings.section);
            self.column.scroll = 0.0;
        }
        let mut page = (self.column).begin(ui, Id::new("settings-column"), place, inner);
        let page = ui
            .scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                ui.set_clip_rect(inner.intersect(ui.clip_rect()));
                self.fill(ui, &mut page, settings, tags, palette);
                page
            })
            .inner;
        self.column.end(ui, &page, place, inner, palette);
        asked.extend(page.asked);

        // Made last, although it is drawn at the top: the last thing one wants to tab to.
        if close_labelled(ui, cross, palette, CLOSE) {
            return Done::Closed;
        }
        if asked.is_empty() {
            Done::Open
        } else {
            Done::Settings(asked)
        }
    }

    /// The keyboard back to `response`, if it is what a question answered in the last
    /// frame was asked from.
    fn give_back(&mut self, response: &egui::Response) {
        if self.returning == Some(response.id) {
            self.returning = None;
            response.request_focus();
        }
    }

    /// What the section shown holds.
    fn fill(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        tags: &[TagCount],
        palette: &Palette,
    ) {
        let section = settings.section;
        self.asked = self.asking.take();
        // The question answered in the last frame, its buttons gone with it: the
        // keyboard, which was on one of them, comes back to what had it when it was asked
        // - as the system's own question the Svelte dialog asks gives it back. On
        // nothing, Space pressed nothing and Tab began again at the top of the dialog.
        // Not asked of egui here, but where that widget is drawn (`give_back`): egui keeps
        // a keyboard asked for in this frame though nothing of that name is drawn in it -
        // a rename's answer that lands in the frame its question is answered closes its
        // field - and AccessKit was then told of a focus on nothing.
        self.returning = None;
        if self.asked.is_some() && settings.question().is_none() {
            self.returning = self.back_to.take();
        }
        match section {
            Section::Folders => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, FOLDERS_HINT, palette);
                self.folders(ui, page, settings, palette);
            }
            Section::Tags => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, TAGS_HINT, palette);
                self.tags(ui, page, settings, tags, palette);
            }
            Section::Statistics => {
                page.heading(ui, section.label(), palette);
                self.statistics(ui, page, settings, palette);
            }
            // The list the `?` sheet draws, in one column.
            Section::Shortcuts => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, crate::shortcuts::SECTION_HINT, palette);
                crate::shortcuts_view::draw_list(ui, page, false, palette);
            }
            Section::Appearance => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, APPEARANCE_HINT, palette);
                let labels: Vec<&str> = THEMES.iter().map(|(_, label)| *label).collect();
                let chosen = THEMES
                    .iter()
                    .position(|(theme, _)| *theme == settings.seen.theme);
                if let Some(index) = segmented(ui, page, "theme", &labels, chosen, true, palette)
                    && Some(index) != chosen
                {
                    page.asked.push(Asked::Theme(THEMES[index].0));
                }
                page.heading(ui, PHOTO_SIZE, palette);
                page.hint(ui, PHOTO_SIZE_HINT, palette);
                let labels: Vec<&str> = SIZES.iter().map(|(_, label)| *label).collect();
                let chosen = SIZES
                    .iter()
                    .position(|(size, _)| *size == settings.seen.size);
                if let Some(index) = segmented(ui, page, "size", &labels, chosen, true, palette)
                    && Some(index) != chosen
                {
                    page.asked.push(Asked::Size(SIZES[index].0));
                }
            }
            Section::Slideshow => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, SLIDESHOW_HINT, palette);
                self.interval(ui, page, settings, palette);
                page.gap(S[2]);
                let stored = settings.stored();
                let shuffle = stored.is_some_and(|stored| stored.shuffle);
                if checkbox(ui, page, SHUFFLE, shuffle, stored.is_some(), palette, None) {
                    page.asked.push(Asked::Shuffle(!shuffle));
                }
            }
            Section::Duplicates => {
                page.heading(ui, LOOK_ALIKES, palette);
                page.hint(ui, LOOK_ALIKES_HINT, palette);
                let stored = settings.stored().map(|stored| stored.distance);
                let labels: Vec<&str> = DISTANCES.iter().map(|distance| distance.label).collect();
                let chosen = stored.and_then(|stored| {
                    DISTANCES
                        .iter()
                        .position(|distance| distance.value == stored)
                });
                if let Some(index) = segmented(
                    ui,
                    page,
                    "look-alikes",
                    &labels,
                    chosen,
                    stored.is_some(),
                    palette,
                ) && Some(index) != chosen
                {
                    page.asked.push(Asked::Distance(DISTANCES[index].value));
                }
                if let Some(stored) = stored {
                    page.hint(ui, distance_hint(stored), palette);
                }
            }
            Section::People => {
                page.heading(ui, FIND_FACES, palette);
                page.hint(ui, FIND_FACES_HINT, palette);
                page.hint(ui, FIND_FACES_PRIVATE, palette);
                match settings.question() {
                    Some(question @ Question::SwitchOff(_)) => {
                        self.question(ui, page, question, box_id(FIND_FACES_BOX), palette);
                    }
                    _ => {
                        let on = settings.seen.find_faces;
                        let back = self.returning.take();
                        if checkbox(ui, page, FIND_FACES_BOX, on, true, palette, back) {
                            page.asked.push(Asked::FindFaces(!on));
                        }
                    }
                }
                if settings.seen.find_faces
                    && let Some(line) = &settings.seen.faces_line
                {
                    page.gap(S[2]);
                    page.hint(ui, line, palette);
                }
            }
            Section::About => {
                page.heading(ui, section.label(), palette);
                self.about(ui, page, settings, palette);
            }
        }
    }

    /// "Show each photo for [ ] seconds": a field that takes digits, stored at Enter and
    /// when it is left. Drawn whether or not it is in sight - egui lets go of a field
    /// that is not drawn, which would store the interval half typed.
    fn interval(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        palette: &Palette,
    ) {
        let row = page.row(CONTROL);
        page.put(row.bottom());
        let font = fonts::regular(T[2]);
        let works = settings.stored().is_some();
        let tint = color(palette.text);
        let before = (page.painter)
            .layout_no_wrap(INTERVAL_BEFORE.to_owned(), font.clone(), tint)
            .size()
            .x
            .ceil();
        if page.sees(row) {
            paint_line(
                ui,
                &page.painter,
                (row.left(), row.center().y),
                before + 1.0,
                INTERVAL_BEFORE,
                font.clone(),
                tint,
            );
        }
        let field = Rect::from_min_size(
            pos2(row.left() + before + S[1], row.top()),
            vec2(FIELD, row.height()),
        );
        let id = interval_id();
        // Enter stores what is typed. Read before the field is drawn and taken out of
        // the frame: egui's field gives the keyboard up at an Enter it sees.
        let in_field = ui.memory(|memory| memory.has_focus(id));
        let enter =
            in_field && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        if page.sees(field) {
            page.painter.rect_filled(field, R[1], color(palette.field));
        }
        let line = line_height(ui, &font);
        let edit = Rect::from_min_max(
            pos2(field.left() + S[1], field.center().y - line / 2.0),
            pos2(field.right() - S[1], field.center().y + line / 2.0),
        );
        let mut text = settings.interval().to_owned();
        let output = ui
            .scope_builder(UiBuilder::new().max_rect(edit), |ui| {
                ui.set_clip_rect(field.intersect(ui.clip_rect()));
                if !works {
                    ui.disable();
                }
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(font.clone())
                    .text_color(tint)
                    .desired_width(edit.width())
                    .show(ui)
            })
            .inner;
        output.response.widget_info(|| {
            let mut info = WidgetInfo::text_edit(works, "", settings.interval(), "");
            info.label = Some(INTERVAL_BEFORE.to_owned());
            info
        });
        if page.sees(row) {
            paint_line(
                ui,
                &page.painter,
                (field.right() + S[1], row.center().y),
                row.right() - field.right() - S[1],
                INTERVAL_AFTER,
                font,
                tint,
            );
        }
        let typed = output.response.changed();
        let left = output.response.lost_focus();
        if typed || enter || left {
            page.asked.push(Asked::Interval {
                text,
                store: enter || left,
            });
        }
    }

    /// The question a row asks, where it arises: its words, Cancel and the button that
    /// does what is asked about. The keyboard is put on Cancel as it is asked, as on the
    /// dialog layer's own question, so an Enter that came with it deletes nothing; and
    /// comes back to `back_to` - what asked - when it is answered.
    fn question(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        question: &Question,
        back_to: Id,
        palette: &Palette,
    ) {
        let first = self.asked.as_ref() != Some(question);
        self.asking = Some(question.clone());
        let words = page.words(
            ui,
            &question.words(),
            fonts::regular(T[2]),
            color(palette.text),
            Id::new("settings-question"),
        );
        page.gap(S[1]);
        let row = page.row(CONTROL);
        page.put(row.bottom());
        let cancel = button_width(ui, CANCEL);
        let label = question.yes();
        let yes = button_width(ui, label);
        let cancel_at = Rect::from_min_size(row.min, vec2(cancel, CONTROL));
        let yes_at = Rect::from_min_size(
            pos2(cancel_at.right() + S[1], row.top()),
            vec2(yes, CONTROL),
        );
        // Drawn in or out of sight: the keyboard put on Cancel must have something to
        // be on.
        let keep = button(ui, cancel_at, no_id(), CANCEL, Look::Plain);
        let off = button(
            ui,
            yes_at,
            Id::new("settings-question-yes"),
            label,
            Look::Danger,
        );
        if first {
            self.back_to = Some(back_to);
            keep.request_focus();
            // In a low window the row can be out of sight, the keyboard with it, and
            // nothing would bring it into sight: the section is scrolled to it.
            let below = row.bottom() - page.clip.bottom();
            let above = words.top() - page.clip.top();
            self.column.reveal = if below > 0.0 { below } else { above.min(0.0) };
            if self.column.reveal != 0.0 {
                ui.ctx().request_repaint();
            }
        }
        if keep.clicked() {
            page.asked.push(Asked::No);
        } else if off.clicked() {
            page.asked.push(Asked::Yes);
        }
    }

    /// Folders: every watched folder with its status and count, Rescan, Reveal in its
    /// place, and Remove… asked in its row; then "Add folder…", or the field a folder to
    /// add is typed in.
    fn folders(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        palette: &Palette,
    ) {
        let folders = &settings.seen.folders;
        if folders.is_empty() {
            page.words(
                ui,
                NO_FOLDERS,
                fonts::regular(T[2]),
                color(palette.text_dim),
                Id::new("settings-no-folders"),
            );
            page.gap(S[2]);
        }
        let name_font = fonts::semibold(ui.ctx(), T[2]);
        let small = fonts::regular(T[1]);
        let (name_line, small_line) = (line_height(ui, &name_font), line_height(ui, &small));
        let height = name_line + 2.0 * small_line + 2.0 * S[1];
        for folder in folders {
            let row = page.row(height);
            page.put(row.bottom());
            if page.sees(row) {
                self.folder_row(ui, page, row, folder, (name_line, small_line), palette);
            }
            if let Some(question @ Question::RemoveFolder { id, .. }) = settings.question()
                && *id == folder.id
            {
                page.gap(S[1]);
                self.question(ui, page, question, remove_folder_id(folder.id), palette);
                page.gap(S[1]);
            }
        }
        page.gap(S[2]);
        match &settings.adding {
            None => {
                let width = button_width(ui, ADD_FOLDER);
                let at = Rect::from_min_size(pos2(page.left, page.y), vec2(width, CONTROL));
                page.put(at.bottom());
                if page.sees(at)
                    && button(
                        ui,
                        at,
                        Id::new("settings-add-open"),
                        ADD_FOLDER,
                        Look::Primary,
                    )
                    .clicked()
                {
                    page.asked.push(Asked::AddOpen);
                }
            }
            Some(adding) => {
                if !self.adding {
                    opened_field(ui, add_id(), adding.path());
                }
                // Enter adds what has passed. Read before the field is drawn and taken
                // out of the frame: egui's field gives the keyboard up at an Enter.
                let in_field = ui.memory(|memory| memory.has_focus(add_id()));
                let enter = in_field
                    && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                let row = page.row(CONTROL);
                page.put(row.bottom());
                let add = button_width(ui, ADD);
                let choose = button_width(ui, CHOOSE);
                let cancel = button_width(ui, CANCEL);
                // Its Cancel: Escape closes the field only while it has the keyboard, and a
                // hand on the mouse has nothing else to close it by.
                let cancel_at = Rect::from_min_max(pos2(row.right() - cancel, row.top()), row.max);
                let add_at = Rect::from_min_size(
                    pos2(cancel_at.left() - S[0] - add, row.top()),
                    vec2(add, CONTROL),
                );
                let choose_at = Rect::from_min_size(
                    pos2(add_at.left() - S[0] - choose, row.top()),
                    vec2(choose, CONTROL),
                );
                let field_at =
                    Rect::from_min_max(row.min, pos2(choose_at.left() - S[0], row.bottom()));
                let (output, text) = text_field(
                    ui,
                    page,
                    field_at,
                    Field {
                        id: add_id(),
                        text: adding.path(),
                        placeholder: watched_list::PLACEHOLDER,
                        label: watched_list::FIELD,
                        refused: adding.problem().is_some(),
                    },
                    palette,
                );
                // The system's picker: the folder chosen is put in the field and asked
                // about as a typed one is. Not dimmed while it is open, which would take
                // the keyboard off it (`app/pick.rs`); while a path is being added, as
                // the field takes nothing then.
                let look = if adding.busy() {
                    Look::Off
                } else {
                    Look::Plain
                };
                if page.sees(choose_at)
                    && button(ui, choose_at, Id::new("settings-add-choose"), CHOOSE, look).clicked()
                {
                    page.asked.push(Asked::AddChoose);
                }
                let passed = matches!(adding.dest(), crate::export_dialog::Dest::Passed(_));
                let look = if passed && !adding.busy() {
                    Look::Primary
                } else {
                    Look::Off
                };
                let pressed = page.sees(add_at)
                    && button(ui, add_at, Id::new("settings-add-send"), ADD, look).clicked();
                let cancelled = page.sees(cancel_at)
                    && button(
                        ui,
                        cancel_at,
                        Id::new("settings-add-cancel"),
                        CANCEL,
                        Look::Plain,
                    )
                    .clicked();
                // What was typed in this frame comes first: the Enter of the same frame
                // would add the path as it was before the letter.
                if output.response.changed() {
                    page.asked.push(Asked::AddPath(text));
                }
                if enter || pressed {
                    page.asked.push(Asked::AddSend);
                }
                if cancelled {
                    page.asked.push(Asked::AddClose);
                }
                // The room of one line is kept under the field, whether or not anything
                // is said there, as under the export's: a reason that came and went as a
                // path was typed moved everything under it up and down.
                page.gap(S[0]);
                let line = small_line;
                let said_at = page.y;
                match adding.problem() {
                    Some(why) => {
                        let rect = page.words(
                            ui,
                            why,
                            small.clone(),
                            color(palette.danger),
                            Id::new("settings-add-problem"),
                        );
                        page.put(rect.bottom().max(said_at + line));
                    }
                    None => page.put(said_at + line),
                }
            }
        }
        self.adding = settings.adding.is_some();
    }

    /// One watched folder's row in `row`: its name, its path and what is said of it, and
    /// its three buttons.
    fn folder_row(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        row: Rect,
        folder: &FolderRow,
        (name_line, small_line): (f32, f32),
        palette: &Palette,
    ) {
        let middle = row.center().y;
        let at = |right: f32, label: &str| {
            let width = button_width(ui, label);
            Rect::from_min_size(
                pos2(right - width, middle - CONTROL / 2.0),
                vec2(width, CONTROL),
            )
        };
        let remove_at = at(row.right(), watched_list::REMOVE);
        let reveal_at = at(remove_at.left() - S[0], watched_list::REVEAL);
        let rescan_at = at(reveal_at.left() - S[0], RESCAN);
        let room = rescan_at.left() - S[2] - row.left();
        let painter = page.painter.clone();
        // An offline folder's name is fainter: it is there to be removed or waited for.
        let name_tint = if folder.online {
            palette.text
        } else {
            palette.text_dim
        };
        let details_tint = match folder.kind {
            Kind::Scanning => palette.accent,
            Kind::Offline | Kind::Degraded => palette.text,
            Kind::Online => palette.text_dim,
        };
        let mut y = row.top() + S[1];
        for (words, font, tint, line) in [
            (
                folder.name.as_str(),
                fonts::semibold(ui.ctx(), T[2]),
                name_tint,
                name_line,
            ),
            (
                folder.path.as_str(),
                fonts::regular(T[1]),
                palette.text_dim,
                small_line,
            ),
            (
                folder.details.as_str(),
                fonts::regular(T[1]),
                details_tint,
                small_line,
            ),
        ] {
            let place = Rect::from_min_size(pos2(row.left(), y), vec2(room, line));
            paint_line(
                ui,
                &painter,
                (place.left(), place.center().y),
                room,
                words,
                font,
                color(tint),
            );
            ui.interact(
                place,
                Id::new(("settings-folder", folder.id, y as i64)),
                Sense::hover(),
            )
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            y += line;
        }
        painter.hline(
            row.x_range(),
            row.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
        // `rescan_folder` does nothing while a scan of the folder runs, and says nothing
        // back: it is not offered then.
        // A folder whose removal is on its way offers neither.
        let look = if folder.scanning || folder.going {
            Look::Off
        } else {
            Look::Plain
        };
        let rescan = button(
            ui,
            rescan_at,
            Id::new(("settings-rescan", folder.id)),
            RESCAN,
            look,
        );
        if folder.scanning {
            rescan.on_hover_text(watched_list::BEING_SCANNED);
        } else if rescan.clicked() {
            page.asked.push(Asked::Rescan(folder.id));
        }
        // In the system's file manager - the likeliest of all to meet a drive that is
        // gone, Settings offering it for a folder that is offline.
        let reveal = button(
            ui,
            reveal_at,
            Id::new(("settings-reveal", folder.id)),
            watched_list::REVEAL,
            // Not of a folder whose removal is on its way, as its other buttons are not.
            if folder.going { Look::Off } else { Look::Plain },
        );
        if reveal.clicked() {
            page.asked.push(Asked::RevealWatched(folder.id));
        }
        let look = if folder.going { Look::Off } else { Look::Warn };
        let remove = button(
            ui,
            remove_at,
            remove_folder_id(folder.id),
            watched_list::REMOVE,
            look,
        );
        self.give_back(&remove);
        if remove.clicked() {
            page.asked.push(Asked::RemoveFolder {
                id: folder.id,
                path: folder.path.clone(),
            });
        }
    }

    /// Tags: the keywords, filtered, each renamed in its row or removed after its row has
    /// asked; then the changes made so far, each with Restore.
    fn tags(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        tags: &[TagCount],
        palette: &Palette,
    ) {
        let editing = settings.renamer.editing();
        if tags.is_empty() {
            page.words(
                ui,
                NO_TAGS,
                fonts::regular(T[2]),
                color(palette.text_dim),
                Id::new("settings-no-tags"),
            );
            page.gap(S[2]);
        } else {
            let row = page.row(CONTROL);
            page.put(row.bottom());
            let (output, text) = text_field(
                ui,
                page,
                row,
                Field {
                    id: filter_id(),
                    text: &settings.filter,
                    placeholder: FILTER,
                    label: FILTER,
                    refused: false,
                },
                palette,
            );
            if output.response.changed() {
                page.asked.push(Asked::Filter(text));
            }
            page.gap(S[1]);
            let shown = filter_tags(tags, &settings.filter);
            if shown.is_empty() {
                page.words(
                    ui,
                    &tag_manager::no_match(settings.filter.trim()),
                    fonts::regular(T[2]),
                    color(palette.text_dim),
                    Id::new("settings-no-match"),
                );
            }
            let name_font = fonts::semibold(ui.ctx(), T[2]);
            let small = fonts::regular(T[1]);
            let (name_line, small_line) = (line_height(ui, &name_font), line_height(ui, &small));
            let height = name_line + small_line + 2.0 * 6.0;
            for tag in shown {
                if editing == Some(tag.tag.as_str()) {
                    self.rename_row(ui, page, settings, palette);
                } else {
                    let row = page.row(height);
                    page.put(row.bottom());
                    // A library's keywords can be thousands: what is out of sight costs
                    // nothing but its height.
                    if page.sees(row) {
                        self.tag_row(ui, page, row, tag, (name_line, small_line), palette);
                    }
                }
                match settings.question() {
                    Some(question @ Question::RemoveTag(asked)) if *asked == tag.tag => {
                        page.gap(S[1]);
                        self.question(ui, page, question, remove_tag_id(&tag.tag), palette);
                        page.gap(S[1]);
                    }
                    Some(question @ Question::Rename(_)) if editing == Some(tag.tag.as_str()) => {
                        page.gap(S[1]);
                        self.question(ui, page, question, rename_id(), palette);
                        page.gap(S[1]);
                    }
                    _ => {}
                }
            }
        }
        if let Some(rules) = &settings.rules
            && !rules.is_empty()
        {
            page.heading(ui, CHANGES, palette);
            for rule in rules {
                let label = rule_label(rule);
                let row = page.row(CONTROL + 2.0 * S[0]);
                page.put(row.bottom());
                if !page.sees(row) {
                    continue;
                }
                let width = button_width(ui, RESTORE);
                let restore_at = Rect::from_min_size(
                    pos2(row.right() - width, row.center().y - CONTROL / 2.0),
                    vec2(width, CONTROL),
                );
                let room = restore_at.left() - S[2] - row.left();
                let place =
                    Rect::from_min_size(pos2(row.left(), row.top()), vec2(room, row.height()));
                paint_line(
                    ui,
                    &page.painter,
                    (place.left(), place.center().y),
                    room,
                    &label,
                    fonts::semibold(ui.ctx(), T[2]),
                    color(palette.text),
                );
                ui.interact(place, Id::new(("settings-rule", &rule.tag)), Sense::hover())
                    .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &label));
                page.painter.hline(
                    row.x_range(),
                    row.bottom() - 0.5,
                    (1.0, color(palette.line)),
                );
                if button(
                    ui,
                    restore_at,
                    Id::new(("settings-restore", &rule.tag)),
                    RESTORE,
                    Look::Plain,
                )
                .clicked()
                {
                    page.asked.push(Asked::Restore(rule.tag.clone()));
                }
            }
        }
        self.renaming = editing.map(str::to_owned);
    }

    /// A keyword's row: its name and count, Rename and Remove….
    fn tag_row(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        row: Rect,
        tag: &TagCount,
        (name_line, small_line): (f32, f32),
        palette: &Palette,
    ) {
        let middle = row.center().y;
        let at = |right: f32, label: &str| {
            let width = button_width(ui, label);
            Rect::from_min_size(
                pos2(right - width, middle - CONTROL / 2.0),
                vec2(width, CONTROL),
            )
        };
        let remove_at = at(row.right(), tag_manager::REMOVE);
        let rename_at = at(remove_at.left() - S[0], RENAME);
        let room = rename_at.left() - S[2] - row.left();
        let painter = page.painter.clone();
        let count = tag_manager::photos(tag.total);
        let mut y = row.top() + 6.0;
        for (words, font, tint, line) in [
            (
                tag.tag.as_str(),
                fonts::semibold(ui.ctx(), T[2]),
                palette.text,
                name_line,
            ),
            (
                count.as_str(),
                fonts::regular(T[1]),
                palette.text_dim,
                small_line,
            ),
        ] {
            let place = Rect::from_min_size(pos2(row.left(), y), vec2(room, line));
            paint_line(
                ui,
                &painter,
                (place.left(), place.center().y),
                room,
                words,
                font,
                color(tint),
            );
            ui.interact(
                place,
                Id::new(("settings-tag", &tag.tag, y as i64)),
                Sense::hover(),
            )
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            y += line;
        }
        painter.hline(
            row.x_range(),
            row.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
        if button(
            ui,
            rename_at,
            Id::new(("settings-tag-rename-button", &tag.tag)),
            RENAME,
            Look::Plain,
        )
        .clicked()
        {
            page.asked.push(Asked::RenameStart(tag.tag.clone()));
        }
        let remove = button(
            ui,
            remove_at,
            remove_tag_id(&tag.tag),
            tag_manager::REMOVE,
            Look::Warn,
        );
        self.give_back(&remove);
        if remove.clicked() {
            page.asked.push(Asked::RemoveTag(tag.tag.clone()));
        }
    }

    /// The row of the keyword being renamed: its field, seeded with its name, and what
    /// was said of the name - drawn in sight or out of it, since it holds the keyboard.
    fn rename_row(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        palette: &Palette,
    ) {
        let renamer = &settings.renamer;
        let tag = renamer.editing().unwrap_or_default();
        if self.renaming.as_deref() != Some(tag) {
            opened_field(ui, rename_id(), renamer.text());
        }
        // Enter stores the name, read before the field is drawn and taken out of the
        // frame: egui's field gives the keyboard up at an Enter, and a name refused or
        // asked about stays in the field to be put right.
        let in_field = ui.memory(|memory| memory.has_focus(rename_id()));
        let enter =
            in_field && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        page.gap(6.0);
        let row = page.row(CONTROL);
        page.put(row.bottom());
        let label = tag_manager::rename_label(tag);
        let (output, text) = text_field(
            ui,
            page,
            row,
            Field {
                id: rename_id(),
                text: renamer.text(),
                placeholder: "",
                label: &label,
                refused: !renamer.error().is_empty(),
            },
            palette,
        );
        self.give_back(&output.response);
        // What was typed is said before what stores it: a letter and Enter in one long
        // frame stored the name without the letter.
        if output.response.changed() {
            page.asked.push(Asked::RenameType(text));
        }
        if enter {
            page.asked.push(Asked::RenameCommit);
        } else if output.response.lost_focus() {
            // Left - by a press elsewhere, by Tab - it is given up, as the Svelte field's
            // blur gives it up; not while its rename is on its way or its row asks, which
            // the renamer itself sees to.
            page.asked.push(Asked::RenameCancel);
        }
        if !renamer.error().is_empty() {
            page.gap(S[0]);
            page.words(
                ui,
                renamer.error(),
                fonts::regular(T[1]),
                color(palette.danger),
                Id::new("settings-rename-error"),
            );
        }
        page.gap(6.0);
    }

    /// Statistics: the summary line, then the years, the cameras and the lenses, each row
    /// a bar of its share of the largest and a search for its photos.
    fn statistics(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        palette: &Palette,
    ) {
        use crate::{settings::Counted, stats};
        let counted = match &settings.stats {
            Counted::Counted(counted) => counted,
            Counted::Failed => {
                page.hint(ui, stats::NOT_COUNTED, palette);
                return;
            }
            Counted::NotAsked | Counted::Counting => {
                page.hint(ui, stats::COUNTING, palette);
                return;
            }
        };
        let summary = format!("{}. {}", stats::summary(counted), stats::SUMMARY_TAIL);
        page.hint(ui, &summary, palette);
        let lists = [
            (stats::YEARS, stats::year_rows(counted)),
            (stats::CAMERAS, stats::camera_rows(counted)),
            (stats::LENSES, stats::lens_rows(counted)),
        ];
        let font = fonts::regular(T[2]);
        for (title, rows) in lists {
            if rows.is_empty() {
                continue;
            }
            page.heading(ui, title, palette);
            for row in &rows {
                let rect = page.row(STAT_ROW);
                page.put(rect.bottom());
                if !page.sees(rect) {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    Id::new(("settings-stat", &row.search)),
                    Sense::click(),
                );
                let count = crate::grid::labels::grouped(usize::try_from(row.count).unwrap_or(0));
                let label = format!("{}, {count}", row.label);
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
                if response.hovered() {
                    page.painter.rect_filled(rect, R[1], color(palette.hover));
                }
                if response.has_focus() {
                    ring(&page.painter, rect, R[1], palette);
                }
                let inner = rect.shrink2(vec2(S[1], 0.0));
                let label_room = STAT_LABEL.min(inner.width() / 3.0);
                let count_at = inner.right() - STAT_COUNT;
                paint_line(
                    ui,
                    &page.painter,
                    (inner.left(), inner.center().y),
                    label_room,
                    &row.label,
                    font.clone(),
                    color(palette.text),
                );
                let track = Rect::from_min_max(
                    pos2(inner.left() + label_room + S[2], inner.center().y - 3.0),
                    pos2(count_at - S[2], inner.center().y + 3.0),
                );
                page.painter.rect_filled(track, R[0], color(palette.field));
                let bar = Rect::from_min_size(
                    track.min,
                    vec2((track.width() * row.share as f32).max(2.0), track.height()),
                );
                page.painter.rect_filled(bar, R[0], color(palette.accent));
                page.painter.text(
                    pos2(inner.right(), inner.center().y),
                    egui::Align2::RIGHT_CENTER,
                    &count,
                    font.clone(),
                    color(palette.text_dim),
                );
                if response.clicked() {
                    page.asked.push(Asked::Search(row.search.clone()));
                }
            }
        }
        // After every list, as the Svelte section says it, and only where there are
        // cameras to set the photos without one beside.
        if counted.no_camera > 0 && !counted.cameras.is_empty() {
            page.gap(S[2]);
            page.hint(ui, &stats::without_camera(counted.no_camera), palette);
        }
    }

    /// About: the version, the library and its Reveal, photon's memory, the licence, as
    /// a list of terms and what they are.
    fn about(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        settings: &Settings,
        palette: &Palette,
    ) {
        let font = fonts::regular(T[2]);
        let line = line_height(ui, &font);
        let terms = [VERSION, LIBRARY, MEMORY, LICENCE];
        let term_width = (terms.iter())
            .map(|term| {
                (page.painter)
                    .layout_no_wrap((*term).to_owned(), font.clone(), Color32::WHITE)
                    .size()
                    .x
                    .ceil()
            })
            .fold(0.0, f32::max);
        let value_left = page.left + term_width + S[3];
        let value_width = page.left + page.width - value_left;
        let about = &settings.about;
        page.gap(S[1]);
        let term = Term {
            width: term_width,
            line,
            value_left,
            font: font.clone(),
            tint: palette.text_dim,
        };
        let version = format!("photon {}", about.version);
        let at = term.pair(ui, page, VERSION, line, false);
        value(ui, page, at, &version, palette.text, &font);

        // The library's path, cut short to the room beside its Reveal.
        let at = term.pair(ui, page, LIBRARY, CONTROL, true);
        let reveal = button_width(ui, REVEAL);
        let reveal_at =
            Rect::from_min_size(pos2(at.right() - reveal, at.top()), vec2(reveal, CONTROL));
        // Its words in the middle of the button's height, as the Svelte row centres them.
        let path_at = Rect::from_min_max(
            pos2(at.left(), at.center().y - line / 2.0),
            pos2(reveal_at.left() - S[1], at.bottom()),
        );
        value(ui, page, path_at, &about.library, palette.text, &font);
        if page.sees(reveal_at)
            && button(
                ui,
                reveal_at,
                Id::new("settings-reveal-library"),
                REVEAL,
                Look::Plain,
            )
            .clicked()
        {
            page.asked.push(Asked::RevealLibrary);
        }

        if let Some(memory) = settings.memory {
            let at = term.pair(ui, page, MEMORY, 2.0 * line, false);
            let top = Rect::from_min_size(at.min, vec2(value_width, line));
            value(
                ui,
                page,
                top,
                &memory_amount(memory.bytes),
                palette.text,
                &font,
            );
            let under = top.translate(vec2(0.0, line));
            value(
                ui,
                page,
                under,
                &memory_scope(memory.processes),
                palette.text_dim,
                &font,
            );
        }
        let at = term.pair(ui, page, LICENCE, line, false);
        value(ui, page, at, &about.licence, palette.text, &font);
    }
}

/// About's column of terms.
struct Term {
    width: f32,
    line: f32,
    value_left: f32,
    font: FontId,
    tint: crate::theme::tokens::Rgba,
}

impl Term {
    /// A row `height` tall with `term` at its left - on its first line, or in its middle
    /// beside a value that is a button's height - and the room its value has.
    fn pair(&self, ui: &egui::Ui, page: &mut Page, term: &str, height: f32, middle: bool) -> Rect {
        let row = Rect::from_min_size(pos2(page.left, page.y), vec2(page.width, height));
        page.put(row.bottom());
        page.gap(S[1]);
        if page.sees(row) {
            let top = if middle {
                row.center().y - self.line / 2.0
            } else {
                row.top()
            };
            let at = Rect::from_min_size(pos2(row.left(), top), vec2(self.width, self.line));
            paint_line(
                ui,
                &page.painter,
                (at.left(), at.center().y),
                self.width + 1.0,
                term,
                self.font.clone(),
                color(self.tint),
            );
            ui.interact(at, Id::new(("settings-term", term)), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, term));
        }
        Rect::from_min_max(pos2(self.value_left, row.top()), row.max)
    }
}

/// A value of About on one line at the top of `at`, cut short to it, and told to
/// AccessKit.
fn value(
    ui: &egui::Ui,
    page: &Page,
    at: Rect,
    words: &str,
    tint: crate::theme::tokens::Rgba,
    font: &FontId,
) {
    let line = Rect::from_min_size(at.min, vec2(at.width(), line_height(ui, font)));
    if !page.sees(line) {
        return;
    }
    paint_line(
        ui,
        &page.painter,
        (line.left(), line.center().y),
        line.width(),
        words,
        font.clone(),
        color(tint),
    );
    ui.interact(line, Id::new(("settings-value", words)), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dialog_view::tests::{Fixture, WINDOW},
        dialogs::Dialog,
        settings::{About, Memory, STOP_FINDING, Stored, switch_off_warning},
    };
    use eframe::egui::{Event, MouseWheelUnit, Vec2};
    use photon_core::library::{GridTile, ThemeChoice};

    fn about() -> About {
        About {
            version: "0.53.1".to_owned(),
            library: "/home/ada/.local/share/photon/library.db".to_owned(),
            licence: "MIT".to_owned(),
        }
    }

    fn stored() -> Stored {
        Stored {
            interval: 4,
            shuffle: false,
            distance: 7,
        }
    }

    /// Settings at `section`, read as the library holds `stored()` when `read`.
    fn settings(section: Section, read: bool) -> Fixture {
        let mut settings = Settings::new(section, about());
        if read {
            settings.read(stored());
        }
        let mut f = Fixture::new(Dialog::Settings(Box::new(settings)));
        f.open();
        f
    }

    fn the(f: &mut Fixture) -> &mut Settings {
        match &mut f.dialog {
            Dialog::Settings(settings) => settings,
            other => panic!("{other:?}"),
        }
    }

    fn asked(done: Done) -> Vec<Asked> {
        match done {
            Done::Settings(asked) => asked,
            Done::Open => Vec::new(),
            other => panic!("{other:?}"),
        }
    }

    /// A click on the words `label`, wherever they are drawn last.
    fn click(f: &mut Fixture, label: &str) -> Vec<Asked> {
        let at = f.at(label);
        asked(f.click(at))
    }

    // The section list down the left, the one shown marked; a row pressed asks for its
    // section, and the one shown for nothing.
    #[test]
    fn a_section_is_chosen_from_the_list_and_shown_beside_it() {
        let mut f = settings(Section::Folders, true);
        let ground = f.ground();
        assert_eq!(ground.width(), 720.0);
        assert_eq!(ground.height(), 520.0);
        let folders = f.drew("Folders").map(|at| at.left()).unwrap();
        assert!(folders < ground.left() + NAV, "the list is at the left");
        assert!(f.drew(FOLDERS_HINT).is_some());
        assert_eq!(click(&mut f, "Folders"), []);
        assert_eq!(
            click(&mut f, "Appearance"),
            [Asked::Section(Section::Appearance)]
        );
        f.frame(Vec::new());
        assert!(f.drew(FOLDERS_HINT).is_none());
        assert!(f.drew(APPEARANCE_HINT).is_some());
        // The one shown has the accent's soft fill under its row.
        let row = f.drew("Appearance").unwrap();
        let soft = f.tint(|palette| palette.accent_soft);
        assert!(
            (f.fills.iter()).any(|(rect, fill)| *fill == soft && rect.contains(row.center())),
            "the section shown is marked"
        );
    }

    // A theme and a size are asked for at their press; the one held is no change.
    #[test]
    fn appearance_asks_for_a_theme_and_a_size() {
        let mut f = settings(Section::Appearance, true);
        the(&mut f).seen.theme = ThemeChoice::Light;
        f.frame(Vec::new());
        assert_eq!(click(&mut f, "Dark"), [Asked::Theme(ThemeChoice::Dark)]);
        assert_eq!(click(&mut f, "Light"), []);
        assert_eq!(click(&mut f, "Large"), [Asked::Size(GridTile::Large)]);
        assert_eq!(click(&mut f, "Medium"), []);
        // The second heading stands its own room under the control above it, the
        // control's own room taken into it, as one CSS margin collapses into the next.
        let tray = f.drew("Dark").unwrap().center().y + SEGMENT / 2.0 + TRAY;
        let heading = f.drew(PHOTO_SIZE).unwrap().top();
        let gap = heading - tray;
        assert!((S[4]..S[4] + 6.0).contains(&gap), "{gap}");
    }

    // Nothing that shows a stored value takes a press before it has been read: what it
    // would show is not the library's.
    #[test]
    fn nothing_stored_is_pressed_before_it_is_read() {
        let mut f = settings(Section::Slideshow, false);
        assert_eq!(click(&mut f, SHUFFLE), []);
        f.typed("9");
        assert_eq!(the(&mut f).interval(), "");
        assert_eq!(
            click(&mut f, "Duplicates"),
            [Asked::Section(Section::Duplicates)]
        );
        f.frame(Vec::new());
        assert_eq!(click(&mut f, "Loose"), []);
        assert!(
            DISTANCES
                .iter()
                .all(|distance| f.drew(distance.hint).is_none()),
            "no hint of a distance not read"
        );
    }

    // The interval is typed, and stored at Enter or when the field is left; a box and
    // the look-alikes are asked for at a press, and say what is stored.
    #[test]
    fn the_slideshow_and_the_look_alikes_ask_for_what_is_pressed() {
        let mut f = settings(Section::Slideshow, true);
        assert!(f.drew("4").is_some(), "the interval stored");
        let field = f.drew("4").unwrap().center();
        f.click(field);
        assert_eq!(f.focused(), Some(interval_id()));
        let typed = asked(f.typed("5"));
        assert!(
            matches!(&typed[..], [Asked::Interval { store: false, .. }]),
            "{typed:?}"
        );
        assert_eq!(
            asked(f.key(Key::Enter)),
            [Asked::Interval {
                text: the(&mut f).interval().to_owned(),
                store: true
            }]
        );
        // The caret stays: the Enter is taken before egui's field would give the
        // keyboard up at it.
        assert_eq!(f.focused(), Some(interval_id()));
        // Left by Tab: stored as well.
        f.click(field);
        let left = asked(f.key(Key::Tab));
        assert!(
            matches!(&left[..], [Asked::Interval { store: true, .. }]),
            "{left:?}"
        );
        assert_eq!(click(&mut f, SHUFFLE), [Asked::Shuffle(true)]);

        assert_eq!(
            click(&mut f, "Duplicates"),
            [Asked::Section(Section::Duplicates)]
        );
        f.frame(Vec::new());
        assert!(f.drew(DISTANCES[1].hint).is_some(), "the stored one's hint");
        assert_eq!(click(&mut f, "Loose"), [Asked::Distance(10)]);
        assert_eq!(click(&mut f, "Conservative"), []);
    }

    // Find faces ticked off asks for it; with people named the row is the question, the
    // keyboard on its Cancel, so an Enter deletes nothing.
    #[test]
    fn find_faces_off_with_people_named_is_asked_in_the_row() {
        let mut f = settings(Section::People, true);
        the(&mut f).seen.find_faces = true;
        f.frame(Vec::new());
        assert_eq!(click(&mut f, FIND_FACES_BOX), [Asked::FindFaces(false)]);
        assert!(the(&mut f).ask_off());
        assert!(!the(&mut f).named(2));
        f.frame(Vec::new());
        assert!(f.drew(FIND_FACES_BOX).is_none(), "the row is the question");
        assert!(f.drew(&switch_off_warning(2)).is_some());
        assert_eq!(f.focused(), Some(no_id()));
        assert_eq!(asked(f.key(Key::Enter)), [Asked::No]);
        assert_eq!(click(&mut f, STOP_FINDING), [Asked::Yes]);
        assert_eq!(click(&mut f, CANCEL), [Asked::No]);
        // Answered, the row is the box again; ticked on is asked for as it is.
        the(&mut f).answered();
        the(&mut f).seen.find_faces = false;
        f.frame(Vec::new());
        assert_eq!(click(&mut f, FIND_FACES_BOX), [Asked::FindFaces(true)]);
    }

    // The pass's line stands under the box while it runs, and only while photon looks.
    #[test]
    fn the_face_pass_is_said_under_the_box() {
        let mut f = settings(Section::People, true);
        let line = "Finding faces: 3 of 10".to_owned();
        the(&mut f).seen.faces_line = Some(line.clone());
        f.frame(Vec::new());
        assert!(f.drew(&line).is_none(), "said while photon does not look");
        the(&mut f).seen.find_faces = true;
        f.frame(Vec::new());
        assert!(f.drew(&line).is_some());
    }

    // About: the version, the library with its Reveal - drawn and taking no press - the
    // memory once it has been read, the licence.
    #[test]
    fn about_says_the_version_the_library_and_the_memory() {
        let mut f = settings(Section::About, true);
        assert!(f.drew("photon 0.53.1").is_some());
        assert!(f.drew(&about().library).is_some());
        assert!(f.drew("MIT").is_some());
        assert!(f.drew(MEMORY).is_none(), "no memory before it is read");
        assert_eq!(asked(f.click(f.at(REVEAL))), [Asked::RevealLibrary]);
        the(&mut f).memory = Some(Memory {
            bytes: 350 * 1_048_576,
            processes: 1,
        });
        f.frame(Vec::new());
        assert!(f.drew("350 MB").is_some());
        assert!(f.drew("photon's own process").is_some());
    }

    // The cross closes, as Escape does.
    #[test]
    fn the_cross_closes() {
        let mut f = settings(Section::About, true);
        let cross = f.ground().right_top() + vec2(-S[2] - CROSS / 2.0, HEADER / 2.0);
        assert_eq!(f.click(cross), Done::Closed);
    }

    fn folder(id: i64, scanning: Option<u64>) -> FolderRow {
        let watched = photon_core::library::WatchedFolder {
            id,
            path: format!("/photos/f{id}"),
            online: true,
        };
        watched_list::row(&watched, scanning, false, Some(3))
    }

    /// Settings at Folders, two folders watched, the second of them being scanned.
    fn folders() -> Fixture {
        let mut f = settings(Section::Folders, true);
        the(&mut f).seen.folders = vec![folder(1, None), folder(2, Some(40))];
        f.frame(Vec::new());
        f
    }

    /// The button called `label` on the row whose words are `row`'s: the nearest to them
    /// of that name.
    fn on_row(f: &Fixture, row: &str, label: &str) -> egui::Pos2 {
        let y = f.at(row).y;
        (f.texts.iter())
            .filter(|(text, _)| text == label)
            .map(|(_, rect)| rect.center())
            .min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs()))
            .unwrap_or_else(|| panic!("no {label}"))
    }
    // A folder whose removal is on its way offers nothing: its Reveal no more than its
    // Rescan and Remove… - pressed after the folder was gone, it said the library's "not
    // found".
    #[test]
    fn a_folder_being_removed_offers_no_reveal() {
        let mut f = folders();
        the(&mut f).seen.folders[0].going = true;
        f.frame(Vec::new());
        let at = on_row(&f, "f1", watched_list::REVEAL);
        assert_eq!(asked(f.click(at)), []);
    }

    // Each folder's row: its name, path and what is said of it, Rescan - no press while
    // it is scanned - Reveal in its place, and Remove…, which its row asks.
    #[test]
    fn a_folders_row_rescans_and_asks_before_it_removes() {
        let mut f = folders();
        assert!(f.drew("f1").is_some() && f.drew("/photos/f1").is_some());
        assert!(f.drew("Online · 3 photos").is_some());
        assert!(f.drew("Scanning… 40 files · 3 photos").is_some());
        let at = on_row(&f, "f1", RESCAN);
        assert_eq!(asked(f.click(at)), [Asked::Rescan(1)]);
        let at = on_row(&f, "f2", RESCAN);
        assert_eq!(asked(f.click(at)), [], "being scanned");
        let at = on_row(&f, "f1", watched_list::REVEAL);
        assert_eq!(asked(f.click(at)), [Asked::RevealWatched(1)]);
        let at = on_row(&f, "f2", watched_list::REMOVE);
        assert_eq!(
            asked(f.click(at)),
            [Asked::RemoveFolder {
                id: 2,
                path: "/photos/f2".to_owned()
            }]
        );
        the(&mut f).ask(Question::RemoveFolder {
            id: 2,
            path: "/photos/f2".to_owned(),
        });
        f.frame(Vec::new());
        let words = f
            .drew(&watched_list::remove_question("/photos/f2"))
            .expect("the row asks");
        assert!(words.top() > f.at("f2").y, "under its own row");
        assert_eq!(f.focused(), Some(no_id()));
        assert_eq!(click(&mut f, watched_list::REMOVE_YES), [Asked::Yes]);
        // Escape is the question's Cancel, and leaves Settings open.
        assert_eq!(asked(f.key(Key::Escape)), [Asked::No]);
        // Answered, the keyboard is back on the button that asked.
        the(&mut f).answered();
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(remove_folder_id(2)));
    }

    // "Add folder…" opens the field, which takes the keyboard; what is typed is asked
    // about, Enter adds, Escape closes the field and not Settings.
    #[test]
    fn a_folder_to_add_is_typed_in_its_field() {
        let mut f = folders();
        assert_eq!(click(&mut f, ADD_FOLDER), [Asked::AddOpen]);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(add_id()));
        assert_eq!(
            asked(f.typed("/photos/new")),
            [Asked::AddPath("/photos/new".to_owned())]
        );
        assert_eq!(asked(f.key(Key::Enter)), [Asked::AddSend]);
        assert_eq!(f.focused(), Some(add_id()), "the caret stays");
        assert_eq!(asked(f.key(Key::Escape)), [Asked::AddClose]);
        f.frame(Vec::new());
        // "Choose…" asks for the system's picker.
        assert_eq!(click(&mut f, ADD_FOLDER), [Asked::AddOpen]);
        f.frame(Vec::new());
        assert_eq!(click(&mut f, CHOOSE), [Asked::AddChoose]);
    }

    fn tag(name: &str, total: i64) -> TagCount {
        TagCount {
            tag: name.to_owned(),
            count: total,
            total,
        }
    }

    /// Settings at Tags, the library holding three keywords and one change.
    fn tags() -> Fixture {
        let mut f = settings(Section::Tags, true);
        f.tags = vec![tag("hiking", 5), tag("mountains", 12), tag("lake", 1)];
        the(&mut f).rules = Some(vec![photon_core::library::TagRule {
            tag: "Faro".to_owned(),
            target: None,
        }]);
        f.frame(Vec::new());
        f
    }

    // The keywords with their counts, filtered without case; Escape in a filter that
    // holds something empties it and closes nothing.
    #[test]
    fn the_keywords_are_listed_and_filtered() {
        let mut f = tags();
        assert!(f.drew("12 photos").is_some() && f.drew("1 photo").is_some());
        let field = f.at(FILTER);
        f.click(field);
        assert_eq!(f.focused(), Some(filter_id()));
        assert_eq!(asked(f.typed("MOUN")), [Asked::Filter("MOUN".to_owned())]);
        f.frame(Vec::new());
        assert!(f.drew("mountains").is_some());
        assert!(f.drew("hiking").is_none());
        assert_eq!(asked(f.key(Key::Escape)), [Asked::Filter(String::new())]);
        f.frame(Vec::new());
        assert!(f.drew("hiking").is_some());
        // An empty filter's Escape is the dialog's.
        assert_eq!(f.key(Key::Escape), Done::Closed);
    }

    #[test]
    fn a_filter_that_holds_no_keyword_says_so() {
        let mut f = tags();
        the(&mut f).filter = "zzz".to_owned();
        f.frame(Vec::new());
        assert!(f.drew(&tag_manager::no_match("zzz")).is_some());
    }

    // Rename opens the keyword's field with its name chosen, so that what is typed
    // replaces it; Enter stores, Escape and leaving the field give it up.
    #[test]
    fn a_keyword_is_renamed_in_its_row() {
        let mut f = tags();
        let at = on_row(&f, "hiking", RENAME);
        assert_eq!(
            asked(f.click(at)),
            [Asked::RenameStart("hiking".to_owned())]
        );
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(rename_id()));
        assert_eq!(
            asked(f.typed("walks")),
            [Asked::RenameType("walks".to_owned())]
        );
        assert_eq!(
            the(&mut f).renamer.text(),
            "walks",
            "the name was chosen and replaced"
        );
        assert_eq!(asked(f.key(Key::Enter)), [Asked::RenameCommit]);
        assert_eq!(asked(f.key(Key::Escape)), [Asked::RenameCancel]);
        the(&mut f).renamer.start("hiking");
        f.frame(Vec::new());
        f.frame(Vec::new());
        let away = f.at("mountains");
        let came: Vec<Asked> = asked(f.click(away));
        assert!(came.contains(&Asked::RenameCancel), "{came:?}");
    }

    // A merge asks in the row, under the field, with its own button.
    #[test]
    fn a_rename_onto_another_keyword_asks_in_its_row() {
        let mut f = tags();
        the(&mut f).renamer.start("hiking");
        the(&mut f).renamer.type_in("mountains");
        let rules = the(&mut f).rules.clone().unwrap_or_default();
        let committed = the(&mut f).renamer.commit(&[tag("mountains", 12)], &rules);
        let tag_manager::Committed::Asks(words) = committed else {
            panic!("{committed:?}");
        };
        the(&mut f).ask(Question::Rename(words.clone()));
        f.frame(Vec::new());
        assert!(f.drew(&words).is_some());
        assert_eq!(f.focused(), Some(no_id()));
        assert_eq!(click(&mut f, tag_manager::MERGE_YES), [Asked::Yes]);
    }

    // The keyboard goes back to what asked only where that is drawn: a rename's answer
    // that lands in the frame its question is answered closes the field, and egui keeps a
    // keyboard asked for in that frame on a widget that is not there - which AccessKit
    // was told of, a focus on nothing (`egui_kittest` panicked on it under load).
    #[test]
    fn the_keyboard_goes_back_to_nothing_that_is_not_drawn() {
        let mut f = tags();
        the(&mut f).renamer.start("lake");
        the(&mut f).renamer.type_in("hiking");
        let rules = the(&mut f).rules.clone().unwrap_or_default();
        let tag_manager::Committed::Asks(words) =
            the(&mut f).renamer.commit(&[tag("hiking", 5)], &rules)
        else {
            panic!("a merge asks");
        };
        the(&mut f).ask(Question::Rename(words));
        f.frame(Vec::new());
        assert_eq!(f.focused(), Some(no_id()));
        // Answered, and the rename it agreed to stored, before the next frame.
        the(&mut f).answered();
        let rename = the(&mut f).renamer.agreed().expect("agreed");
        the(&mut f).renamer.stored(rename.sent);
        f.frame(Vec::new());
        assert_ne!(f.focused(), Some(rename_id()), "the field is gone");
    }

    // A question a filter hides is not on screen, and takes no Escape: the filter's is to
    // empty it.
    #[test]
    fn a_question_the_filter_hides_takes_no_escape() {
        let mut f = tags();
        the(&mut f).ask(Question::RemoveTag("lake".to_owned()));
        f.frame(Vec::new());
        let field = f.at(FILTER);
        f.click(field);
        f.typed("moun");
        f.frame(Vec::new());
        assert!(f.drew(&tag_manager::remove_question("lake")).is_none());
        assert_eq!(asked(f.key(Key::Escape)), [Asked::Filter(String::new())]);
    }

    // Remove… asks in its row; a change is restored by its button.
    #[test]
    fn a_keyword_is_removed_after_its_row_asks_and_a_change_restored() {
        let mut f = tags();
        let at = on_row(&f, "lake", tag_manager::REMOVE);
        assert_eq!(asked(f.click(at)), [Asked::RemoveTag("lake".to_owned())]);
        the(&mut f).ask(Question::RemoveTag("lake".to_owned()));
        f.frame(Vec::new());
        assert!(f.drew(&tag_manager::remove_question("lake")).is_some());
        assert!(f.drew("Faro — removed").is_some());
        assert_eq!(click(&mut f, RESTORE), [Asked::Restore("Faro".to_owned())]);
    }

    // Statistics: counting, failed, then the summary and the lists, a row a search; the
    // cameras say how many have no camera data.
    #[test]
    fn statistics_counts_and_a_row_asks_for_its_search() {
        use crate::settings::Counted;
        use photon_core::library::{CameraCount, LibraryStats, YearCount};
        let mut f = settings(Section::Statistics, true);
        assert!(f.drew(crate::stats::COUNTING).is_some());
        the(&mut f).stats = Counted::Failed;
        f.frame(Vec::new());
        assert!(f.drew(crate::stats::NOT_COUNTED).is_some());
        the(&mut f).stats = Counted::Counted(Box::new(LibraryStats {
            photos: 3,
            videos: 0,
            bytes: 2 * 1_048_576,
            oldest: None,
            newest: None,
            years: vec![YearCount {
                year: 2024,
                count: 3,
            }],
            cameras: vec![CameraCount {
                make: Some("Canon".to_owned()),
                model: Some("Canon EOS 5D".to_owned()),
                count: 2,
            }],
            no_camera: 1,
            lenses: Vec::new(),
        }));
        f.frame(Vec::new());
        assert!(
            f.drew("3 photos · 2 MB. Hidden photos are not counted. Click a row to search for it.")
                .is_some()
        );
        assert!(f.drew(crate::stats::LENSES).is_none(), "no lens, no list");
        let note = f.drew("1 without camera data.").expect("the note");
        let canon = f.drew("Canon EOS 5D").expect("the camera's row");
        assert!(note.top() > canon.bottom(), "{note:?}");
        assert_eq!(
            click(&mut f, "2024"),
            [Asked::Search("from:2024 to:2024".to_owned())]
        );
        assert_eq!(
            click(&mut f, "Canon EOS 5D"),
            [Asked::Search("camera:\"Canon EOS 5D\"".to_owned())]
        );
        // With lenses too, the note comes after them: after every list, as the Svelte
        // section says it.
        if let Counted::Counted(counted) = &mut the(&mut f).stats {
            counted.lenses = vec![photon_core::library::LensCount {
                lens: "EF 50mm f/1.8".to_owned(),
                count: 2,
            }];
        }
        f.frame(Vec::new());
        let note = f.drew("1 without camera data.").expect("the note");
        let lens = f.drew("EF 50mm f/1.8").expect("the lens's row");
        assert!(note.top() > lens.bottom(), "{note:?} over {lens:?}");
    }

    // Shortcuts: the hint and the list, in one column.
    #[test]
    fn the_shortcuts_section_is_the_list_in_one_column() {
        let f = settings(Section::Shortcuts, true);
        assert!(f.drew(crate::shortcuts::SECTION_HINT).is_some());
        let everywhere = f.drew("Everywhere").unwrap();
        let grid = f.drew("Grid").unwrap();
        assert!((everywhere.left() - grid.left()).abs() < 3.0, "one column");
        assert!(grid.top() > everywhere.top());
    }

    // In a window too low to hold it, the question is scrolled into sight as it is asked:
    // the keyboard is put on its Cancel, and nothing else would bring that into sight.
    #[test]
    fn the_question_is_brought_into_sight_in_a_low_window() {
        let mut f = settings(Section::People, true);
        f.size = Vec2::new(WINDOW.x, 240.0);
        the(&mut f).seen.find_faces = true;
        f.open();
        let ground = f.ground();
        assert!(f.drew(CANCEL).is_none(), "nothing asked yet");
        the(&mut f).ask_off();
        the(&mut f).named(2);
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        let cancel = f.drew(CANCEL).expect("the question in sight");
        assert!(
            cancel.bottom() <= ground.bottom(),
            "{cancel:?} in {ground:?}"
        );
        assert_eq!(f.focused(), Some(no_id()));
    }

    // A section taller than its place scrolls by the wheel over it, as far as its end;
    // what is scrolled out of it is neither drawn nor pressed.
    #[test]
    fn a_section_taller_than_its_place_scrolls_as_far_as_its_end() {
        let mut f = settings(Section::People, true);
        f.size = Vec2::new(WINDOW.x, 240.0);
        f.open();
        let ground = f.ground();
        assert!(f.drew(FIND_FACES_BOX).is_none(), "below the section's foot");
        let middle = pos2(ground.right() - 100.0, ground.center().y);
        f.frame(vec![Event::PointerMoved(middle)]);
        for _ in 0..20 {
            f.frame(vec![Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
        }
        f.frame(Vec::new());
        let row = f.drew(FIND_FACES_BOX).expect("scrolled into sight");
        assert!(row.bottom() <= ground.bottom(), "{row:?} in {ground:?}");
        assert!(f.drew(FIND_FACES).is_none(), "the heading scrolled out");
        // Scrolled to its end and no further: the box stands near the section's foot.
        assert!(row.bottom() > ground.bottom() - 3.0 * CONTROL, "{row:?}");
    }

    // The field a folder is typed in has its Cancel: Escape closes it only while it has the
    // keyboard, and once that was elsewhere a hand on the mouse could not close it at all.
    #[test]
    fn the_field_a_folder_is_typed_in_has_its_cancel() {
        let mut f = folders();
        assert_eq!(click(&mut f, ADD_FOLDER), [Asked::AddOpen]);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(click(&mut f, CANCEL), [Asked::AddClose]);
    }
}
