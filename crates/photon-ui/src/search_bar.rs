//! The search box in the top bar, drawn: the field, the button that opens what it
//! understands, the bookmark that saves a search, and that panel. `SearchBar.svelte` and
//! `SearchHelp.svelte`.
//!
//! A view: the text is the caller's (`search_box.rs` holds it and what typing makes due),
//! and what the user did is answered. The field is egui's own, which does not reorder text
//! that mixes writing directions and whose input-method support cannot be tried without a
//! window: both are on the smoke checklist, and neither is mended here.

use crate::{
    icons::Icon,
    search_help::{HelpGroup, SEARCH_HELP, insert_term},
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, SHADOW_MENU, T},
    },
};
use eframe::egui::{
    self, Event, Id, Key, Order, Rect, Sense, Stroke, StrokeKind, UiBuilder, WidgetInfo,
    WidgetType, epaint::Shadow, pos2, text::CCursor, text::CCursorRange, vec2,
};

/// The field: at most this wide, and this tall in the middle of the bar.
pub const FIELD_WIDTH: f32 = 320.0;
const FIELD_HEIGHT: f32 = 30.0;
/// Where the text starts, clear of the icon at the field's left.
const TEXT_LEFT: f32 = 30.0;
/// A button at the field's right: the help's always, the bookmark's inside it.
const BUTTON: f32 = 22.0;
const ICON: f32 = 14.0;
/// The panel: at most this wide, its term column this wide, two columns of groups.
const PANEL_WIDTH: f32 = 760.0;
const TERM_COLUMN: f32 = 120.0;
/// How many groups stand in the panel's left column: the three shorter ones, against the
/// three longer in the right, which is how a browser balances them.
const LEFT_GROUPS: usize = 3;

const PLACEHOLDER: &str = "Search names, camera, keywords, dates…";
const FIELD_LABEL: &str = "Search photos by file or folder name, camera, lens, keyword or date";
const HELP_LABEL: &str = "What you can search for";
const LEAD: &str = "Everything typed must match. Click a term to add it to the search.";

pub struct SearchBarData<'a> {
    /// The name the box's search is saved under, when it is.
    pub saved_as: Option<&'a str>,
    /// Whether the bookmark can save what the box holds.
    pub can_save: bool,
    /// Whether the user is in a search, or on their way to one.
    pub in_search: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchAction {
    /// The text was changed by the user: it is to be searched for once they stop.
    Typed,
    /// The box was emptied with Escape: the empty search is to be made now.
    Clear,
    /// The bookmark: the search is to be saved.
    Save,
}

#[derive(Default)]
pub struct SearchBar {
    /// Whether the panel that lists the grammar is open.
    help: bool,
    /// Whether the `/` that put the focus in the box is still down. Until it is let go
    /// the slashes it repeats are not typed: the key was pressed to get to the box.
    slash_held: bool,
    /// Where the panel was last drawn, for a press outside it.
    panel: Option<Rect>,
    /// Whether the field had the keyboard when it was last drawn. egui takes the focus
    /// from whatever has it at the start of a frame with Escape in it, before anything
    /// is drawn: asked of egui in that frame, the box never had it.
    focused: bool,
}

/// The field's own name to egui: where the keyboard is, and the caret.
fn field_id() -> Id {
    Id::new("search-box")
}

impl SearchBar {
    /// Whether the panel is open.
    pub fn help_open(&self) -> bool {
        self.help
    }

