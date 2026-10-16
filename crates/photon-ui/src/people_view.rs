//! The People page as it is drawn - `PeoplePage.svelte`, `FaceStrip.svelte` and
//! `NameBox.svelte` - over the grid's area, the sidebar and the top bar staying: a column
//! painted by hand and scrolled by a number of its own (`settings_view::Column`), each
//! group a card with its strip of faces under its section's heading.
//!
//! What the page holds is `people_page.rs`'s; this draws it and answers what was pressed,
//! which the application does (`app/people.rs`).

use crate::{
    dialog_view::{Look, button, button_width},
    face_crops::{CROP_PX, CropOf, Crops, Made},
    icons::Icon,
    people_page::{FaceAction, NameChoice, PeoplePage, Strip, counted, name_hint},
    settings_view::{Column, Field, Page, line_height, opened_field, text_field},
    theme::{
        apply::{color, menu_shadow, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, ColorImage, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    TextureHandle, TextureOptions, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
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
/// The ring of a selected face, inside it, and the ground's line inside that, as a
/// selected tile of the grid has them.
const RING: f32 = 2.0;
const RING_EDGE: f32 = 1.0;

/// The hint beside the faces selected.
pub const OPEN_HINT: &str = "Double-click a face to open its photo";

/// What a press on the page asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum PeopleAction {
    /// A group named, by a name typed or by Picasa's name offered.
    Name {
        group: i64,
        name: String,
    },
    /// A face's photo, in the viewer: a double click on the face, or Enter on it.
    Open(i64),
    /// "Open Settings", where Find faces is switched on.
    OpenSettings,
    /// A face picked or put back, by a click on it.
    Toggle {
        strip: Strip,
        face: i64,
    },
    /// An action of the bar under a strip's faces selected.
    Act {
        strip: Strip,
        action: FaceAction,
    },
    /// The bar's "Clear".
    Clear,
    ShowMore(Strip),
    ShowFewer(Strip),
    /// The single faces selected, named.
    NameSingles(String),
    /// A person's suggestions on screen, confirmed.
    ConfirmAll(i64),
    /// A person's name typed in their row.
    Rename {
        person: i64,
        name: String,
    },
    /// "Merge into…", at the foot of its button: the menu of the others.
    Merge {
        from: i64,
        at: Pos2,
    },
    /// "Delete", asked before it is done.
    Delete(i64),
    /// A group ignored, or no longer.
    Ignore {
        group: i64,
        ignored: bool,
    },
}

/// What the page is drawn from in a frame.
pub struct PeopleData<'a> {
    pub model: &'a PeoplePage,
    /// Whether photon looks for faces: the engine's switch, a flag in memory.
    pub finds_faces: bool,
    /// The face pass's line, while one is running.
    pub progress: Option<&'a str>,
    /// A dialog or a menu over the page closed, or a menu's item was picked, within a
    /// double click's time: a press now is that double click's second.
    pub just_covered: bool,
}

/// A name being typed: for a group, for the single faces selected, or as a person's new
/// name. Each field is its own, with its own history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Naming {
    Group(i64),
    Singles,
    Rename(i64),
}

impl Naming {
    fn id(self) -> Id {
        Id::new(("people-name", self))
    }
}

/// What the page keeps from frame to frame.
#[derive(Default)]
pub struct PeopleView {
    column: Column,
    /// The crops made, by face and key, and those that could not be.
    textures: HashMap<CropOf, TextureHandle>,
    missing: HashSet<CropOf>,
    /// The names being typed.
    names: HashMap<Naming, String>,
    /// The name box whose people are offered under it, and the person the arrows are on -
    /// by who they are, not by their place in the list, which a read of the page that
    /// lands between the arrow and Enter can change under the arrow.
    offered: Option<(Naming, Option<i64>)>,
    /// The person whose name is being typed in their row, and whether the field has been
    /// drawn since it opened.
    renaming: Option<(i64, bool)>,
    /// Whether the single faces and the ignored ones are unfolded.
    singles: bool,
    ignored: bool,
    /// The face last clicked, and when: a second click on it within a double click's
    /// time opens its photo.
    clicked: Option<(i64, f64)>,
    /// What had the keyboard when the page was last drawn.
    focused: Option<Id>,
    /// When the page last did something that moves what is drawn under the pointer.
    moved_at: Option<f64>,
    /// A press is the second of a double click whose first moved the page: nothing it
    /// lands on is done, until it has come up.
    deaf: bool,
    /// Whether single faces were selected when the page was last drawn.
    singles_bar: bool,
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

    /// How far the page is scrolled, and whether its scrollbar is held: for a test.
    pub fn scrolled(&self) -> (f32, bool) {
        self.column.scrolled()
    }

