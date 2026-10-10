//! The three controls at the right of the top bar, drawn: the sort with the toggle that
//! reverses it, the grouping, and the size of the tiles. What each offers and what a
//! choice makes of the sort is `controls.rs`.
//!
//! They hold nothing of their own: the sort drawn is the one the user is going to, so a
//! second change made before the first has landed builds on the first, and a refused one
//! puts the controls back.

use crate::{
    controls::{
        GROUPING_IDLE, GROUPINGS, SIZES, SORT_KEYS, grouping_applies, held, reversed,
        with_grouping, with_key,
    },
    icons::Icon,
    select_view::{self, SelectData, SelectView},
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{self, Align2, Rect, Sense, WidgetInfo, WidgetType, pos2, vec2};
use photon_core::{library::GridTile, sort::Sort};

/// The toggle that reverses the order: a button of the top bar, its icon smaller.
const BUTTON: f32 = 30.0;
const BUTTON_ICON: f32 = 14.0;
/// A segment of the size control: its words and this much room either side of them, in
/// a tray two points larger all round, with two points between segments.
const SEGMENT_PAD: f32 = 14.0;
const TRAY: f32 = 2.0;

pub struct ControlsData {
    /// The sort the user is going to.
    pub sort: Sort,
    pub size: GridTile,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlAction {
    /// The sort wanted: the one drawn, with one field of it changed.
    Sort(Sort),
    Size(GridTile),
}

#[derive(Default)]
pub struct ControlsBar {
    sort: SelectView,
    group: SelectView,
}

/// Where each control stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Places {
    pub sort: Rect,
    pub reverse: Rect,
    pub group: Rect,
    /// The size control's tray, and its three segments.
    pub tray: Rect,
    pub sizes: [Rect; 3],
}

fn labels<T>(options: &[(T, &'static str)]) -> Vec<&'static str> {
    options.iter().map(|(_, label)| *label).collect()
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), fonts::regular(T[2]), egui::Color32::WHITE)
        .size()
        .x
        .ceil()
}

/// Where the controls stand when the first begins at `left`, centred on `middle`. Fixed
/// by what they offer and not by what they hold, so the bar does not shift with a choice.
pub fn places(ui: &egui::Ui, left: f32, middle: f32) -> Places {
    let across = |left: f32, width: f32, height: f32| {
        Rect::from_min_size(pos2(left, middle - height / 2.0), vec2(width, height))
    };
    let sort = across(
        left,
        select_view::width(ui, &labels(&SORT_KEYS)),
        select_view::HEIGHT,
    );
    let reverse = across(sort.right() + S[0], BUTTON, BUTTON);
    let group = across(
        reverse.right() + S[1],
        select_view::width(ui, &labels(&GROUPINGS)),
        select_view::HEIGHT,
    );
    let mut at = group.right() + S[1] + TRAY;
    let sizes = SIZES.map(|(_, label)| {
        let segment = across(
            at,
            text_width(ui, label) + 2.0 * SEGMENT_PAD,
            select_view::HEIGHT - 2.0 * TRAY,
        );
        at = segment.right() + TRAY;
        segment
    });
    let tray = Rect::from_min_max(
        pos2(sizes[0].left() - TRAY, middle - select_view::HEIGHT / 2.0),
        pos2(sizes[2].right() + TRAY, middle + select_view::HEIGHT / 2.0),
    );
    Places {
        sort,
        reverse,
        group,
        tray,
        sizes,
    }
}

/// How wide the controls are together.
pub fn width(ui: &egui::Ui) -> f32 {
    places(ui, 0.0, 0.0).tray.right()
}

impl ControlsBar {
    /// Whether one of the two lists is open.
    pub fn open(&self) -> bool {
        self.sort.open() || self.group.open()
    }

    /// Draws the controls from `left`, centred on `middle`. Answers what the user chose.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        left: f32,
        middle: f32,
        data: &ControlsData,
    ) -> Vec<ControlAction> {
        let palette = palette(ui.ctx());
        let places = places(ui, left, middle);
        let mut actions = Vec::new();

        let chosen = self.sort.show(
            ui,
            places.sort,
            &SelectData {
                id: "sort",
                label: "Sort by",
                options: &labels(&SORT_KEYS),
                selected: held(&SORT_KEYS, data.sort.key),
                disabled: false,
                hint: None,
            },
        );
        if let Some(index) = chosen {
            actions.push(ControlAction::Sort(with_key(data.sort, index)));
        }
        if reverse(ui, places.reverse, data.sort.reverse, palette) {
            actions.push(ControlAction::Sort(reversed(data.sort)));
        }

        let applies = grouping_applies(data.sort);
        let chosen = self.group.show(
            ui,
            places.group,
            &SelectData {
                id: "group",
                label: "Group by",
                options: &labels(&GROUPINGS),
                selected: held(&GROUPINGS, data.sort.group),
                disabled: !applies,
                hint: (!applies).then_some(GROUPING_IDLE),
            },
        );
        if let Some(index) = chosen {
            actions.push(ControlAction::Sort(with_grouping(data.sort, index)));
        }

        ui.painter()
            .rect_filled(places.tray, R[2], color(palette.field));
        for ((size, label), rect) in SIZES.into_iter().zip(places.sizes) {
            if segment(ui, rect, label, size == data.size, palette) && size != data.size {
                actions.push(ControlAction::Size(size));
            }
        }
        actions
    }
}

/// The toggle that turns the order over, shown pressed while it is. Answers a press.
fn reverse(ui: &mut egui::Ui, rect: Rect, on: bool, palette: &Palette) -> bool {
    let response = ui
        .interact(rect, ui.id().with("reverse-order"), Sense::click())
        .on_hover_text("Reverse order");
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, "Reverse order"));
    // Pressed keeps its accent under the pointer, as the chosen size does.
    let (fill, tint) = if on {
        (Some(palette.accent), palette.on_accent)
    } else if response.hovered() {
        (Some(palette.hover), palette.text)
    } else {
        (None, palette.text_dim)
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, R[2], color(fill));
    }
    Icon::ArrowDownUp.paint(ui, rect, BUTTON_ICON, false, color(tint));
    response.clicked()
}