    /// Draws the box at the left of `rect` and, when it is open, the panel under it.
    /// Answers what the user did.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        text: &mut String,
        data: &SearchBarData<'_>,
    ) -> Vec<SearchAction> {
        let palette = palette(ui.ctx());
        let mut actions = Vec::new();
        let id = field_id();
        let field = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - FIELD_HEIGHT / 2.0),
            vec2(FIELD_WIDTH.min(rect.width()).max(0.0), FIELD_HEIGHT),
        );
        let had_focus = self.focused;
        let select = self.take_keys(ui, had_focus);

        // The buttons stand inside the field's right end, and the text stops short of them.
        let holding = !text.trim().is_empty();
        let button_at = |from_right: f32| {
            Rect::from_center_size(
                pos2(field.right() - from_right - BUTTON / 2.0, field.center().y),
                vec2(BUTTON, BUTTON),
            )
        };
        let help_button = button_at(4.0);
        let save_button = button_at(4.0 + BUTTON + 2.0);
        let text_right = if holding {
            save_button.left()
        } else {
            help_button.left()
        } - 2.0;

        let painter = ui.painter_at(rect);
        painter.rect_filled(field, R[2], color(palette.field));
        let icon = Rect::from_center_size(
            pos2(field.left() + 9.0 + ICON / 2.0, field.center().y),
            vec2(ICON, ICON),
        );
        Icon::Search.paint(ui, icon, ICON, false, color(palette.text_dim));

        // The field's line, in the middle of the field: egui's text field stands at the
        // top of whatever room it is given.
        let font = fonts::regular(T[2]);
        let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
        let text_left = field.left() + TEXT_LEFT;
        let edit_rect = Rect::from_min_max(
            pos2(text_left, field.center().y - line / 2.0),
            pos2(text_right.max(text_left), field.center().y + line / 2.0),
        );
        let escape = ui.input(|input| input.key_pressed(Key::Escape));
        let output = ui
            .scope_builder(UiBuilder::new().max_rect(edit_rect), |ui| {
                // Cut at the buttons, and not at the line's own height: a letter's tail
                // hangs under it.
                let room = Rect::from_x_y_ranges(edit_rect.x_range(), field.y_range());
                ui.set_clip_rect(room.intersect(ui.clip_rect()));
                egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(font)
                    .text_color(color(palette.text))
                    .hint_text(egui::RichText::new(PLACEHOLDER).color(color(palette.text_dim)))
                    .desired_width(edit_rect.width())
                    .show(ui)
            })
            .inner;
        output
            .response
            .widget_info(|| WidgetInfo::text_edit(true, "", text.as_str(), FIELD_LABEL));
        if output.response.changed() {
            actions.push(SearchAction::Typed);
        }
        if select {
            // Ctrl+F and `/`: the caret in the box with what it holds selected, so that
            // typing replaces the last search and an arrow key keeps it to refine.
            output.response.request_focus();
            let mut state = output.state.clone();
            let end = CCursor::new(text.chars().count());
            state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
            state.store(ui.ctx(), id);
        }

        // Escape closes the nearest thing: the panel first, and the search stays. Then it
        // clears the box - when there is something to clear. An empty box outside a
        // search has nothing: clearing sends the empty query, which is All photos, and
        // would throw the user out of Starred with a key that cleared nothing. There the
        // key only leaves the box, which egui's field does by itself.
        if escape && self.help {
            self.help = false;
            if had_focus {
                ui.memory_mut(|memory| memory.request_focus(id));
            }
        } else if escape && had_focus && (!text.is_empty() || data.in_search) {
            text.clear();
            actions.push(SearchAction::Clear);
            ui.memory_mut(|memory| memory.request_focus(id));
        }

        self.focused = ui.memory(|memory| memory.has_focus(id));
        if self.focused {
            painter.rect_stroke(
                field,
                R[2],
                Stroke::new(2.0, color(palette.accent)),
                StrokeKind::Inside,
            );
        } else {
            self.slash_held = false;
        }

        // What the box understands.
        if small_button(
            ui,
            help_button,
            "search-help",
            HELP_LABEL,
            Icon::CircleHelp,
            Look::of(self.help),
            palette,
        ) {
            self.help = !self.help;
        }
        // The bookmark, while there is something to save. Filled and taking no press once
        // saved: a saved search is removed from the sidebar, which asks first, so there is
        // no one-click undo of a thing the sidebar guards.
        if holding {
            let label = match data.saved_as {
                Some(name) => format!("Saved as “{name}”"),
                None => "Save this search".to_owned(),
            };
            let look = Look {
                filled: data.saved_as.is_some(),
                enabled: data.can_save,
                lit: false,
            };
            if small_button(
                ui,
                save_button,
                "search-save",
                &label,
                Icon::Bookmark,
                look,
                palette,
            ) {
                actions.push(SearchAction::Save);
            }
        }

        if self.help {
            // A press anywhere but the box and its panel closes the panel, as leaving a
            // menu does.
            let pressed = ui.input(|input| {
                (input.pointer.any_pressed())
                    .then(|| input.pointer.interact_pos())
                    .flatten()
            });
            let inside =
                |at| field.contains(at) || self.panel.is_some_and(|panel| panel.contains(at));
            if pressed.is_some_and(|at| !inside(at)) {
                self.help = false;
            }
        }
        if self.help {
            if let Some(term) = self.panel(ui, field, palette) {
                // A term clicked goes into the box as though it had been typed there.
                // Then the caret, after it: a prefix like `camera:` is half a term, and
                // the rest is the user's to type.
                *text = insert_term(text, term);
                self.help = false;
                actions.push(SearchAction::Typed);
                ui.memory_mut(|memory| memory.request_focus(id));
                let mut state = output.state;
                let end = CCursor::new(text.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), id);
            }
        } else {
            self.panel = None;
        }
        actions
    }

    /// Reads the keys that are the box's wherever the keyboard is, and keeps the slash
    /// that opened the box from being typed into it. Answers whether the box is to take
    /// the keyboard with its text selected.
    fn take_keys(&mut self, ui: &egui::Ui, had_focus: bool) -> bool {
        let typing = had_focus || ui.ctx().text_edit_focused();
        let (mut find, mut slash, mut let_go) = (false, false, false);
        ui.input(|input| {
            for event in &input.events {
                match event {
                    Event::Key {
                        key: Key::F,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                        find = true;
                    }
                    // Not with Ctrl or Alt, which make it another key. Shift is how some
                    // keyboards reach it at all.
                    Event::Key {
                        key: Key::Slash,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                        slash = true;
                    }
                    Event::Key {
                        key: Key::Slash,
                        pressed: false,
                        ..
                    } => let_go = true,
                    _ => {}
                }
            }
        });
        // In a text field a slash is a slash.
        let slash = slash && !typing;
        if let_go {
            self.slash_held = false;
        }
        if slash {
            self.slash_held = true;
        }
        if self.slash_held {
            ui.input_mut(|input| {
                input
                    .events
                    .retain(|event| !matches!(event, Event::Text(typed) if typed == "/"));
            });
        }
        find || slash
    }

    /// The panel under the box. Answers the term that was clicked.
    fn panel(&mut self, ui: &egui::Ui, field: Rect, palette: &Palette) -> Option<&'static str> {
        let window = ui.ctx().content_rect();
        let width = PANEL_WIDTH.min(window.width() - 2.0 * S[1]).max(0.0);
        // Never taller than the window under the top bar: it scrolls instead.
        let tallest = (window.height() - 64.0).max(0.0);
        let mut picked = None;
        let shown = egui::Area::new(Id::new("search-help-panel"))
            .order(Order::Foreground)
            .fixed_pos(pos2(field.left(), field.bottom() + S[1]))
            // An area starts at egui's own default size, four hundred points tall, and
            // what scrolls inside it never asks for more: the panel was cut there.
            .default_size(vec2(width, tallest))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(color(palette.raised))
                    .stroke(Stroke::new(1.0, color(palette.line)))
                    .corner_radius(R[3])
                    .shadow(Shadow {
                        offset: [SHADOW_MENU.x as i8, SHADOW_MENU.y as i8],
                        blur: SHADOW_MENU.blur as u8,
                        spread: 0,
                        color: color(SHADOW_MENU.ink),
                    })
                    .inner_margin(egui::Margin::symmetric(S[3] as i8, S[2] as i8))
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * S[3]);
                        egui::ScrollArea::vertical()
                            .max_height((tallest - 2.0 * S[2]).max(0.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(LEAD)
                                        .size(T[2])
                                        .color(color(palette.text_dim)),
                                );
                                ui.add_space(S[2]);
                                ui.columns(2, |columns| {
                                    let (left, right) =
                                        SEARCH_HELP.split_at(LEFT_GROUPS.min(SEARCH_HELP.len()));
                                    for (column, groups) in columns.iter_mut().zip([left, right]) {
                                        for group in groups {
                                            if let Some(term) = self::group(column, group, palette)
                                            {
                                                picked = Some(term);
                                            }
                                        }
                                    }
                                });
                            });
                    });
            });
        self.panel = Some(shown.response.rect);
        picked
    }
}

