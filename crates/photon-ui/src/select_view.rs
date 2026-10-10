//! photon's own dropdown, drawn: the closed control with the option it holds, and its list
//! in an area of its own under it. `Select.svelte`, over `select.rs`.
//!
//! The control takes the keyboard and the list never does: the keys are read where the
//! focus is, and the list is only pointed at. A key the list used is taken out of the
//! frame's input - an arrow, a letter typed and the key that typed it - so that the grid
//! under it does not scroll by the same arrow, or hide a photo by the same letter.
//!
//! The keyboard is the control's for as long as the keyboard is what is being used. A
//! pointer that closes the list - by choosing from it, by pressing the control again, by
//! pressing anywhere else - gives the keyboard up, back to the photos: egui lets go of
//! the focus only at a click on something else, so a sort chosen with the mouse left
//! End, the arrows and every letter opening the list again from wherever the wheel had
//! scrolled to.

use crate::{
    icons::Icon,
    select::{Options, Select, SelectKey},
    text::paint_line,
    theme::{
        apply::{color, menu_shadow, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Event, EventFilter, Id, Key, Modifiers, Order, PointerButton, Rect, Sense, Stroke,
    StrokeKind, WidgetInfo, WidgetType, pos2, vec2,
};

/// The closed control, as tall as the top bar's buttons.
pub const HEIGHT: f32 = 30.0;
const ICON: f32 = 14.0;
/// A row of the list: its height, the room before its words, which the mark of the
/// option held stands in, and the air after them.
const OPTION: f32 = 28.0;
const OPTION_LEAD: f32 = S[1] + ICON + S[1];
const OPTION_TRAIL: f32 = 10.0;

pub struct SelectData<'a> {
    /// The control's own name among the window's: its focus and its list are kept by it.
    pub id: &'a str,
    /// What it is called to whoever reads the window without seeing it: "Sort by".
    pub label: &'a str,
    pub options: &'a [&'a str],
    pub selected: Option<usize>,
    /// Dimmed, and its list cannot be opened.
    pub disabled: bool,
    /// What it says under a resting pointer: why it is dimmed.
    pub hint: Option<&'a str>,
}

#[derive(Default)]
pub struct SelectView {
    select: Select,
    /// Where the list was last drawn, for a press outside it.
    list: Option<Rect>,
}

/// How wide the closed control is for these options: the longest of them, the chevron and
/// the room around both. Fixed by the options and not by the one held, so the bar does
/// not shift when another is chosen.
pub fn width(ui: &egui::Ui, options: &[&str]) -> f32 {
    (S[2] + widest(ui, options) + S[1] + ICON + S[1]).ceil()
}

