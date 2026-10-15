//! The People page as it is drawn - `PeoplePage.svelte`, `FaceStrip.svelte` and
//! `NameBox.svelte` - over the grid's area, the sidebar and the top bar staying: a column
//! painted by hand and scrolled by a number of its own (`settings_view::Column`), each
//! group a card with its strip of faces under its section's heading.
//!
//! What the page holds is `people_page.rs`'s; this draws it and answers what was pressed.
//! The corrections - a person's suggestions, rename, merge, delete, ignoring and showing
//! more - are the next pull request's: their buttons stand in their places and take no
//! press, so that nothing moves when they start to work.

use crate::{
    dialog_view::{Look, button, button_width},
    face_crops::{CROP_PX, CropOf, Crops, Made},
    icons::Icon,
    people_page::{NameChoice, PeoplePage, Strip, counted, name_hint},
    settings_view::{Column, Field, Page, line_height, text_field},
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, ColorImage, Id, Key, Modifiers, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle,
    TextureOptions, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use photon_core::library::{PageFace, PageGroup};
use std::collections::{HashMap, HashSet};

/// The side a face is drawn at, in points: large enough to tell two people apart at a
/// glance, which 48 was not, and twelve to a strip still fit one row (`FACE_PX`). Its crop
/// is cut at twice this, for a screen of two pixels a point.
pub const FACE: f32 = 72.0;
/// Between two faces of a strip.
const FACE_GAP: f32 = S[0];
/// A name box's field.
const FIELD_WIDTH: f32 = 224.0;
const CONTROL: f32 = 28.0;

/// What a press on the page asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleAction {
    /// A group named, by a name typed or by Picasa's name offered for it.
    Name { group: i64, name: String },
    /// A face's photo, in the viewer: a double click on the face, or Enter on it.
    Open(i64),
    /// "Open Settings", where Find faces is switched on.
    OpenSettings,
}

/// What the page is drawn from in a frame.
pub struct PeopleData<'a> {
    pub model: &'a PeoplePage,
    /// Whether photon looks for faces: the engine's switch, a flag in memory.
    pub finds_faces: bool,
    /// The face pass's line, while one is running.
    pub progress: Option<&'a str>,
}

/// What the page keeps from frame to frame.
#[derive(Default)]
pub struct PeopleView {
    column: Column,
    /// The crops made, by face and key, and those that could not be.
    textures: HashMap<CropOf, TextureHandle>,
    missing: HashSet<CropOf>,
    /// The names being typed, by group.
    names: HashMap<i64, String>,
    /// Whether the single faces and the ignored ones are unfolded.
    singles: bool,
    ignored: bool,
    /// The face last clicked, and when: a second click on it within a double click's
    /// time opens its photo.
    clicked: Option<(i64, f64)>,
}

impl PeopleView {
    /// The crops cut since the last frame, made into textures.
    pub fn took(&mut self, ctx: &egui::Context, made: Vec<(CropOf, Made)>) {
        for (of, made) in made {
            match made {
                Made::Pixels(rgba) => {
                    let side = CROP_PX as usize;
                    let image = ColorImage::from_rgba_unmultiplied([side, side], &rgba);
                    let name = format!("face-crop-{}-{}", of.0, of.1);
                    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.textures.insert(of, texture);
                }
                Made::Missing | Made::Failed => {
                    self.missing.insert(of);
                }
            }
        }
    }

    /// Whether a crop is held for `of`: for a test.
    pub fn holds_crop(&self, of: &CropOf) -> bool {
        self.textures.contains_key(of)
    }

