//! Draws a context menu and turns the pointer and the keys into a pick: `Menu.svelte`.
//!
//! One menu serves the whole window, held open by the application, which calls this
//! **before anything else is drawn in the frame**. That order is the point: while a menu
//! is open the keys are its own, as a desktop's menus have them: it takes every key
//! press out of the frame's input before the search box, the sidebar or the grid can
//! read it - an `h` typed over an open menu hid the photos the menu was about, when the
//! Svelte one did not stop it. A chord too, where the Svelte menu lets one through to
//! the window: Ctrl+F put the caret in the search box behind the menu, where nothing
//! typed arrives, and Ctrl+A selected a folder the menu was not about. A key the
//! window is to answer whatever is open is read before the menu is shown.
//!
//! It is placed in every frame from its own size, which is worked out from its items
//! before it is drawn: a menu whose items change, or a window that is resized, needs
//! nothing more.

use crate::{
    menu::{Item, Look, MENU_MARGIN, MenuKey, menu_step, place_menu},
    text::paint_line,
    theme::{
        apply::{color, menu_shadow, palette},
        fonts,
        tokens::{R, S, T},
    },
};
use eframe::egui::{
    self, Event, Id, Key, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};

/// The least a menu is wide.
const MIN_WIDTH: f32 = 220.0;
/// An item that can be picked: its line, and six points over and under it.
const ROW: f32 = 28.0;
/// A separator's line, and the air either side of it.
const SEPARATOR: f32 = 1.0 + 2.0 * S[0];
/// A heading: air, a line, and its small words.
const HEADING: f32 = 27.0;
const NOTE: f32 = 25.0;
/// How far an item's words stand in from the menu's edge, and those under a heading.
const LEAD: f32 = 10.0;
const SUB_LEAD: f32 = 18.0;
/// The least room between an item's words and the key beside them.
const HINT_GAP: f32 = S[4];

/// What a frame of an open menu came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<A> {
    /// It is still open.
    Open,
    /// An item was picked: the menu is to close, and this is what the item does.
    Picked(A),
    /// It was closed without a pick: Escape, or a press outside it.
    Closed,
}

#[derive(Default)]
pub struct MenuView {
    /// The item the keys are on, counted among the items that work; none while they are
    /// on the menu itself, as when it has just opened.
    active: Option<usize>,
    /// How far a menu taller than the window is scrolled.
    scrolled: f32,
}

fn height<A>(item: &Item<A>) -> f32 {
    match item {
        Item::Pick { .. } => ROW,
        Item::Separator => SEPARATOR,
        Item::Heading(_) => HEADING,
        Item::Note(_) => NOTE,
    }
}

