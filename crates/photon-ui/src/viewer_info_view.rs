//! The viewer's info panel, drawn: what photon knows of the photo on screen, beside it.
//! What it says is `viewer_info.rs`'s; where it stands is `viewer_layout.rs`'s; what was
//! pressed in it is answered as `InfoAction`s, for the application to act on.
//!
//! The panel is one column that can be taller than the window: it is laid out from the
//! top in every frame it is drawn, at a position this view keeps, and cut off at the
//! panel's edge. Nothing in it is a stop of the Tab key, as nothing in the viewer is; the
//! keyword's field takes the keyboard at a press on it, and gives it up at Escape, at a
//! press elsewhere and when the viewer goes.

use crate::{
    grid::labels::format_duration,
    icons::Icon,
    text::paint_line,
    theme::{
        apply::color,
        fonts,
        tokens::{ACCENT_GLASS, DARK, GLASS, GLASS_LINE, R, S, T},
    },
    viewer_histogram::histogram_columns,
    viewer_info::{
        Camera, camera_rows, copy_groups, date_rows, format_coordinates, format_dimensions,
        near_query, no_faces_label, own_albums, people, picasa_albums_of, show_copies_label,
        unnamed_faces_label,
    },
};
use eframe::egui::{
    self, Color32, Event, EventFilter, FontId, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind,
    UiBuilder, WidgetInfo, WidgetType, pos2, text::LayoutJob, vec2,
};
use jiff::tz::TimeZone;
use photon_core::{library::AlbumSummary, media::MediaKind};
use photon_engine::commands::{CopyKind, ViewerItem};
use std::sync::Arc;

/// How far the panel's words stand from its edges.
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
/// The histogram's height.
const HISTOGRAM: f32 = 56.0;
/// The field a keyword is typed into: its height.
const FIELD: f32 = 26.0;
/// A tick box, and the cross that takes a keyword off.
const BOX: f32 = 14.0;
const CROSS: f32 = 14.0;

/// What the panel is drawn from.
pub struct InfoData<'a> {
    /// The photo, as the library last said it.
    pub item: &'a ViewerItem,
    /// Every album there is, photon's own and Picasa's.
    pub albums: &'a [AlbumSummary],
    /// Whether photon looks for faces itself, and whether a pass is looking now: what a
    /// photo with no face says of itself.
    pub finds_faces: bool,
    pub looking: bool,
    /// The machine's zone: a file's dates are read in it.
    pub zone: &'a TimeZone,
}

/// What was pressed in the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum InfoAction {
    /// A search to go to: a camera's, a lens's, the photos nearby.
    Search(String),
    /// A person's view, by their key.
    Person(String),
    /// A copy to find in the grid.
    Locate(i64),
    /// The photo's copies, in the grid's Copies view.
    ShowCopies(i64),
    AddTag(String),
    RemoveTag(String),
    /// One of photon's own albums, ticked or not.
    Album {
        id: i64,
        on: bool,
    },
    /// The place the photo was taken, on OpenStreetMap in the browser.
    Map,
}

#[derive(Default)]
pub struct InfoView {
    /// How far the panel is scrolled, in points.
    scroll: f32,
    /// Its scrollbar, the grid's own.
    bar: crate::scroll_bar::Bar,
    /// How tall everything in it came to when it was last drawn.
    content: f32,
    /// What is typed into the keyword's field, and the photo it was typed for: another
    /// photo's panel begins with an empty field.
    draft: String,
    of: Option<i64>,
    /// Whether the keyboard is the field's. Kept here and not asked of egui, which
    /// lets go of a field in any frame that does not draw it - and at a step there are
    /// such frames, as many as the library takes to say what the next photo is: by
    /// egui's word alone the field kept the keyboard across a step when the answer was
    /// quick and lost it when it was not, and what was typed next was a keyword or the
    /// viewer's keys by a matter of milliseconds.
    held: bool,
}

/// The keyword's field, to egui.
fn field_id() -> Id {
    Id::new("viewer-keyword")
}

impl InfoView {
    /// Whether the keyword's field has the keyboard: the viewer's keys are the field's
    /// then, and `h` is a letter of a keyword and hides no photo. As it was when the
    /// panel was last drawn.
    pub fn typing(&self) -> bool {
        self.held
    }

    /// The field gives the keyboard up: Escape, or a press anywhere else.
    fn leave(&mut self, ctx: &egui::Context) {
        self.held = false;
        Self::let_go(ctx);
    }

    /// egui is told that the field has the keyboard no longer: the viewer has closed.
    pub fn let_go(ctx: &egui::Context) {
        ctx.memory_mut(|memory| memory.surrender_focus(field_id()));
    }

    /// The wheel turned over the panel, by `points`: the panel's, and no step to another
    /// photo.
    pub fn turned(&mut self, points: f32) {
        self.scroll -= points;
    }

