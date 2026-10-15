//! Compare, drawn: two to four panes over the whole window, each a photo at the one zoom
//! and the one pan every pane shares, with what distinguishes it beneath - and the
//! window's input answered as `CompareAction`s. `Compare.svelte`, over `compare.rs`.
//!
//! **Dark in both themes**, as the viewer is: a photo is judged against the same ground
//! whatever the window's theme. **Tab moves between the panes**, and egui's own walk of
//! the keyboard is called off (`App::show_compare`): a message's button over the panes is
//! the one widget here that could take it.

use crate::{
    compare::{Facts, Pane},
    icons::Icon,
    text,
    theme::{
        apply::color,
        fonts,
        tokens::{DARK, R, S, SCRIM, T},
    },
    viewer_layout::{Frame, contained},
    viewer_levels::level_for,
    viewer_view::Level,
    viewer_zoom::{Pan, Zoom},
};
use eframe::egui::{
    self, Color32, CursorIcon, Event, Id, Key, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, WidgetInfo, WidgetType, pos2, vec2,
};

/// How much one notch of the wheel zooms. Smaller than the viewer's steps: the wheel is
/// the only zoom here, and overshooting the detail being compared costs the gesture.
pub const WHEEL_FACTOR: f32 = 1.15;
/// How many points of a trackpad's stream are one notch.
const POINTS_A_NOTCH: f32 = 100.0;
/// The button that closes, in the corner.
const CLOSE: f32 = 32.0;
/// What the button that closes is called.
pub const CLOSE_COMPARE: &str = "Close compare";

/// What one pane draws.
pub struct PaneData<'a> {
    pub pane: &'a Pane,
    pub facts: &'a Facts,
    pub preview: Option<&'a TextureHandle>,
    /// Its full picture's levels, largest first, while the zoom has left the fit.
    pub levels: &'a [Level<'a>],
}

pub struct CompareData<'a> {
    pub panes: Vec<PaneData<'a>>,
    /// The pane the keys act on.
    pub focus: usize,
}

/// What the user did in a frame of compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareAction {
    Close,
    /// A pane pressed, or its number typed: its keys.
    Focus(usize),
    /// Tab and Shift+Tab.
    Next,
    Prev,
    /// `S` or `.`: the star of the pane with the keys.
    Star,
    /// Enter: the pane with the keys, in the viewer.
    Open,
}

/// Where the panes stand in the window: two columns always - two panes side by side, three
/// and four a 2x2, three leaving the fourth place empty so that every photo is the same
/// size, which is the comparison.
pub fn places(window: Rect, count: usize) -> Vec<Rect> {
    let rows = count.div_ceil(2).max(1);
    let inner = window.shrink(S[0]);
    let width = (inner.width() - S[0]) / 2.0;
    let height = (inner.height() - S[0] * (rows - 1) as f32) / rows as f32;
    (0..count)
        .map(|n| {
            let (column, row) = ((n % 2) as f32, (n / 2) as f32);
            Rect::from_min_size(
                inner.min + vec2(column * (width + S[0]), row * (height + S[0])),
                vec2(width, height),
            )
        })
        .collect()
}

/// The first button held on a pane while the photos are zoomed: the pan, from where it
/// went down and from the pan as it was then.
#[derive(Clone, Copy, Debug)]
struct Held {
    at: Pos2,
    from: Pan,
}

#[derive(Default)]
pub struct CompareView {
    held: Option<Held>,
}

impl CompareView {
    /// Whether a pan is held: the keys are its, as a band's are the grid's.
    pub fn panning(&self) -> bool {
        self.held.is_some()
    }

    /// A pan held is let go of where it stands: a dialog has opened over it.
    fn let_go(&mut self) {
        self.held = None;
    }