    /// The person whose name is typed in their row, if one is: for a test.
    pub fn renaming(&self) -> Option<i64> {
        self.renaming.map(|(person, _)| person)
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
        // What the keyboard has just been taken to is brought into sight, as a browser
        // brings it: a name box is drawn in sight or out of it (egui lets go of a field
        // that is not drawn) and a face half in sight takes the keyboard, so Tab walked on
        // into groups below the window's edge, and what was typed next and its Enter named
        // a group nobody could see. Once, as it gets there, so that the wheel can take the
        // page away from it after; and before the page is laid out, from where it was last
        // drawn, since egui moves the keyboard at the end of the frame Tab is pressed in.
        let focused = ui.memory(|memory| memory.focused());
        if focused != self.focused {
            self.focused = focused;
            let at = focused
                .and_then(|id| ui.ctx().read_response(id))
                .map(|r| r.rect);
            if let Some(at) = at.filter(|at| rect.x_range().contains(at.center().x)) {
                if at.top() < inner.top() {
                    self.column.reveal = at.top() - inner.top() - S[2];
                } else if at.bottom() > inner.bottom() {
                    self.column.reveal = at.bottom() - inner.bottom() + S[4];
                }
                if self.column.reveal != 0.0 {
                    ui.ctx().request_repaint();
                }
            }
        }
        let mut page = (self.column).begin(ui, Id::new("people-column"), rect, inner);
        let mut actions = Vec::new();
        let double = (ui.ctx()).options(|options| options.input_options.max_double_click_delay);
        // The second press of a double click lands on what the first put under the
        // pointer: an action of a bar takes the bar away, and what stood under it moves up
        // - the suggestion's "Confirm" became "Confirm these 6", a group's "Ignore these
        // faces" the group's own "Ignore". A press that goes down within a double click's
        // time of the page moving, or of a dialog or menu over it closing, presses nothing
        // until it has come up (`DialogView::deaf` the other way round).
        let (now, pressed, released) = ui.input(|input| {
            (
                input.time,
                input.pointer.any_pressed(),
                input.pointer.any_released(),
            )
        });
        let moved = self.moved_at.is_some_and(|at| now - at < double);
        if pressed && (moved || data.just_covered) {
            self.deaf = true;
        }
        let deaf = self.deaf;
        if released {
            self.deaf = false;
        }
        // The single faces' name box is one field for every selection of them: a new
        // selection opens it empty and with a history of its own, or the name typed for
        // the last faces, and Ctrl+Z, came back for the next.
        let singles = !data.model.selected(Strip::Single).is_empty();
        if singles && !self.singles_bar {
            self.names.remove(&Naming::Singles);
            egui::text_edit::TextEditState::default().store(ui.ctx(), Naming::Singles.id());
        }
        self.singles_bar = singles;
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
                names: &mut self.names,
                offered: &mut self.offered,
                renaming: &mut self.renaming,
                double,
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
            draw.unnamed(model, &mut self.singles);
            draw.suggestions(model);
            draw.people(model);
            draw.ignored(model, &mut self.ignored);
        });
        self.column.end(ui, &page, rect, inner, palette);
        // A rename whose person has gone - merged away by their new name, deleted - is no
        // longer typed anywhere.
        if let Some((person, _)) = self.renaming
            && !data.model.people().iter().any(|named| named.id == person)
        {
            self.renaming = None;
        }
        // A crop is let go of once the page no longer lists its face under that key - its
        // photo turned or renamed, its face gone - and not only when the page closes: kept,
        // every key a face had while the page stood open was a texture held.
        if data.model.page().is_some() {
            let listed: HashSet<CropOf> = (data.model.every_face())
                .map(|face| (face.id, face.thumb_key.clone()))
                .collect();
            self.textures.retain(|of, _| listed.contains(of));
            self.missing.retain(|of| listed.contains(of));
        }
        if deaf {
            actions.clear();
        }
        // A face picked or put back moves nothing above the pointer (its bar comes and
        // goes under the strip), and a photo opened leaves the page.
        if (actions.iter()).any(|action| {
            !matches!(
                action,
                PeopleAction::Toggle { .. } | PeopleAction::Open(_) | PeopleAction::OpenSettings
            )
        }) {
            self.moved_at = Some(now);
        }
        actions
    }
}

/// Where the next thing of a line of controls goes: on from the last, or at the start of
/// the next line where the room runs out, as a wrapping flex line lays them out.
struct Line {
    left: f32,
    right: f32,
    x: f32,
    top: f32,
    /// The foot of the line, and of all before it.
    bottom: f32,
}

impl Line {
    fn new(left: f32, top: f32, width: f32) -> Self {
        Self {
            left,
            right: left + width,
            x: left,
            top,
            bottom: top,
        }
    }

    /// The place of a thing `width` wide and `height` tall.
    fn place(&mut self, width: f32, height: f32) -> Pos2 {
        if self.x > self.left && self.x + width > self.right {
            self.x = self.left;
            self.top = self.bottom + S[1];
        }
        let at = pos2(self.x, self.top);
        self.x += width + S[1];
        self.bottom = self.bottom.max(self.top + height);
        at
    }

    fn bottom(&self) -> f32 {
        self.bottom
    }
}

/// What a name box's Enter or Escape came to.
enum Typed {
    /// Enter, on what the name will do: the field's words, as typed.
    Commit(String),
    /// Escape, or Enter on a person's own name.
    Cancel,
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
    names: &'a mut HashMap<Naming, String>,
    offered: &'a mut Option<(Naming, Option<i64>)>,
    renaming: &'a mut Option<(i64, bool)>,
    /// egui's double-click time.
    double: f64,
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

    /// A button at the next place in a row of them, from `left`: it, and whether it was
    /// pressed. `key` tells it from another of the same words on the page.
    fn inline_button(
        &mut self,
        left: &mut f32,
        top: f32,
        label: &str,
        look: Look,
        key: impl std::hash::Hash + std::fmt::Debug,
    ) -> (Rect, bool) {
        let width = button_width(self.ui, label);
        let rect = Rect::from_min_size(pos2(*left, top), vec2(width, CONTROL));
        *left = rect.right() + S[1];
        if !self.page.sees(rect) {
            return (rect, false);
        }
        let id = Id::new(("people-button", label, key));
        let pressed = button(self.ui, rect, id, label, look).clicked() && look != Look::Off;
        (rect, pressed)
    }

    /// A button alone on its line.
    fn button(&mut self, label: &str, look: Look) -> bool {
        let top = self.page.y;
        let mut left = self.page.left;
        let (_, pressed) = self.inline_button(&mut left, top, label, look, ());
        self.page.put(top + CONTROL);
        pressed
    }

    /// Words in the accent's colour that do what they say: "Show more", "Show fewer".
    fn link(
        &mut self,
        left: &mut f32,
        top: f32,
        words: &str,
        key: impl std::hash::Hash + std::fmt::Debug,
    ) -> bool {
        let font = fonts::regular(T[2]);
        let galley =
            (self.page.painter).layout_no_wrap(words.to_owned(), font, color(self.palette.accent));
        let rect = Rect::from_min_size(pos2(*left, top), galley.size());
        *left = rect.right() + S[2];
        if !self.page.sees(rect) {
            return false;
        }
        let id = Id::new(("people-link", words, key));
        let response = self.ui.interact(rect, id, Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Link, true, words));
        let lit = response.hovered() || response.has_focus();
        self.page
            .painter
            .galley(rect.min, galley, color(self.palette.accent));
        if lit {
            self.page.painter.hline(
                rect.x_range(),
                rect.bottom(),
                Stroke::new(1.0, color(self.palette.accent)),
            );
        }
        response.clicked()
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