    /// Draws the panel in `place` and answers what was pressed in it. With no `data` -
    /// the library has not said yet what the photo stepped to is - its ground alone, so
    /// that it does not come and go at every step.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        place: Rect,
        data: Option<&InfoData<'_>>,
    ) -> Vec<InfoAction> {
        glass(ui, place);
        // The field's own keys. Enter adds what is typed, once the field has taken this
        // frame's letters, and is taken out of the frame before the field is drawn:
        // egui's field gives the keyboard up at an Enter it sees, and the next keyword
        // is typed straight on. Escape leaves the field; it is not the viewer's as
        // well, because the viewer reads no key while a keyword is typed
        // (`InfoView::typing`, asked before this frame's keys were read).
        let typing = self.held;
        let (mut enter, mut escape) = (false, false);
        if typing {
            ui.input_mut(|input| {
                input.events.retain(|event| match event {
                    Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        repeat,
                        ..
                    } => {
                        enter |= !repeat;
                        false
                    }
                    Event::Key {
                        key: Key::Escape,
                        pressed: true,
                        ..
                    } => {
                        escape = true;
                        true
                    }
                    _ => true,
                });
            });
        }
        let Some(data) = data else {
            // Nothing is known of the photo yet - it was stepped to a moment ago - and
            // nothing is drawn. The field keeps the keyboard meanwhile, for the next
            // photo's keyword, unless it is left: by Escape, or by a press anywhere.
            let pressed = ui.input(|input| {
                (input.events.iter())
                    .any(|event| matches!(event, Event::PointerButton { pressed: true, .. }))
            });
            if typing && (escape || pressed) {
                self.leave(ui.ctx());
            }
            return Vec::new();
        };
        if self.of != Some(data.item.id) {
            self.of = Some(data.item.id);
            self.draft.clear();
            self.scroll = 0.0;
            // And a history of its own: one field under one name serves every photo,
            // egui keeps a field's undo by that name, and Ctrl+Z in the next photo's
            // field brought back the keyword typed for the last.
            egui::text_edit::TextEditState::default().store(ui.ctx(), field_id());
        }

        let inner = Rect::from_min_max(
            pos2(place.left() + PAD_X, place.top() + PAD_Y),
            pos2(place.right() - PAD_X, place.bottom() - PAD_Y),
        );
        // Held to what there is to scroll, as it was when last drawn.
        let most = (self.content - inner.height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, most);
        let track = crate::scroll_bar::track(place);
        self.scroll = (self.bar).hold(
            ui,
            Id::new("viewer-info-bar"),
            track,
            self.scroll,
            self.content,
            inner.height(),
        );
        // Through a child cut off at the panel's own edge: what is scrolled out of it
        // is neither drawn nor pressed, an icon among it (`Icon::paint` paints through
        // the `Ui` it is given).
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
                    actions: Vec::new(),
                    field: None,
                };
                self.fill(ui, &mut page, data, enter);
                page
            })
            .inner;
        self.content = page.y + self.scroll - inner.top();
        // Shorter than it was - a keyword taken off, another photo - and scrolled past
        // its new end: the frame that puts it right is asked for.
        if self.scroll > (self.content - inner.height()).max(0.0) {
            ui.ctx().request_repaint();
        }
        (self.bar).paint(
            ui,
            track,
            self.scroll,
            self.content,
            inner.height(),
            color(GLASS_LINE).gamma_multiply(2.0),
            color(GLASS_LINE).gamma_multiply(4.0),
        );
        if typing {
            // A press of any button anywhere but on the field leaves it: egui gives the
            // keyboard up at a click elsewhere, which a press that becomes a drag of
            // the photo is not.
            let elsewhere = ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, Event::PointerButton { pos, pressed: true, .. }
                        if !page.field.is_some_and(|field| field.contains(*pos)))
                })
            });
            if escape || elsewhere {
                self.leave(ui.ctx());
            }
        }
        page.actions
    }

    /// Everything in the panel, from the top.
    fn fill(&mut self, ui: &mut egui::Ui, page: &mut Page, data: &InfoData<'_>, enter: bool) {
        let item = data.item;
        let ctx = ui.ctx().clone();
        let (text, dim) = (color(DARK.text), color(DARK.text_dim));
        let body = fonts::regular(T[2]);

        // The file: its name, and where it is, on one line cut short.
        page.wrapped(ui, &item.file_name, fonts::semibold(&ctx, T[3]), text);
        page.gap(2.0);
        page.line(ui, &item.path, fonts::regular(T[0]), dim);
        page.gap(10.0);

        if let Some(caption) = item
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            page.heading(ui, "Caption", false);
            page.wrapped(ui, caption, body.clone(), text);
            page.gap(S[2]);
        }

        let camera = camera_rows(Camera {
            make: item.make.as_deref(),
            model: item.model.as_deref(),
            lens: item.lens.as_deref(),
            focal_mm: item.focal_mm,
            aperture: item.aperture,
            exposure_s: item.exposure_s,
            iso: item.iso,
        });
        let length = (item.kind == MediaKind::Video)
            .then_some(item.duration_ms)
            .flatten()
            .map(format_duration);
        if camera.is_empty() && length.is_none() {
            page.wrapped(ui, "No camera data.", body.clone(), dim);
        } else {
            let mut rows: Vec<Pair> = (camera.into_iter())
                .map(|row| (row.label, row.value, row.search.map(InfoAction::Search)))
                .collect();
            rows.extend(length.map(|length| ("Length".to_owned(), length, None)));
            page.rows(ui, "camera", &rows, false);
        }

        if let Some(columns) = item.histogram.as_deref().and_then(histogram_columns) {
            page.gap(S[2]);
            page.histogram(&columns);
        }

        page.heading(ui, "Dates", true);
        let dates: Vec<Pair> = (date_rows(&item.dates, data.zone))
            .into_iter()
            .map(|row| (row.label, row.value, None))
            .collect();
        page.rows(ui, "dates", &dates, true);

        if let Some(gps) = &item.gps {
            page.heading(ui, "Location", true);
            page.wrapped(
                ui,
                &format_coordinates(gps.lat, gps.lon),
                body.clone(),
                text,
            );
            page.gap(S[0]);
            let nearby = InfoAction::Search(near_query(gps.lat, gps.lon));
            let mut flow = page.flow();
            if flow.link(ui, page, Id::new("viewer-info-nearby"), "Photos nearby") {
                page.actions.push(nearby);
            }
            flow.words(ui, page, "·", dim);
            let map = Id::new("viewer-info-map");
            if flow.link(ui, page, map, "Open in OpenStreetMap") {
                page.actions.push(InfoAction::Map);
            }
            flow.end(page);
        }

        page.heading(ui, "People", true);
        let named = people(&item.faces);
        if !named.is_empty() {
            let mut flow = page.flow();
            for (key, name) in &named {
                if flow.link(ui, page, Id::new(("viewer-info-person", *key)), name) {
                    page.actions.push(InfoAction::Person((*key).to_owned()));
                }
            }
            flow.end(page);
        }
        if let Some(unnamed) = unnamed_faces_label(item.unnamed_faces.len()) {
            page.wrapped(ui, &unnamed, body.clone(), dim);
        } else if item.faces.is_empty() {
            let none = no_faces_label(data.finds_faces, data.looking);
            page.wrapped(ui, none, body.clone(), dim);
        }

        page.heading(ui, "Keywords", true);
        if item.tags.is_empty() {
            page.wrapped(ui, "No keywords yet.", body.clone(), dim);
        } else {
            let mut flow = page.flow();
            for tag in &item.tags {
                if flow.chip(ui, page, tag) {
                    page.actions.push(InfoAction::RemoveTag(tag.clone()));
                }
            }
            flow.end(page);
        }
        page.gap(6.0);
        self.field(ui, page, enter, &item.tags);

        page.heading(ui, "Albums", true);
        let own = own_albums(data.albums);
        let picasa = picasa_albums_of(data.albums, &item.albums);
        for album in &own {
            let on = item.albums.contains(&album.id);
            if page.tick(ui, album.id, &album.name, on) {
                page.actions.push(InfoAction::Album {
                    id: album.id,
                    on: !on,
                });
            }
        }
        if own.is_empty() && picasa.is_empty() {
            page.wrapped(
                ui,
                "No albums of your own yet. Create one in the sidebar.",
                body.clone(),
                dim,
            );
        }
        for album in &picasa {
            page.listed(ui, Icon::Images, &album.name);
        }

        // The copies: absent, not "none" - nearly every photo has no copy, and the panel
        // is long enough. A press locates the copy in the grid.
        for group in copy_groups(&item.copies) {
            page.heading(ui, group.label, true);
            for copy in &group.copies {
                if page.link(ui, Id::new(("viewer-info-copy", copy.id)), &copy.path) {
                    page.actions.push(InfoAction::Locate(copy.id));
                }
                // An identical copy is the same size by what it is; of a look-alike,
                // "which is the large one" is the next question.
                if group.kind == CopyKind::Similar {
                    let size = format_dimensions(copy.width, copy.height);
                    page.wrapped(ui, &size, fonts::regular(T[1]), dim);
                }
                page.gap(S[0]);
            }
        }
        if !item.copies.is_empty() {
            page.gap(S[0]);
            let all = format!("{} in the grid", show_copies_label(item.copies.len()));
            if page.link(ui, Id::new("viewer-info-all-copies"), &all) {
                page.actions.push(InfoAction::ShowCopies(item.id));
            }
        }
    }

    /// The field a keyword is typed into. Enter adds what it holds, once it has taken
    /// the frame's letters: a letter and the Enter after it can come in one frame.
    fn field(&mut self, ui: &mut egui::Ui, page: &mut Page, enter: bool, has: &[String]) {
        let ground = Rect::from_min_size(pos2(page.left, page.y), vec2(page.width, FIELD));
        page.y += FIELD;
        page.field = Some(ground);
        // (Drawn in sight or out of it: egui lets go of a field that is not drawn, and
        // a keyword's field scrolled out of sight would give the keyboard to the
        // viewer, where the next letter typed is a key - `h` hides the photo.)
        page.painter.rect_stroke(
            ground,
            R[1],
            Stroke::new(1.0, color(GLASS_LINE)),
            StrokeKind::Inside,
        );
        let font = fonts::regular(T[2]);
        let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
        let edit = Rect::from_min_max(
            pos2(ground.left() + S[1], ground.center().y - line / 2.0),
            pos2(ground.right() - S[1], ground.center().y + line / 2.0),
        );
        let clip = page.clip;
        let draft = &mut self.draft;
        let output = ui
            .scope_builder(UiBuilder::new().max_rect(edit), |ui| {
                ui.set_clip_rect(ground.intersect(clip));
                // The glass is dark in both themes, and egui colours a field's
                // placeholder and its selection by the theme in force: in a light
                // window the placeholder was all but invisible. The field is drawn in
                // the dark theme's, whatever the window's.
                ui.set_visuals(ui.ctx().style_of(egui::Theme::Dark).visuals.clone());
                egui::TextEdit::singleline(draft)
                    .id(field_id())
                    // Escape is read here and the arrows move the caret: a filter built
                    // from the default leaves the arrows to egui, which walks the
                    // keyboard off with them.
                    .event_filter(EventFilter {
                        escape: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        tab: false,
                    })
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(font)
                    .text_color(color(DARK.text))
                    .hint_text(egui::RichText::new("Add a keyword").color(color(DARK.text_dim)))
                    .desired_width(edit.width())
                    .show(ui)
            })
            .inner;
        let typed = self.draft.clone();
        (output.response).widget_info(|| WidgetInfo::text_edit(true, "", &typed, "Add a keyword"));
        // The keyboard is the field's from the click that gave it, and is asked back
        // where egui has let go of it without the field having been left: over the
        // frames of a step in which there was no field to draw.
        if output.response.has_focus() {
            self.held = true;
        } else if self.held {
            output.response.request_focus();
        }
        if enter {
            let tag = self.draft.trim().to_owned();
            self.draft.clear();
            // One the photo has already, in whatever case, is not asked for again: the
            // library would take it as nothing and the engine build the grid anew.
            let had = (has.iter()).any(|had| had.to_lowercase() == tag.to_lowercase());
            if !tag.is_empty() && !had {
                page.actions.push(InfoAction::AddTag(tag));
            }
        }
    }
}