/// One group of the panel: its title, and a row to each entry - what is typed, set apart
/// as a key cap is, and what it finds. Answers the term that was clicked.
fn group(ui: &mut egui::Ui, group: &HelpGroup, palette: &Palette) -> Option<&'static str> {
    let mut picked = None;
    ui.label(
        egui::RichText::new(group.title)
            .font(fonts::semibold(ui.ctx(), T[2]))
            .color(color(palette.text)),
    );
    let does_width = (ui.available_width() - TERM_COLUMN - S[1]).max(0.0);
    egui::Grid::new(("search-help", group.title))
        .num_columns(2)
        .min_col_width(TERM_COLUMN)
        .max_col_width(does_width)
        .spacing(vec2(S[1], S[0]))
        .show(ui, |ui| {
            for entry in group.entries {
                let cap = egui::RichText::new(entry.text)
                    .size(T[1])
                    .color(color(palette.text));
                if entry.insert {
                    // A term is a button, and says so under the pointer.
                    let button = egui::Button::new(cap)
                        .fill(color(palette.field))
                        .stroke(Stroke::new(1.0, color(palette.line)))
                        .corner_radius(R[1]);
                    if ui.add(button).clicked() {
                        picked = Some(entry.text);
                    }
                } else {
                    // A sample is the same words without the invitation: the user's own
                    // word, phrase or date is what belongs in the box.
                    ui.label(cap);
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(entry.does)
                            .size(T[1])
                            .color(color(palette.text_dim)),
                    )
                    .wrap(),
                );
                ui.end_row();
            }
        });
    ui.add_space(S[2]);
    picked
}