/// How wide the longest of `options` is printed.
fn widest(ui: &egui::Ui, options: &[&str]) -> f32 {
    let font = fonts::regular(T[2]);
    (options.iter())
        .map(|option| {
            ui.painter()
                .layout_no_wrap((*option).to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max)
}

/// Where a key the control read came from, for taking it out of the frame's input.
enum Used {
    Key(Key, Modifiers),
    Typed(char),
}

/// The key that types `letter`, where egui knows one by it: a letter, a digit, the space
/// bar, the punctuation it names by its sign.
fn key_of(letter: char) -> Option<Key> {
    Key::from_name(&letter.to_uppercase().to_string())
}

impl SelectView {
    pub fn open(&self) -> bool {
        self.select.open()
    }

    /// Draws the control in `rect` and, while it is open, its list under it. Answers the
    /// option chosen, when it is another than the one held.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, data: &SelectData<'_>) -> Option<usize> {
        let palette = palette(ui.ctx());
        let options = Options {
            labels: data.options,
            selected: data.selected,
            disabled: data.disabled,
        };
        let id = Id::new(("select", data.id));
        // Dimmed, it is not a stop of the Tab key either: there is nothing to do on it.
        let sense = if data.disabled {
            Sense::hover()
        } else {
            Sense::click()
        };
        let mut response = ui.interact(rect, id, sense);
        response.widget_info(|| {
            let held = data.selected.and_then(|at| data.options.get(at)).copied();
            WidgetInfo::labeled(
                WidgetType::ComboBox,
                !data.disabled,
                format!("{}: {}", data.label, held.unwrap_or_default()),
            )
        });
        if let Some(hint) = data.hint {
            response = response.on_hover_text(hint);
        }
        let mut chosen = None;
        // A press - the pointer's, or the one a screen reader makes for its user - and
        // not the "click" egui makes of Enter or Space on what has the keyboard: those
        // are keys, read below, and counted twice they opened the list and closed it
        // again in one frame.
        let by_key = response.has_focus()
            && ui.input(|input| input.key_pressed(Key::Enter) || input.key_pressed(Key::Space));
        let pressed = response.clicked() && !by_key;
        if pressed {
            self.select.toggle(&options);
            if self.select.open() {
                // A press gives it the keyboard, as it does not by itself in egui: the
                // keys are read where the focus is, and a list whose control has lost
                // it closes.
                response.request_focus();
            } else if response.clicked_by(PointerButton::Primary) {
                // Opened and shut by the pointer: the keyboard is not left behind.
                response.surrender_focus();
            }
        }
        // The keys, while the control has the keyboard - and as it loses it, which is
        // when the Tab arrives that takes the option the list is on: egui has moved the
        // focus on by the time the control is drawn.
        if response.has_focus() || response.lost_focus() {
            chosen = self.take_keys(ui, id, &options).or(chosen);
        }
        // Without the keyboard an open list is closed: the focus has walked on, or a
        // click took it elsewhere. Not in the frame of a click on the list itself, which
        // is a click elsewhere to egui: the list has to be there for the click to choose
        // from, and choosing closes it.
        let click_on_list = ui
            .input(|input| {
                (input.pointer.any_click())
                    .then(|| input.pointer.interact_pos())
                    .flatten()
            })
            .is_some_and(|at| self.list.is_some_and(|list| list.contains(at)));
        if !response.has_focus() && self.select.open() && !click_on_list {
            self.select.close();
        }

        let open = self.select.open();
        let fill = if !data.disabled && (open || response.hovered()) {
            palette.field_hover
        } else {
            palette.field
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, R[2], color(fill));
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                R[2],
                Stroke::new(2.0, color(palette.accent)),
                StrokeKind::Inside,
            );
        }
        let tint = if data.disabled {
            palette.text_dim
        } else {
            palette.text
        };
        let chevron = Rect::from_center_size(
            pos2(rect.right() - S[1] - ICON / 2.0, rect.center().y),
            vec2(ICON, ICON),
        );
        Icon::ChevronDown.paint(ui, chevron, ICON, false, color(palette.text_dim));
        if let Some(held) = data.selected.and_then(|at| data.options.get(at)) {
            let left = rect.left() + S[2];
            paint_line(
                ui,
                &painter,
                (left, rect.center().y),
                chevron.left() - S[1] - left,
                held,
                fonts::regular(T[2]),
                color(tint),
            );
        }

        if open {
            // A press anywhere but the control and its list closes the list, at the press:
            // egui takes the keyboard from the control at a click elsewhere, which is the
            // button let go of, and never at a press that becomes a drag - of the
            // grid's scrollbar, say, with the list still open over the photos.
            let pressed = ui.input(|input| {
                (input.pointer.any_pressed())
                    .then(|| input.pointer.interact_pos())
                    .flatten()
            });
            let inside = |at| rect.contains(at) || self.list.is_some_and(|list| list.contains(at));
            if pressed.is_some_and(|at| !inside(at)) {
                self.select.close();
                // And the keyboard goes with the pointer.
                response.surrender_focus();
            }
        }
        if self.select.open() {
            chosen = self.list(ui, rect, data, &options, palette).or(chosen);
        } else {
            self.list = None;
        }
        chosen
    }

    /// Reads the keys of this frame for the control. One the list used is taken out of
    /// the input: the grid is drawn after the bar, and would scroll by the same arrow.
    fn take_keys(&mut self, ui: &egui::Ui, id: Id, options: &Options<'_>) -> Option<usize> {
        // The arrows that open the list and move through it are the control's, and not
        // egui's for moving the focus on; while the list is open so are the sideways
        // ones, which do nothing in it - left to egui they walked the keyboard to the
        // next control, and the list closed behind it - and Escape, which egui would
        // drop the focus for. Tab is left to egui: it chooses here and still has to move
        // the focus.
        let open = self.select.open();
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                EventFilter {
                    vertical_arrows: true,
                    horizontal_arrows: open,
                    escape: open,
                    ..EventFilter::default()
                },
            );
        });
        let now_ms = ui.input(|input| input.time) * 1000.0;
        let events: Vec<(SelectKey, Used)> = ui.input(|input| {
            (input.events.iter())
                .filter_map(|event| match event {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let named = match key {
                            Key::ArrowDown => SelectKey::ArrowDown,
                            Key::ArrowUp => SelectKey::ArrowUp { alt: modifiers.alt },
                            Key::Home => SelectKey::Home,
                            Key::End => SelectKey::End,
                            Key::PageDown => SelectKey::PageDown,
                            Key::PageUp => SelectKey::PageUp,
                            Key::Escape => SelectKey::Escape,
                            Key::Enter => SelectKey::Enter,
                            Key::Tab => SelectKey::Tab,
                            _ => return None,
                        };
                        Some((named, Used::Key(*key, *modifiers)))
                    }
                    // What is typed, for the type-ahead - a space among it. A shortcut
                    // types nothing, so Ctrl+A never arrives here.
                    Event::Text(typed) => {
                        let mut letters = typed.chars();
                        let letter = letters.next()?;
                        letters
                            .next()
                            .is_none()
                            .then_some((SelectKey::Char(letter), Used::Typed(letter)))
                    }
                    _ => None,
                })
                .collect()
        });
        let mut chosen = None;
        for (key, from) in events {
            let answer = self.select.key(key, now_ms, options);
            chosen = answer.chosen.or(chosen);
            if !answer.used {
                continue;
            }
            match from {
                Used::Key(key, modifiers) => {
                    ui.input_mut(|input| input.consume_key(modifiers, key));
                }
                // What was typed, and the key that typed it, whatever was held with it.
                Used::Typed(letter) => {
                    let typed_by = key_of(letter);
                    ui.input_mut(|input| {
                        input.events.retain(|event| match event {
                            Event::Text(typed) => !typed.chars().eq([letter]),
                            Event::Key {
                                key, pressed: true, ..
                            } => Some(*key) != typed_by,
                            _ => true,
                        });
                    });
                }
            }
        }
        chosen
    }

    /// The list, under the control. Answers the option clicked.
    fn list(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        data: &SelectData<'_>,
        options: &Options<'_>,
        palette: &Palette,
    ) -> Option<usize> {
        // As wide as the control at least, and as its longest option needs in a row: the
        // mark's room is more than the chevron's, and a list as wide as a control that
        // just fits its options printed the longest of them short.
        let needed = (OPTION_LEAD + widest(ui, data.options) + OPTION_TRAIL).ceil();
        let inner = (rect.width() - 2.0 * S[0]).max(needed);
        let size = vec2(
            inner + 2.0 * S[0],
            OPTION * data.options.len() as f32 + 2.0 * S[0],
        );
        let at = pos2(rect.left(), rect.bottom() + S[0]);
        let mut chosen = None;
        let mut hovered = None;
        egui::Area::new(Id::new(("select-list", data.id)))
            .order(Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                let (list, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter();
                painter.add(menu_shadow().as_shape(list, R[2]));
                painter.rect_filled(list, R[2], color(palette.raised));
                painter.rect_stroke(
                    list,
                    R[2],
                    Stroke::new(1.0, color(palette.line)),
                    StrokeKind::Outside,
                );
                for (index, option) in data.options.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(
                            list.left() + S[0],
                            list.top() + S[0] + OPTION * index as f32,
                        ),
                        vec2(inner, OPTION),
                    );
                    // Pressed, never focused: the keys are read on the control.
                    let response = ui.interact(row, ui.id().with(index), Sense::CLICK);
                    response.widget_info(|| {
                        WidgetInfo::selected(
                            WidgetType::SelectableLabel,
                            true,
                            data.selected == Some(index),
                            *option,
                        )
                    });
                    // The pointer over an option makes it the active one, as a native
                    // list does - when it moves, not while it rests where the list
                    // happened to open under it, or the keys could not leave that row.
                    let moved = ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO);
                    if response.hovered() && moved {
                        hovered = Some(index);
                    }
                    if response.clicked() {
                        chosen = Some(index);
                    }
                    if index == self.select.active() {
                        painter.rect_filled(row, R[1], color(palette.hover));
                    }
                    let mark = Rect::from_center_size(
                        pos2(row.left() + OPTION_LEAD / 2.0, row.center().y),
                        vec2(ICON, ICON),
                    );
                    if data.selected == Some(index) {
                        Icon::Check.paint(ui, mark, ICON, false, color(palette.accent));
                    }
                    let left = row.left() + OPTION_LEAD;
                    paint_line(
                        ui,
                        painter,
                        (left, row.center().y),
                        row.right() - OPTION_TRAIL - left,
                        option,
                        fonts::regular(T[2]),
                        color(palette.text),
                    );
                }
            });
        self.list = Some(Rect::from_min_size(at, size));
        if let Some(index) = hovered {
            self.select.hover(index, options);
        }
        chosen.and_then(|index| self.select.commit(index, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, RawInput};

    const LABELS: [&str; 4] = ["Date taken", "Date modified", "Name", "Size"];
    const AT: Rect = Rect {
        min: pos2(100.0, 8.0),
        max: pos2(240.0, 38.0),
    };

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// One select in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        view: SelectView,
        held: usize,
        at: Rect,
        disabled: bool,
        chosen: Vec<usize>,
        time: f64,
        texts: Vec<(String, Rect)>,
        /// The texts the last frame drew short of their end, for want of room.
        cut_short: Vec<String>,
        /// Whether a key of the last frame was still there for what is drawn after, and
        /// whether anything typed in it was: a letter, or the key that typed it.
        down_left: bool,
        typed_left: bool,
    }

    impl Fixture {
        fn new(held: usize) -> Self {
            Self {
                ctx: egui::Context::default(),
                view: SelectView::default(),
                held,
                at: AT,
                disabled: false,
                chosen: Vec::new(),
                time: 0.0,
                texts: Vec::new(),
                cut_short: Vec::new(),
                down_left: false,
                typed_left: false,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = SelectData {
                id: "sort",
                label: "Sort by",
                options: &LABELS,
                selected: Some(self.held),
                disabled: self.disabled,
                hint: None,
            };
            let (view, at) = (&mut self.view, self.at);
            let (mut chosen, mut down_left, mut typed_left) = (None, false, false);
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        chosen = view.show(ui, at, &data);
                        // What the grid would still see of the arrow.
                        down_left = ui.input(|input| input.key_pressed(Key::ArrowDown));
                        typed_left = ui.input(|input| {
                            let typed = |event: &Event| matches!(event, Event::Text(_));
                            input.events.iter().any(typed)
                                || input.key_pressed(Key::S)
                                || input.key_pressed(Key::Space)
                        });
                    });
            });
            full.textures_delta.clear();
            self.down_left = down_left;
            self.typed_left = typed_left;
            if let Some(index) = chosen {
                self.chosen.push(index);
                self.held = index;
            }
            self.texts.clear();
            self.cut_short.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                        if text.galley.elided {
                            self.cut_short.push(text.galley.text().to_owned());
                        }
                    }
                });
            }
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn click(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn drew(&self, text: &str) -> Vec<Rect> {
            (self.texts.iter())
                .filter(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
                .collect()
        }

        /// The middle of option `index` in the open list.
        fn option(&self, index: usize) -> Pos2 {
            pos2(
                AT.left() + 60.0,
                AT.bottom() + S[0] + S[0] + OPTION * (index as f32 + 0.5),
            )
        }
    }

    #[test]
    fn the_closed_control_shows_what_it_holds_and_a_click_opens_its_list_under_it() {
        let mut f = Fixture::new(2);
        f.frame(Vec::new());
        assert_eq!(f.drew("Name").len(), 1);
        assert!(f.drew("Size").is_empty(), "the list is closed");
        f.click(AT.center());
        assert!(f.view.open());
        for label in LABELS {
            let drawn = f.drew(label);
            let in_list = drawn.iter().filter(|at| at.top() > AT.bottom()).count();
            assert_eq!(in_list, 1, "{label}");
        }
        // Each option where its row is.
        let size = f
            .drew("Size")
            .into_iter()
            .find(|at| at.top() > AT.bottom())
            .unwrap();
        assert!((size.center().y - f.option(3).y).abs() < 3.0, "{size:?}");
        // The list is as wide as the control, which is wider than these options need.
        assert_eq!(f.view.list.unwrap().width(), AT.width());
        // The control again closes it, and the keyboard is not left behind in it: the
        // pointer opened the list and the pointer closed it, and End is the grid's.
        f.click(AT.center());
        assert!(!f.view.open());
        f.key(Key::End);
        assert!(!f.view.open());
    }

    #[test]
    fn an_option_clicked_is_chosen_and_the_list_closes() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        let at = f.option(3);
        f.click(at);
        assert_eq!(f.chosen, [3]);
        assert!(!f.view.open());
        // The pointer chose, and the keyboard is not left in the control: scrolled away
        // from with the wheel, End opened the list, and Enter after it sorted by size.
        f.key(Key::End);
        assert!(!f.view.open());
        assert_eq!(f.ctx.memory(|memory| memory.focused()), None);
        // The one held, clicked, is no change.
        f.click(AT.center());
        let at = f.option(3);
        f.click(at);
        assert_eq!(f.chosen, [3]);
        assert!(!f.view.open());
    }

    #[test]
    fn a_press_outside_the_control_and_its_list_closes_the_list() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        assert!(f.view.open());
        f.click(pos2(600.0, 400.0));
        assert!(!f.view.open());
        assert!(f.chosen.is_empty());
    }

    // The list is spared for the click that chooses from it, and for nothing else: a
    // pointer that merely rests on it does not hold it open when the keyboard goes
    // elsewhere - to the search box, by Ctrl+F.
    #[test]
    fn a_list_whose_control_lost_the_keyboard_closes_though_the_pointer_rests_on_it() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        assert!(f.view.open());
        let over = f.option(2);
        f.frame(vec![Event::PointerMoved(over)]);
        f.frame(Vec::new());
        assert!(f.view.open());
        // Something else takes the keyboard.
        f.ctx.memory_mut(|memory| {
            let control = memory.focused().expect("the control has the keyboard");
            memory.surrender_focus(control);
        });
        f.frame(Vec::new());
        assert!(!f.view.open());
        assert!(f.chosen.is_empty());
    }

    // At the press, not when the button is let go of: a press that becomes a drag is
    // never a click, and a list waiting for one stayed open over whatever was dragged.
    #[test]
    fn a_press_outside_closes_the_list_before_it_is_let_go() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        assert!(f.view.open());
        let outside = pos2(600.0, 400.0);
        f.frame(vec![Event::PointerMoved(outside)]);
        f.frame(vec![Event::PointerButton {
            pos: outside,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!f.view.open());
        // Held and dragged, it stays closed, and nothing was chosen.
        f.frame(vec![Event::PointerMoved(pos2(600.0, 100.0))]);
        assert!(!f.view.open());
        assert!(f.chosen.is_empty());
        // The keyboard went with the pointer, though no click has taken it: the arrows
        // do not open the list again under whatever is being dragged.
        f.key(Key::ArrowDown);
        assert!(!f.view.open());
    }

    // The keys are the control's while it has the keyboard, and the list's own while it
    // is open: an arrow that moved through the list must not scroll the grid as well.
    #[test]
    fn the_keys_move_through_the_list_and_do_not_reach_what_is_drawn_after() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        // The keyboard comes to the control by Tab.
        f.key(Key::Tab);
        assert!(!f.view.open());
        f.frame(vec![Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(f.view.open(), "the arrow opened the list");
        assert!(!f.down_left, "and was used up");
        f.key(Key::ArrowDown);
        f.key(Key::ArrowDown);
        f.key(Key::Enter);
        assert_eq!(f.chosen, [2]);
        assert!(!f.view.open());
    }

    #[test]
    fn escape_closes_the_list_without_choosing_and_typing_goes_to_an_option() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        f.key(Key::ArrowDown);
        f.key(Key::Escape);
        assert!(!f.view.open());
        assert!(f.chosen.is_empty());
        // A letter opens it on the option that begins so, and Enter takes it. The letter
        // is the list's, as an arrow is: neither it nor the key that typed it is left
        // for the photos, where a letter will hide one.
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![key(Key::S), Event::Text("s".to_owned())]);
        assert!(f.view.open());
        assert!(!f.typed_left);
        f.key(Key::Enter);
        assert_eq!(f.chosen, [3]);
        // A space in an open list takes the option it is on, and is used up too.
        f.key(Key::ArrowUp);
        f.key(Key::ArrowUp);
        f.frame(vec![key(Key::Space), Event::Text(" ".to_owned())]);
        assert_eq!(f.chosen, [3, 2]);
        assert!(!f.typed_left);
        // What a closed control without the keyboard does not use stays.
        let mut idle = Fixture::new(0);
        idle.frame(Vec::new());
        idle.frame(vec![key(Key::S), Event::Text("s".to_owned())]);
        assert!(idle.typed_left && !idle.view.open());
    }

    // Tab takes the option the list is on and lets the keyboard go on, as it does in a
    // native list. egui has taken the focus from the control by the time the control is
    // drawn in that frame: the key is read all the same, and the focus does not stop on a
    // row of the list on its way - the list is gone a frame later, and the focus with it.
    #[test]
    fn tab_takes_the_option_the_list_is_on_and_the_keyboard_moves_on() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.click(AT.center());
        f.key(Key::ArrowDown);
        f.key(Key::ArrowDown);
        let control = f.ctx.memory(|memory| memory.focused());
        assert!(control.is_some());
        f.frame(vec![Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert_eq!(f.chosen, [2]);
        assert!(!f.view.open());
        // Nothing else here takes the keyboard: it is the control's again, or nobody's,
        // and never a row's of a list that is no longer drawn.
        f.frame(Vec::new());
        f.frame(Vec::new());
        let now = f.ctx.memory(|memory| memory.focused());
        assert!(now.is_none() || now == control, "{now:?}");
    }

    // The pointer over an option makes it the one the list is on when it moves there, not
    // while it rests where the list happened to open: the keys could not leave that row.
    #[test]
    fn a_pointer_at_rest_over_the_list_does_not_hold_the_keys_to_its_row() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        // The keyboard in the control, the list closed, the pointer where its third row
        // will be, and at rest there.
        f.key(Key::Tab);
        let over = f.option(2);
        f.frame(vec![Event::PointerMoved(over)]);
        for _ in 0..20 {
            f.frame(Vec::new());
        }
        f.key(Key::ArrowDown);
        assert!(f.view.open());
        f.key(Key::ArrowDown);
        f.key(Key::Enter);
        assert_eq!(f.chosen, [1]);

        // Moved, the pointer takes the list with it - by the one move, in the frame it
        // is made: asked whether the pointer "is moving", egui wants several samples
        // of it, and a single step onto a row after a rest lit nothing.
        f.key(Key::ArrowDown);
        assert!(f.view.open());
        // A still window draws no frame: the next one is a second later.
        f.time += 1.0;
        f.frame(vec![Event::PointerMoved(over + vec2(3.0, OPTION))]);
        f.key(Key::Enter);
        assert_eq!(f.chosen, [1, 3]);
    }

    // Without a key of its own a control that is not focused leaves every key alone.
    #[test]
    fn a_control_without_the_keyboard_takes_no_key() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        f.frame(vec![Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!f.view.open());
        assert!(f.down_left, "the arrow is the grid's");
    }

    #[test]
    fn a_disabled_control_does_not_open() {
        let mut f = Fixture::new(1);
        f.disabled = true;
        f.frame(Vec::new());
        f.click(AT.center());
        assert!(!f.view.open());
        f.key(Key::ArrowDown);
        assert!(!f.view.open());
        // What it holds is still said.
        assert_eq!(f.drew("Date modified").len(), 1);
    }

    // The control is as wide as its longest option beside a chevron; a row of the list has
    // the mark's room before the option instead, which is more. A list as wide as the
    // control printed "Date modi…".
    #[test]
    fn every_option_is_printed_whole_in_the_list_of_a_control_at_its_own_width() {
        let mut f = Fixture::new(0);
        f.frame(Vec::new());
        let mut wide = 0.0;
        let mut full = f
            .ctx
            .run_ui(RawInput::default(), |ui| wide = width(ui, &LABELS));
        full.textures_delta.clear();
        f.at = Rect::from_min_size(AT.min, vec2(wide, HEIGHT));
        f.click(f.at.center());
        assert!(f.view.open());
        for label in LABELS {
            let in_list = f
                .drew(label)
                .into_iter()
                .filter(|at| at.top() > AT.bottom());
            assert_eq!(in_list.count(), 1, "{label} in {:?}", f.texts);
        }
        assert_eq!(f.cut_short, Vec::<String>::new());
    }

    // Tab stops on a control that can be opened, and passes one that cannot.
    #[test]
    fn a_disabled_control_is_no_stop_of_the_tab_key() {
        let focused = |f: &Fixture| f.ctx.memory(|memory| memory.focused());
        let mut f = Fixture::new(1);
        f.frame(Vec::new());
        f.key(Key::Tab);
        f.frame(Vec::new());
        assert!(focused(&f).is_some());

        let mut f = Fixture::new(1);
        f.disabled = true;
        f.frame(Vec::new());
        f.key(Key::Tab);
        f.frame(Vec::new());
        assert_eq!(focused(&f), None);
    }

    #[test]
    fn the_control_is_as_wide_as_its_longest_option_needs() {
        let ctx = egui::Context::default();
        let mut widths = (0.0, 0.0);
        let mut full = ctx.run_ui(RawInput::default(), |ui| {
            widths = (width(ui, &LABELS), width(ui, &["Name", "Size"]));
        });
        full.textures_delta.clear();
        assert!(widths.0 > widths.1 + 40.0, "{widths:?}");
        assert!(widths.1 > S[2] + ICON + 2.0 * S[1]);
    }
}