    /// The page is closed: its crops are let go of, and it opens again at its top.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The page in `rect`. Answers what was pressed.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        data: &PeopleData<'_>,
        crops: &mut Crops,
    ) -> Vec<PeopleAction> {
        let palette = palette(ui.ctx());
        // The page's padding, `--s-4`.
        let inner = rect.shrink(S[3]);
        let mut page = self.column.begin(ui, rect, inner);
        let mut actions = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            let mut draw = Draw {
                ui,
                page: &mut page,
                palette,
                crops,
                textures: &self.textures,
                missing: &self.missing,
                actions: &mut actions,
                clicked: &mut self.clicked,
            };
            draw.header(data.progress);
            if !data.finds_faces {
                draw.page.gap(S[4]);
                draw.text("photon finds and groups faces only while Find faces is on.");
                draw.page.gap(S[1]);
                if draw.button("Open Settings", Look::Primary) {
                    draw.actions.push(PeopleAction::OpenSettings);
                }
                return;
            }
            let model = data.model;
            if model.page().is_none() {
                return;
            }
            if model.empty() {
                draw.page.gap(S[4]);
                draw.text("No faces grouped yet.");
                if data.progress.is_none() {
                    draw.page.gap(S[1]);
                    draw.hint(
                        "photon groups faces once it has found them; this page fills in as it goes.",
                    );
                }
            }
            draw.unnamed(model, &mut self.names, &mut self.singles);
            draw.suggestions(model);
            draw.people(model);
            draw.ignored(model, &mut self.ignored);
        });
        self.column.end(ui, &page, rect, inner, palette);
        actions
    }
}

/// One frame of drawing the page.
struct Draw<'a, 'u> {
    ui: &'u mut egui::Ui,
    page: &'a mut Page,
    palette: &'a Palette,
    crops: &'a mut Crops,
    textures: &'a HashMap<CropOf, TextureHandle>,
    missing: &'a HashSet<CropOf>,
    actions: &'a mut Vec<PeopleAction>,
    clicked: &'a mut Option<(i64, f64)>,
}

impl Draw<'_, '_> {
    fn header(&mut self, progress: Option<&str>) {
        let font = fonts::semibold(self.ui.ctx(), T[4]);
        let line = line_height(self.ui, &font);
        let row = self.page.row(line);
        self.page.put(row.bottom());
        if self.page.sees(row) {
            let title = self.page.painter.text(
                row.left_center(),
                egui::Align2::LEFT_CENTER,
                "People",
                font,
                color(self.palette.text),
            );
            self.label(row, "People");
            if let Some(progress) = progress {
                let at = pos2(title.right() + S[2], row.center().y);
                let place = self.page.painter.text(
                    at,
                    egui::Align2::LEFT_CENTER,
                    progress,
                    fonts::regular(T[1]),
                    color(self.palette.text_dim),
                );
                self.label(place, progress);
            }
        }
        self.page.gap(S[2]);
    }

    /// Words in the text's own colour, wrapped to the column.
    fn text(&mut self, words: &str) {
        let tint = color(self.palette.text);
        let id = Id::new(("people-text", words));
        self.page
            .words(self.ui, words, fonts::regular(T[2]), tint, id);
    }

    fn hint(&mut self, words: &str) {
        let tint = color(self.palette.text_dim);
        let id = Id::new(("people-hint", words));
        self.page
            .words(self.ui, words, fonts::regular(T[1]), tint, id);
    }