    /// Draws compare over the whole window and answers what was done in it; the zoom
    /// and the pan are `zoom`'s, changed here by the wheel and a drag.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &CompareData<'_>,
        zoom: &mut Zoom,
        paused: bool,
    ) -> Vec<CompareAction> {
        let window = ui.ctx().content_rect();
        ui.painter().rect_filled(window, 0.0, color(DARK.surface));
        let places = places(window, data.panes.len());
        let size = places
            .first()
            .map_or((0.0, 0.0), |pane| (pane.width(), pane.height()));
        zoom.held_to(size);
        let close = Rect::from_min_size(
            pos2(window.right() - S[2] - CLOSE, window.top() + S[2]),
            vec2(CLOSE, CLOSE),
        );
        // The ground under the controls: a press beside the cross is not the cross's.
        ui.interact(window, Id::new("compare-ground"), Sense::CLICK);
        // Under a dialog nothing is read: the wheel would zoom the panes under the sheet,
        // and a drag held when it opened would go on panning them.
        let mut actions = if paused {
            self.let_go();
            Vec::new()
        } else {
            self.input(ui, &places, close, zoom, size)
        };

        for (number, (pane, place)) in data.panes.iter().zip(&places).enumerate() {
            self.pane(ui, pane, *place, number, number == data.focus, zoom);
        }

        let response = ui.interact(close, Id::new("compare-close"), Sense::CLICK);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, CLOSE_COMPARE));
        ui.painter()
            .circle_filled(close.center(), CLOSE / 2.0, color(SCRIM));
        let tint = if response.hovered() {
            color(DARK.text)
        } else {
            color(DARK.text_dim)
        };
        Icon::X.paint(ui, close, 16.0, false, tint);
        if response.clicked() {
            actions.push(CompareAction::Close);
        }
        actions
    }

    /// The frame's keys, its wheel and the first button, in the order they came.
    fn input(
        &mut self,
        ui: &egui::Ui,
        places: &[Rect],
        close: Rect,
        zoom: &mut Zoom,
        size: (f32, f32),
    ) -> Vec<CompareAction> {
        let mut actions = Vec::new();
        let (events, pointer) =
            ui.input(|input| (input.events.clone(), input.pointer.latest_pos()));
        // A point is on a pane where it is on no control and under nothing that lies over
        // compare - a message - asked of the layer egui finds on top there, the one judge
        // (`grid/view.rs`). (`layer_id_at` locks egui's memory: not inside `input`.)
        let on = |at: Pos2| {
            let top = ui.ctx().layer_id_at(at) == Some(ui.layer_id());
            (!close.contains(at) && top)
                .then(|| places.iter().position(|place| place.contains(at)))
                .flatten()
        };
        for event in &events {
            match event {
                // A pan held owns the keyboard: Escape lets go of it, and nothing else is
                // answered - compare outlives an abandoned drag.
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } if self.held.is_some() => self.held = None,
                Event::Key { pressed: true, .. } if self.held.is_some() => {}
                Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } => {
                    // Tab, with Shift or without, moves between the panes.
                    if *key == Key::Tab {
                        actions.push(if modifiers.shift {
                            CompareAction::Prev
                        } else {
                            CompareAction::Next
                        });
                        continue;
                    }
                    if modifiers.command || modifiers.ctrl || modifiers.alt || *repeat {
                        continue;
                    }
                    actions.extend(match key {
                        Key::Escape => Some(CompareAction::Close),
                        Key::Num1 => Some(CompareAction::Focus(0)),
                        Key::Num2 => Some(CompareAction::Focus(1)),
                        Key::Num3 => Some(CompareAction::Focus(2)),
                        Key::Num4 => Some(CompareAction::Focus(3)),
                        Key::S | Key::Period => Some(CompareAction::Star),
                        Key::Enter => Some(CompareAction::Open),
                        _ => None,
                    });
                }
                // The wheel zooms every pane about the point under the pointer, measured
                // from the middle of the pane it is over.
                Event::MouseWheel { unit, delta, .. } => {
                    let Some(at) = pointer else { continue };
                    let Some(over) = on(at) else { continue };
                    let notches = match unit {
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / POINTS_A_NOTCH,
                    };
                    let middle = places[over].center();
                    let point = Pan {
                        x: at.x - middle.x,
                        y: at.y - middle.y,
                    };
                    let moved = zoom.by(WHEEL_FACTOR.powf(notches), point, size);
                    // A pan held is measured from where it began: moved with the zoom.
                    if let Some(held) = self.held.as_mut() {
                        held.from.x += moved.x;
                        held.from.y += moved.y;
                    }
                }
                // A pinch the system reports as one (a Mac's trackpad), by its own factor,
                // about the same point.
                Event::Zoom(factor) => {
                    let Some(at) = pointer else { continue };
                    let Some(over) = on(at) else { continue };
                    let middle = places[over].center();
                    let point = Pan {
                        x: at.x - middle.x,
                        y: at.y - middle.y,
                    };
                    let moved = zoom.by(*factor, point, size);
                    if let Some(held) = self.held.as_mut() {
                        held.from.x += moved.x;
                        held.from.y += moved.y;
                    }
                }
                // A press on a pane gives it the keys, and on zoomed photos holds them to
                // be panned.
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    let Some(over) = on(*pos) else { continue };
                    actions.push(CompareAction::Focus(over));
                    self.held = (!zoom.is_fit()).then_some(Held {
                        at: *pos,
                        from: zoom.pan(),
                    });
                }
                Event::PointerMoved(to) => {
                    if let Some(held) = self.held {
                        let by = *to - held.at;
                        zoom.pan_to(held.from.x + by.x, held.from.y + by.y, size);
                    }
                }
                // The pan's endings: the button up, the window left - a button let go of
                // over another window may never be heard of - and a touch cancelled.
                Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                }
                | Event::WindowFocused(false)
                | Event::Touch {
                    phase: egui::TouchPhase::Cancel,
                    ..
                } => self.held = None,
                _ => {}
            }
        }
        if self.held.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if !zoom.is_fit() && pointer.is_some_and(|at| on(at).is_some()) {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        actions
    }

    /// One pane: its photo at the shared zoom and pan, cut off at its edge; the ring of
    /// the pane with the keys; its label.
    fn pane(
        &self,
        ui: &egui::Ui,
        data: &PaneData<'_>,
        place: Rect,
        number: usize,
        focused: bool,
        zoom: &Zoom,
    ) {
        let painter = ui.painter().with_clip_rect(place);
        painter.rect_filled(place, R[1], color(DARK.chrome));
        let texture = (data.levels.first().map(|level| level.texture)).or(data.preview);
        if let Some(texture) = texture {
            let [width, height] = texture.size();
            let frame = Frame {
                left: place.left(),
                top: place.top(),
                width: place.width(),
                height: place.height(),
            };
            let fitted = contained(width as f32, height as f32, frame);
            let fit = Rect::from_min_size(
                pos2(fitted.left, fitted.top),
                vec2(fitted.width, fitted.height),
            );
            let middle = place.center();
            let pan = vec2(zoom.pan().x, zoom.pan().y);
            let at = Rect::from_min_size(
                middle + pan + (fit.min - middle) * zoom.zoom(),
                fit.size() * zoom.zoom(),
            );
            // The level at least as large as what is drawn, where the full picture is
            // made; the preview until then.
            let texture = if data.levels.is_empty() {
                texture
            } else {
                let widths: Vec<u32> = data.levels.iter().map(|level| level.width).collect();
                let drawn = at.width() * ui.ctx().pixels_per_point();
                data.levels[level_for(&widths, drawn)].texture
            };
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), at, whole, Color32::WHITE);
        }
        // Inside the pane's own box: the panes stand close, and a ring outside one is
        // cut off by its neighbour.
        if focused {
            painter.rect_stroke(
                place,
                R[1],
                Stroke::new(2.0, color(DARK.accent)),
                StrokeKind::Inside,
            );
        }

        // The label, on the scrim every plate on a photo has: the pane's number - the key
        // that gives it the keys - its star, its name, and the facts that differ.
        let font = fonts::regular(T[1]);
        let tall = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font)) + 2.0 * S[0];
        let label = Rect::from_min_max(pos2(place.left(), place.bottom() - tall), place.max);
        painter.rect_filled(label, 0.0, color(SCRIM));
        let middle = label.center().y;
        let mut left = label.left() + S[1];
        let chip = Rect::from_min_size(pos2(left, middle - 8.0), vec2(16.0, 16.0));
        painter.rect_filled(chip, R[0], color(DARK.field));
        let index = (number + 1).to_string();
        left += chip.width() + S[1];
        let words = painter.layout_no_wrap(index.clone(), font.clone(), color(DARK.text_dim));
        painter.galley(
            chip.center() - words.size() / 2.0,
            words,
            color(DARK.text_dim),
        );
        if data.pane.starred {
            let star = Rect::from_min_size(pos2(left, middle - 7.0), vec2(14.0, 14.0));
            Icon::Star.paint(ui, star, 13.0, true, color(DARK.star));
            left += star.width() + S[1];
        }
        let facts: Vec<&str> = [data.facts.size.as_str(), data.facts.taken.as_str()]
            .into_iter()
            .filter(|fact| !fact.is_empty())
            .collect();
        let facts_width: f32 = (facts.iter())
            .map(|fact| {
                painter
                    .layout_no_wrap((*fact).to_owned(), font.clone(), color(DARK.text_dim))
                    .size()
                    .x
                    + S[1]
            })
            .sum();
        let room = (label.right() - S[1] - facts_width - left).max(0.0);
        let used = text::paint_line(
            ui,
            &painter,
            (left, middle),
            room,
            &data.pane.file_name,
            font.clone(),
            color(DARK.text),
        );
        left += used + S[1];
        for fact in facts {
            let words = painter.layout_no_wrap(fact.to_owned(), font.clone(), color(DARK.text_dim));
            let width = words.size().x;
            painter.galley(
                pos2(left, middle - words.size().y / 2.0),
                words,
                color(DARK.text_dim),
            );
            left += width + S[1];
        }
        // Told to whoever reads the window without seeing it: which pane, and whether it
        // has the keys.
        ui.interact(place, Id::new(("compare-pane", number)), Sense::hover())
            .widget_info(|| {
                WidgetInfo::selected(
                    WidgetType::Label,
                    true,
                    focused,
                    // What the pane says of itself: its star, and the facts that differ.
                    [
                        format!("Pane {}: {}", number + 1, data.pane.file_name),
                        if data.pane.starred {
                            "starred".to_owned()
                        } else {
                            String::new()
                        },
                        data.facts.size.clone(),
                        data.facts.taken.clone(),
                    ]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
                )
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{ColorImage, Modifiers, RawInput, TextureOptions};

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    const WINDOW: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(1000.0, 700.0),
    };

    struct Fixture {
        ctx: egui::Context,
        view: CompareView,
        panes: Vec<Pane>,
        previews: Vec<TextureHandle>,
        focus: usize,
        zoom: Zoom,
        time: f64,
        /// The pictures painted, each with where and what it was cut off at; the texts.
        images: Vec<(egui::TextureId, Rect, Rect)>,
        texts: Vec<(String, Rect)>,
        lines: Vec<(Rect, Color32)>,
        /// Which way each line was drawn: inside its rectangle or outside.
        kinds: Vec<StrokeKind>,
        /// The first pane's levels, by their sizes, where it has a full picture.
        levels: Vec<(TextureHandle, u32)>,
        cursor: CursorIcon,
    }

    fn pane(id: i64) -> Pane {
        Pane {
            id,
            file_name: format!("IMG_{id}.jpg"),
            width: 4000,
            height: 3000,
            orientation: 1,
            taken_at: 1_700_000_000,
            key: id as u64,
            starred: false,
        }
    }

    impl Fixture {
        fn new(count: usize) -> Self {
            let ctx = egui::Context::default();
            let previews = (0..count)
                .map(|n| {
                    ctx.load_texture(
                        format!("preview-{n}"),
                        ColorImage::filled([40, 30], Color32::GRAY),
                        TextureOptions::LINEAR,
                    )
                })
                .collect();
            let mut f = Self {
                ctx,
                view: CompareView::default(),
                panes: (1..=count as i64).map(pane).collect(),
                previews,
                focus: 0,
                zoom: Zoom::default(),
                time: 0.0,
                images: Vec::new(),
                texts: Vec::new(),
                lines: Vec::new(),
                kinds: Vec::new(),
                levels: Vec::new(),
                cursor: CursorIcon::Default,
            };
            f.frame(Vec::new());
            f
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<CompareAction> {
            let input = RawInput {
                screen_rect: Some(WINDOW),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let facts = crate::compare::differing_facts(&self.panes);
            let levels: Vec<Level<'_>> = (self.levels.iter())
                .map(|(texture, width)| Level {
                    texture,
                    width: *width,
                })
                .collect();
            let data = CompareData {
                panes: (self.panes.iter().enumerate())
                    .map(|(n, pane)| PaneData {
                        pane,
                        facts: &facts[n],
                        preview: Some(&self.previews[n]),
                        levels: if n == 0 { &levels } else { &[] },
                    })
                    .collect(),
                focus: self.focus,
            };
            let (view, zoom) = (&mut self.view, &mut self.zoom);
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| actions = view.show(ui, &data, zoom, false));
            });
            full.textures_delta.clear();
            self.cursor = full.platform_output.cursor_icon;
            self.kinds.clear();
            self.images.clear();
            self.texts.clear();
            self.lines.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => self
                        .texts
                        .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                    egui::Shape::Rect(rect) => {
                        if let Some(brush) = &rect.brush {
                            self.images
                                .push((brush.fill_texture_id, rect.rect, clipped.clip_rect));
                        } else if rect.stroke.width > 0.0 {
                            self.lines.push((rect.rect, rect.stroke.color));
                            self.kinds.push(rect.stroke_kind);
                        }
                    }
                    egui::Shape::Mesh(mesh) => {
                        self.images
                            .push((mesh.texture_id, mesh.calc_bounds(), clipped.clip_rect));
                    }
                    _ => {}
                });
            }
            for action in &actions {
                if let CompareAction::Focus(n) = action {
                    self.focus = *n;
                }
            }
            actions
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<CompareAction> {
            let event = |pressed| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            };
            let actions = self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
            actions
        }

        fn button(&mut self, at: Pos2, pressed: bool) -> Vec<CompareAction> {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        /// Where the preview of pane `n` was drawn.
        fn drawn(&self, n: usize) -> Rect {
            let id = self.previews[n].id();
            (self.images.iter()).find(|(of, ..)| *of == id).unwrap().1
        }
    }

    // Two panes side by side, three and four a 2x2 with the fourth place empty: every
    // photo the same size.
    #[test]
    fn the_panes_stand_in_two_columns_all_one_size() {
        let two = places(WINDOW, 2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].top(), two[1].top());
        assert!((two[0].height() - (700.0 - 2.0 * S[0])).abs() < 0.01);
        let three = places(WINDOW, 3);
        assert_eq!(three[2].left(), three[0].left());
        assert!(three[2].top() > three[0].bottom());
        assert!(three.iter().all(|place| place.size() == three[0].size()));
        assert!(two[1].left() - two[0].right() > 0.0, "a gap between");
    }

    // Each pane draws its photo fitted inside it, cut off at its edge, and its name.
    #[test]
    fn each_pane_draws_its_photo_fitted_and_its_name() {
        let f = Fixture::new(3);
        let places = places(WINDOW, 3);
        for (n, place) in places.iter().enumerate() {
            let at = f.drawn(n);
            assert!(
                place.contains_rect(at.shrink(0.5)),
                "{n}: {at:?} in {place:?}"
            );
            assert!((at.center() - place.center()).length() < 0.5);
            assert!(
                f.texts
                    .iter()
                    .any(|(text, _)| *text == format!("IMG_{}.jpg", n + 1))
            );
        }
        let clip = (f.images.iter())
            .find(|(of, ..)| *of == f.previews[1].id())
            .unwrap()
            .2;
        assert_eq!(clip, places[1]);
    }

    // The keys: the numbers and Tab move the keys between the panes, `S` and `.` star the
    // one with them, Enter opens it, Escape closes compare. A chord is not compare's.
    #[test]
    fn the_keys_of_compare() {
        let mut f = Fixture::new(3);
        let none = Modifiers::NONE;
        assert_eq!(f.key(Key::Num3, none), [CompareAction::Focus(2)]);
        assert_eq!(f.key(Key::Num4, none), [CompareAction::Focus(3)]);
        assert_eq!(f.key(Key::Tab, none), [CompareAction::Next]);
        assert_eq!(f.key(Key::Tab, Modifiers::SHIFT), [CompareAction::Prev]);
        assert_eq!(f.key(Key::S, none), [CompareAction::Star]);
        assert_eq!(f.key(Key::Period, none), [CompareAction::Star]);
        assert_eq!(f.key(Key::Enter, none), [CompareAction::Open]);
        assert_eq!(f.key(Key::Escape, none), [CompareAction::Close]);
        assert_eq!(f.key(Key::S, Modifiers::CTRL), []);
    }

    // The pane with the keys has its ring, inside its own box.
    #[test]
    fn the_pane_with_the_keys_has_its_ring() {
        let mut f = Fixture::new(2);
        let ring = |f: &Fixture| {
            (f.lines.iter())
                .filter(|(_, line)| *line == color(DARK.accent))
                .map(|(at, _)| *at)
                .collect::<Vec<_>>()
        };
        assert_eq!(ring(&f), [places(WINDOW, 2)[0]]);
        f.focus = 1;
        f.frame(Vec::new());
        assert_eq!(ring(&f), [places(WINDOW, 2)[1]]);
    }

    // The wheel zooms every pane at once, about the point under the pointer in the pane
    // it is over; a press on a pane gives it the keys.
    #[test]
    fn the_wheel_zooms_every_pane_and_a_press_gives_a_pane_the_keys() {
        let mut f = Fixture::new(2);
        let places = places(WINDOW, 2);
        let at = places[1].center() + vec2(50.0, 20.0);
        f.frame(vec![Event::PointerMoved(at)]);
        f.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 2.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        assert!((f.zoom.zoom() - WHEEL_FACTOR.powi(2)).abs() < 1e-4);
        let (zero, one) = (f.drawn(0), f.drawn(1));
        assert!((zero.width() - one.width()).abs() < 0.01, "one zoom");
        assert!(zero.width() > places[0].width());
        // The point under the pointer stays where it was.
        let before = f.zoom.pan();
        assert!(before.x < 0.0 && before.y < 0.0, "{before:?}");
        assert_eq!(
            f.button(places[1].center(), true),
            [CompareAction::Focus(1)]
        );
        f.button(places[1].center(), false);
    }

    // A drag on zoomed photos moves every one of them; at fit there is nothing to move.
    // Its endings: the button up, the window left, a touch cancelled, Escape - which lets
    // go of the drag and closes nothing.
    #[test]
    fn a_drag_pans_every_pane_and_ends_four_ways() {
        let mut f = Fixture::new(2);
        let middle = places(WINDOW, 2)[0].center();
        f.button(middle, true);
        assert!(
            !f.view.panning(),
            "a press at fit holds nothing, nor the keys"
        );
        f.frame(vec![Event::PointerMoved(middle + vec2(40.0, 0.0))]);
        f.button(middle + vec2(40.0, 0.0), false);
        assert_eq!(f.zoom.pan(), Pan::default(), "nothing to move at fit");
        f.zoom.by(
            3.0,
            Pan::default(),
            (places(WINDOW, 2)[0].width(), places(WINDOW, 2)[0].height()),
        );
        for ending in [
            vec![Event::PointerButton {
                pos: middle,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            vec![Event::WindowFocused(false)],
            vec![Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(0),
                phase: egui::TouchPhase::Cancel,
                pos: middle,
                force: None,
            }],
            vec![Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        ] {
            let pan = f.zoom.pan();
            f.button(middle, true);
            assert!(f.view.panning());
            f.frame(vec![Event::PointerMoved(middle + vec2(30.0, 10.0))]);
            let moved = f.zoom.pan();
            assert!((moved.x - pan.x - 30.0).abs() < 0.01, "{moved:?}");
            assert_eq!(f.frame(ending), [], "no action of its own");
            assert!(!f.view.panning());
            f.frame(vec![Event::PointerMoved(middle + vec2(90.0, 10.0))]);
            assert_eq!(f.zoom.pan(), moved, "let go of");
            f.frame(vec![Event::PointerMoved(middle)]);
            f.frame(vec![Event::WindowFocused(true)]);
        }
    }

    // While a pan is held the keys are its: Escape lets go of it, and nothing else is
    // answered.
    #[test]
    fn a_held_pan_owns_the_keys() {
        let mut f = Fixture::new(2);
        let place = places(WINDOW, 2)[0];
        f.zoom
            .by(3.0, Pan::default(), (place.width(), place.height()));
        f.button(place.center(), true);
        assert_eq!(f.key(Key::Num2, Modifiers::NONE), []);
        assert_eq!(f.key(Key::S, Modifiers::NONE), []);
        assert_eq!(f.key(Key::Escape, Modifiers::NONE), []);
        assert!(!f.view.panning());
        assert_eq!(f.key(Key::Escape, Modifiers::NONE), [CompareAction::Close]);
    }

    // The cross closes, and a press beside it is not its.
    #[test]
    fn the_cross_closes() {
        let mut f = Fixture::new(2);
        let close = pos2(1000.0 - S[2] - CLOSE / 2.0, S[2] + CLOSE / 2.0);
        f.frame(vec![Event::PointerMoved(close)]);
        // On the cross, over a pane: the cross's, and no pane's keys.
        assert_eq!(f.button(close, true), []);
        assert_eq!(f.button(close, false), [CompareAction::Close]);
        let beside = close + vec2(-CLOSE / 2.0 - 3.0, 0.0);
        f.frame(vec![Event::PointerMoved(beside)]);
        f.button(beside, true);
        assert!(!f.button(beside, false).contains(&CompareAction::Close));
    }

    // Only what the panes disagree about is said under them.
    #[test]
    fn the_facts_that_differ_are_said_under_every_pane() {
        let mut f = Fixture::new(2);
        assert!(!f.texts.iter().any(|(text, _)| text.contains(" × ")));
        f.panes[1].width = 2000;
        f.panes[1].height = 1500;
        f.frame(Vec::new());
        assert!(f.texts.iter().any(|(text, _)| text == "4000 × 3000"));
        assert!(f.texts.iter().any(|(text, _)| text == "2000 × 1500"));
    }

    fn wheel(f: &mut Fixture, unit: egui::MouseWheelUnit, y: f32) -> Vec<CompareAction> {
        f.frame(vec![Event::MouseWheel {
            unit,
            delta: vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }])
    }

    // A key held down is answered once: each press of `S` is a write.
    #[test]
    fn a_held_key_is_answered_once() {
        let mut f = Fixture::new(2);
        let event = |repeat| Event::Key {
            key: Key::S,
            physical_key: Some(Key::S),
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![event(false)]), [CompareAction::Star]);
        assert_eq!(f.frame(vec![event(true)]), []);
    }

    // A zoom made under a held drag moves the drag's origin along: the next move of the
    // pointer goes on from where the zoom left the photos.
    #[test]
    fn a_zoom_during_a_drag_moves_the_drag_along() {
        let mut f = Fixture::new(2);
        let place = places(WINDOW, 2)[0];
        f.zoom
            .by(2.0, Pan::default(), (place.width(), place.height()));
        // Off the middle, where a zoom moves the pan.
        let at = place.center() + vec2(60.0, 30.0);
        f.frame(vec![Event::PointerMoved(at)]);
        f.button(at, true);
        wheel(&mut f, egui::MouseWheelUnit::Line, 1.0);
        let zoomed = f.zoom.pan();
        assert!(zoomed.x.abs() > 1.0, "the zoom moved the pan: {zoomed:?}");
        f.frame(vec![Event::PointerMoved(at + vec2(5.0, 0.0))]);
        assert!(
            (f.zoom.pan().x - zoomed.x - 5.0).abs() < 0.01,
            "{:?}",
            f.zoom.pan()
        );
    }

    // A drag holds the photos at any zoom past the fit, the least included.
    #[test]
    fn a_drag_pans_at_any_zoom_past_the_fit() {
        let mut f = Fixture::new(2);
        let place = places(WINDOW, 2)[0];
        f.frame(vec![Event::PointerMoved(place.center())]);
        wheel(&mut f, egui::MouseWheelUnit::Line, 1.0);
        f.button(place.center(), true);
        assert!(f.view.panning());
    }

    // The hand over a pane only where there is something to move: at fit, none; zoomed, an
    // open hand, and a closed one while held.
    #[test]
    fn the_hand_is_shown_only_over_photos_that_can_be_moved() {
        let mut f = Fixture::new(2);
        let middle = places(WINDOW, 2)[0].center();
        f.frame(vec![Event::PointerMoved(middle)]);
        assert_eq!(f.cursor, CursorIcon::Default);
        wheel(&mut f, egui::MouseWheelUnit::Line, 1.0);
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grab);
        f.button(middle, true);
        assert_eq!(f.cursor, CursorIcon::Grabbing);
    }

    // A trackpad's points add up: half of a hundred is half a notch.
    #[test]
    fn a_trackpads_points_are_a_share_of_a_notch() {
        let mut f = Fixture::new(2);
        f.frame(vec![Event::PointerMoved(places(WINDOW, 2)[0].center())]);
        wheel(&mut f, egui::MouseWheelUnit::Point, 50.0);
        assert!(
            (f.zoom.zoom() - WHEEL_FACTOR.powf(0.5)).abs() < 1e-4,
            "{}",
            f.zoom.zoom()
        );
    }

    // A pane with its full picture draws the level at least as large as it is drawn, not
    // the largest.
    #[test]
    fn a_pane_draws_the_level_that_fits_what_is_drawn() {
        let mut f = Fixture::new(2);
        f.levels = [4000, 2000, 1000, 500]
            .iter()
            .enumerate()
            .map(|(n, width)| {
                let texture = f.ctx.load_texture(
                    format!("level-{n}"),
                    ColorImage::filled([40, 30], Color32::GRAY),
                    TextureOptions::LINEAR,
                );
                (texture, *width as u32)
            })
            .collect();
        f.frame(Vec::new());
        // A pane about 494 points wide at a scale of one: the level of 500.
        let id = f.levels[3].0.id();
        assert!(
            f.images.iter().any(|(of, ..)| *of == id),
            "the level of 500"
        );
        assert!(!f.images.iter().any(|(of, ..)| *of == f.levels[0].0.id()));
    }

    // The ring is drawn inside the pane's own box: the panes stand close, and a ring
    // outside one would be cut off by its neighbour.
    #[test]
    fn the_ring_is_drawn_inside_the_pane() {
        let f = Fixture::new(2);
        let at = (f.lines.iter())
            .position(|(_, line)| *line == color(DARK.accent))
            .unwrap();
        assert_eq!(f.kinds[at], StrokeKind::Inside);
    }
}