/// How a small button of the field is drawn.
#[derive(Clone, Copy)]
struct Look {
    /// Its icon filled, in the accent colour: the search is saved.
    filled: bool,
    enabled: bool,
    /// Drawn as under the pointer whether it is or not: its panel is open.
    lit: bool,
}

impl Look {
    fn of(open: bool) -> Self {
        Self {
            filled: false,
            enabled: true,
            lit: open,
        }
    }
}

/// A button inside the field. Answers whether it was pressed, which one that is not
/// enabled never is.
fn small_button(
    ui: &egui::Ui,
    rect: Rect,
    name: &str,
    label: &str,
    icon: Icon,
    look: Look,
    palette: &Palette,
) -> bool {
    let sense = if look.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui
        .interact(rect, ui.id().with(name), sense)
        .on_hover_text(label);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, look.enabled, label));
    let lit = look.lit || (look.enabled && response.hovered());
    if lit {
        ui.painter().rect_filled(rect, R[1], color(palette.hover));
    }
    let tint = if look.filled {
        palette.accent
    } else if lit {
        palette.text
    } else {
        palette.text_dim
    };
    icon.paint(ui, rect, ICON, look.filled, color(tint));
    look.enabled && response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Modifiers, PointerButton, Pos2, RawInput};

    /// The bar's room, as the shell gives it: right of the toggle, in a bar 46 points tall.
    const ROOM: Rect = Rect {
        min: pos2(46.0, 0.0),
        max: pos2(1234.0, 45.0),
    };
    const HELP: Pos2 = pos2(46.0 + FIELD_WIDTH - 4.0 - BUTTON / 2.0, 22.5);
    const SAVE: Pos2 = pos2(46.0 + FIELD_WIDTH - 4.0 - BUTTON - 2.0 - BUTTON / 2.0, 22.5);

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// The search bar in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        bar: SearchBar,
        text: String,
        saved_as: Option<String>,
        can_save: bool,
        in_search: bool,
        /// Another text field, under the bar, with the keyboard: a name being typed.
        other: Option<String>,
        time: f64,
        /// Every text the last frame drew, with where.
        texts: Vec<(String, Rect)>,
    }

    impl Fixture {
        fn new(text: &str) -> Self {
            Self {
                ctx: egui::Context::default(),
                bar: SearchBar::default(),
                text: text.to_owned(),
                saved_as: None,
                can_save: true,
                in_search: false,
                other: None,
                time: 0.0,
                texts: Vec::new(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<SearchAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = SearchBarData {
                saved_as: self.saved_as.as_deref(),
                can_save: self.can_save,
                in_search: self.in_search,
            };
            let (bar, text, other) = (&mut self.bar, &mut self.text, &mut self.other);
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        actions = bar.show(ui, ROOM, text, &data);
                        if let Some(typed) = other {
                            let at = Rect::from_min_size(pos2(400.0, 300.0), vec2(200.0, 24.0));
                            ui.put(at, egui::TextEdit::singleline(typed))
                                .request_focus();
                        }
                    });
            });
            full.textures_delta.clear();
            self.texts.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                    }
                });
            }
            actions
        }

        fn key_with(
            &mut self,
            key: Key,
            modifiers: Modifiers,
            typed: Option<&str>,
        ) -> Vec<SearchAction> {
            let mut events = vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }];
            events.extend(typed.map(|typed| Event::Text(typed.to_owned())));
            self.frame(events)
        }

        fn key(&mut self, key: Key) -> Vec<SearchAction> {
            let actions = self.key_with(key, Modifiers::NONE, None);
            self.key_up(key);
            actions
        }

        fn key_up(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn typed(&mut self, text: &str) -> Vec<SearchAction> {
            self.frame(vec![Event::Text(text.to_owned())])
        }

        fn click(&mut self, at: Pos2) -> Vec<SearchAction> {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let mut actions = self.frame(vec![Event::PointerMoved(at)]);
            actions.extend(self.frame(vec![button(true)]));
            actions.extend(self.frame(vec![button(false)]));
            actions
        }

        /// Ctrl+F, and the frame after it.
        fn find(&mut self) {
            self.key_with(Key::F, Modifiers::COMMAND, None);
            self.key_up(Key::F);
        }

        fn focused(&self) -> bool {
            self.ctx.memory(|memory| memory.has_focus(field_id()))
        }

        /// The characters selected in the field, as `(from, to)`.
        fn selection(&self) -> Option<(usize, usize)> {
            let state = egui::TextEdit::load_state(&self.ctx, field_id())?;
            let range = state.cursor.char_range()?;
            let [from, to] = range.sorted_cursors();
            Some((from.index.0, to.index.0))
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }

        /// Opens the panel and draws it until it stands still: a new area is laid out a
        /// frame before it shows, and its grids find their columns in the frames after.
        fn open_help(&mut self) {
            self.click(HELP);
            for _ in 0..4 {
                self.frame(Vec::new());
            }
            assert!(self.bar.help_open());
        }
    }

    #[test]
    fn the_empty_box_says_what_it_is_for_and_a_box_with_text_does_not() {
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        let hint = f.drew(PLACEHOLDER).expect("the placeholder is drawn");
        assert!(hint.left() >= 46.0 + TEXT_LEFT - 2.0 && hint.right() < 46.0 + FIELD_WIDTH);
        // In the middle of the field, not at its top.
        assert!((hint.center().y - 22.5).abs() < 2.5, "{hint:?}");
        f.text = "lake".to_owned();
        f.frame(Vec::new());
        assert!(f.drew(PLACEHOLDER).is_none());
        let typed = f.drew("lake").expect("the text is drawn");
        assert!((typed.center().y - 22.5).abs() < 2.5, "{typed:?}");
    }

    #[test]
    fn typing_in_the_box_changes_its_text_and_asks_for_the_search() {
        let mut f = Fixture::new("");
        f.find();
        assert!(f.focused());
        assert_eq!(f.typed("la"), [SearchAction::Typed]);
        assert_eq!(f.typed("ke"), [SearchAction::Typed]);
        assert_eq!(f.text, "lake");
        // A frame in which nothing is typed asks for nothing.
        assert_eq!(f.frame(Vec::new()), []);
    }

    // The search is already on its way - it runs as it is typed - so Enter only moves on
    // to its results, where the arrow keys are.
    #[test]
    fn enter_leaves_the_box_and_asks_for_nothing() {
        let mut f = Fixture::new("");
        f.find();
        f.typed("lake");
        assert_eq!(f.key(Key::Enter), []);
        assert!(!f.focused());
        assert_eq!(f.text, "lake");
    }

    #[test]
    fn escape_clears_a_box_that_holds_something_and_stays_in_it() {
        let mut f = Fixture::new("lake");
        f.find();
        assert_eq!(f.key(Key::Escape), [SearchAction::Clear]);
        assert_eq!(f.text, "");
        assert!(f.focused(), "the box is still where the keys go");
        // And typing goes on from there.
        f.typed("pond");
        assert_eq!(f.text, "pond");
    }

    // The box was emptied by a view switch and the user came back to a search by its row:
    // the box may be empty while the grid is a search. Escape still leaves the search.
    #[test]
    fn escape_in_an_empty_box_inside_a_search_clears_the_search() {
        let mut f = Fixture::new("");
        f.in_search = true;
        f.find();
        assert_eq!(f.key(Key::Escape), [SearchAction::Clear]);
    }

    // Clearing sends the empty query, which is All photos: from Starred, a key that
    // cleared nothing would throw the user out of the view. It only leaves the box.
    #[test]
    fn escape_in_an_empty_box_outside_a_search_clears_nothing_and_leaves_the_box() {
        let mut f = Fixture::new("");
        f.find();
        assert!(f.focused());
        assert_eq!(f.key(Key::Escape), []);
        assert!(!f.focused());
    }

    #[test]
    fn ctrl_f_puts_the_keys_in_the_box_with_what_it_holds_selected() {
        let mut f = Fixture::new("lake");
        f.frame(Vec::new());
        assert!(!f.focused());
        f.find();
        assert!(f.focused());
        assert_eq!(f.selection(), Some((0, 4)));
        // So typing replaces the last search.
        f.typed("pond");
        assert_eq!(f.text, "pond");
        // With Shift beside it, it is another chord.
        let mut f = Fixture::new("lake");
        f.key_with(Key::F, Modifiers::COMMAND | Modifiers::SHIFT, None);
        f.frame(Vec::new());
        assert!(!f.focused());
    }

    // The key was pressed to get to the box, not to search for a slash - and held, it
    // repeats.
    #[test]
    fn a_slash_puts_the_keys_in_the_box_and_is_not_typed_however_long_it_is_held() {
        let mut f = Fixture::new("lake");
        f.frame(Vec::new());
        assert_eq!(f.key_with(Key::Slash, Modifiers::NONE, Some("/")), []);
        assert!(f.focused());
        assert_eq!(f.selection(), Some((0, 4)));
        for _ in 0..5 {
            // egui works out for itself that a key going down again is a repeat.
            assert_eq!(f.key_with(Key::Slash, Modifiers::NONE, Some("/")), []);
        }
        assert_eq!(f.text, "lake");
        // Let go, a slash is a character again: a date, a path.
        f.key_up(Key::Slash);
        f.key(Key::End);
        assert_eq!(
            f.key_with(Key::Slash, Modifiers::NONE, Some("/")),
            [SearchAction::Typed]
        );
        assert_eq!(f.text, "lake/");
    }

    #[test]
    fn a_slash_typed_into_another_field_is_that_fields() {
        let mut f = Fixture::new("");
        f.other = Some("2024".to_owned());
        f.frame(Vec::new());
        f.frame(Vec::new());
        f.key_with(Key::Slash, Modifiers::NONE, Some("/"));
        f.key_up(Key::Slash);
        assert!(!f.focused());
        assert_eq!(f.other.as_deref(), Some("2024/"));
        assert_eq!(f.text, "");
        // And with Ctrl it is another key altogether.
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        f.key_with(Key::Slash, Modifiers::CTRL, None);
        f.frame(Vec::new());
        assert!(!f.focused());
    }

    #[test]
    fn the_help_lists_the_grammar_under_the_box() {
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        assert!(f.drew(LEAD).is_none());
        f.open_help();
        let lead = f.drew(LEAD).expect("the panel is drawn");
        assert!(lead.top() > 22.5 + FIELD_HEIGHT / 2.0, "under the box");
        for group in SEARCH_HELP {
            assert!(f.drew(group.title).is_some(), "{}", group.title);
            for entry in group.entries {
                assert!(f.drew(entry.text).is_some(), "{}", entry.text);
            }
        }
        // Two columns: the fourth group stands beside the first, not under the third.
        let (first, fourth) = (f.drew("Words").unwrap(), f.drew("What it is").unwrap());
        assert!(fourth.left() > first.left() + 200.0);
        assert!((fourth.top() - first.top()).abs() < 2.0);
        // The button closes it again.
        f.click(HELP);
        f.frame(Vec::new());
        assert!(!f.bar.help_open());
        assert!(f.drew(LEAD).is_none());
    }

    #[test]
    fn a_term_clicked_goes_into_the_box_as_though_typed_and_closes_the_panel() {
        let mut f = Fixture::new("lisbon");
        // The caret is somewhere of the user's choosing: here, the whole word selected.
        f.find();
        assert_eq!(f.selection(), Some((0, 6)));
        f.open_help();
        let term = f.drew("is:starred").unwrap().center();
        assert_eq!(f.click(term), [SearchAction::Typed]);
        assert_eq!(f.text, "lisbon is:starred");
        assert!(!f.bar.help_open());
        // The caret after it, in the box: a prefix is half a term, and the rest is typed.
        f.frame(Vec::new());
        assert!(f.focused());
        assert_eq!(f.selection(), Some((17, 17)));

        // A phrase left open is closed first, or the term would be read as part of it.
        let mut f = Fixture::new("\"summer hike");
        f.frame(Vec::new());
        f.open_help();
        let term = f.drew("camera:").unwrap().center();
        f.click(term);
        assert_eq!(f.text, "\"summer hike\" camera:");
    }

    // A word, a phrase, a date: the user's own is what belongs in the box.
    #[test]
    fn an_example_is_not_a_button() {
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        f.open_help();
        let sample = f.drew("lisbon tram").unwrap().center();
        assert_eq!(f.click(sample), []);
        assert_eq!(f.text, "");
        assert!(f.bar.help_open(), "a press inside the panel leaves it open");
    }

    #[test]
    fn a_press_outside_the_box_and_its_panel_closes_the_panel() {
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        f.open_help();
        // In the box itself it stays: the user is typing beside it.
        f.click(pos2(120.0, 22.5));
        assert!(f.bar.help_open());
        f.click(pos2(1200.0, 780.0));
        assert!(!f.bar.help_open());
    }

    // Escape closes the nearest thing: the panel, and the search stays.
    #[test]
    fn escape_with_the_panel_open_closes_the_panel_and_nothing_else() {
        let mut f = Fixture::new("lake");
        f.find();
        f.open_help();
        assert_eq!(f.key(Key::Escape), []);
        assert!(!f.bar.help_open());
        assert_eq!(f.text, "lake");
        // The second Escape is the box's.
        f.find();
        assert_eq!(f.key(Key::Escape), [SearchAction::Clear]);
    }

    #[test]
    fn the_bookmark_is_there_with_a_search_and_saves_it_once() {
        let mut f = Fixture::new("");
        f.frame(Vec::new());
        assert_eq!(f.click(SAVE), [], "nothing to save in an empty box");
        f.text = "  ".to_owned();
        assert_eq!(f.click(SAVE), [], "nor in one of spaces");
        f.text = "lake 2024".to_owned();
        assert_eq!(f.click(SAVE), [SearchAction::Save]);
        // Saved: filled, and it takes no press - a saved search is removed in the sidebar.
        f.saved_as = Some("lake 2024".to_owned());
        f.can_save = false;
        assert_eq!(f.click(SAVE), []);
    }
}