/// One segment of the size control. Answers a press.
fn segment(ui: &mut egui::Ui, rect: Rect, label: &str, chosen: bool, palette: &Palette) -> bool {
    let response = ui.interact(rect, ui.id().with(("tile-size", label)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, chosen, label));
    let (fill, tint) = if chosen {
        (Some(palette.accent), palette.on_accent)
    } else if response.hovered() {
        (Some(palette.hover), palette.text)
    } else {
        (None, palette.text)
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, R[1], color(fill));
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::regular(T[2]),
        color(tint),
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use photon_core::sort::{Grouping, SortKey};

    const LEFT: f32 = 300.0;
    const MIDDLE: f32 = 22.0;

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// The controls in a window of their own, a frame at a time. What they answer is
    /// applied as the application applies it.
    struct Fixture {
        ctx: egui::Context,
        bar: ControlsBar,
        sort: Sort,
        size: GridTile,
        time: f64,
        places: Option<Places>,
        texts: Vec<(String, Rect)>,
        /// The texts the last frame drew short of their end, for want of room.
        cut_short: Vec<String>,
        fills: Vec<(Rect, egui::Color32)>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut fixture = Self {
                ctx: egui::Context::default(),
                bar: ControlsBar::default(),
                sort: Sort::default(),
                size: GridTile::Medium,
                time: 0.0,
                places: None,
                texts: Vec::new(),
                cut_short: Vec::new(),
                fills: Vec::new(),
            };
            fixture.frame(Vec::new());
            fixture
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ControlAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = ControlsData {
                sort: self.sort,
                size: self.size,
            };
            let bar = &mut self.bar;
            let (mut actions, mut found) = (Vec::new(), None);
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        found = Some(places(ui, LEFT, MIDDLE));
                        actions = bar.show(ui, LEFT, MIDDLE, &data);
                    });
            });
            full.textures_delta.clear();
            self.places = found;
            self.texts.clear();
            self.cut_short.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                        if text.galley.elided {
                            self.cut_short.push(text.galley.text().to_owned());
                        }
                    }
                    egui::Shape::Rect(rect) => self.fills.push((rect.rect, rect.fill)),
                    _ => {}
                });
            }
            for action in &actions {
                match action {
                    ControlAction::Sort(sort) => self.sort = *sort,
                    ControlAction::Size(size) => self.size = *size,
                }
            }
            actions
        }

        /// A press and a release at `at`, and everything it did.
        fn click(&mut self, at: Pos2) -> Vec<ControlAction> {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let mut actions = self.frame(vec![Event::PointerMoved(at)]);
            actions.extend(self.frame(vec![button(true)]));
            actions.extend(self.frame(vec![button(false)]));
            actions.extend(self.frame(Vec::new()));
            actions
        }

        fn places(&self) -> Places {
            self.places.unwrap()
        }

        /// Where `text` is drawn inside `within`.
        fn drew_in(&self, text: &str, within: Rect) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, place)| drawn == text && within.contains(place.center()))
                .map(|(_, place)| *place)
        }

        /// Where `text` is drawn in an open list, under the bar.
        fn option(&self, text: &str) -> Pos2 {
            (self.texts.iter())
                .find(|(drawn, place)| drawn == text && place.top() > MIDDLE + 15.0)
                .map(|(_, place)| place.center())
                .unwrap_or_else(|| panic!("{text} is not in an open list: {:?}", self.texts))
        }

        fn filled(&self, rect: Rect) -> Option<egui::Color32> {
            (self.fills.iter())
                .find(|(filled, _)| *filled == rect)
                .map(|(_, fill)| *fill)
        }

        fn accent(&self) -> egui::Color32 {
            color(palette(&self.ctx).accent)
        }
    }

    const BY_MONTH_REVERSED: Sort = Sort {
        key: SortKey::Date,
        reverse: true,
        group: Grouping::Month,
    };

    #[test]
    fn the_controls_stand_in_a_row_and_say_what_the_sort_and_the_size_are() {
        let mut f = Fixture::new();
        let places = f.places();
        // In the Svelte bar's order, none over another, all on one line.
        let row = [places.sort, places.reverse, places.group, places.tray];
        assert_eq!(row[0].left(), LEFT);
        for pair in row.windows(2) {
            assert!(pair[0].right() < pair[1].left(), "{pair:?}");
        }
        for rect in row {
            assert_eq!(rect.center().y, MIDDLE);
        }
        assert!(f.drew_in("Date taken", places.sort).is_some());
        assert!(f.drew_in("By folder", places.group).is_some());
        for ((_, label), rect) in SIZES.into_iter().zip(places.sizes) {
            assert!(f.drew_in(label, rect).is_some(), "{label}");
            assert!(places.tray.contains_rect(rect));
        }
        // The size held is the segment filled, and no other; the order is not reversed.
        assert_eq!(f.filled(places.sizes[1]), Some(f.accent()));
        assert_eq!(f.filled(places.sizes[0]), None);
        assert_eq!(f.filled(places.reverse), None);

        f.sort = BY_MONTH_REVERSED;
        f.size = GridTile::Large;
        f.frame(Vec::new());
        assert!(f.drew_in("By month", places.group).is_some());
        assert_eq!(f.filled(places.reverse), Some(f.accent()));
        assert_eq!(f.filled(places.sizes[2]), Some(f.accent()));
        assert_eq!(f.filled(places.sizes[1]), None);
        // Nothing moved for it.
        assert_eq!(f.places(), places);
    }

    #[test]
    fn the_width_is_where_the_last_control_ends() {
        let f = Fixture::new();
        let mut wide = 0.0;
        let mut full = f.ctx.run_ui(RawInput::default(), |ui| wide = width(ui));
        full.textures_delta.clear();
        assert_eq!(LEFT + wide, f.places().tray.right());
        assert!(wide > 400.0 && wide < 620.0, "{wide}");
    }

    // Each control is as wide as the longest thing it offers: whatever it holds is
    // printed whole, and nothing in the bar moves for it.
    #[test]
    fn whatever_a_control_holds_is_printed_whole() {
        let mut f = Fixture::new();
        let places = f.places();
        for (key, (_, sorted)) in SORT_KEYS.iter().enumerate() {
            for (group, (_, grouped)) in GROUPINGS.iter().enumerate() {
                f.sort = with_grouping(with_key(Sort::default(), key), group);
                f.frame(Vec::new());
                assert_eq!(f.cut_short, Vec::<String>::new());
                assert!(f.drew_in(sorted, places.sort).is_some());
                assert!(f.drew_in(grouped, places.group).is_some());
                assert_eq!(f.places(), places);
            }
        }
    }

    // One field of the sort drawn, the other two as they were.
    #[test]
    fn a_key_chosen_keeps_the_direction_and_the_grouping() {
        let mut f = Fixture::new();
        f.sort = BY_MONTH_REVERSED;
        f.frame(Vec::new());
        f.click(f.places().sort.center());
        let name = f.option("Name");
        assert_eq!(
            f.click(name),
            [ControlAction::Sort(Sort {
                key: SortKey::Name,
                ..BY_MONTH_REVERSED
            })]
        );
        assert!(!f.bar.open());
    }

    #[test]
    fn the_toggle_turns_the_order_over_and_back() {
        let mut f = Fixture::new();
        f.sort = BY_MONTH_REVERSED;
        f.frame(Vec::new());
        let on = f.places().reverse.center();
        assert_eq!(
            f.click(on),
            [ControlAction::Sort(Sort {
                reverse: false,
                ..BY_MONTH_REVERSED
            })]
        );
        assert_eq!(f.click(on), [ControlAction::Sort(BY_MONTH_REVERSED)]);
    }

    #[test]
    fn a_grouping_chosen_keeps_the_key_and_the_direction() {
        let mut f = Fixture::new();
        f.sort = BY_MONTH_REVERSED;
        f.frame(Vec::new());
        f.click(f.places().group.center());
        let by_year = f.option("By year");
        assert_eq!(
            f.click(by_year),
            [ControlAction::Sort(Sort {
                group: Grouping::Year,
                ..BY_MONTH_REVERSED
            })]
        );
    }

    // Dimmed, not removed: the bar does not shift, and the grouping that comes back with
    // Date taken is still in sight.
    #[test]
    fn the_grouping_is_there_and_takes_no_press_under_a_sort_that_ignores_it() {
        let mut f = Fixture::new();
        f.sort = Sort {
            key: SortKey::Name,
            ..BY_MONTH_REVERSED
        };
        f.frame(Vec::new());
        let places = f.places();
        assert!(f.drew_in("Name", places.sort).is_some());
        assert!(f.drew_in("By month", places.group).is_some());
        assert_eq!(f.click(places.group.center()), []);
        assert!(!f.bar.open());
        // By date it opens again.
        f.sort = BY_MONTH_REVERSED;
        f.frame(Vec::new());
        f.click(places.group.center());
        assert!(f.bar.open());
    }

    // "Dimmed, and says why": under a pointer that rests on it.
    #[test]
    fn the_dimmed_grouping_says_why_under_a_resting_pointer() {
        let said = |f: &Fixture| f.texts.iter().any(|(text, _)| text == GROUPING_IDLE);
        let mut f = Fixture::new();
        f.sort = Sort {
            key: SortKey::Size,
            ..Sort::default()
        };
        f.frame(vec![Event::PointerMoved(f.places().group.center())]);
        for _ in 0..120 {
            f.frame(Vec::new());
        }
        assert!(said(&f), "{:?}", f.texts);

        // Where it applies there is nothing to explain.
        let mut f = Fixture::new();
        f.frame(vec![Event::PointerMoved(f.places().group.center())]);
        for _ in 0..120 {
            f.frame(Vec::new());
        }
        assert!(!said(&f));
    }

    #[test]
    fn a_segment_pressed_is_the_size_wanted_and_the_one_held_is_no_change() {
        let mut f = Fixture::new();
        let sizes = f.places().sizes;
        assert_eq!(
            f.click(sizes[2].center()),
            [ControlAction::Size(GridTile::Large)]
        );
        assert_eq!(f.click(sizes[2].center()), []);
        assert_eq!(
            f.click(sizes[0].center()),
            [ControlAction::Size(GridTile::Small)]
        );
        assert_eq!(f.sort, Sort::default(), "the size is no part of the sort");
    }

    // A press on the other control is a press outside the first one's list.
    #[test]
    fn one_list_is_open_at_a_time() {
        let mut f = Fixture::new();
        let places = f.places();
        f.click(places.sort.center());
        assert!(f.bar.sort.open());
        f.click(places.group.center());
        assert!(!f.bar.sort.open() && f.bar.group.open());
        f.click(places.sizes[0].center());
        assert!(!f.bar.open());
    }
}