impl MenuView {
    /// Forgets where the keys were and how far it was scrolled: for a menu that has
    /// just been opened, or opened again somewhere else.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Draws `items` as a menu opened at `at`, and answers what the frame came to.
    pub fn show<A: Clone>(&mut self, ui: &egui::Ui, at: Pos2, items: &[Item<A>]) -> Outcome<A> {
        let ctx = ui.ctx();
        let palette = palette(ctx);
        let measure = |text: &str, size: f32| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), fonts::regular(size), egui::Color32::WHITE)
                .size()
                .x
        };
        let needs = |item: &Item<A>| match item {
            Item::Pick {
                label, hint, look, ..
            } => {
                let lead = if *look == Look::Sub { SUB_LEAD } else { LEAD };
                let hint = hint.map_or(0.0, |hint| HINT_GAP + measure(hint, T[1]));
                lead + measure(label, T[2]) + hint + LEAD
            }
            Item::Separator => 0.0,
            Item::Heading(heading) => LEAD + measure(heading, T[0]) + LEAD,
            Item::Note(note) => SUB_LEAD + measure(note, T[1]) + SUB_LEAD,
        };
        let inner = (items.iter().map(needs))
            .fold(MIN_WIDTH - 2.0 * S[0], f32::max)
            .ceil();
        let whole: f32 = items.iter().map(height).sum::<f32>() + 2.0 * S[0];
        let window = ctx.content_rect();
        // Never taller than the window, so that all of it can be placed on screen: what
        // does not fit is scrolled to.
        let size = vec2(
            inner + 2.0 * S[0],
            whole.min(window.height() - 2.0 * MENU_MARGIN),
        );
        let (x, y) = place_menu(
            (at.x - window.left(), at.y - window.top()),
            (size.x, size.y),
            (window.width(), window.height()),
            MENU_MARGIN,
        );
        let rect = Rect::from_min_size(window.min + vec2(x, y), size);
        let reach = (whole - size.y).max(0.0);

        // The items the keys stop at, by their place among all of them.
        let working: Vec<usize> = (items.iter().enumerate())
            .filter(|(_, item)| item.works())
            .map(|(index, _)| index)
            .collect();
        self.active = self.active.filter(|active| *active < working.len());

        // The keys, before anything else in the frame reads them.
        let mut pressed = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    // A chord steps and picks nothing: it is only kept from the rest.
                    if !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                        pressed.push(*key);
                    }
                    false
                }
                // What a key types is the menu's too, and it types nothing.
                Event::Text(_) => false,
                _ => true,
            });
        });
        // Tab is read by egui before anything is drawn, from the frame's raw input: it
        // would hand the keyboard to the first widget behind the menu.
        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        let mut picked = None;
        let mut stepped = false;
        for key in pressed {
            let step = match key {
                Key::ArrowDown => MenuKey::Down,
                Key::ArrowUp => MenuKey::Up,
                Key::Home => MenuKey::Home,
                Key::End => MenuKey::End,
                Key::Escape => return Outcome::Closed,
                Key::Enter | Key::Space => {
                    picked = self.active.map(|active| working[active]);
                    continue;
                }
                _ => continue,
            };
            self.active = menu_step(step, self.active, working.len());
            stepped = true;
        }

        // A press outside closes it, at the press and not at the click: a press that
        // becomes a drag is never a click, and the menu stayed open over a rubber band
        // that had selected other photos than the ones it was about. The press is left
        // for whatever it landed on.
        let outside = ctx.input(|input| {
            input.pointer.any_pressed()
                && (input.pointer.interact_pos()).is_some_and(|pointer| !rect.contains(pointer))
        });
        if outside {
            return Outcome::Closed;
        }

        // The wheel over a menu taller than the window scrolls the menu, and nothing
        // under it.
        let over = ctx.input(|input| {
            input
                .pointer
                .latest_pos()
                .is_some_and(|at| rect.contains(at))
        });
        if over {
            let turned = ctx.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta));
            self.scrolled -= turned.y;
        }
        // The item the keys went to is brought into sight.
        if stepped && let Some(active) = self.active {
            let top: f32 = items[..working[active]].iter().map(height).sum();
            self.scrolled = self
                .scrolled
                .min(top)
                .max(top + ROW - (size.y - 2.0 * S[0]));
        }
        self.scrolled = self.scrolled.clamp(0.0, reach);

        let moved = ctx.input(|input| input.pointer.delta() != Vec2::ZERO);
        let mut hovered = None;
        egui::Area::new(Id::new("context-menu"))
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            // There at once, as a menu is: egui fades a new area in over a few frames,
            // each of them asked for.
            .fade_in(false)
            .show(ctx, |ui| {
                let (menu, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter().clone();
                painter.add(menu_shadow().as_shape(menu, R[2]));
                painter.rect_filled(menu, R[2], color(palette.raised));
                // The hairline is what tells a white menu from a white grid.
                painter.rect_stroke(
                    menu,
                    R[2],
                    Stroke::new(1.0, color(palette.line)),
                    StrokeKind::Outside,
                );
                let inside = menu.shrink(S[0]);
                let painter = painter.with_clip_rect(inside.intersect(ui.clip_rect()));
                let mut top = inside.top() - self.scrolled;
                let mut place = 0;
                for (index, item) in items.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(inside.left(), top),
                        vec2(inside.width(), height(item)),
                    );
                    top += row.height();
                    match item {
                        Item::Pick {
                            label,
                            hint,
                            works,
                            look,
                            ..
                        } => {
                            let shown = row.intersect(inside);
                            // Pressed, never given the keyboard: the keys are the
                            // menu's because it is open.
                            let response =
                                ui.interact(shown, ui.id().with(("item", index)), Sense::CLICK);
                            response.widget_info(|| {
                                WidgetInfo::labeled(WidgetType::Button, *works, label)
                            });
                            if *works {
                                // The pointer makes an item the active one when it
                                // moves, not while it rests where the menu opened
                                // under it, or the keys could not leave that item.
                                if response.hovered() && moved {
                                    hovered = Some(place);
                                }
                                if response.clicked() {
                                    picked = Some(index);
                                }
                                if self.active == Some(place) {
                                    painter.rect_filled(row, R[1], color(palette.hover));
                                }
                                place += 1;
                            }
                            let tint = match (works, look) {
                                (false, _) => palette.text_dim,
                                (true, Look::Danger) => palette.danger,
                                (true, _) => palette.text,
                            };
                            let lead = if *look == Look::Sub { SUB_LEAD } else { LEAD };
                            let mut right = row.right() - LEAD;
                            if let Some(hint) = hint {
                                let drawn = painter.text(
                                    pos2(right, row.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    hint,
                                    fonts::regular(T[1]),
                                    color(palette.text_dim),
                                );
                                right = drawn.left() - S[1];
                            }
                            let left = row.left() + lead;
                            paint_line(
                                ui,
                                &painter,
                                (left, row.center().y),
                                right - left,
                                label,
                                fonts::regular(T[2]),
                                color(tint),
                            );
                        }
                        Item::Separator => {
                            painter.hline(
                                row.x_range(),
                                row.center().y,
                                (1.0, color(palette.line)),
                            );
                        }
                        Item::Heading(heading) => {
                            painter.hline(
                                row.x_range(),
                                row.top() + S[0] + 0.5,
                                (1.0, color(palette.line)),
                            );
                            painter.text(
                                pos2(row.left() + LEAD, row.bottom() - 2.0),
                                egui::Align2::LEFT_BOTTOM,
                                heading,
                                fonts::semibold(ui.ctx(), T[0]),
                                color(palette.text_dim),
                            );
                        }
                        Item::Note(note) => {
                            let left = row.left() + SUB_LEAD;
                            paint_line(
                                ui,
                                &painter,
                                (left, row.top() + S[0] + 7.5),
                                row.right() - SUB_LEAD - left,
                                note,
                                fonts::regular(T[1]),
                                color(palette.text_dim),
                            );
                        }
                    }
                }
            });
        if let Some(hovered) = hovered {
            self.active = Some(hovered);
        }
        match picked.map(|index| &items[index]) {
            Some(Item::Pick { does, .. }) => Outcome::Picked(does.clone()),
            _ => Outcome::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Modifiers, PointerButton, RawInput};

    const WINDOW: Vec2 = vec2(1000.0, 700.0);

    fn pick(label: &str, hint: Option<&'static str>, works: bool, does: u8) -> Item<u8> {
        Item::Pick {
            label: label.to_owned(),
            hint,
            works,
            look: Look::Plain,
            does,
        }
    }

    /// Five items that can be picked - the second of them not working - a separator
    /// and a heading among them.
    fn items() -> Vec<Item<u8>> {
        vec![
            pick("Star photo", Some("."), true, 1),
            pick("Compare photos", Some("C"), false, 2),
            pick("Hide photo", Some("H"), true, 3),
            Item::Separator,
            pick("Export photo…", None, true, 4),
            Item::Heading("Add to album"),
            Item::Pick {
                label: "Lisbon".to_owned(),
                hint: None,
                works: true,
                look: Look::Sub,
                does: 5,
            },
        ]
    }

    struct Fixture {
        ctx: egui::Context,
        view: MenuView,
        at: Pos2,
        items: Vec<Item<u8>>,
        size: Vec2,
        time: f64,
        /// Every text the last frame drew, with where, and every filled rectangle.
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, egui::Color32)>,
        /// The keys the frame's input still held after the menu had read it.
        left_over: Vec<Key>,
        typed_left_over: bool,
        repaint: std::time::Duration,
        /// Whether something that takes the keyboard is drawn behind the menu.
        button: bool,
    }

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    impl Fixture {
        fn new(at: Pos2) -> Self {
            Self {
                ctx: egui::Context::default(),
                view: MenuView::default(),
                at,
                items: items(),
                size: WINDOW,
                time: 0.0,
                texts: Vec::new(),
                fills: Vec::new(),
                left_over: Vec::new(),
                typed_left_over: false,
                repaint: std::time::Duration::MAX,
                button: false,
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Outcome<u8> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let (view, at, items) = (&mut self.view, self.at, &self.items);
            let button = self.button;
            let mut outcome = Outcome::Open;
            let (mut left_over, mut typed) = (Vec::new(), false);
            let mut full = self.ctx.run_ui(input, |ui| {
                outcome = view.show(ui, at, items);
                if button {
                    let _ = ui.button("behind");
                }
                ui.input(|input| {
                    for event in &input.events {
                        match event {
                            Event::Key {
                                key, pressed: true, ..
                            } => left_over.push(*key),
                            Event::Text(_) => typed = true,
                            _ => {}
                        }
                    }
                });
            });
            full.textures_delta.clear();
            self.left_over = left_over;
            self.typed_left_over = typed;
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
            outcome
        }

        /// The menu's first two frames: egui measures a new area in one frame and shows
        /// it in the next.
        fn open(&mut self) {
            self.frame(Vec::new());
            self.frame(Vec::new());
        }

        fn key(&mut self, key: Key) -> Outcome<u8> {
            self.key_with(key, Modifiers::NONE)
        }

        fn key_with(&mut self, key: Key, modifiers: Modifiers) -> Outcome<u8> {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        }

        /// The menu's own rectangle: the one filled in the raised colour.
        fn menu(&self) -> Rect {
            let raised = color(palette(&self.ctx).raised);
            (self.fills.iter())
                .find(|(_, fill)| *fill == raised)
                .map(|(rect, _)| *rect)
                .expect("the menu is drawn")
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }

        /// The label of the item drawn as the active one.
        fn active(&self) -> Option<String> {
            let hover = color(palette(&self.ctx).hover);
            let row = (self.fills.iter()).find(|(_, fill)| *fill == hover)?.0;
            (self.texts.iter())
                .find(|(_, place)| row.contains(place.center()) && place.left() < row.center().x)
                .map(|(text, _)| text.clone())
        }

        fn press(&mut self, at: Pos2, pressed: bool) -> Outcome<u8> {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        fn click(&mut self, at: Pos2) -> Outcome<u8> {
            self.frame(vec![Event::PointerMoved(at)]);
            self.press(at, true);
            self.press(at, false)
        }
    }

    #[test]
    fn a_menu_opens_down_and_to_the_right_of_the_pointer_with_its_items_in_order() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        assert_eq!(f.frame(Vec::new()), Outcome::Open);
        assert_eq!(f.frame(Vec::new()), Outcome::Open);
        let menu = f.menu();
        assert_eq!(menu.min, pos2(300.0, 200.0));
        assert!(menu.width() >= MIN_WIDTH);
        let tops: Vec<f32> = [
            "Star photo",
            "Compare photos",
            "Hide photo",
            "Export photo…",
            "Add to album",
            "Lisbon",
        ]
        .iter()
        .map(|label| {
            f.drew(label)
                .unwrap_or_else(|| panic!("{label} is drawn"))
                .top()
        })
        .collect();
        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
        // A key's hint is at the trailing edge of its item, on its line.
        let (item, hint) = (f.drew("Hide photo").unwrap(), f.drew("H").unwrap());
        assert!(hint.left() > item.right());
        assert!(hint.right() > menu.right() - 20.0);
        assert!((hint.center().y - item.center().y).abs() < 2.0);
        // An item under a heading is set in from it.
        assert!(f.drew("Lisbon").unwrap().left() > item.left() + 6.0);
    }

    #[test]
    fn near_an_edge_it_turns_to_the_other_side_and_stays_in_the_window() {
        for at in [
            pos2(990.0, 690.0),
            pos2(990.0, 10.0),
            pos2(10.0, 690.0),
            pos2(500.0, 600.0),
        ] {
            let mut f = Fixture::new(at);
            f.open();
            let menu = f.menu();
            let window = Rect::from_min_size(Pos2::ZERO, WINDOW).shrink(MENU_MARGIN - 0.5);
            assert!(window.contains_rect(menu), "{at:?}: {menu:?}");
        }
        // In the bottom right corner it is up and to the left of the pointer.
        let mut f = Fixture::new(pos2(900.0, 600.0));
        f.open();
        assert!((f.menu().max - pos2(900.0, 600.0)).length() < 1.0);
    }

    #[test]
    fn the_arrows_step_through_the_items_that_work_and_wrap() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        assert_eq!(f.active(), None);
        f.key(Key::ArrowDown);
        assert_eq!(f.active().as_deref(), Some("Star photo"));
        // Over the item that does not work.
        f.key(Key::ArrowDown);
        assert_eq!(f.active().as_deref(), Some("Hide photo"));
        f.key(Key::End);
        assert_eq!(f.active().as_deref(), Some("Lisbon"));
        f.key(Key::ArrowDown);
        assert_eq!(f.active().as_deref(), Some("Star photo"));
        f.key(Key::ArrowUp);
        assert_eq!(f.active().as_deref(), Some("Lisbon"));
        f.key(Key::Home);
        assert_eq!(f.active().as_deref(), Some("Star photo"));
    }

    #[test]
    fn enter_and_space_pick_the_item_the_keys_are_on() {
        for key in [Key::Enter, Key::Space] {
            let mut f = Fixture::new(pos2(300.0, 200.0));
            f.open();
            // With the keys on the menu itself there is nothing to pick.
            assert_eq!(f.key(key), Outcome::Open, "{key:?}");
            f.key(Key::ArrowDown);
            f.key(Key::ArrowDown);
            assert_eq!(f.key(key), Outcome::Picked(3), "{key:?}");
        }
    }

    #[test]
    fn escape_closes_it_without_a_pick() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        f.key(Key::ArrowDown);
        assert_eq!(f.key(Key::Escape), Outcome::Closed);
        assert_eq!(f.left_over, []);
    }

    // The window's own keys must not reach what lies behind an open menu: an `h` typed
    // over it hid the photos it was about.
    #[test]
    fn every_plain_key_is_the_menus_and_nothing_after_it_in_the_frame_reads_one() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        for key in [
            Key::H,
            Key::Period,
            Key::Slash,
            Key::ArrowLeft,
            Key::ArrowDown,
            Key::Enter,
            Key::Backspace,
            Key::F,
            Key::PageDown,
        ] {
            f.key(key);
            assert_eq!(f.left_over, [], "{key:?}");
            f.key_with(key, Modifiers::SHIFT);
            assert_eq!(f.left_over, [], "Shift+{key:?}");
        }
        f.frame(vec![Event::Text("h".to_owned())]);
        assert!(!f.typed_left_over);
    }

    // A chord too: Ctrl+F under a menu put the caret in the search box behind it, where
    // nothing typed arrives, and Ctrl+A selected a folder the menu was not about.
    #[test]
    fn a_chord_and_a_function_key_are_the_menus_too() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        for (key, held) in [
            (Key::B, Modifiers::COMMAND),
            (Key::A, Modifiers::CTRL),
            (Key::F, Modifiers::COMMAND),
            (Key::F4, Modifiers::ALT),
            (Key::F11, Modifiers::NONE),
        ] {
            assert_eq!(f.key_with(key, held), Outcome::Open, "{key:?}");
            assert_eq!(f.left_over, [], "{key:?}");
        }
        // And does nothing in the menu: it is only kept from the rest.
        f.key(Key::ArrowDown);
        for held in [Modifiers::COMMAND, Modifiers::CTRL, Modifiers::ALT] {
            assert_eq!(f.key_with(Key::ArrowDown, held), Outcome::Open);
            assert_eq!(f.active().as_deref(), Some("Star photo"), "{held:?}");
            assert_eq!(f.key_with(Key::Enter, held), Outcome::Open, "{held:?}");
            assert_eq!(f.key_with(Key::Escape, held), Outcome::Open, "{held:?}");
        }
    }

    // egui walks the keyboard to the next widget at Tab before anything is drawn, from
    // the frame's raw input: taking the key out of the frame is too late for that.
    #[test]
    fn tab_under_a_menu_gives_the_keyboard_to_nothing_behind_it() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.button = true;
        f.open();
        for held in [Modifiers::NONE, Modifiers::SHIFT] {
            f.key_with(Key::Tab, held);
            f.frame(Vec::new());
            assert_eq!(f.ctx.memory(|memory| memory.focused()), None, "{held:?}");
        }
    }

    #[test]
    fn a_click_on_an_item_picks_it_and_one_that_does_not_work_takes_no_press() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        let off = f.drew("Compare photos").unwrap().center();
        assert_eq!(f.click(off), Outcome::Open);
        let hide = f.drew("Hide photo").unwrap().center();
        assert_eq!(f.click(hide), Outcome::Picked(3));
        // On a separator, a heading and the menu's own edge: nothing, and it stays open.
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        let heading = f.drew("Add to album").unwrap().center();
        assert_eq!(f.click(heading), Outcome::Open);
        let edge = f.menu().min + vec2(1.0, 1.0);
        assert_eq!(f.click(edge), Outcome::Open);
    }

    // At the press, not at the click: a press that becomes a drag is never a click.
    #[test]
    fn a_press_outside_closes_it_at_the_press() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        let elsewhere = pos2(800.0, 500.0);
        f.frame(vec![Event::PointerMoved(elsewhere)]);
        assert_eq!(f.press(elsewhere, true), Outcome::Closed);
        // The other button too: a right-click elsewhere is another menu's.
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        f.frame(vec![Event::PointerMoved(elsewhere)]);
        let other = f.frame(vec![Event::PointerButton {
            pos: elsewhere,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert_eq!(other, Outcome::Closed);
    }

    // When it moves, not while it rests where the menu opened under it: there the keys
    // could not leave that item.
    #[test]
    fn the_pointer_makes_an_item_the_active_one_when_it_moves() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        // The pointer is where the menu was opened, and goes to an item.
        f.frame(vec![Event::PointerMoved(pos2(300.0, 200.0))]);
        assert_eq!(f.active(), None);
        let hide = f.drew("Hide photo").unwrap().center();
        f.frame(vec![Event::PointerMoved(hide)]);
        f.frame(Vec::new());
        assert_eq!(f.active().as_deref(), Some("Hide photo"));
        // It rests there, and the keys move on.
        f.key(Key::ArrowDown);
        f.frame(Vec::new());
        assert_eq!(f.active().as_deref(), Some("Export photo…"));
        // Over an item that does not work, nothing is lit.
        let off = f.drew("Compare photos").unwrap().center();
        f.frame(vec![Event::PointerMoved(off)]);
        f.frame(Vec::new());
        assert_eq!(f.active().as_deref(), Some("Export photo…"));
    }

    #[test]
    fn a_menu_taller_than_the_window_is_held_to_it_and_scrolls_to_the_item_the_keys_are_on() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.items = (0..60)
            .map(|n| pick(&format!("Album {n}"), None, true, n))
            .collect();
        f.open();
        let menu = f.menu();
        assert_eq!(menu.top(), MENU_MARGIN);
        assert_eq!(menu.bottom(), WINDOW.y - MENU_MARGIN);
        let inside = |f: &Fixture, label: &str| {
            let menu = f.menu();
            f.drew(label)
                .is_some_and(|place| place.top() >= menu.top() && place.bottom() <= menu.bottom())
        };
        assert!(inside(&f, "Album 0"));
        assert!(!inside(&f, "Album 59"));
        f.key(Key::End);
        f.frame(Vec::new());
        assert!(inside(&f, "Album 59"));
        assert_eq!(f.active().as_deref(), Some("Album 59"));
        assert!(!inside(&f, "Album 0"));
        f.key(Key::Home);
        f.frame(Vec::new());
        assert!(inside(&f, "Album 0"));
        // The wheel over it scrolls it, and is nobody's after that.
        f.frame(vec![Event::PointerMoved(menu.center())]);
        for _ in 0..30 {
            f.frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -200.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
        }
        assert!(inside(&f, "Album 59"));
    }

    #[test]
    fn items_that_change_under_it_keep_the_keys_on_an_item_there_is() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        f.key(Key::End);
        f.items.truncate(3);
        assert_eq!(f.frame(Vec::new()), Outcome::Open);
        // The keys are on the menu itself again, and Enter picks nothing.
        assert_eq!(f.active(), None);
        assert_eq!(f.key(Key::Enter), Outcome::Open);
    }

    #[test]
    fn an_open_menu_asks_for_no_frame() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.repaint, std::time::Duration::MAX);
    }

    #[test]
    fn opened_again_it_forgets_where_the_keys_were() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        f.key(Key::End);
        f.view.reset();
        f.at = pos2(600.0, 300.0);
        f.frame(Vec::new());
        assert_eq!(f.active(), None);
        assert_eq!(f.menu().min, pos2(600.0, 300.0));
    }
}