/// The panel's ground: the glass the viewer's controls stand on.
fn glass(ui: &egui::Ui, rect: Rect) {
    let painter = ui.painter();
    painter.rect_filled(rect, R[3], color(GLASS));
    painter.rect_stroke(
        rect,
        R[3],
        Stroke::new(1.0, color(GLASS_LINE)),
        StrokeKind::Outside,
    );
    // Something to be hit, all over the panel: where a press hits nothing egui hands
    // it to a widget within five points of it, so one in the panel's own margin, just
    // above a link that the edge cuts off, pressed the link. (In the viewer the
    // photo's surface lies under the panel and would do as much; the panel does not
    // lean on it.)
    ui.interact(rect, Id::new("viewer-info-panel"), Sense::CLICK);
}

/// A row of two columns: its label, what it is of, and where that leads if it is a link.
type Pair = (String, String, Option<InfoAction>);

/// The column being laid out: where the next thing goes, and what has been pressed.
struct Page {
    painter: egui::Painter,
    /// What is in sight of the panel. Nothing outside it is drawn or pressed, by the
    /// cut of the `Ui` the column is drawn through; and nothing outside it is handed to
    /// egui at all (`sees`), or AccessKit is told of things that are scrolled out of
    /// sight - which is how a test of the whole application found a line that was not
    /// on screen. But for the keyword's field, which is always drawn.
    clip: Rect,
    left: f32,
    width: f32,
    /// The top of the next thing, on screen: above the panel for what is scrolled past.
    y: f32,
    actions: Vec<InfoAction>,
    /// Where the keyword's field stands in this frame.
    field: Option<Rect>,
}

/// Words that wrap within `width`: at a word's end where there is one, and inside a
/// word only where it is longer than the line - a path, a name with no space in it -
/// which egui does by itself. (Told to break anywhere, it never looks for a word's end:
/// "Canon EOS 5D Mark IV Special E" over "dition".)
fn wrap(ui: &egui::Ui, words: &str, font: FontId, tint: Color32, width: f32) -> Arc<egui::Galley> {
    let job = LayoutJob::simple(words.to_owned(), font, tint, width);
    ui.painter().layout_job(job)
}

impl Page {
    fn gap(&mut self, by: f32) {
        self.y += by;
    }

    /// Whether anything at `rect` is in sight.
    fn sees(&self, rect: Rect) -> bool {
        rect.intersects(self.clip)
    }

    /// Words over the column's whole width, wrapped. They tell AccessKit what they say,
    /// which is how a test of the whole application reads the panel.
    fn wrapped(&mut self, ui: &egui::Ui, words: &str, font: FontId, tint: Color32) {
        let galley = wrap(ui, words, font, tint, self.width);
        let rect = Rect::from_min_size(pos2(self.left, self.y), galley.size());
        self.y += galley.size().y;
        if self.sees(rect) {
            ui.interact(rect, Id::new(("viewer-info-words", words)), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            self.painter.galley(rect.min, galley, tint);
        }
    }

    /// One line, cut short where the column ends: a path. The whole of it is said under
    /// the pointer, as the Svelte panel's `title` says it: cut short, the end of a long
    /// path - the file's own folder - could not be read anywhere.
    fn line(&mut self, ui: &egui::Ui, words: &str, font: FontId, tint: Color32) {
        let height = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
        let rect = Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, height));
        self.y += height;
        if self.sees(rect) {
            ui.interact(rect, Id::new(("viewer-info-line", words)), Sense::hover())
                .on_hover_text(words)
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            let middle = rect.center().y;
            paint_line(
                ui,
                &self.painter,
                (self.left, middle),
                self.width,
                words,
                font,
                tint,
            );
        }
    }

    /// A section's name, small and in capitals, with air above it where it is not the
    /// panel's first.
    fn heading(&mut self, ui: &egui::Ui, words: &str, air: bool) {
        if air {
            self.gap(S[2]);
        }
        let font = fonts::semibold(ui.ctx(), T[0]);
        let tint = color(DARK.text_dim);
        let galley = (ui.painter()).layout_no_wrap(words.to_uppercase(), font, tint);
        let rect = Rect::from_min_size(pos2(self.left, self.y), galley.size());
        self.y += galley.size().y + S[0];
        if self.sees(rect) {
            ui.interact(
                rect,
                Id::new(("viewer-info-heading", words)),
                Sense::hover(),
            )
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            self.painter.galley(rect.min, galley, tint);
        }
    }

    /// A link over the column's width: its words in the link's colour, underlined under
    /// the pointer. Answers whether it was pressed.
    fn link(&mut self, ui: &egui::Ui, id: Id, words: &str) -> bool {
        let tint = color(ACCENT_GLASS);
        let galley = wrap(ui, words, fonts::regular(T[2]), tint, self.width);
        let rect = Rect::from_min_size(pos2(self.left, self.y), galley.size());
        self.y += galley.size().y;
        self.pressed_link(ui, id, words, rect, galley)
    }

    fn pressed_link(
        &mut self,
        ui: &egui::Ui,
        id: Id,
        words: &str,
        rect: Rect,
        galley: Arc<egui::Galley>,
    ) -> bool {
        if !self.sees(rect) {
            return false;
        }
        let response = ui.interact(rect, id.with("link"), Sense::CLICK);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Link, true, words));
        let tint = color(ACCENT_GLASS);
        self.painter.galley(rect.min, galley.clone(), tint);
        if response.hovered() {
            for row in &galley.rows {
                let line = row.rect().translate(rect.min.to_vec2());
                (self.painter).hline(line.x_range(), line.bottom() - 1.0, Stroke::new(1.0, tint));
            }
        }
        response.clicked()
    }

    /// Rows of a label and what it is of, in two columns. The camera's: the labels as
    /// wide as the widest, the values wrapped in what is left. The dates': the values as
    /// wide as the widest and never wrapped, the labels in what is left.
    fn rows(&mut self, ui: &egui::Ui, id: &str, rows: &[Pair], dates: bool) {
        const BETWEEN: f32 = 10.0;
        let font = fonts::regular(T[2]);
        let (text, dim) = (color(DARK.text), color(DARK.text_dim));
        let widest = |of: &dyn Fn(&Pair) -> &str| {
            (rows.iter())
                .map(|row| {
                    (ui.painter())
                        .layout_no_wrap(of(row).to_owned(), font.clone(), text)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max)
        };
        let (label_width, value_width) = if dates {
            let value = widest(&|row| &row.1).min(self.width);
            ((self.width - BETWEEN - value).max(0.0), value)
        } else {
            let label = widest(&|row| &row.0).min(self.width / 2.0);
            (label, (self.width - BETWEEN - label).max(0.0))
        };
        let value_left = self.left + self.width - value_width;
        for (n, (label, value, link)) in rows.iter().enumerate() {
            let label_galley = wrap(ui, label, font.clone(), dim, label_width);
            let tint = if link.is_some() {
                color(ACCENT_GLASS)
            } else {
                text
            };
            let value_galley = wrap(ui, value, font.clone(), tint, value_width);
            let height = label_galley.size().y.max(value_galley.size().y);
            let label_rect = Rect::from_min_size(pos2(self.left, self.y), label_galley.size());
            let value_rect = Rect::from_min_size(pos2(value_left, self.y), value_galley.size());
            self.y += height + 4.0;
            if !self.sees(label_rect.union(value_rect)) {
                continue;
            }
            ui.interact(
                label_rect,
                Id::new(("viewer-info-label", id, n)),
                Sense::hover(),
            )
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
            self.painter.galley(label_rect.min, label_galley, dim);
            match link {
                Some(action) => {
                    if self.pressed_link(
                        ui,
                        Id::new(("viewer-info-row", id, n)),
                        value,
                        value_rect,
                        value_galley,
                    ) {
                        self.actions.push(action.clone());
                    }
                }
                None => {
                    ui.interact(
                        value_rect,
                        Id::new(("viewer-info-value", id, n)),
                        Sense::hover(),
                    )
                    .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, value));
                    self.painter.galley(value_rect.min, value_galley, text);
                }
            }
        }
        self.y -= 4.0;
    }

    /// The histogram: one flat-topped column a step, over the panel's width.
    fn histogram(&mut self, columns: &[f32]) {
        let rect = Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, HISTOGRAM));
        self.y += HISTOGRAM;
        if !self.sees(rect) {
            return;
        }
        self.painter.rect_filled(rect, R[0], color(DARK.field));
        let step = rect.width() / columns.len() as f32;
        let tint = color(DARK.text_dim);
        let mut mesh = egui::Mesh::default();
        for (n, share) in columns.iter().enumerate() {
            if *share <= 0.0 {
                continue;
            }
            let left = rect.left() + n as f32 * step;
            let column = Rect::from_min_max(
                pos2(left, rect.bottom() - share * rect.height()),
                pos2(left + step, rect.bottom()),
            );
            mesh.add_colored_rect(column, tint);
        }
        self.painter.add(egui::Shape::mesh(mesh));
    }

    /// A row of things side by side that goes on to the next line where the column ends.
    fn flow(&self) -> Flow {
        Flow {
            x: self.left,
            y: self.y,
            row: 0.0,
        }
    }

    /// One of photon's own albums: a box, ticked or not, and its name. A press anywhere
    /// on the row ticks it. Answers whether it was pressed.
    fn tick(&mut self, ui: &egui::Ui, id: i64, name: &str, on: bool) -> bool {
        const BETWEEN: f32 = 8.0;
        let font = fonts::regular(T[2]);
        let text = color(DARK.text);
        let room = (self.width - BOX - BETWEEN).max(0.0);
        let galley = wrap(ui, name, font, text, room);
        let height = galley.size().y.max(BOX) + 4.0;
        let row = Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, height));
        self.y += height;
        if !self.sees(row) {
            return false;
        }
        let response = ui.interact(row, Id::new(("viewer-info-album", id)), Sense::CLICK);
        response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name));
        // Beside the name's first line.
        let first = galley
            .rows
            .first()
            .map_or(BOX, |first| first.rect().height());
        let square = Rect::from_min_size(
            pos2(row.left(), row.top() + 2.0 + (first - BOX) / 2.0),
            vec2(BOX, BOX),
        );
        if on {
            self.painter.rect_filled(square, R[0], color(ACCENT_GLASS));
            Icon::Check.paint(ui, square, 12.0, false, color(DARK.on_accent));
        } else {
            let line = if response.hovered() {
                color(DARK.text_dim)
            } else {
                color(GLASS_LINE).gamma_multiply(2.0)
            };
            self.painter
                .rect_stroke(square, R[0], Stroke::new(1.0, line), StrokeKind::Inside);
        }
        (self.painter).galley(
            pos2(row.left() + BOX + BETWEEN, row.top() + 2.0),
            galley,
            text,
        );
        response.clicked()
    }

    /// A thing listed with an icon before it and not pressed: one of Picasa's albums.
    fn listed(&mut self, ui: &egui::Ui, icon: Icon, name: &str) {
        const BETWEEN: f32 = 8.0;
        let font = fonts::regular(T[2]);
        let dim = color(DARK.text_dim);
        let room = (self.width - BOX - BETWEEN).max(0.0);
        let galley = wrap(ui, name, font, dim, room);
        let height = galley.size().y.max(BOX) + 4.0;
        let row = Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, height));
        self.y += height;
        if !self.sees(row) {
            return;
        }
        ui.interact(row, Id::new(("viewer-info-listed", name)), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, name));
        let square = Rect::from_min_size(pos2(row.left(), row.top() + 2.0), vec2(BOX, BOX));
        icon.paint(ui, square, 12.0, false, dim);
        (self.painter).galley(
            pos2(row.left() + BOX + BETWEEN, row.top() + 2.0),
            galley,
            dim,
        );
    }
}

