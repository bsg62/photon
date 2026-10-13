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
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Event, Id, Key, LayerId, Modifiers, Order, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2, WidgetInfo, WidgetType, pos2, vec2,
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
/// The menu's area, by name.
const AREA: &str = "context-menu";
/// How far the area reaches beyond the menu drawn in it.
const RIM: f32 = 1.0;

/// Whether a button going down or coming up is the one that asks for the menu: the
/// other button, and on a Mac the first one with Control held, which is how a mouse
/// with one button asks - the system reports it as the first button, and the web view
/// opened its menu for it.
pub fn asks_for_menu(button: PointerButton, modifiers: Modifiers, mac: bool) -> bool {
    button == PointerButton::Secondary
        || (mac && button == PointerButton::Primary && modifiers.ctrl)
}

/// Where a button went down in this frame that asks for a menu, if one did: for what is
/// made of egui's own widgets, a row in the sidebar. (The grid reads its presses itself,
/// in their order with the first button's.)
pub fn menu_press(ui: &egui::Ui) -> Option<Pos2> {
    ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers,
            } if asks_for_menu(*button, *modifiers, cfg!(target_os = "macos")) => Some(*pos),
            _ => None,
        })
    })
}

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
    /// What the items were when the keys were last put somewhere: `active` is a number
    /// among them, and means nothing among others.
    items: u64,
}