    /// A strip of faces, wrapped to the column, and under it its paging and - while faces
    /// of it are selected - its bar: each face a crop, asked for when it is first in sight,
    /// and an empty square until it is cut or where it cannot be.
    fn strip(&mut self, model: &PeoplePage, strip: Strip, label: &str, suggestion: bool) {
        let faces = model.faces(strip);
        if faces.is_empty() {
            return;
        }
        let count = model.count(strip);
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
                let selected = model.is_selected(strip, face.id);
                let shown = Shown {
                    strip,
                    n,
                    count,
                    label,
                    suggestion,
                    selected,
                };
                self.face(rect, face, &shown);
            }
        }
        let rows = faces.len().div_ceil(across);
        self.page
            .put(top + rows as f32 * (FACE + FACE_GAP) - FACE_GAP);
        // "Show more", never "Show all": a press loads one page, and a group can hold more
        // faces than that.
        let more = model.can_show_more(strip);
        let expanded = model.is_expanded(strip);
        if more || expanded {
            self.page.gap(S[1]);
            let top = self.page.y;
            let mut left = self.page.left;
            if more {
                let left_over = usize::try_from(count)
                    .unwrap_or(0)
                    .saturating_sub(faces.len());
                let left_over = crate::grid::labels::grouped(left_over);
                if self.link(
                    &mut left,
                    top,
                    &format!("Show more ({left_over} left)"),
                    strip,
                ) {
                    self.actions.push(PeopleAction::ShowMore(strip));
                }
            }
            if expanded && self.link(&mut left, top, "Show fewer", strip) {
                self.actions.push(PeopleAction::ShowFewer(strip));
            }
            self.page
                .put(top + line_height(self.ui, &fonts::regular(T[2])));
        }
        let selected = model.selected(strip).len();
        if selected > 0 {
            self.page.gap(S[1]);
            self.bar(model, strip, selected);
        }
    }

    /// The bar under a strip with faces selected: how many, what can be done to them, a
    /// name box for single faces, "Clear", and how a photo is opened.
    fn bar(&mut self, model: &PeoplePage, strip: Strip, selected: usize) {
        // On to the next line where the page is too narrow, as the Svelte `.actions` wraps:
        // in one row, "Clear" stood past the card's edge in a window of the least width.
        let mut line = Line::new(self.page.left, self.page.y, self.page.width);
        let words = format!("{selected} selected");
        let font = fonts::semibold(self.ui.ctx(), T[2]);
        let width = (self.page.painter)
            .layout_no_wrap(words.clone(), font.clone(), egui::Color32::WHITE)
            .size()
            .x;
        let at = line.place(width, CONTROL);
        self.words(at, &words, font, self.palette.text);
        for action in strip.actions() {
            let label = action.label();
            let at = line.place(button_width(self.ui, label), CONTROL);
            let (_, pressed) =
                self.inline_button(&mut at.x.clone(), at.y, label, Look::Plain, strip);
            if pressed {
                self.actions.push(PeopleAction::Act {
                    strip,
                    action: *action,
                });
            }
        }
        if strip == Strip::Single {
            // Room for the line under the field whether or not anything is said there, as
            // a group's box keeps it: it comes with the first letter typed.
            let hint = line_height(self.ui, &fonts::regular(T[1])) + 2.0;
            let at = line.place(FIELD_WIDTH.min(self.page.width), CONTROL + hint);
            let label = "Name the selected faces";
            let (_, typed) = self.name_box(model, Naming::Singles, at, label);
            if let Some(Typed::Commit(name)) = typed {
                self.actions.push(PeopleAction::NameSingles(name));
            }
        }
        let at = line.place(button_width(self.ui, "Clear"), CONTROL);
        let (_, cleared) = self.inline_button(&mut at.x.clone(), at.y, "Clear", Look::Plain, strip);
        if cleared {
            self.actions.push(PeopleAction::Clear);
        }
        let font = fonts::regular(T[1]);
        let width = (self.page.painter)
            .layout_no_wrap(OPEN_HINT.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x;
        let at = line.place(width, CONTROL);
        self.words(at, OPEN_HINT, font, self.palette.text_dim);
        self.page.put(line.bottom());
    }

    /// Words at the left of a control's height at `at`, where they are in sight.
    fn words(
        &mut self,
        at: Pos2,
        words: &str,
        font: egui::FontId,
        tint: crate::theme::tokens::Rgba,
    ) {
        let at = pos2(at.x, at.y + CONTROL / 2.0);
        if !self
            .page
            .sees(Rect::from_center_size(at, vec2(1.0, CONTROL)))
        {
            return;
        }
        let place =
            (self.page.painter).text(at, egui::Align2::LEFT_CENTER, words, font, color(tint));
        self.label(place, words);
    }

    fn face(&mut self, rect: Rect, face: &PageFace, shown: &Shown<'_>) {
        let of: CropOf = (face.id, face.thumb_key.clone());
        let words = format!("{}: face {} of {}", shown.label, shown.n + 1, shown.count);
        let id = Id::new(("people-face", face.id));
        let response = self.ui.interact(rect, id, Sense::click());
        let selected = shown.selected;
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &words));
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
        // A suggestion is a guess: dashed, inside the square, so that the ring can stand
        // inside it too.
        if shown.suggestion {
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
        // Inside the square, from the selection and not the focus: a face that is both
        // focused and selected shows both marks.
        if selected {
            painter.rect_stroke(
                rect,
                R[1],
                Stroke::new(RING, color(self.palette.accent)),
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                rect.shrink(RING),
                (R[1] - RING).max(0.0),
                Stroke::new(RING_EDGE, color(self.palette.surface)),
                StrokeKind::Inside,
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                R[1] + 2.0,
                Stroke::new(2.0, color(self.palette.accent)),
                StrokeKind::Inside,
            );
        }
        // A click picks the face or puts it back. Enter on a face is a click to egui;
        // read as the key, it opens the photo and picks nothing. A double click is read
        // from two plain clicks on the face, not from what egui calls one: egui counts the
        // click before as well, and a click on the sidebar's People a moment before made
        // the double click a triple, which is no double to it. Each of the two clicks
        // picks or puts back, as in the Svelte strip, and the second opens the photo.
        // (The focus is asked outside `input`, which holds egui's context locked.)
        let focused = response.has_focus();
        let (enter, now) =
            (self.ui).input(|input| (focused && input.key_pressed(Key::Enter), input.time));
        if response.clicked() {
            if enter {
                self.actions.push(PeopleAction::Open(face.item_id));
                return;
            }
            self.actions.push(PeopleAction::Toggle {
                strip: shown.strip,
                face: face.id,
            });
            let double = self.double;
            let again = (*self.clicked).is_some_and(|(id, at)| id == face.id && now - at < double);
            if again {
                *self.clicked = None;
                self.actions.push(PeopleAction::Open(face.item_id));
            } else {
                *self.clicked = Some((face.id, now));
            }
        }
    }

    /// A name box at `at`: a field, and under it what Enter will do.
    /// Enter answers with what the field holds once it has taken the frame's letters;
    /// Escape empties it, or gives up a rename. Answers the box's foot, and what was done.
    fn name_box(
        &mut self,
        model: &PeoplePage,
        naming: Naming,
        at: Pos2,
        label: &str,
    ) -> (f32, Option<Typed>) {
        let id = naming.id();
        let own = match naming {
            Naming::Rename(person) => Some(person),
            Naming::Group(_) | Naming::Singles => None,
        };
        let width = FIELD_WIDTH.min(self.page.width);
        let rect = Rect::from_min_size(at, vec2(width, CONTROL));
        // Read before the field is drawn and taken out of the frame: egui's field gives the
        // keyboard up at an Enter it sees, and an Escape left in the frame is another's.
        let held = self.ui.memory(|memory| memory.has_focus(id));
        let typed = self.names.get(&naming).cloned().unwrap_or_default();
        // The named people offered under a group's or the single faces' name, while it is
        // typed (`name_suggestions`): the arrows move through them, taken out of the frame
        // before the field is drawn, and Enter or a click names the faces as the one
        // picked. Not under a person's new name, where another's is a merge nobody asked
        // for by typing a rename.
        let offers = held && !matches!(naming, Naming::Rename(_));
        let before = if offers {
            model.people_offered(&typed)
        } else {
            Vec::new()
        };
        let mut pick = (*self.offered)
            .filter(|(of, _)| *of == naming)
            .and_then(|(_, person)| person)
            .and_then(|person| before.iter().position(|(id, _)| *id == person));
        if !before.is_empty() {
            let (down, up) = self.ui.input_mut(|input| {
                (
                    input.consume_key(Modifiers::NONE, Key::ArrowDown),
                    input.consume_key(Modifiers::NONE, Key::ArrowUp),
                )
            });
            if down {
                pick = Some(pick.map_or(0, |at| (at + 1).min(before.len() - 1)));
            }
            if up {
                pick = pick.and_then(|at| at.checked_sub(1));
            }
        }
        let enter = held
            && self
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        let escape = held
            && self
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
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
            typed.clone()
        };
        let mut done = None;
        if escape {
            text.clear();
            done = Some(Typed::Cancel);
        }
        // A letter typed moves the list under the arrows: what they were on is gone.
        if text != typed {
            pick = None;
        }
        let picked = pick.map(|at| before[at].clone());
        let choice = match &picked {
            Some((id, name)) => NameChoice::Merge {
                id: *id,
                name: name.clone(),
            },
            None => model.choice(&text, own),
        };
        if enter && let Some((_, name)) = &picked {
            done = Some(Typed::Commit(name.clone()));
            text.clear();
        } else if enter {
            match choice {
                NameChoice::New(_) | NameChoice::Merge { .. } => {
                    done = Some(Typed::Commit(text.clone()));
                    text.clear();
                }
                // Enter on a person's own name leaves their name as it is.
                NameChoice::Same => done = Some(Typed::Cancel),
                NameChoice::Empty => {}
            }
        }
        let hint = match (&done, &picked) {
            (None, Some(_)) => name_hint(&choice, own.is_some()),
            _ => name_hint(&model.choice(&text, own), own.is_some()),
        };
        if text.is_empty() {
            self.names.remove(&naming);
        } else {
            self.names.insert(naming, text);
        }
        let mut bottom = rect.bottom();
        if let Some(hint) = hint {
            let tint = match choice {
                // A merge puts these faces with someone already named: the one outcome the
                // user must not commit without noticing, so the one coloured.
                NameChoice::Merge { .. } => self.palette.accent,
                _ => self.palette.text_dim,
            };
            let at = pos2(rect.left(), rect.bottom() + 2.0);
            let font = fonts::regular(T[1]);
            if self
                .page
                .sees(Rect::from_min_size(at, vec2(rect.width(), 16.0)))
            {
                let place = (self.page.painter).text(
                    at,
                    egui::Align2::LEFT_TOP,
                    &hint,
                    font.clone(),
                    color(tint),
                );
                self.label(place, &hint);
            }
            bottom += 2.0 + line_height(self.ui, &font);
        }
        let after = match self.names.get(&naming) {
            Some(text) if offers && done.is_none() && self.page.sees(rect) => {
                model.people_offered(text)
            }
            _ => Vec::new(),
        };
        if after.is_empty() {
            if (*self.offered).is_some_and(|(of, _)| of == naming) {
                *self.offered = None;
            }
        } else {
            *self.offered = Some((naming, picked.as_ref().map(|(id, _)| *id)));
            let on = (picked.as_ref())
                .and_then(|(person, _)| after.iter().position(|(id, _)| id == person));
            if let Some(name) = self.offered_list(id, rect, bottom, &after, on) {
                done = Some(Typed::Commit(name));
                self.names.remove(&naming);
                *self.offered = None;
            }
        }
        (bottom, done)
    }

    /// The people offered under a name box at `field`, whose words end at `bottom`, over
    /// everything on the page: under the box, or above it where the page has no room
    /// below. Answers the name of one clicked.
    fn offered_list(
        &mut self,
        id: Id,
        field: Rect,
        bottom: f32,
        offered: &[(i64, String)],
        pick: Option<usize>,
    ) -> Option<String> {
        const ROW: f32 = 26.0;
        let size = vec2(field.width(), offered.len() as f32 * ROW + 2.0 * S[0]);
        let below = pos2(field.left(), bottom + S[0]);
        let at = if below.y + size.y > self.page.clip.bottom() {
            pos2(field.left(), field.top() - S[0] - size.y)
        } else {
            below
        };
        let palette = self.palette;
        let font = fonts::regular(T[2]);
        let mut clicked = None;
        egui::Area::new(id.with("offered"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            // There at once, as a menu is: egui fades a new area in over a few frames.
            .fade_in(false)
            .show(self.ui.ctx(), |ui| {
                let (ground, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter().clone();
                painter.add(menu_shadow().as_shape(ground, R[2]));
                painter.rect_filled(ground, R[2], color(palette.raised));
                painter.rect_stroke(
                    ground,
                    R[2],
                    Stroke::new(1.0, color(palette.line)),
                    StrokeKind::Outside,
                );
                for (n, (person, name)) in offered.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(ground.left() + S[0], ground.top() + S[0] + n as f32 * ROW),
                        vec2(ground.width() - 2.0 * S[0], ROW),
                    );
                    let response = ui.interact(row, id.with(("offered", *person)), Sense::CLICK);
                    let on = pick == Some(n);
                    response.widget_info(|| {
                        WidgetInfo::selected(WidgetType::SelectableLabel, true, on, name)
                    });
                    if on {
                        painter.rect_filled(row, R[1], color(palette.accent_soft));
                    } else if response.hovered() {
                        painter.rect_filled(row, R[1], color(palette.hover));
                    }
                    painter.with_clip_rect(row).text(
                        pos2(row.left() + S[1], row.center().y),
                        egui::Align2::LEFT_CENTER,
                        name,
                        font.clone(),
                        color(palette.text),
                    );
                    if response.clicked() {
                        clicked = Some(name.clone());
                    }
                }
            });
        clicked
    }

    fn unnamed(&mut self, model: &PeoplePage, singles: &mut bool) {
        let groups = model.unnamed();
        let single = model.count(Strip::Single);
        if groups.is_empty() && single == 0 {
            return;
        }
        let note = (!groups.is_empty()).then(|| counted(model.unnamed_count(), "group", "groups"));
        self.heading("Unnamed", note.as_deref());
        for group in &groups {
            self.unnamed_group(model, group);
        }
        if model.unnamed_count() > groups.len() as i64 {
            let listed = crate::grid::labels::grouped(model.unnamed_listed());
            self.hint(&format!(
                "Showing the {listed} largest groups. Name or ignore some to see the rest."
            ));
            self.page.gap(S[1]);
        }
        if single > 0 {
            let shown = model.faces(Strip::Single).len();
            let summary = counted(single, "single face", "single faces");
            self.card(|draw| {
                if draw.fold(&summary, singles) {
                    draw.page.gap(S[1]);
                    draw.strip(model, Strip::Single, "Single faces", false);
                    if single > shown as i64 {
                        draw.page.gap(S[1]);
                        let shown = crate::grid::labels::grouped(shown);
                        draw.hint(&format!(
                            "Showing the first {shown}. Name or ignore some to see the rest."
                        ));
                    }
                }
            });
        }
    }

    fn unnamed_group(&mut self, model: &PeoplePage, group: &PageGroup) {
        let strip = Strip::Unnamed(group.id);
        self.card(|draw| {
            draw.strip(model, strip, "Unnamed group", false);
            draw.page.gap(S[1]);
            // Picasa's offer, the box and Ignore go on to the next line where the page is
            // too narrow for them, as the Svelte `.line` wraps.
            let mut line = Line::new(draw.page.left, draw.page.y, draw.page.width);
            if let Some(offer) = &group.offer {
                let words = format!("Yes, this is {}", offer.name);
                let at = line.place(button_width(draw.ui, &words), CONTROL);
                let (_, pressed) =
                    draw.inline_button(&mut at.x.clone(), at.y, &words, Look::Primary, group.id);
                if pressed {
                    draw.actions.push(PeopleAction::Name {
                        group: group.id,
                        name: offer.name.clone(),
                    });
                }
            }
            // The box with room for the line under it whether or not anything is said
            // there: a line that came at the first letter typed and went at Enter moved
            // every card under it as a name was typed, the export dialog's lesson.
            let hint = line_height(draw.ui, &fonts::regular(T[1])) + 2.0;
            let at = line.place(FIELD_WIDTH.min(draw.page.width), CONTROL + hint);
            let (_, typed) = draw.name_box(model, Naming::Group(group.id), at, "Name this group");
            if let Some(Typed::Commit(name)) = typed {
                draw.actions.push(PeopleAction::Name {
                    group: group.id,
                    name: name.trim().to_owned(),
                });
            }
            let at = line.place(button_width(draw.ui, "Ignore"), CONTROL);
            let (_, ignore) =
                draw.inline_button(&mut at.x.clone(), at.y, "Ignore", Look::Plain, group.id);
            if ignore {
                draw.actions.push(PeopleAction::Ignore {
                    group: group.id,
                    ignored: true,
                });
            }
            draw.page.put(line.bottom());
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
                let count = usize::try_from(model.count(strip)).unwrap_or(0);
                let count = crate::grid::labels::grouped(count);
                draw.name_line(&name, &format!("{count} to check"));
                draw.page.gap(S[1]);
                draw.strip(model, strip, &format!("Suggested faces for {name}"), true);
                draw.page.gap(S[1]);
                let top = draw.page.y;
                let mut left = draw.page.left;
                let label = model.confirm_all_label(person.id);
                let (_, pressed) =
                    draw.inline_button(&mut left, top, &label, Look::Primary, person.id);
                if pressed {
                    draw.actions.push(PeopleAction::ConfirmAll(person.id));
                }
                draw.page.put(top + CONTROL);
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
            self.card(|draw| draw.person(model, person));
        }
    }

    /// A named person's row: their name - or the field it is typed in - their count, and
    /// Rename, Merge into… and Delete at the right; their faces under it.
    fn person(&mut self, model: &PeoplePage, person: &PageGroup) {
        let strip = Strip::Person(person.id);
        let name = person.name.clone().unwrap_or_default();
        let n = model.count(strip);
        let faces = if n > 0 {
            counted(n, "face", "faces")
        } else {
            "No faces shown".to_owned()
        };
        let top = self.page.y;
        let mut bottom = top + CONTROL;
        let used = match self.renaming.filter(|(at, _)| *at == person.id) {
            Some((_, drawn)) => {
                let naming = Naming::Rename(person.id);
                if !drawn {
                    // The field takes the keyboard as it opens, its words chosen, with a
                    // history of its own: the user has just asked for it.
                    self.names.insert(naming, name.clone());
                    opened_field(self.ui, naming.id(), &name);
                    *self.renaming = Some((person.id, true));
                }
                let label = format!("Rename {name}");
                let at = pos2(self.page.left, top);
                let (under, typed) = self.name_box(model, naming, at, &label);
                // Room for the line under the field whether or not anything is said
                // there: it comes with the first letter that is not the person's own name,
                // and every card under the row moved down a line with it.
                let hint = line_height(self.ui, &fonts::regular(T[1])) + 2.0;
                bottom = bottom.max(under).max(top + CONTROL + hint);
                match typed {
                    Some(Typed::Commit(typed)) => {
                        self.actions.push(PeopleAction::Rename {
                            person: person.id,
                            name: typed,
                        });
                        *self.renaming = None;
                    }
                    Some(Typed::Cancel) => {
                        self.names.remove(&naming);
                        *self.renaming = None;
                    }
                    None => {}
                }
                let at = pos2(self.page.left + FIELD_WIDTH + S[1], top + CONTROL / 2.0);
                let place = self.page.painter.text(
                    at,
                    egui::Align2::LEFT_CENTER,
                    &faces,
                    fonts::regular(T[2]),
                    color(self.palette.text_dim),
                );
                self.label(place, &format!("{name}: {faces}"));
                place.right()
            }
            None => self.name_line(&name, &faces),
        };
        let labels = ["Rename", "Merge into…", "Delete"];
        let widths: f32 = (labels.iter())
            .map(|label| button_width(self.ui, label) + S[1])
            .sum();
        // At the right of the line - or under it, where the name or its field leaves them
        // no room, as the Svelte `.line` wraps: placed from the right regardless, they lay
        // over a long name.
        let mut left = self.page.left + self.page.width - widths + S[1];
        let mut top = top;
        if used + S[1] > left {
            top = bottom + S[1];
            left = left.max(self.page.left);
            bottom = top + CONTROL;
        }
        let (_, rename) = self.inline_button(&mut left, top, "Rename", Look::Plain, person.id);
        if rename {
            *self.renaming = Some((person.id, false));
        }
        // Merging needs someone to merge into.
        let look = if model.people().len() < 2 {
            Look::Off
        } else {
            Look::Plain
        };
        let (merge, pressed) = self.inline_button(&mut left, top, "Merge into…", look, person.id);
        if pressed {
            self.actions.push(PeopleAction::Merge {
                from: person.id,
                at: merge.left_bottom(),
            });
        }
        let (_, delete) = self.inline_button(&mut left, top, "Delete", Look::Warn, person.id);
        if delete {
            self.actions.push(PeopleAction::Delete(person.id));
        }
        self.page.put(bottom);
        self.page.gap(S[1]);
        self.strip(model, strip, &format!("Faces of {name}"), false);
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
                draw.strip(model, strip, "Ignored group", false);
                draw.page.gap(S[1]);
                let top = draw.page.y;
                let mut left = draw.page.left;
                let (_, pressed) =
                    draw.inline_button(&mut left, top, "Stop ignoring", Look::Plain, group.id);
                if pressed {
                    draw.actions.push(PeopleAction::Ignore {
                        group: group.id,
                        ignored: false,
                    });
                }
                draw.page.put(top + CONTROL);
            }
            draw.page.gap(S[1]);
            draw.strip(model, Strip::IgnoredFaces, "Ignored faces", false);
        });
    }

    /// A name in bold and a dim count beside it, on one line.
    /// Answers where the line's words end.
    fn name_line(&mut self, name: &str, count: &str) -> f32 {
        let row = self.page.row(CONTROL);
        self.page.put(row.bottom());
        let bold = fonts::semibold(self.ui.ctx(), T[2]);
        let (painter, white) = (&self.page.painter, egui::Color32::WHITE);
        let named = painter
            .layout_no_wrap(name.to_owned(), bold.clone(), white)
            .size()
            .x;
        let counted = (painter.layout_no_wrap(count.to_owned(), fonts::regular(T[2]), white))
            .size()
            .x;
        let end = row.left() + named + S[1] + counted;
        if !self.page.sees(row) {
            return end;
        }
        let at = self.page.painter.text(
            row.left_center(),
            egui::Align2::LEFT_CENTER,
            name,
            bold,
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
        end
    }

    /// A summary that folds what is under it, as `<details>` does: whether it is open.
    fn fold(&mut self, summary: &str, open: &mut bool) -> bool {
        let row = self.page.row(CONTROL);
        self.page.put(row.bottom());
        if self.page.sees(row) {
            let id = Id::new(("people-fold", summary));
            let response = self.ui.interact(row, id, Sense::click());
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
            let middle = pos2(row.left() + 6.0, row.center().y);
            let chevron = Rect::from_center_size(middle, vec2(12.0, 12.0));
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

/// How a face stands in its strip.
struct Shown<'a> {
    strip: Strip,
    n: usize,
    count: i64,
    label: &'a str,
    suggestion: bool,
    selected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::people_page::{Loaded, PeoplePage};
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
        /// The window's width.
        width: f32,
        /// Whether a dialog or a menu over the page has just closed.
        just_covered: bool,
    }

    impl Fixture {
        fn new(page: Page) -> Self {
            let ctx = egui::Context::default();
            crate::icons::install(&ctx);
            let mut model = PeoplePage::default();
            let ask = model.reload(0.0);
            let kept = Default::default();
            model.loaded(ask.number, Loaded { page, kept });
            Self {
                ctx,
                view: PeopleView::default(),
                model,
                crops: Crops::spawn(|_| Made::Missing, || {}),
                finds_faces: true,
                time: 0.0,
                texts: Vec::new(),
                width: 1000.0,
                just_covered: false,
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<PeopleAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = PeopleData {
                model: &self.model,
                finds_faces: self.finds_faces,
                progress: None,
                just_covered: self.just_covered,
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
    // under it; Ignore beside it puts the group away.
    #[test]
    fn picasas_name_is_offered_and_a_group_is_ignored_beside_it() {
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
        // A click of its own, not the second of a double click.
        f.time += 1.0;
        assert_eq!(
            f.click(ignore),
            [PeopleAction::Ignore {
                group: 1,
                ignored: true
            }]
        );
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

    fn tab() -> Event {
        Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    // Tab walks the keyboard through the page, and what it lands on is in sight. A
    // name box is drawn in sight or out of it, so Tab from the first group's box went on
    // into the boxes of groups below the window's edge, the page standing still: the
    // caret, the hint and the group were nowhere to be seen, and Enter named a group the
    // user could not see. A browser scrolls a field the keyboard is taken to into view.
    #[test]
    fn tab_lands_on_nothing_out_of_sight() {
        let groups: Vec<PageGroup> = (1..=30)
            .map(|n| group(n, vec![face(2 * n), face(2 * n + 1)], None))
            .collect();
        let page = Page {
            unnamed_count: groups.len() as i64,
            unnamed: groups,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        let field = f.drew("Add a name").expect("the first box").center();
        f.click(field);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0));
        for n in 0..60 {
            f.frame(vec![tab()]);
            // A few frames for it to settle: egui moves the keyboard in the frame of the
            // Tab, or - wrapping round from the last box to the first - in the one after;
            // the page is moved in the frame after that; and egui swaps its two passes at
            // the end of a frame (`Context::end_pass`), so `read_response` answers from the
            // frame before the last one drawn.
            for _ in 0..3 {
                f.frame(Vec::new());
            }
            let Some(id) = f.ctx.memory(|memory| memory.focused()) else {
                continue;
            };
            let rect = f.ctx.read_response(id).map(|response| response.rect);
            assert!(
                rect.is_some_and(|rect| screen.contains_rect(rect)),
                "after {} Tabs the keyboard is on {rect:?}, out of sight",
                n + 1
            );
        }
    }

    // a crop is let go of once the page no longer lists it under that key - its
    // photo turned or rewritten, its face gone - and not only when the page closes. Kept,
    // every key a face has had while the page stood open is a texture of 144 by 144.
    #[test]
    fn a_crop_of_a_key_no_longer_listed_is_let_go_of() {
        let page = Page {
            unnamed: vec![group(1, vec![face(1), face(2)], None)],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        let old: CropOf = (1, face(1).thumb_key);
        let side = CROP_PX as usize;
        f.view.took(
            &f.ctx,
            vec![(old.clone(), Made::Pixels(vec![0; side * side * 4]))],
        );
        f.frame(Vec::new());
        assert!(f.view.holds_crop(&old));
        // The photo turned: the face is listed under its new key.
        let mut turned = face(1);
        turned.thumb_key = "00000000000000ff".to_owned();
        let ask = f.model.reload(1.0);
        let page = Page {
            unnamed: vec![group(1, vec![turned, face(2)], None)],
            unnamed_count: 1,
            ..Page::default()
        };
        f.model.loaded(
            ask.number,
            crate::people_page::Loaded {
                page,
                kept: HashMap::new(),
            },
        );
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(!f.view.holds_crop(&old), "the crop of the old key is kept");
    }

    // a group's line - Picasa's offer, the name box, Ignore - wraps where the page
    // is narrow, as the Svelte `.line` does (`flex-wrap: wrap`). Laid out in one row from
    // the card's left, a long offer pushed the box and Ignore past the card's right edge
    // and out of the window: a sidebar dragged wide, or a window put beside another.
    #[test]
    fn a_groups_line_stays_inside_a_narrow_page() {
        let page = Page {
            unnamed: vec![group(
                1,
                vec![face(1), face(2)],
                Some("Alexandra Konstantinidou"),
            )],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.width = 420.0;
        f.frame(Vec::new());
        f.frame(Vec::new());
        // Past the edge it is not drawn at all (`inline_button` draws only in sight).
        let ignore = f.drew("Ignore");
        assert!(
            ignore.is_some_and(|ignore| ignore.right() <= f.width),
            "Ignore is at {ignore:?}, not inside the window's {}",
            f.width
        );
    }

    // A strip of 12 of a group's 20 faces says under it how many more there are to show.
    #[test]
    fn show_more_stands_under_a_long_strip() {
        let faces: Vec<PageFace> = (1..=12).map(face).collect();
        let mut long = group(1, faces, None);
        long.face_count = 20;
        let page = Page {
            unnamed: vec![long],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(
            f.drew("Show more (8 left)").is_some(),
            "no Show more in its place"
        );
    }

    // A person's Rename, Merge into… and Delete stand at the right of their line, and go
    // on to the next line where a long name leaves them no room, as the Svelte `.line`
    // wraps: placed from the right regardless, they lay over the name.
    #[test]
    fn a_persons_buttons_go_under_a_long_name() {
        let name = "Alexandra Konstantinidou-Papadopoulou";
        let mut person = group(5, vec![face(1)], None);
        person.name = Some(name.to_owned());
        let page = Page {
            people: vec![person],
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.width = 420.0;
        f.frame(Vec::new());
        f.frame(Vec::new());
        let named = f.drew(name).expect("the name");
        let rename = f.drew("Rename").expect("Rename");
        assert!(
            rename.top() > named.bottom() || rename.left() > named.right(),
            "Rename at {rename:?} lies over the name at {named:?}"
        );
        let delete = f.drew("Delete").expect("Delete");
        assert!(delete.right() <= f.width, "{delete:?}");
    }

    /// Where the face `id` was drawn: from the frame before the last, egui swapping its
    /// two passes at the end of a frame.
    fn face_at(f: &Fixture, id: i64) -> Pos2 {
        f.ctx
            .read_response(Id::new(("people-face", id)))
            .expect("the face drawn")
            .rect
            .center()
    }

    fn typed(text: &str) -> Event {
        Event::Text(text.to_owned())
    }

    // A crop is kept for every face the page shows, the faces "Show more" loaded among
    // them: kept only for the page's first faces, a face past the first twelve had its
    // crop let go of in the frame after it was cut, and - asked for once an opening - it
    // stood an empty square for good.
    #[test]
    fn a_crop_of_a_face_show_more_loaded_is_kept() {
        let mut long = group(1, (1..=12).map(face).collect(), None);
        long.face_count = 20;
        let page = Page {
            unnamed: vec![long],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        let more = f.model.show_more(Strip::Unnamed(1)).unwrap();
        f.model.more_landed(more, vec![face(13)]);
        f.frame(Vec::new());
        let of: CropOf = (13, face(13).thumb_key);
        let side = CROP_PX as usize;
        f.view.took(
            &f.ctx,
            vec![(of.clone(), Made::Pixels(vec![0; side * side * 4]))],
        );
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(
            f.view.holds_crop(&of),
            "the crop of a face Show more loaded"
        );
    }

    // The second press of a double click on a bar's button does nothing: the first takes
    // the bar away, and what stood under it moves up into its place - here "Confirm all",
    // which confirmed every suggestion on screen where the user had picked one.
    #[test]
    fn a_double_click_on_a_bars_button_presses_nothing_under_it() {
        let mut anna = group(5, (1..=6).map(face).collect(), None);
        anna.name = Some("Anna".to_owned());
        anna.face_count = 20;
        let page = Page {
            suggestions: vec![anna],
            ..Page::default()
        };
        let strip = Strip::Suggestion(5);
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let picked = face_at(&f, 1);
        assert_eq!(f.click(picked), [PeopleAction::Toggle { strip, face: 1 }]);
        f.model.toggle(strip, 1);
        f.time += 1.0;
        f.frame(Vec::new());
        let confirm = f.drew("Confirm").expect("the bar's Confirm").center();
        let action = PeopleAction::Act {
            strip,
            action: FaceAction::Confirm,
        };
        assert_eq!(f.click(confirm), [action]);
        f.model.act(strip, FaceAction::Confirm);
        assert_eq!(f.click(confirm), [], "the second click of the double click");
        // What stands there now - a click of its own presses it.
        f.time += 1.0;
        f.frame(Vec::new());
        f.frame(Vec::new());
        let all = (f.ctx)
            .read_response(Id::new(("people-button", "Confirm these 5", 5_i64)))
            .expect("Confirm these 5");
        assert!(
            all.rect.contains(confirm),
            "{:?} is not under {confirm:?}",
            all.rect
        );
        assert_eq!(f.click(confirm), [PeopleAction::ConfirmAll(5)]);
    }

    // A press within a double click's time of a dialog or a menu over the page closing is
    // that double click's second, and presses nothing: the second click on a question's
    // "Delete" landed on whatever button of the page stood under it.
    #[test]
    fn a_press_just_after_a_dialog_closed_presses_nothing() {
        let page = Page {
            unnamed: vec![group(1, vec![face(1), face(2)], None)],
            unnamed_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let ignore = f.drew("Ignore").expect("Ignore").center();
        f.just_covered = true;
        assert_eq!(f.click(ignore), []);
        f.just_covered = false;
        assert_eq!(
            f.click(ignore),
            [PeopleAction::Ignore {
                group: 1,
                ignored: true
            }]
        );
    }

    // A bar's line wraps where the page is narrow, as the Svelte `.actions` does: in one
    // row the single faces' "Clear" stood past the card's edge in a window of the least
    // width.
    #[test]
    fn a_bar_stays_inside_a_narrow_page() {
        let mut single = face(7);
        single.person_id = Some(70);
        let page = Page {
            single_faces: vec![single],
            single_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.width = 420.0;
        f.frame(Vec::new());
        let summary = f.drew("1 single face").expect("the fold").center();
        f.click(summary);
        f.model.toggle(Strip::Single, 7);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let clear = f.drew("Clear").expect("Clear in sight");
        assert!(clear.right() <= f.width - S[3], "Clear at {clear:?}");
        let hint = f.drew(OPEN_HINT).expect("the hint in sight");
        assert!(hint.right() <= f.width - S[3], "the hint at {hint:?}");
    }

    // Typing a person's new name moves nothing under their row: the line under the field
    // comes with the first letter that is not their own name, and its room is kept.
    #[test]
    fn typing_a_new_name_moves_nothing_under_the_row() {
        let named = |id: i64, name: &str, face_id: i64| {
            let mut person = group(id, vec![face(face_id)], None);
            person.name = Some(name.to_owned());
            person
        };
        let page = Page {
            people: vec![named(1, "Anna", 1), named(2, "Ben", 2)],
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let rename = f.drew("Rename").expect("Anna's Rename").center();
        f.click(rename);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let before = f.drew("Ben").expect("Ben's row");
        f.frame(vec![typed("C")]);
        f.frame(Vec::new());
        assert!(
            f.drew("Rename to “C” · Enter").is_some(),
            "the line under the field"
        );
        let after = f.drew("Ben").expect("Ben's row");
        assert_eq!(after.top(), before.top(), "Ben's row moved");
    }

    // The single faces' name box opens empty for each new selection: one field serves
    // every selection, and the name typed for the last faces stood in it for the next.
    #[test]
    fn the_single_faces_name_box_opens_empty_for_a_new_selection() {
        let mut single = face(7);
        single.person_id = Some(70);
        let page = Page {
            single_faces: vec![single],
            single_count: 1,
            ..Page::default()
        };
        let mut f = Fixture::new(page);
        f.frame(Vec::new());
        let summary = f.drew("1 single face").expect("the fold").center();
        f.click(summary);
        f.model.toggle(Strip::Single, 7);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let field = f.drew("Add a name").expect("the box").center();
        f.click(field);
        // Typed with rests, as a person types: egui notes a point to go back to when the
        // text has rested.
        for letter in ["Z", "e", "d"] {
            f.frame(vec![typed(letter)]);
            for _ in 0..90 {
                f.frame(Vec::new());
            }
        }
        assert!(f.drew("Zed").is_some(), "typed into the box");
        f.model.clear_selection();
        f.frame(Vec::new());
        f.model.toggle(Strip::Single, 7);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(f.drew("Zed").is_none(), "the last selection's name");
        // And with a history of its own: Ctrl+Z in it brings nothing of the last back.
        let field = f.drew("Add a name").expect("the box").center();
        f.click(field);
        for _ in 0..90 {
            f.frame(Vec::new());
        }
        let undo = |pressed| Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        };
        f.frame(vec![undo(true), undo(false)]);
        f.frame(Vec::new());
        assert!(
            !f.texts.iter().any(|(text, _)| text.starts_with('Z')),
            "the last selection's name came back by Ctrl+Z"
        );
    }
}