/// Where the next thing of a flowing row goes.
struct Flow {
    x: f32,
    y: f32,
    /// How tall the row is so far.
    row: f32,
}

impl Flow {
    const BETWEEN: f32 = 6.0;

    /// The place for something `size` large: on this row if it fits, else at the start
    /// of the next.
    fn place(&mut self, page: &Page, size: egui::Vec2) -> Pos2 {
        if self.x + size.x > page.left + page.width {
            self.x = page.left;
            self.y += self.row + 4.0;
            self.row = 0.0;
        }
        let at = pos2(self.x, self.y);
        self.x += size.x + Self::BETWEEN;
        self.row = self.row.max(size.y);
        at
    }

    /// The row is over: the column goes on below it.
    fn end(self, page: &mut Page) {
        page.y = self.y + self.row;
    }

    fn link(&mut self, ui: &egui::Ui, page: &mut Page, id: Id, words: &str) -> bool {
        let tint = color(ACCENT_GLASS);
        let galley = wrap(ui, words, fonts::regular(T[2]), tint, page.width);
        let at = self.place(page, galley.size());
        let rect = Rect::from_min_size(at, galley.size());
        page.pressed_link(ui, id, words, rect, galley)
    }

    fn words(&mut self, ui: &egui::Ui, page: &mut Page, words: &str, tint: Color32) {
        let galley = wrap(ui, words, fonts::regular(T[2]), tint, page.width);
        let at = self.place(page, galley.size());
        let rect = Rect::from_min_size(at, galley.size());
        if page.sees(rect) {
            ui.interact(rect, Id::new(("viewer-info-words", words)), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            page.painter.galley(at, galley, tint);
        }
    }

    /// A keyword on its ground, with the cross that takes it off. Answers whether the
    /// cross was pressed.
    fn chip(&mut self, ui: &egui::Ui, page: &mut Page, tag: &str) -> bool {
        let text = color(DARK.text);
        let most = (page.width - 2.0 * S[1] - CROSS - 2.0).max(0.0);
        let galley = wrap(ui, tag, fonts::regular(T[1]), text, most);
        let size = vec2(
            S[1] + galley.size().x + 2.0 + CROSS + 4.0,
            galley.size().y + 4.0,
        );
        let at = self.place(page, size);
        let rect = Rect::from_min_size(at, size);
        if !page.sees(rect) {
            return false;
        }
        (page.painter).rect_filled(rect, rect.height().min(22.0) / 2.0, color(DARK.field));
        ui.interact(rect, Id::new(("viewer-info-tag", tag)), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, tag));
        page.painter.galley(at + vec2(S[1], 2.0), galley, text);
        let cross = Rect::from_center_size(
            pos2(rect.right() - 4.0 - CROSS / 2.0, rect.center().y),
            vec2(CROSS, CROSS),
        );
        let response = ui.interact(cross, Id::new(("viewer-info-untag", tag)), Sense::CLICK);
        let label = format!("Remove {tag}");
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
        let tint = if response.hovered() {
            text
        } else {
            color(DARK.text_dim)
        };
        Icon::X.paint(ui, cross, 10.0, false, tint);
        response.clicked()
    }
}

/// A photo with everything the panel can say, for the tests of the panel and of the
/// viewer around it.
#[cfg(test)]
pub(crate) mod testing {
    use photon_core::{library::ItemFace, media::MediaKind};
    use photon_engine::commands::{CopyKind, ItemCopy, ItemDates, ItemGps, ViewerItem};

    /// 2024-06-15 12:30, as a camera's clock and as an instant.
    pub const NOON: i64 = 1_718_454_645;

    /// A photo with nothing but what every photo has.
    pub fn bare() -> ViewerItem {
        ViewerItem {
            id: 7,
            path: "/photos/2024/Lisbon/IMG_0412.jpg".to_owned(),
            file_name: "IMG_0412.jpg".to_owned(),
            width: 4000,
            height: 3000,
            orientation: 1,
            taken_at: NOON,
            size: 2_516_582,
            thumb_key: "7".to_owned(),
            thumb_state: "ready",
            thumb_error: None,
            starred: false,
            hidden: false,
            make: None,
            model: None,
            lens: None,
            focal_mm: None,
            aperture: None,
            exposure_s: None,
            iso: None,
            tags: Vec::new(),
            caption: None,
            faces: Vec::new(),
            unnamed_faces: Vec::new(),
            kind: MediaKind::Image,
            duration_ms: None,
            video_crashed: false,
            albums: Vec::new(),
            copies: Vec::new(),
            uncropped_width: 4000,
            uncropped_height: 3000,
            edit: None,
            dates: ItemDates {
                taken: None,
                digitized: None,
                edited: None,
                file_created_ms: None,
                file_modified_ms: NOON * 1000,
            },
            gps: None,
            histogram: None,
        }
    }

    pub fn face(key: &str, name: &str) -> ItemFace {
        ItemFace {
            key: key.to_owned(),
            name: name.to_owned(),
            left: 0.1,
            top: 0.1,
            right: 0.2,
            bottom: 0.2,
            face_id: None,
        }
    }