/// A menu's items as one number: which can be picked, under which words.
fn signature<A>(items: &[Item<A>]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in items {
        if let Item::Pick { label, works, .. } = item {
            (label, works).hash(&mut hasher);
        }
    }
    hasher.finish()
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
        self.show_in(ui, at, items, palette(ui.ctx()))
    }

    /// The same, in `palette` whatever the window's theme: the viewer is dark in both,
    /// and so is its menu.
    pub fn show_in<A: Clone>(
        &mut self,
        ui: &egui::Ui,
        at: Pos2,
        items: &[Item<A>],
        palette: &Palette,
    ) -> Outcome<A> {
        let ctx = ui.ctx();
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
        // Never larger than the window, so that all of it can be placed on screen: what
        // is too tall is scrolled to, and words too long - an album's name is as long
        // as its owner likes - are cut short.
        let size = vec2(
            (inner + 2.0 * S[0]).min(window.width() - 2.0 * MENU_MARGIN),
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
        // The keys are on an item by its number, and the items are built again in
        // every frame: when they are other items - a grid took one of two photos, and
        // two star items became one - the number names another, and Enter would pick
        // what the keys were never taken to. They go back to the menu itself.
        let now = signature(items);
        if std::mem::replace(&mut self.items, now) != now {
            self.active = None;
        }

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
                // What a key types is the menu's too, and it types nothing. So are a
                // cut, a copy and a paste, which is what egui makes of their chords.
                Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_) => false,
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
        //
        // Outside is where egui says the menu's area is not the top one - the question
        // the grid asks of a press too (`GridView::take_pointer`), so the two cannot
        // come to different answers about one press: asked of this rectangle here and
        // of egui there, a press where the menu had opened was inside the menu and the
        // photo's under it as well, egui putting an area on whole points whatever it is
        // asked for.
        let layer = LayerId::new(Order::Foreground, Id::new(AREA));
        let on_menu = |at: Pos2| ctx.layer_id_at(at) == Some(layer);
        // (Asked after the input is let go of: egui's memory is another lock.)
        let pressed = ctx.input(|input| {
            (input.pointer.any_pressed())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let outside = pressed.is_some_and(|pointer| !on_menu(pointer));
        if outside {
            return Outcome::Closed;
        }

        // The wheel over a menu taller than the window scrolls the menu. (Nothing under
        // it does: what scrolls asks egui whether the pointer is over its own layer.)
        if ctx
            .input(|input| input.pointer.latest_pos())
            .is_some_and(on_menu)
        {
            self.scrolled -= ctx.input(|input| input.smooth_scroll_delta.y);
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
        // The area is a point larger all round than the menu drawn in it: the pointer
        // rests exactly where the menu opened, at its corner, and egui puts an area on
        // whole points - half a point away, the corner was outside the menu or inside
        // it by where on a point the pointer happened to be.
        egui::Area::new(Id::new(AREA))
            .order(Order::Foreground)
            .fixed_pos(rect.min - Vec2::splat(RIM))
            // There at once, as a menu is: egui fades a new area in over a few frames,
            // each of them asked for.
            .fade_in(false)
            .show(ctx, |ui| {
                let (ground, _) =
                    ui.allocate_exact_size(size + Vec2::splat(2.0 * RIM), Sense::hover());
                let menu = ground.shrink(RIM);
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
                            // By its words as well as its place: a press on an item
                            // that is another by the time the button comes up is no
                            // click of the new one.
                            let id = ui.id().with(("item", index, label));
                            let response = ui.interact(shown, id, Sense::CLICK);
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
    use crate::theme::tokens::DARK;
    use eframe::egui::RawInput;

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
        /// A button behind the menu, where it is, and whether the last frame clicked it.
        behind: Option<Rect>,
        behind_clicked: bool,
        /// Whether a cut, a copy or a paste was left in the frame's input.
        clipboard_left_over: bool,
        /// Drawn in the dark palette, whatever the window's theme.
        dark: bool,
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
                behind: None,
                behind_clicked: false,
                clipboard_left_over: false,
                dark: false,
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
            let (button, behind) = (self.button, self.behind);
            let mut outcome = Outcome::Open;
            let (mut left_over, mut typed, mut clipboard) = (Vec::new(), false, false);
            let mut behind_clicked = false;
            let dark = self.dark;
            let mut full = self.ctx.run_ui(input, |ui| {
                outcome = if dark {
                    view.show_in(ui, at, items, &DARK)
                } else {
                    view.show(ui, at, items)
                };
                if button {
                    let _ = ui.button("behind");
                }
                if let Some(behind) = behind {
                    behind_clicked = ui.put(behind, egui::Button::new("under")).clicked();
                }
                ui.input(|input| {
                    for event in &input.events {
                        match event {
                            Event::Key {
                                key, pressed: true, ..
                            } => left_over.push(*key),
                            Event::Text(_) => typed = true,
                            Event::Copy | Event::Cut | Event::Paste(_) => clipboard = true,
                            _ => {}
                        }
                    }
                });
            });
            self.behind_clicked = behind_clicked;
            self.clipboard_left_over = clipboard;
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

    // The wheel is the menu's only over the menu: beside it, it is whatever lies there.
    #[test]
    fn the_wheel_beside_a_menu_is_not_the_menus() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.items = (0..60)
            .map(|n| pick(&format!("Album {n}"), None, true, n))
            .collect();
        f.open();
        let first = f.drew("Album 0").unwrap();
        f.frame(vec![Event::PointerMoved(pos2(40.0, 300.0))]);
        for _ in 0..3 {
            f.frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -200.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
        }
        f.frame(Vec::new());
        assert_eq!(f.drew("Album 0").unwrap(), first);
    }

    // ---- from the review ----

    // The keys are on an item by its number among those that work, and the tile's menu
    // is built again in every frame: a grid that takes one of two selected photos turns
    // "Star 2 photos" and "Unstar 2 photos" into one item, and the number that was
    // "Hide 2 photos" is another item's. Enter picked what the keys were never taken
    // to. Items that are other items put the keys back on the menu itself.
    #[test]
    fn the_keys_go_back_to_the_menu_itself_when_its_items_become_others() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.items = vec![
            pick("Star 2 photos", Some("."), true, 1),
            pick("Unstar 2 photos", None, true, 2),
            pick("Hide 2 photos", Some("H"), true, 3),
            Item::Separator,
            pick("Lisbon", None, true, 5),
        ];
        f.open();
        f.key(Key::ArrowDown);
        f.key(Key::ArrowDown);
        f.key(Key::ArrowDown);
        assert_eq!(f.active().as_deref(), Some("Hide 2 photos"));
        // The same items drawn again are the same items.
        f.frame(Vec::new());
        assert_eq!(f.active().as_deref(), Some("Hide 2 photos"));
        // One of the two photos leaves the grid: the menu is of one photo now.
        f.items = vec![
            pick("Star photo", Some("."), true, 1),
            pick("Hide photo", Some("H"), true, 3),
            Item::Separator,
            pick("Lisbon", None, true, 5),
        ];
        assert_eq!(f.key(Key::Enter), Outcome::Open);
        assert_eq!(f.active(), None);
        // And an item that only stops working is another item too.
        f.key(Key::End);
        assert_eq!(f.active().as_deref(), Some("Lisbon"));
        f.items[3] = pick("Lisbon", None, false, 5);
        assert_eq!(f.key(Key::Enter), Outcome::Open);
        assert_eq!(f.active(), None);
    }

    // A press on an item that has become another between the button going down and
    // coming up is no pick of the new one.
    #[test]
    fn a_click_begun_on_an_item_that_is_replaced_under_it_picks_nothing() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        let hide = f.drew("Hide photo").unwrap().center();
        f.frame(vec![Event::PointerMoved(hide)]);
        f.press(hide, true);
        // What stands there now is another item.
        f.items[2] = pick("Hide 2 photos", Some("H"), true, 9);
        assert_eq!(f.press(hide, false), Outcome::Open);
    }

    // An album's name, or a person's in "Remove photo from", can be as long as it likes.
    #[test]
    fn a_menu_wider_than_the_window_is_held_to_it_and_its_words_cut_short() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        let long = "Summer in the mountains ".repeat(12);
        f.items = vec![
            pick(&long, Some("H"), true, 1),
            pick("Lisbon", None, true, 2),
        ];
        f.open();
        let menu = f.menu();
        let window = Rect::from_min_size(Pos2::ZERO, WINDOW).shrink(MENU_MARGIN - 0.5);
        assert!(window.contains_rect(menu), "{menu:?}");
        // Its key's hint is still at the trailing edge, inside the menu.
        let hint = f.drew("H").expect("the hint is drawn");
        assert!(hint.right() < menu.right() && hint.left() > menu.center().x);
        // And the words end before it.
        let words = f.drew(&long).expect("the item's words are drawn");
        assert!(words.left() > menu.left() && words.right() < hint.left());
    }

    // A button under the menu's edge is not pressed through it: an area takes the
    // clicks on its own ground. (What reads a press from the frame's events, as the
    // grid does, has to ask which layer is on top itself.)
    #[test]
    fn a_press_just_inside_the_menus_edge_is_not_a_buttons_under_it() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.behind = Some(Rect::from_min_size(pos2(280.0, 180.0), vec2(120.0, 60.0)));
        f.open();
        for at in [pos2(300.0, 200.0), pos2(302.0, 215.0), pos2(350.0, 202.0)] {
            assert_eq!(f.click(at), Outcome::Open, "{at:?}");
            assert!(!f.behind_clicked, "{at:?}");
        }
        // Beside the menu it is pressed as ever, and the menu closes at that press.
        let beside = pos2(290.0, 190.0);
        f.frame(vec![Event::PointerMoved(beside)]);
        assert_eq!(f.press(beside, true), Outcome::Closed);
    }

    // The pointer rests where the menu opened, and that place is the menu's, wherever
    // on a point it is and whichever way the menu stands from it: a click there closes
    // nothing and is nobody else's.
    #[test]
    fn the_place_a_menu_opened_at_is_inside_it() {
        for at in [
            pos2(300.0, 200.0),
            pos2(300.5, 200.5),
            pos2(300.4, 200.6),
            // Up and to the left of the pointer.
            pos2(900.5, 600.5),
            pos2(900.6, 600.4),
        ] {
            let mut f = Fixture::new(at);
            f.open();
            f.frame(vec![Event::PointerMoved(at)]);
            assert_eq!(f.press(at, true), Outcome::Open, "{at:?}");
            assert_eq!(f.press(at, false), Outcome::Open, "{at:?}");
        }
    }

    // Ctrl+C is no key to egui: it arrives as a copy.
    #[test]
    fn a_cut_a_copy_and_a_paste_are_the_menus_too() {
        let mut f = Fixture::new(pos2(300.0, 200.0));
        f.open();
        for event in [Event::Copy, Event::Cut, Event::Paste("lisbon".to_owned())] {
            assert_eq!(f.frame(vec![event]), Outcome::Open);
            assert!(!f.clipboard_left_over);
        }
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

    // A mouse with one button asks for the menu with Control held, and the system
    // reports that as the first button.
    #[test]
    fn on_a_mac_the_first_button_with_control_asks_for_the_menu() {
        let (first, other) = (PointerButton::Primary, PointerButton::Secondary);
        for mac in [true, false] {
            assert!(asks_for_menu(other, Modifiers::NONE, mac));
            assert!(asks_for_menu(other, Modifiers::CTRL, mac));
            assert!(!asks_for_menu(first, Modifiers::NONE, mac));
            assert!(!asks_for_menu(first, Modifiers::SHIFT, mac));
            assert!(!asks_for_menu(first, Modifiers::MAC_CMD, mac));
        }
        assert!(asks_for_menu(first, Modifiers::CTRL, true));
        // Elsewhere Control with a click takes a photo in or out of the selection.
        assert!(!asks_for_menu(first, Modifiers::CTRL, false));
    }

    // The viewer's menu is dark in a light window, as the viewer is: its ground, the
    // row under the pointer and its words are the dark palette's.
    #[test]
    fn a_menu_is_drawn_in_the_palette_it_is_given() {
        use crate::theme::tokens::{DARK, LIGHT};
        let mut f = Fixture::new(pos2(100.0, 100.0));
        f.ctx.set_theme(egui::Theme::Light);
        f.open();
        let fills = |f: &Fixture, tint| f.fills.iter().any(|(_, fill)| *fill == color(tint));
        assert!(fills(&f, LIGHT.raised) && !fills(&f, DARK.raised));
        f.dark = true;
        f.frame(Vec::new());
        assert!(fills(&f, DARK.raised) && !fills(&f, LIGHT.raised));
        f.key(Key::ArrowDown);
        f.frame(Vec::new());
        assert!(fills(&f, DARK.hover) && !fills(&f, LIGHT.hover));
    }
}
