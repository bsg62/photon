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
        REVEAL, SECTIONS, SHUFFLE, SLIDESHOW_HINT, STOP_FINDING, Section, Settings, TAGS_HINT,
        THEMES, TITLE, VERSION, distance_hint, memory_amount, memory_scope, switch_off_warning,
    },
    text::paint_line,
    theme::{
        apply::color,
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Color32, CornerRadius, Event, FontId, Id, Key, Modifiers, Rect, Sense, Stroke,
    StrokeKind, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use std::sync::Arc;

/// The header: the title and the cross, ten points over and under them.
const HEADER: f32 = 48.0;
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
/// A notch of the wheel over the section, whatever the system's: the info panel's.
const NOTCH: f32 = 40.0;

/// The interval's field, to egui.
pub(crate) fn interval_id() -> Id {
    Id::new("settings-interval")
}

/// A checkbox, to egui, by its words.
fn box_id(label: &str) -> Id {
    Id::new(("settings-box", label))
}

/// The question's "Cancel", to egui: the keyboard is put on it as the question is asked.
pub(crate) fn keep_id() -> Id {
    Id::new("settings-keep-faces")
}

/// What the view keeps from frame to frame.
#[derive(Default)]
pub struct SettingsView {
    /// How far the section is scrolled, and how tall it came to when last drawn.
    scroll: f32,
    content: f32,
    /// The section last drawn: another begins at its top.
    of: Option<Section>,
    /// How far the section is to be scrolled in the next frame to bring the question
    /// into sight.
    reveal: f32,
    /// Whether the People row's question was asked in the last frame: the keyboard is
    /// put on its Cancel in the frame it is first asked.
    asked: bool,
}

/// Where the next thing goes in the section, and what is in sight of it.
struct Page {
    painter: egui::Painter,
    clip: Rect,
    left: f32,
    width: f32,
    /// The top of the next thing, on screen: above the section for what is scrolled
    /// past.
    y: f32,
    /// The room already left under what was put on it last, or none while nothing has
    /// been: a heading after something stands clear of it by its own room, which takes
    /// in what is already there as one CSS margin collapses into the next.
    after: Option<f32>,
    asked: Vec<Asked>,
}

impl Page {
    fn sees(&self, rect: Rect) -> bool {
        rect.intersects(self.clip)
    }

    /// Room under what was put on it last.
    fn gap(&mut self, by: f32) {
        self.y += by;
        self.after = Some(self.after.unwrap_or(0.0) + by);
    }

    /// Something put on it, ending at `bottom`.
    fn put(&mut self, bottom: f32) {
        self.y = bottom;
        self.after = Some(0.0);
    }

    fn row(&self, height: f32) -> Rect {
        Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, height))
    }

    /// Words wrapped to the column, at `self.y`, which they move on past. They tell
    /// AccessKit what they say, which is how a test of the whole application reads them.
    fn words(&mut self, ui: &egui::Ui, words: &str, font: FontId, tint: Color32, id: Id) -> Rect {
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
    fn heading(&mut self, ui: &egui::Ui, words: &str, palette: &Palette) {
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
    fn hint(&mut self, ui: &egui::Ui, words: &str, palette: &Palette) {
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

fn wrap(ui: &egui::Ui, words: &str, font: FontId, tint: Color32, width: f32) -> Arc<egui::Galley> {
    ui.painter().layout(words.to_owned(), font, tint, width)
}

fn line_height(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font)).ceil()
}

/// The ring round what has the keyboard: Enter or Space presses it.
fn ring(painter: &egui::Painter, rect: Rect, radius: f32, palette: &Palette) {
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
        palette: &Palette,
        opened: bool,
    ) -> Done {
        // The interval's field opens with a history of its own: it is one field under one
        // id for every Settings opened, egui keeps a field's undo by id, and Ctrl+Z in a
        // Settings opened again brought back what was typed in the last one.
        if opened {
            egui::text_edit::TextEditState::default().store(ui.ctx(), interval_id());
        }
        let painter = ui.painter().clone();
        let mut asked = Vec::new();

        // The header, on the chrome, its corners the dialog's.
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
            &painter,
            (header.left() + S[3], header.center().y),
            cross.left() - S[1] - header.left() - S[3],
            TITLE,
            fonts::semibold(ui.ctx(), T[4]),
            color(palette.text),
        );
        ui.interact(
            header.shrink(1.0),
            Id::new("settings-title"),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, TITLE));

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
            self.scroll = 0.0;
        }
        if ui.rect_contains_pointer(place) {
            let turned: f32 = ui.input(|input| {
                (input.events.iter())
                    .map(|event| match event {
                        Event::MouseWheel { unit, delta, .. } => match unit {
                            egui::MouseWheelUnit::Line => delta.y * NOTCH,
                            egui::MouseWheelUnit::Point => delta.y,
                            egui::MouseWheelUnit::Page => delta.y * inner.height(),
                        },
                        _ => 0.0,
                    })
                    .sum()
            });
            self.scroll -= turned;
        }
        self.scroll += std::mem::take(&mut self.reveal);
        // Held to what there is to scroll, as it was when last drawn.
        let most = (self.content - inner.height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, most);
        let scroll = self.scroll;
        let page = ui
            .scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                ui.set_clip_rect(inner.intersect(ui.clip_rect()));
                let mut page = Page {
                    painter: ui.painter().clone(),
                    clip: inner,
                    left: inner.left(),
                    width: inner.width(),
                    y: inner.top() - scroll,
                    after: None,
                    asked: Vec::new(),
                };
                self.fill(ui, &mut page, settings, palette);
                page
            })
            .inner;
        self.content = page.y + self.scroll - inner.top();
        // Shorter than it was and scrolled past its new end: the frame that puts it
        // right is asked for.
        if self.scroll > (self.content - inner.height()).max(0.0) {
            ui.ctx().request_repaint();
        }
        if self.content > inner.height() {
            let share = inner.height() / self.content;
            let mark = Rect::from_min_size(
                pos2(place.right() - 6.0, inner.top() + self.scroll * share),
                vec2(3.0, inner.height() * share),
            );
            painter.rect_filled(mark, 1.5, color(palette.line).gamma_multiply(2.0));
        }
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

    /// What the section shown holds.
    fn fill(&mut self, ui: &mut egui::Ui, page: &mut Page, settings: &Settings, palette: &Palette) {
        let section = settings.section;
        let asked_before = std::mem::replace(&mut self.asked, false);
        match section {
            // Their sections are built by the pull requests after this one: their
            // heading, and what can be said of them already.
            Section::Folders => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, FOLDERS_HINT, palette);
            }
            Section::Tags => {
                page.heading(ui, section.label(), palette);
                page.hint(ui, TAGS_HINT, palette);
            }
            Section::Statistics | Section::Shortcuts => {
                page.heading(ui, section.label(), palette);
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
                if checkbox(ui, page, SHUFFLE, shuffle, stored.is_some(), palette) {
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
                    Some(named) => {
                        self.asked = true;
                        self.question(ui, page, named, !asked_before, palette);
                    }
                    None => {
                        let on = settings.seen.find_faces;
                        if checkbox(ui, page, FIND_FACES_BOX, on, true, palette) {
                            page.asked.push(Asked::FindFaces(!on));
                        }
                        // The question was answered in the last frame, and its buttons
                        // went with it: the keyboard, which was on one of them, comes
                        // back to the box that asked - as the system's own question the
                        // Svelte dialog asks gives it back. On nothing, Space ticked
                        // nothing and Tab began again at the top of the dialog.
                        if asked_before {
                            ui.memory_mut(|memory| memory.request_focus(box_id(FIND_FACES_BOX)));
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

    /// The People row turned into its question: what switching off deletes, Cancel and
    /// the button that switches off. The keyboard is put on Cancel as it is asked, as on
    /// the dialog layer's own question, so an Enter that came with it deletes nothing.
    fn question(
        &mut self,
        ui: &mut egui::Ui,
        page: &mut Page,
        named: i64,
        first: bool,
        palette: &Palette,
    ) {
        let warning = switch_off_warning(named);
        let words = page.words(
            ui,
            &warning,
            fonts::regular(T[2]),
            color(palette.text),
            Id::new("settings-switch-off"),
        );
        page.gap(S[1]);
        let row = page.row(CONTROL);
        page.put(row.bottom());
        let cancel = button_width(ui, CANCEL);
        let stop = button_width(ui, STOP_FINDING);
        let cancel_at = Rect::from_min_size(row.min, vec2(cancel, CONTROL));
        let stop_at = Rect::from_min_size(
            pos2(cancel_at.right() + S[1], row.top()),
            vec2(stop, CONTROL),
        );
        // Drawn in or out of sight: the keyboard put on Cancel must have something to
        // be on.
        let keep = button(ui, cancel_at, keep_id(), CANCEL, Look::Plain);
        let off = button(
            ui,
            stop_at,
            Id::new("settings-stop-faces"),
            STOP_FINDING,
            Look::Danger,
        );
        if first {
            keep.request_focus();
            // In a low window the row can be out of sight, the keyboard with it, and
            // nothing would bring it into sight: the section is scrolled to it.
            let below = row.bottom() - page.clip.bottom();
            let above = words.top() - page.clip.top();
            self.reveal = if below > 0.0 { below } else { above.min(0.0) };
            if self.reveal != 0.0 {
                ui.ctx().request_repaint();
            }
        }
        if keep.clicked() {
            page.asked.push(Asked::KeepFaces);
        } else if off.clicked() {
            page.asked.push(Asked::SwitchOff);
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

        // The library's path, cut short to the room beside its Reveal, which waits for
        // a later part of the native interface and takes no press.
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
        if page.sees(reveal_at) {
            button(
                ui,
                reveal_at,
                Id::new("settings-reveal-library"),
                REVEAL,
                Look::Off,
            );
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
        settings::{About, Memory, Stored},
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
        let mut f = Fixture::new(Dialog::Settings(settings));
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
        assert_eq!(f.focused(), Some(keep_id()));
        assert_eq!(asked(f.key(Key::Enter)), [Asked::KeepFaces]);
        assert_eq!(click(&mut f, STOP_FINDING), [Asked::SwitchOff]);
        assert_eq!(click(&mut f, CANCEL), [Asked::KeepFaces]);
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
        assert_eq!(asked(f.click(f.at(REVEAL))), []);
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
        assert_eq!(f.focused(), Some(keep_id()));
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
}