    /// Tells AccessKit what is drawn at `rect`, which is how a test of the whole
    /// application reads it.
    fn label(&self, rect: Rect, words: &str) {
        self.ui
            .interact(rect, Id::new(("people-label", words)), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
    }

    /// A section's heading, with a dim note after it.
    fn heading(&mut self, words: &str, note: Option<&str>) {
        self.page.gap(S[4]);
        let font = fonts::semibold(self.ui.ctx(), T[3]);
        let line = line_height(self.ui, &font);
        let row = self.page.row(line);
        self.page.put(row.bottom());
        if self.page.sees(row) {
            let at = self.page.painter.text(
                row.left_center(),
                egui::Align2::LEFT_CENTER,
                words,
                font,
                color(self.palette.text),
            );
            self.label(row, words);
            if let Some(note) = note {
                let place = self.page.painter.text(
                    pos2(at.right() + S[1], row.center().y),
                    egui::Align2::LEFT_CENTER,
                    note,
                    fonts::regular(T[1]),
                    color(self.palette.text_dim),
                );
                self.label(place, note);
            }
        }
        self.page.gap(S[1]);
    }

    /// A button at the next place in a row of them, from `left`: whether it was pressed.
    fn inline_button(&mut self, left: &mut f32, top: f32, label: &str, look: Look) -> bool {
        let width = button_width(self.ui, label);
        let rect = Rect::from_min_size(pos2(*left, top), vec2(width, CONTROL));
        *left = rect.right() + S[1];
        if !self.page.sees(rect) {
            return false;
        }
        let id = Id::new(("people-button", label, top as i64));
        button(self.ui, rect, id, label, look).clicked() && look != Look::Off
    }

    /// A button alone on its line.
    fn button(&mut self, label: &str, look: Look) -> bool {
        let top = self.page.y;
        let mut left = self.page.left;
        let pressed = self.inline_button(&mut left, top, label, look);
        self.page.put(top + CONTROL);
        pressed
    }

    /// A card: `content` drawn in it, its ground and line under what it drew.
    fn card(&mut self, content: impl FnOnce(&mut Self)) {
        let top = self.page.y;
        let ground = self.page.painter.add(Shape::Noop);
        let (left, width) = (self.page.left, self.page.width);
        // `--s-3` inside, as the Svelte `.row`.
        self.page.left += S[2];
        self.page.width -= 2.0 * S[2];
        self.page.put(top + S[2]);
        content(self);
        self.page.put(self.page.y + S[2]);
        self.page.left = left;
        self.page.width = width;
        let rect = Rect::from_min_max(pos2(left, top), pos2(left + width, self.page.y));
        if self.page.sees(rect) {
            self.page.painter.set(
                ground,
                Shape::rect_filled(rect, R[2], color(self.palette.surface)),
            );
            self.page.painter.rect_stroke(
                rect,
                R[2],
                Stroke::new(1.0, color(self.palette.line)),
                StrokeKind::Inside,
            );
        }
        self.page.gap(S[1]);
    }

    /// A strip of faces, wrapped to the column: each a crop, asked for when it is first in
    /// sight, and an empty square until it is cut or where it cannot be. A double click on
    /// a face, or Enter on it, opens its photo.
    fn strip(&mut self, faces: &[PageFace], count: i64, label: &str, suggestion: bool) {
        if faces.is_empty() {
            return;
        }
        let across = (((self.page.width + FACE_GAP) / (FACE + FACE_GAP)).floor() as usize).max(1);
        let top = self.page.y;
        for (n, face) in faces.iter().enumerate() {
            let (row, column) = (n / across, n % across);
            let rect = Rect::from_min_size(
                pos2(
                    self.page.left + column as f32 * (FACE + FACE_GAP),
                    top + row as f32 * (FACE + FACE_GAP),
                ),
                vec2(FACE, FACE),
            );
            if self.page.sees(rect) {
                self.face(rect, face, n, count, label, suggestion);
            }
        }
        let rows = faces.len().div_ceil(across);
        self.page
            .put(top + rows as f32 * (FACE + FACE_GAP) - FACE_GAP);
    }

    fn face(
        &mut self,
        rect: Rect,
        face: &PageFace,
        n: usize,
        count: i64,
        label: &str,
        suggestion: bool,
    ) {
        let of: CropOf = (face.id, face.thumb_key.clone());
        let words = format!("{label}: face {} of {count}", n + 1);
        let response = self
            .ui
            .interact(rect, Id::new(("people-face", face.id)), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &words));
        let painter = &self.page.painter;
        painter.rect_filled(rect, R[1], color(self.palette.field));
        match self.textures.get(&of) {
            Some(texture) => {
                egui::Image::new((texture.id(), rect.size()))
                    .corner_radius(R[1])
                    .paint_at(self.ui, rect);
            }
            None if self.missing.contains(&of) => {
                Icon::User.paint(self.ui, rect, 28.0, false, color(self.palette.text_dim));
            }
            None => self.crops.ask(&of),
        }
        // A suggestion is a guess: dashed, inside the square.
        if suggestion {
            let line = Stroke::new(1.0, color(self.palette.accent));
            let r = rect.shrink(0.5);
            for (from, to) in [
                (r.left_top(), r.right_top()),
                (r.right_top(), r.right_bottom()),
                (r.right_bottom(), r.left_bottom()),
                (r.left_bottom(), r.left_top()),
            ] {
                painter.extend(Shape::dashed_line(&[from, to], line, 4.0, 3.0));
            }
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                R[1] + 2.0,
                Stroke::new(2.0, color(self.palette.accent)),
                StrokeKind::Inside,
            );
        }
        // Enter on a face is a click to egui; read as the key, it opens the photo. A double
        // click is read from two plain clicks on the face, not from what egui calls one:
        // egui counts the click before as well, and a click on the sidebar's People a
        // moment before made the double click a triple, which is no double to it.
        // (The focus is asked outside `input`, which holds egui's context locked.)
        let focused = response.has_focus();
        let (enter, now) = self
            .ui
            .input(|input| (focused && input.key_pressed(Key::Enter), input.time));
        let double =
            (self.ui.ctx()).options(|options| options.input_options.max_double_click_delay);
        if response.clicked() {
            let again = (*self.clicked).is_some_and(|(id, at)| id == face.id && now - at < double);
            if enter || again {
                *self.clicked = None;
                self.actions.push(PeopleAction::Open(face.item_id));
            } else {
                *self.clicked = Some((face.id, now));
            }
        }
    }

    /// A group's name box: a field, and under it what Enter will do. Enter names the group
    /// with what the field holds once it has taken the frame's letters; Escape empties it.
    fn name_box(
        &mut self,
        model: &PeoplePage,
        names: &mut HashMap<i64, String>,
        group: i64,
        left: f32,
    ) -> f32 {
        let id = Id::new(("people-name", group));
        let top = self.page.y;
        let rect = Rect::from_min_size(
            pos2(left, top),
            vec2(FIELD_WIDTH.min(self.page.width), CONTROL),
        );
        // Read before the field is drawn and taken out of the frame: egui's field gives the
        // keyboard up at an Enter it sees, and an Escape left in the frame is another's.
        let held = self.ui.memory(|memory| memory.has_focus(id));
        let enter = held
            && self
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        let escape = held
            && self
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
        let typed = names.get(&group).cloned().unwrap_or_default();
        let label = "Name this group";
        let (output, text) = text_field(
            self.ui,
            self.page,
            rect,
            Field {
                id,
                text: &typed,
                placeholder: "Add a name",
                label,
                refused: false,
            },
            self.palette,
        );
        let mut text = if output.response.changed() {
            text
        } else {
            typed
        };
        if escape {
            text.clear();
        }
        let choice = model.choice(&text, None);
        if enter && matches!(choice, NameChoice::New(_) | NameChoice::Merge { .. }) {
            self.actions.push(PeopleAction::Name {
                group,
                name: text.trim().to_owned(),
            });
            text.clear();
        }
        let hint = name_hint(&model.choice(&text, None));
        if text.is_empty() {
            names.remove(&group);
        } else {
            names.insert(group, text);
        }
        if let Some(hint) = hint {
            let tint = match choice {
                // A merge puts these faces with someone already named: the one outcome the
                // user must not commit without noticing, so the one coloured.
                NameChoice::Merge { .. } => self.palette.accent,
                _ => self.palette.text_dim,
            };
            let at = pos2(rect.left(), rect.bottom() + 2.0);
            if self
                .page
                .sees(Rect::from_min_size(at, vec2(rect.width(), 16.0)))
            {
                let place = self.page.painter.text(
                    at,
                    egui::Align2::LEFT_TOP,
                    &hint,
                    fonts::regular(T[1]),
                    color(tint),
                );
                self.label(place, &hint);
            }
            return rect.bottom() + 2.0 + line_height(self.ui, &fonts::regular(T[1]));
        }
        rect.bottom()
    }

    fn unnamed(
        &mut self,
        model: &PeoplePage,
        names: &mut HashMap<i64, String>,
        singles: &mut bool,
    ) {
        let groups = model.unnamed();
        let single = model.count(Strip::Single);
        if groups.is_empty() && single == 0 {
            return;
        }
        let note = (!groups.is_empty()).then(|| counted(model.unnamed_count(), "group", "groups"));
        self.heading("Unnamed", note.as_deref());
        for group in &groups {
            self.unnamed_group(model, names, group);
        }
        if model.unnamed_count() > groups.len() as i64 {
            let listed = crate::grid::labels::grouped(model.unnamed_listed());
            self.hint(&format!(
                "Showing the {listed} largest groups. Name or ignore some to see the rest."
            ));
            self.page.gap(S[1]);
        }
        if single > 0 {
            let faces = model.faces(Strip::Single);
            let summary = counted(single, "single face", "single faces");
            self.card(|draw| {
                if draw.fold(&summary, singles) {
                    draw.page.gap(S[1]);
                    draw.strip(faces, single, "Single faces", false);
                    if single > faces.len() as i64 {
                        draw.page.gap(S[1]);
                        let shown = crate::grid::labels::grouped(faces.len());
                        draw.hint(&format!(
                            "Showing the first {shown}. Name or ignore some to see the rest."
                        ));
                    }
                }
            });
        }
    }

    fn unnamed_group(
        &mut self,
        model: &PeoplePage,
        names: &mut HashMap<i64, String>,
        group: &PageGroup,
    ) {
        let strip = Strip::Unnamed(group.id);
        self.card(|draw| {
            draw.strip(
                model.faces(strip),
                model.count(strip),
                "Unnamed group",
                false,
            );
            draw.page.gap(S[1]);
            let top = draw.page.y;
            let mut left = draw.page.left;
            if let Some(offer) = &group.offer {
                let words = format!("Yes, this is {}", offer.name);
                if draw.inline_button(&mut left, top, &words, Look::Primary) {
                    draw.actions.push(PeopleAction::Name {
                        group: group.id,
                        name: offer.name.clone(),
                    });
                }
            }
            let field_left = left;
            let bottom = draw.name_box(model, names, group.id, field_left);
            left = field_left + FIELD_WIDTH + S[1];
            draw.inline_button(&mut left, top, "Ignore", Look::Off);
            draw.page.put(bottom.max(top + CONTROL));
            if let Some(offer) = &group.offer {
                draw.page.gap(S[1]);
                draw.hint(&format!(
                    "Offered because {} of these faces are ones Picasa named {}.",
                    offer.faces, offer.name
                ));
            }
        });
    }

    fn suggestions(&mut self, model: &PeoplePage) {
        if model.suggestions().is_empty() {
            return;
        }
        self.heading(
            "Suggestions",
            Some("faces photon thinks are someone you named"),
        );
        for person in model.suggestions() {
            let strip = Strip::Suggestion(person.id);
            let name = person.name.clone().unwrap_or_default();
            self.card(|draw| {
                let count =
                    crate::grid::labels::grouped(usize::try_from(model.count(strip)).unwrap_or(0));
                draw.name_line(&name, &format!("{count} to check"));
                draw.page.gap(S[1]);
                draw.strip(
                    model.faces(strip),
                    model.count(strip),
                    &format!("Suggested faces for {name}"),
                    true,
                );
                draw.page.gap(S[1]);
                draw.button("Confirm all", Look::Off);
            });
        }
    }

    fn people(&mut self, model: &PeoplePage) {
        let people = model.people();
        if people.is_empty() {
            return;
        }
        let count = crate::grid::labels::grouped(people.len());
        self.heading(&format!("People · {count}"), None);
        for person in people {
            let strip = Strip::Person(person.id);
            let name = person.name.clone().unwrap_or_default();
            let n = model.count(strip);
            let faces = if n > 0 {
                counted(n, "face", "faces")
            } else {
                "No faces shown".to_owned()
            };
            self.card(|draw| {
                let top = draw.page.y;
                draw.name_line(&name, &faces);
                // Rename, Merge into… and Delete, at the right of the line.
                let labels = ["Rename", "Merge into…", "Delete"];
                let widths: f32 = labels.iter().map(|l| button_width(draw.ui, l) + S[1]).sum();
                let mut left = draw.page.left + draw.page.width - widths + S[1];
                for label in labels {
                    draw.inline_button(&mut left, top, label, Look::Off);
                }
                draw.page.put(draw.page.y.max(top + CONTROL));
                draw.page.gap(S[1]);
                draw.strip(model.faces(strip), n, &format!("Faces of {name}"), false);
            });
        }
    }

    fn ignored(&mut self, model: &PeoplePage, open: &mut bool) {
        let groups = model.ignored_groups();
        let faces = model.count(Strip::IgnoredFaces);
        if groups.is_empty() && faces == 0 {
            return;
        }
        self.page.gap(S[4]);
        let summary = format!(
            "Ignored · {}, {}",
            counted(groups.len() as i64, "group", "groups"),
            counted(faces, "face", "faces")
        );
        self.card(|draw| {
            if !draw.fold(&summary, open) {
                return;
            }
            for group in &groups {
                let strip = Strip::Ignored(group.id);
                draw.page.gap(S[1]);
                draw.strip(
                    model.faces(strip),
                    model.count(strip),
                    "Ignored group",
                    false,
                );
                draw.page.gap(S[1]);
                draw.button("Stop ignoring", Look::Off);
            }
            draw.page.gap(S[1]);
            draw.strip(
                model.faces(Strip::IgnoredFaces),
                faces,
                "Ignored faces",
                false,
            );
        });
    }

    /// A name in bold and a dim count beside it, on one line.
    fn name_line(&mut self, name: &str, count: &str) {
        let row = self.page.row(CONTROL);
        self.page.put(row.bottom());
        if !self.page.sees(row) {
            return;
        }
        let at = self.page.painter.text(
            row.left_center(),
            egui::Align2::LEFT_CENTER,
            name,
            fonts::semibold(self.ui.ctx(), T[2]),
            color(self.palette.text),
        );
        self.label(at, name);
        let place = self.page.painter.text(
            pos2(at.right() + S[1], row.center().y),
            egui::Align2::LEFT_CENTER,
            count,
            fonts::regular(T[2]),
            color(self.palette.text_dim),
        );
        self.label(place, &format!("{name}: {count}"));
    }

    /// A summary that folds what is under it, as `<details>` does: whether it is open.
    fn fold(&mut self, summary: &str, open: &mut bool) -> bool {
        let row = self.page.row(CONTROL);
        self.page.put(row.bottom());
        if self.page.sees(row) {
            let response = self
                .ui
                .interact(row, Id::new(("people-fold", summary)), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, summary));
            if response.clicked() {
                *open = !*open;
            }
            if response.has_focus() {
                self.page.painter.rect_stroke(
                    row.expand(2.0),
                    R[1] + 2.0,
                    Stroke::new(2.0, color(self.palette.accent)),
                    StrokeKind::Inside,
                );
            }
            let icon = if *open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            let chevron =
                Rect::from_center_size(pos2(row.left() + 6.0, row.center().y), vec2(12.0, 12.0));
            icon.paint(self.ui, chevron, 12.0, false, color(self.palette.text_dim));
            self.page.painter.text(
                pos2(row.left() + 12.0 + S[1], row.center().y),
                egui::Align2::LEFT_CENTER,
                summary,
                fonts::semibold(self.ui.ctx(), T[2]),
                color(self.palette.text),
            );
        }
        *open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::people_page::PeoplePage;
    use eframe::egui::{Event, PointerButton, Pos2, RawInput};
    use photon_core::library::{Offer, PeoplePage as Page};

    fn face(id: i64) -> PageFace {
        PageFace {
            id,
            item_id: 100 + id,
            thumb_key: format!("{id:016x}"),
            confirmed: false,
            person_id: None,
        }
    }

    fn group(id: i64, faces: Vec<PageFace>, offer: Option<&str>) -> PageGroup {
        PageGroup {
            id,
            name: None,
            face_count: faces.len() as i64,
            faces,
            offer: offer.map(|name| Offer {
                name: name.to_owned(),
                contact: "c1".to_owned(),
                faces: 2,
            }),
        }
    }

    /// The page in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        view: PeopleView,
        model: PeoplePage,
        crops: Crops,
        finds_faces: bool,
        time: f64,
        texts: Vec<(String, Rect)>,
    }

    impl Fixture {
        fn new(page: Page) -> Self {
            let ctx = egui::Context::default();
            crate::icons::install(&ctx);
            let mut model = PeoplePage::default();
            let number = model.reload(0.0);
            model.loaded(number, page);
            Self {
                ctx,
                view: PeopleView::default(),
                model,
                crops: Crops::spawn(|_| Made::Missing, || {}),
                finds_faces: true,
                time: 0.0,
                texts: Vec::new(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<PeopleAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = PeopleData {
                model: &self.model,
                finds_faces: self.finds_faces,
                progress: None,
            };
            let (view, crops) = (&mut self.view, &mut self.crops);
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                let rect = ui.max_rect();
                actions = view.show(ui, rect, &data, crops);
            });
            full.textures_delta.clear();
            self.texts.clear();
            for clipped in &full.shapes {
                collect(&clipped.shape, &mut self.texts);
            }
            actions
        }

        fn drew(&self, words: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(text, _)| text == words)
                .map(|(_, rect)| *rect)
        }

        fn click(&mut self, at: Pos2) -> Vec<PeopleAction> {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![press(true)]);
            self.frame(vec![press(false)])
        }
    }

    fn collect(shape: &Shape, texts: &mut Vec<(String, Rect)>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, texts)),
            Shape::Text(text) => {
                texts.push((text.galley.text().to_owned(), text.visual_bounding_rect()))
            }
            _ => {}
        }
    }

    // Picasa's name for a group is offered as a button that names it so, with the reason
    // under it; the corrections of the next pull request stand in their places and take no
    // press.
    #[test]
    fn picasas_name_is_offered_and_the_corrections_take_no_press_yet() {
        let page = Page {
            unnamed: vec![group(1, vec![face(1), face(2)], Some("Anna"))],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(
            f.drew("Offered because 2 of these faces are ones Picasa named Anna.")
                .is_some()
        );
        let yes = f.drew("Yes, this is Anna").expect("the offer").center();
        assert_eq!(
            f.click(yes),
            [PeopleAction::Name {
                group: 1,
                name: "Anna".to_owned()
            }]
        );
        let ignore = f.drew("Ignore").expect("in its place").center();
        assert_eq!(f.click(ignore), []);
    }

    // The single faces are a fold, shut until it is opened.
    #[test]
    fn the_single_faces_are_a_fold() {
        let page = Page {
            single_faces: vec![face(7)],
            single_count: 3,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        let summary = f.drew("3 single faces").expect("the fold").center();
        assert!(
            f.drew("Showing the first 1. Name or ignore some to see the rest.")
                .is_none()
        );
        f.click(summary);
        f.frame(Vec::new());
        assert!(
            f.drew("Showing the first 1. Name or ignore some to see the rest.")
                .is_some()
        );
    }

    // Switched off, the page says so and offers Settings, and draws nothing else.
    #[test]
    fn switched_off_it_says_so_and_offers_settings() {
        let page = Page {
            unnamed: vec![group(1, vec![face(1), face(2)], None)],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.finds_faces = false;
        f.frame(Vec::new());
        assert!(
            f.drew("photon finds and groups faces only while Find faces is on.")
                .is_some()
        );
        assert!(f.drew("Unnamed").is_none());
        let open = f.drew("Open Settings").unwrap().center();
        assert_eq!(f.click(open), [PeopleAction::OpenSettings]);
    }

    // The words are the Svelte page's.
    #[test]
    fn the_words_are_the_svelte_pages() {
        let page = include_str!("../../../ui/src/components/PeoplePage.svelte");
        for words in [
            "photon finds and groups faces only while Find faces is on.",
            "No faces grouped yet.",
            "photon groups faces once it has found them; this page fills in as it goes.",
            "Yes, this is {offer.name}",
            "Offered because {g.offer.faces} of these faces are ones Picasa named {g.offer.name}.",
            "Name or ignore some to see the rest.",
            "faces photon thinks are someone you named",
            "Rename",
            "Merge into…",
            "Stop ignoring",
            "No faces shown",
            "Open Settings",
        ] {
            assert!(page.contains(words), "{words}");
        }
    }
}