    /// The same photo with something in every section.
    pub fn full() -> ViewerItem {
        ViewerItem {
            make: Some("Canon".to_owned()),
            model: Some("Canon EOS 5D".to_owned()),
            lens: Some("EF50mm f/1.8 STM".to_owned()),
            focal_mm: Some(50.0),
            aperture: Some(1.8),
            exposure_s: Some(0.004),
            iso: Some(400),
            tags: vec!["mountains".to_owned(), "dusk".to_owned()],
            caption: Some("  Lisbon, at dusk ".to_owned()),
            faces: vec![face("p:1", "Anna"), face("c:9", "Ben"), face("p:1", "Anna")],
            albums: vec![3, 5],
            copies: vec![
                ItemCopy {
                    id: 21,
                    path: "/backup/IMG_0412.jpg".to_owned(),
                    kind: CopyKind::Identical,
                    width: 4000,
                    height: 3000,
                },
                ItemCopy {
                    id: 22,
                    path: "/web/small.jpg".to_owned(),
                    kind: CopyKind::Similar,
                    width: 1600,
                    height: 1200,
                },
            ],
            dates: ItemDates {
                taken: Some(NOON),
                digitized: None,
                edited: None,
                file_created_ms: None,
                file_modified_ms: (NOON + 3600) * 1000,
            },
            gps: Some(ItemGps {
                lat: 48.1374,
                lon: 11.5755,
            }),
            histogram: Some(vec![900, 10, 40, 20, 5]),
            ..bare()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};
    use egui::{Modifiers, PointerButton, RawInput};

    const PLACE: Rect = Rect {
        min: pos2(600.0, 12.0),
        max: pos2(880.0, 1500.0),
    };

    fn albums() -> Vec<AlbumSummary> {
        let album = |id: i64, name: &str, picasa: bool| AlbumSummary {
            id,
            name: name.to_owned(),
            count: 1,
            picasa,
        };
        vec![
            album(3, "Best of Lisbon", false),
            album(4, "To print", false),
            album(5, "From Picasa", true),
            album(6, "Another of Picasa's", true),
        ]
    }

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    struct Fixture {
        ctx: egui::Context,
        view: InfoView,
        time: f64,
        place: Rect,
        item: Option<ViewerItem>,
        albums: Vec<AlbumSummary>,
        finds_faces: bool,
        looking: bool,
        zone: TimeZone,
        /// What the last frame wrote, each with where; and its icons.
        texts: Vec<(String, Rect)>,
        /// The colour each was written in.
        tints: Vec<(String, Color32)>,
        icons: Vec<Rect>,
        fills: Vec<Rect>,
        /// The colours of the lines drawn: a text field's caret is one.
        lines: Vec<Color32>,
        repaint: std::time::Duration,
        /// Each text drawn, line by line as it was broken.
        broken: Vec<(String, Vec<String>)>,
    }

    impl Fixture {
        fn of(item: ViewerItem) -> Self {
            let mut f = Self {
                ctx: egui::Context::default(),
                view: InfoView::default(),
                time: 0.0,
                place: PLACE,
                item: Some(item),
                albums: albums(),
                finds_faces: false,
                looking: false,
                zone: TimeZone::UTC,
                texts: Vec::new(),
                tints: Vec::new(),
                icons: Vec::new(),
                fills: Vec::new(),
                lines: Vec::new(),
                repaint: std::time::Duration::MAX,
                broken: Vec::new(),
            };
            f.frame(Vec::new());
            f
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<InfoAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = self.item.as_ref().map(|item| InfoData {
                item,
                albums: &self.albums,
                finds_faces: self.finds_faces,
                looking: self.looking,
                zone: &self.zone,
            });
            let (view, place) = (&mut self.view, self.place);
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| actions = view.show(ui, place, data.as_ref()));
            });
            full.textures_delta.clear();
            self.texts.clear();
            self.tints.clear();
            self.icons.clear();
            self.fills.clear();
            self.lines.clear();
            self.broken.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => {
                        self.broken.push((
                            text.galley.text().to_owned(),
                            (text.galley.rows.iter()).map(|row| row.text()).collect(),
                        ));
                        let seen = text.visual_bounding_rect().intersect(clipped.clip_rect);
                        if seen.is_positive() {
                            (self.texts).push((text.galley.text().to_owned(), seen));
                            // Its own colour, or the one it was painted with where it
                            // was laid out with none.
                            let own = (text.galley.job.sections.first())
                                .map(|section| section.format.color)
                                .filter(|own| *own != Color32::PLACEHOLDER);
                            self.tints.push((
                                text.galley.text().to_owned(),
                                own.unwrap_or(text.fallback_color),
                            ));
                        }
                    }
                    egui::Shape::LineSegment { stroke, .. } => self.lines.push(stroke.color),
                    egui::Shape::Rect(rect) => match &rect.brush {
                        Some(_) => self.icons.push(rect.rect),
                        None => self.fills.push(rect.rect),
                    },
                    _ => {}
                });
            }
            self.repaint = full
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay);
            actions
        }

        fn drew(&self, words: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == words)
                .map(|(_, at)| *at)
        }

        /// Everything written, from the top.
        fn said(&self) -> Vec<&str> {
            let mut texts: Vec<&(String, Rect)> = (self.texts.iter())
                .filter(|(words, _)| !words.is_empty())
                .collect();
            // Line by line: what stands on one line - its middles within a few points
            // of each other - from the left.
            texts.sort_by(|a, b| a.1.center().y.total_cmp(&b.1.center().y));
            let mut lines: Vec<Vec<&(String, Rect)>> = Vec::new();
            for text in texts {
                match lines.last_mut() {
                    Some(line) if (line[0].1.center().y - text.1.center().y).abs() < 6.0 => {
                        line.push(text);
                    }
                    _ => lines.push(vec![text]),
                }
            }
            for line in &mut lines {
                line.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
            }
            (lines.into_iter().flatten())
                .map(|(words, _)| words.as_str())
                .collect()
        }

        fn click(&mut self, at: Pos2) -> Vec<InfoAction> {
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)])
        }

        /// A click on what says `words`.
        fn press(&mut self, words: &str) -> Vec<InfoAction> {
            let at = self
                .drew(words)
                .unwrap_or_else(|| panic!("{words} is not drawn"));
            self.click(at.center())
        }

        fn key(&mut self, key: Key) -> Vec<InfoAction> {
            let event = |pressed| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            };
            let actions = self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
            actions
        }

        fn typing(&self) -> bool {
            self.view.typing()
        }
    }

    // Everything the library knows of the photo, in the panel's order: the file, its
    // caption, the camera, the dates, where it was taken, who is on it, its keywords,
    // its albums and its copies.
    #[test]
    fn the_panel_says_what_the_library_knows_of_the_photo() {
        let f = Fixture::of(full());
        assert_eq!(
            f.said(),
            [
                "IMG_0412.jpg",
                "/photos/2024/Lisbon/IMG_0412.jpg",
                "CAPTION",
                "Lisbon, at dusk",
                "Camera",
                "Canon EOS 5D",
                "Lens",
                "EF50mm f/1.8 STM",
                "Exposure",
                "50 mm · f/1.8 · 1/250 s · ISO 400",
                "DATES",
                "Taken",
                "Jun 15, 2024, 12:30 PM",
                "File modified",
                "Jun 15, 2024, 1:30 PM",
                "LOCATION",
                "48.13740° N, 11.57550° E",
                "Photos nearby",
                "·",
                "Open in OpenStreetMap",
                "PEOPLE",
                "Anna",
                "Ben",
                "KEYWORDS",
                "mountains",
                "dusk",
                "Add a keyword",
                "ALBUMS",
                "Best of Lisbon",
                "To print",
                "From Picasa",
                "IDENTICAL",
                "/backup/IMG_0412.jpg",
                "LOOKS THE SAME",
                "/web/small.jpg",
                "1600 × 1200",
                "Show 2 duplicates in the grid",
            ]
        );
        // On its ground, which is the place it was given.
        assert!(f.fills.contains(&PLACE));
        // The histogram stands between the camera and the dates.
        let (camera, dates) = (f.drew("Exposure").unwrap(), f.drew("DATES").unwrap());
        assert!(
            (f.fills.iter()).any(|fill| fill.height() == HISTOGRAM
                && fill.top() > camera.bottom()
                && fill.bottom() < dates.top()),
            "{:?}",
            f.fills
        );
    }

    // What a photo has none of is left out, or said in so many words where a section
    // would stand empty.
    #[test]
    fn what_a_photo_lacks_is_left_out_or_said() {
        let mut f = Fixture::of(bare());
        f.albums.clear();
        f.frame(Vec::new());
        assert_eq!(
            f.said(),
            [
                "IMG_0412.jpg",
                "/photos/2024/Lisbon/IMG_0412.jpg",
                "No camera data.",
                "DATES",
                "File modified",
                "Jun 15, 2024, 12:30 PM",
                "PEOPLE",
                "No faces named in Picasa.",
                "KEYWORDS",
                "No keywords yet.",
                "Add a keyword",
                "ALBUMS",
                "No albums of your own yet. Create one in the sidebar.",
            ]
        );
        assert!(!f.fills.iter().any(|fill| fill.height() == HISTOGRAM));
        // Who looks for faces decides what "none" means.
        f.finds_faces = true;
        f.frame(Vec::new());
        assert!(f.drew("No faces found.").is_some());
        f.looking = true;
        f.frame(Vec::new());
        assert!(f.drew("No faces found yet.").is_some());
        // Faces nobody has named are counted, and then nothing says there are none.
        let mut item = bare();
        item.unnamed_faces = vec![
            photon_engine::commands::UnnamedFace {
                left: 0.1,
                top: 0.1,
                right: 0.2,
                bottom: 0.2,
                face_id: None,
            };
            2
        ];
        f.item = Some(item);
        f.frame(Vec::new());
        assert!(f.drew("2 faces not named").is_some());
        assert_eq!(f.drew("No faces found yet."), None);
        // A caption of spaces is none; a video says how long it runs, with or without
        // a camera's rows; and the file's dates are read in the machine's zone.
        let mut item = bare();
        item.caption = Some("  \n ".to_owned());
        item.kind = MediaKind::Video;
        item.duration_ms = Some(75_000);
        f.item = Some(item);
        f.zone = TimeZone::get("Europe/Berlin").unwrap();
        f.frame(Vec::new());
        assert_eq!(f.drew("CAPTION"), None);
        assert_eq!(f.drew("No camera data."), None);
        assert!(f.drew("Length").is_some() && f.drew("1:15").is_some());
        assert!(f.drew("Jun 15, 2024, 2:30 PM").is_some());
        // Albums of Picasa's alone that the photo is in: listed, with no line about
        // photon's own.
        let mut item = bare();
        item.albums = vec![5];
        f.item = Some(item);
        f.albums = albums().into_iter().filter(|album| album.picasa).collect();
        f.frame(Vec::new());
        assert!(f.drew("From Picasa").is_some());
        assert_eq!(f.drew("Another of Picasa's"), None);
        assert_eq!(
            f.drew("No albums of your own yet. Create one in the sidebar."),
            None
        );
    }

    // The camera, the lens and "Photos nearby" are links to their searches; the
    // exposure is no link, and "Open in OpenStreetMap" waits for a later sub-project.
    #[test]
    fn a_camera_a_lens_and_the_photos_nearby_are_links_to_their_searches() {
        let mut f = Fixture::of(full());
        assert_eq!(
            f.press("Canon EOS 5D"),
            [InfoAction::Search("camera:\"Canon EOS 5D\"".to_owned())]
        );
        assert_eq!(
            f.press("EF50mm f/1.8 STM"),
            [InfoAction::Search("lens:\"EF50mm f/1.8 STM\"".to_owned())]
        );
        assert_eq!(
            f.press("Photos nearby"),
            [InfoAction::Search("near:48.1374,11.5755".to_owned())]
        );
        assert_eq!(f.press("Open in OpenStreetMap"), [InfoAction::Map]);
        for words in [
            "50 mm · f/1.8 · 1/250 s · ISO 400",
            "Camera",
            "IMG_0412.jpg",
            "Jun 15, 2024, 12:30 PM",
            "48.13740° N, 11.57550° E",
        ] {
            assert_eq!(f.press(words), [], "{words}");
        }
    }

    // A person on the photo is a link to their view, by their key, and is there once
    // however many of their faces the photo has.
    #[test]
    fn a_person_is_a_link_to_their_view() {
        let mut f = Fixture::of(full());
        assert_eq!(
            f.texts.iter().filter(|(words, _)| words == "Anna").count(),
            1
        );
        assert_eq!(f.press("Anna"), [InfoAction::Person("p:1".to_owned())]);
        assert_eq!(f.press("Ben"), [InfoAction::Person("c:9".to_owned())]);
    }

    // A keyword is taken off by the cross beside it, and by nothing else on it.
    #[test]
    fn a_keyword_is_taken_off_by_its_cross() {
        let mut f = Fixture::of(full());
        let cross_of = |f: &Fixture, tag: &str| {
            let words = f.drew(tag).unwrap();
            (f.icons.iter())
                .copied()
                .filter(|icon| {
                    (icon.center().y - words.center().y).abs() < 4.0 && icon.left() > words.right()
                })
                .min_by(|a, b| a.left().total_cmp(&b.left()))
                .expect("its cross")
        };
        let cross = cross_of(&f, "mountains");
        assert_eq!(
            f.click(cross.center()),
            [InfoAction::RemoveTag("mountains".to_owned())]
        );
        let cross = cross_of(&f, "dusk");
        assert_eq!(
            f.click(cross.center()),
            [InfoAction::RemoveTag("dusk".to_owned())]
        );
        assert_eq!(f.press("mountains"), []);
    }

    // A keyword is typed into the field and added by Enter: what is typed, trimmed, and
    // the field is empty for the next. A letter and the Enter after it can come in one
    // frame, and the letter is in the keyword; nothing typed adds nothing.
    #[test]
    fn a_keyword_is_added_by_enter_in_its_field() {
        let mut f = Fixture::of(full());
        assert!(!f.typing());
        f.press("Add a keyword");
        assert!(f.typing(), "a press on the field gives it the keyboard");
        f.frame(vec![Event::Text(" alps ".to_owned())]);
        assert_eq!(f.key(Key::Enter), [InfoAction::AddTag("alps".to_owned())]);
        assert!(f.typing(), "and the field is ready for the next");
        assert!(f.drew("Add a keyword").is_some(), "empty again");
        assert_eq!(f.key(Key::Enter), [], "nothing typed is no keyword");
        f.frame(vec![Event::Text("   ".to_owned())]);
        assert_eq!(f.key(Key::Enter), []);
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![Event::Text("se".to_owned())]);
        assert_eq!(
            f.frame(vec![Event::Text("a".to_owned()), enter.clone()]),
            [InfoAction::AddTag("sea".to_owned())]
        );
        // Held, Enter adds once.
        f.frame(vec![Event::Text("x".to_owned())]);
        let held = Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed: true,
            repeat: true,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![held]), []);
    }

    // The field is left by Escape, which is its own key and adds nothing, and by a
    // press anywhere else - a press, not a click: one that becomes a drag of the photo
    // leaves it too.
    #[test]
    fn the_field_is_left_by_escape_and_by_a_press_elsewhere() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("al".to_owned())]);
        assert_eq!(f.key(Key::Escape), []);
        assert!(!f.typing());
        assert!(f.drew("al").is_some(), "what was typed stays in it");
        let field = f.drew("al").unwrap().center();
        f.click(field);
        assert!(f.typing());
        // A press on the field itself is not "elsewhere": the keyboard stays with the
        // button still down, and is not given up and taken again at the click.
        f.frame(vec![Event::PointerMoved(field)]);
        f.frame(vec![Event::PointerButton {
            pos: field,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(f.typing(), "at the press on the field");
        f.frame(vec![Event::PointerButton {
            pos: field,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(f.typing());
        let elsewhere = pos2(100.0, 100.0);
        f.frame(vec![Event::PointerMoved(elsewhere)]);
        f.frame(vec![Event::PointerButton {
            pos: elsewhere,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!f.typing(), "at the press, with the button still down");
        // Whichever button: the other one, where a menu will open, leaves it too.
        f.click(field);
        assert!(f.typing());
        f.frame(vec![Event::PointerButton {
            pos: elsewhere,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!f.typing());
    }

    // At a step there is nothing to draw until the library has said what the next photo
    // is, and egui lets go of a field that is not drawn. The keyboard is the field's all
    // the same, for the next photo's keyword - however many frames the answer takes -
    // unless it is left meanwhile, by Escape or a press.
    #[test]
    fn the_field_keeps_the_keyboard_over_frames_with_nothing_to_draw() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("al".to_owned())]);
        f.item = None;
        for _ in 0..5 {
            f.frame(Vec::new());
            assert!(f.typing());
        }
        let mut next = full();
        next.id = 8;
        f.item = Some(next);
        f.frame(Vec::new());
        f.frame(vec![Event::Text("porto".to_owned())]);
        f.frame(Vec::new());
        assert!(
            f.drew("porto").is_some(),
            "typed into the next photo's field"
        );
        assert_eq!(f.key(Key::Enter), [InfoAction::AddTag("porto".to_owned())]);
        // Left while there is nothing to draw: it is not taken back.
        f.item = None;
        f.frame(Vec::new());
        assert_eq!(f.key(Key::Escape), []);
        assert!(!f.typing());
        f.item = Some(full());
        f.frame(Vec::new());
        f.frame(vec![Event::Text("x".to_owned())]);
        f.frame(Vec::new());
        assert_eq!(f.drew("x"), None);
        assert!(!f.typing());
        // And by a press, of anything, anywhere.
        f.press("Add a keyword");
        assert!(f.typing());
        f.item = None;
        f.frame(Vec::new());
        f.frame(vec![Event::PointerButton {
            pos: pos2(100.0, 100.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!f.typing());
    }

    // A keyword the photo has already is not asked for again, in whatever case it is
    // typed: the field is emptied and nothing is written.
    #[test]
    fn a_keyword_the_photo_has_is_not_added_again() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text(" Mountains ".to_owned())]);
        assert_eq!(f.key(Key::Enter), []);
        f.frame(Vec::new());
        assert!(
            f.drew("Add a keyword").is_some(),
            "the field is empty again"
        );
        f.frame(vec![Event::Text("mountain".to_owned())]);
        assert_eq!(
            f.key(Key::Enter),
            [InfoAction::AddTag("mountain".to_owned())]
        );
    }

    // A field scrolled out of sight keeps the keyboard, and what is typed is typed into
    // it: let go of there, the letters typed next would be the viewer's keys, and `h`
    // hide the photo.
    #[test]
    fn a_field_scrolled_out_of_sight_keeps_the_keyboard() {
        let mut f = Fixture::of(full());
        f.place = Rect::from_min_size(PLACE.min, vec2(280.0, 300.0));
        f.frame(Vec::new());
        f.view.turned(-100_000.0);
        f.frame(Vec::new());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("al".to_owned())]);
        f.frame(Vec::new());
        assert!(f.drew("al").is_some());
        f.view.turned(100_000.0);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.drew("al"), None, "out of sight");
        assert!(f.typing());
        f.frame(vec![Event::Text("ps".to_owned())]);
        assert!(f.typing());
        f.view.turned(-100_000.0);
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert!(f.drew("alps").is_some());
        assert_eq!(f.key(Key::Enter), [InfoAction::AddTag("alps".to_owned())]);
    }

    // The viewer's glass is dark in both themes, and egui colours a field's placeholder
    // and its caret by the theme the window is in: under the light theme the words
    // "Add a keyword" were all but invisible, which a picture showed, and the caret is
    // that theme's dark blue.
    #[test]
    fn the_fields_placeholder_and_caret_are_the_dark_themes_in_a_light_window() {
        let mut f = Fixture::of(full());
        crate::theme::apply::install(&f.ctx);
        f.ctx.set_theme(egui::ThemePreference::Light);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let tint = |f: &Fixture, words: &str| {
            (f.tints.iter())
                .find(|(drawn, _)| drawn == words)
                .map(|(_, tint)| *tint)
        };
        assert_eq!(tint(&f, "Add a keyword"), Some(color(DARK.text_dim)));
        let (light, dark) = (egui::Visuals::light(), egui::Visuals::dark());
        let (light, dark) = (
            light.text_cursor.stroke.color,
            dark.text_cursor.stroke.color,
        );
        assert_ne!(light, dark);
        f.press("Add a keyword");
        f.frame(Vec::new());
        assert!(f.typing());
        assert!(f.lines.contains(&dark), "{:?}", f.lines);
        assert!(!f.lines.contains(&light), "{:?}", f.lines);
        // What is typed is in the glass's own text colour.
        f.frame(vec![Event::Text("alp".to_owned())]);
        f.frame(Vec::new());
        assert_eq!(tint(&f, "alp"), Some(color(DARK.text)));
    }

    // One of photon's own albums is ticked and unticked by a press on its row; Picasa's
    // are listed where the photo is in them, and take no press.
    #[test]
    fn an_album_of_photons_own_is_ticked_and_picasas_is_only_listed() {
        let mut f = Fixture::of(full());
        assert_eq!(f.press("To print"), [InfoAction::Album { id: 4, on: true }]);
        assert_eq!(
            f.press("Best of Lisbon"),
            [InfoAction::Album { id: 3, on: false }]
        );
        assert_eq!(f.press("From Picasa"), []);
        // The box before the name is the row's too.
        let name = f.drew("To print").unwrap();
        assert_eq!(
            f.click(pos2(PLACE.left() + PAD_X + BOX / 2.0, name.center().y)),
            [InfoAction::Album { id: 4, on: true }]
        );
    }

    // A copy is a link that finds it in the grid, and the line under them shows them all
    // together in the view of the photo's copies.
    #[test]
    fn a_copy_is_a_link_that_locates_it() {
        let mut f = Fixture::of(full());
        assert_eq!(f.press("/backup/IMG_0412.jpg"), [InfoAction::Locate(21)]);
        assert_eq!(f.press("/web/small.jpg"), [InfoAction::Locate(22)]);
        assert_eq!(
            f.press("Show 2 duplicates in the grid"),
            [InfoAction::ShowCopies(7)]
        );
        assert_eq!(f.press("1600 × 1200"), []);
    }

    // The path, cut short in a narrow panel, is said whole under the pointer.
    #[test]
    fn the_whole_path_is_said_under_the_pointer() {
        let mut f = Fixture::of(full());
        let path = "/photos/2024/Lisbon/IMG_0412.jpg";
        let at = f.drew(path).expect("the path").center();
        let seen = |f: &Fixture| (f.texts.iter()).filter(|(words, _)| words == path).count();
        assert_eq!(seen(&f), 1);
        f.frame(vec![Event::PointerMoved(at)]);
        for _ in 0..60 {
            f.frame(Vec::new());
        }
        assert_eq!(seen(&f), 2, "said again beside the pointer");
    }

    // A panel taller than its place scrolls by what the wheel turns, as far as its end
    // and no further, and what is scrolled out of it is neither written nor pressed.
    #[test]
    fn a_panel_taller_than_its_place_scrolls_as_far_as_its_ends() {
        let mut f = Fixture::of(full());
        f.place = Rect::from_min_size(PLACE.min, vec2(280.0, 300.0));
        f.frame(Vec::new());
        assert!(f.drew("IMG_0412.jpg").is_some());
        assert_eq!(f.drew("ALBUMS"), None, "below the panel's foot");
        let top = f.drew("DATES").unwrap().top();
        f.view.turned(-100.0);
        f.frame(Vec::new());
        assert_eq!(f.drew("DATES").unwrap().top(), top - 100.0);
        assert_eq!(f.drew("IMG_0412.jpg"), None, "above its top");
        // Where the file's name was there is nothing to press now, and what is there
        // is pressed where it is.
        f.view.turned(100_000.0);
        f.frame(Vec::new());
        assert_eq!(
            f.drew("DATES").unwrap().top(),
            top,
            "no further than its top"
        );
        f.view.turned(-100_000.0);
        f.frame(Vec::new());
        let last = f.drew("Show 2 duplicates in the grid").unwrap();
        let inner_foot = f.place.bottom() - PAD_Y;
        // (Its letters' own height is a point or two less than its line's.)
        assert!((last.bottom() - inner_foot).abs() < 4.0, "{last:?}");
        assert_eq!(f.press("/web/small.jpg"), [InfoAction::Locate(22)]);
        // A link cut off by the panel's edge is pressed where it is seen and not where
        // it is not: the lens, with its top six points above the edge.
        f.view.turned(100_000.0);
        f.frame(Vec::new());
        let lens = f.drew("EF50mm f/1.8 STM").expect("at the top");
        let edge = f.place.top() + PAD_Y;
        f.view.turned(-(lens.top() - edge + 6.0));
        f.frame(Vec::new());
        let search = InfoAction::Search("lens:\"EF50mm f/1.8 STM\"".to_owned());
        assert_eq!(f.click(pos2(lens.center().x, edge + 3.0)), [search]);
        assert_eq!(f.click(pos2(lens.center().x, edge - 3.0)), []);
    }

    // Things side by side go on to the next line where the column ends: more keywords
    // than a line holds stand in lines, each from the column's left edge, and nothing
    // runs out of the panel.
    #[test]
    fn keywords_that_do_not_fit_a_line_go_on_to_the_next() {
        let mut item = full();
        item.tags = (1..=12).map(|n| format!("keyword {n}")).collect();
        let f = Fixture::of(item);
        let chips: Vec<Rect> = (1..=12)
            .map(|n| f.drew(&format!("keyword {n}")).expect("drawn"))
            .collect();
        let inner = PLACE.left() + PAD_X..=PLACE.right() - PAD_X;
        for chip in &chips {
            assert!(
                inner.contains(&chip.left()) && inner.contains(&chip.right()),
                "{chip:?}"
            );
        }
        let mut lines: Vec<f32> = chips.iter().map(|chip| chip.top()).collect();
        lines.dedup();
        assert!(lines.len() >= 3, "{lines:?}");
        assert!(
            lines.is_sorted(),
            "each line under the one before: {lines:?}"
        );
        // The first of every line stands where the first of all does.
        let first = chips[0].left();
        for line in &lines {
            let starts = (chips.iter()).find(|chip| chip.top() == *line).unwrap();
            assert_eq!(starts.left(), first);
        }
        // And what comes after the keywords is under their last line.
        let field = f.drew("Add a keyword").expect("the field");
        assert!(field.top() > chips[11].bottom());
    }

    // Scrolled to its end, and then shorter - a photo with less to say, a keyword taken
    // off: the panel is not left scrolled past its end, and the frame that puts it right
    // is asked for.
    #[test]
    fn a_panel_that_gets_shorter_is_not_left_scrolled_past_its_end() {
        let mut f = Fixture::of(full());
        f.place = Rect::from_min_size(PLACE.min, vec2(280.0, 300.0));
        f.frame(Vec::new());
        f.view.turned(-100_000.0);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let mut item = full();
        item.copies.clear();
        item.caption = None;
        f.item = Some(item);
        f.frame(Vec::new());
        assert_eq!(
            f.repaint,
            std::time::Duration::ZERO,
            "the next frame is asked for"
        );
        f.frame(Vec::new());
        let last = f.drew("From Picasa").unwrap();
        assert!(
            (last.bottom() - (f.place.bottom() - PAD_Y)).abs() < 6.0,
            "{last:?}"
        );
    }

    // Another photo's panel begins at its top, with an empty field.
    #[test]
    fn another_photo_begins_with_an_empty_field_at_the_top() {
        let low = Rect::from_min_size(PLACE.min, vec2(280.0, 300.0));
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("alp".to_owned())]);
        // (egui's field draws what was typed in the frame after it took it.)
        f.frame(Vec::new());
        assert!(f.drew("alp").is_some());
        // In a low panel, scrolled past the file's name.
        f.place = low;
        f.frame(Vec::new());
        let top = f.drew("IMG_0412.jpg").unwrap().top();
        f.view.turned(-150.0);
        f.frame(Vec::new());
        assert_eq!(f.drew("IMG_0412.jpg"), None);
        // The same photo, read again by the library: nothing is lost, and it stays
        // where it was scrolled to.
        f.item = Some(full());
        f.frame(Vec::new());
        assert_eq!(f.drew("IMG_0412.jpg"), None);
        f.place = PLACE;
        f.frame(Vec::new());
        assert!(f.drew("alp").is_some());
        // Another photo: from its top, with nothing typed.
        f.place = low;
        f.frame(Vec::new());
        f.view.turned(-150.0);
        f.frame(Vec::new());
        let mut next = full();
        next.id = 8;
        f.item = Some(next);
        f.frame(Vec::new());
        assert_eq!(f.drew("IMG_0412.jpg").unwrap().top(), top);
        f.place = PLACE;
        f.frame(Vec::new());
        assert_eq!(f.drew("alp"), None);
        assert!(
            f.drew("Add a keyword").is_some(),
            "the empty field's own words"
        );
    }

    // Until the library has said what the photo is, the panel is its ground alone: it
    // does not come and go at every step.
    #[test]
    fn with_nothing_said_yet_the_panel_is_its_ground_alone() {
        let mut f = Fixture::of(full());
        f.item = None;
        assert_eq!(f.frame(Vec::new()), []);
        assert!(f.texts.is_empty());
        assert!(f.fills.contains(&PLACE));
    }

    // A panel on screen asks for no frame, with the pointer over it or not.
    #[test]
    fn the_panel_asks_for_no_frame() {
        let mut f = Fixture::of(full());
        f.frame(vec![Event::PointerMoved(PLACE.center())]);
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        for _ in 0..4 {
            f.frame(Vec::new());
            assert_eq!(f.repaint, std::time::Duration::MAX);
        }
    }

    // One field under one name for every photo and every keyword: egui keeps a field's
    // history by that name. Ctrl+Z in the field of another photo brings back the keyword
    // typed for the photo before - which Enter then adds to this one. The sidebar's
    // field and the picker's each learnt this (CLAUDE.md, "A field opens with a history
    // of its own"); this third field has no history of its own.
    #[test]
    fn undo_in_another_photos_field_does_not_bring_back_the_last_photos_keyword() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        // Typed with pauses, as a person types: egui notes a point to go back to when
        // the text has rested.
        for letter in ["p", "o", "r", "t", "o"] {
            f.frame(vec![Event::Text(letter.to_owned())]);
            for _ in 0..90 {
                f.frame(Vec::new());
            }
        }
        assert_eq!(f.key(Key::Enter), [InfoAction::AddTag("porto".to_owned())]);
        for _ in 0..90 {
            f.frame(Vec::new());
        }
        // Another photo, and its field.
        let mut next = full();
        next.id = 8;
        f.item = Some(next);
        f.frame(Vec::new());
        f.press("Add a keyword");
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
        f.frame(Vec::new());
        assert!(
            f.drew("Add a keyword").is_some(),
            "the field of another photo is empty still; it holds {:?}",
            f.view.draft
        );
    }

    // "Words that wrap within `width`, anywhere in a word where `anywhere` (a path, a
    // name with no space in it)". egui breaks a word that is too long for its line
    // without being told to (`row_break_candidates.get`: a word's end, else a dash,
    // else anywhere); told `break_anywhere`, it breaks at the last letter that fits
    // and never looks for a word's end. So everything laid out with `anywhere` - a
    // camera, a lens, the exposure, an album's name, a keyword, a person - is cut in
    // the middle of a word wherever it does not fit its line.
    #[test]
    fn words_that_have_spaces_are_broken_at_them() {
        let mut item = full();
        item.model = Some("Canon EOS 5D Mark IV Special Edition".to_owned());
        item.lens = Some("EF 24-105mm f/4L IS II USM Zoom Lens".to_owned());
        let mut f = Fixture::of(item);
        f.albums[0].name = "Best of Lisbon and of the Algarve coast, 2024".to_owned();
        f.frame(Vec::new());
        for whole in [
            "Canon EOS 5D Mark IV Special Edition",
            "EF 24-105mm f/4L IS II USM Zoom Lens",
            "Best of Lisbon and of the Algarve coast, 2024",
        ] {
            let lines = (f.broken.iter())
                .find(|(text, _)| text == whole)
                .map(|(_, lines)| lines.clone())
                .unwrap_or_else(|| panic!("{whole} is not drawn"));
            assert!(lines.len() > 1, "{whole} is on one line: widen the test");
            for line in &lines[..lines.len() - 1] {
                let next_begins = whole[whole.find(line.as_str()).unwrap() + line.len()..]
                    .chars()
                    .next();
                assert!(
                    line.ends_with(' ') || next_begins == Some(' '),
                    "{whole:?} is broken in the middle of a word: {lines:?}"
                );
            }
        }
    }

    // The arrows and Home move the caret, and the keyboard stays in the field: with the
    // field's filter giving the arrows up, egui walks the keyboard off with them.
    #[test]
    fn the_arrows_move_the_caret_in_the_keywords_field() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("abc".to_owned())]);
        f.key(Key::ArrowLeft);
        assert!(f.typing(), "the keyboard stays in the field");
        f.frame(vec![Event::Text("x".to_owned())]);
        f.frame(Vec::new());
        assert_eq!(f.view.draft, "abxc");
        f.key(Key::Home);
        f.frame(vec![Event::Text("y".to_owned())]);
        f.frame(Vec::new());
        assert_eq!(f.view.draft, "yabxc");
    }

    // Enter is the field's only while the field has the keyboard: read whether or not
    // it has, an Enter pressed anywhere in the viewer added what was once left in the
    // field.
    #[test]
    fn enter_adds_nothing_while_the_field_has_not_the_keyboard() {
        let mut f = Fixture::of(full());
        f.press("Add a keyword");
        f.frame(vec![Event::Text("al".to_owned())]);
        assert_eq!(f.key(Key::Escape), []);
        assert!(!f.typing());
        assert_eq!(f.key(Key::Enter), []);
        f.frame(Vec::new());
        assert!(
            f.drew("al").is_some(),
            "and what was typed is still in the field"
        );
    }
}
