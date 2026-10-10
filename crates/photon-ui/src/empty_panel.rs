//! What an empty library says where its photos would be, drawn: a title, a sentence
//! wrapped under it, and the buttons that help. `Grid.svelte`'s `.welcome`; what it says
//! is `empty.rs`'s.
//!
//! The scanning and the nothing-found states are one block with one sentence changing: a
//! watched folder is rescanned at any time. A button is known by what it is and not by
//! the sentence over it, so that when these buttons take a press - they do not yet - one
//! is the same widget before and after a scan, and keeps the focus and a press on its
//! way. What does move them is the sentence's height: the block is centred, as the Svelte
//! one is, and a longer sentence pushes its buttons down.

use crate::{
    empty::{Panel, PanelButton},
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{R, S, T},
    },
};
use eframe::egui::{
    self, Align, Align2, Rect, Sense, WidgetInfo, WidgetType, pos2, text::LayoutJob, vec2,
};

/// The widest the sentence is set: `26rem`.
const TEXT_WIDTH: f32 = 416.0;
const BUTTON_HEIGHT: f32 = 30.0;

/// Draws `panel` in the middle of `place`. Answers the button pressed, which one that
/// does nothing yet never is.
pub fn show(ui: &mut egui::Ui, place: Rect, panel: &Panel) -> Option<PanelButton> {
    let palette = palette(ui.ctx());
    let painter = ui.painter_at(place);
    let room = (place.width() - 2.0 * S[4]).clamp(0.0, TEXT_WIDTH);

    let title = painter.layout_no_wrap(
        panel.title.to_owned(),
        fonts::semibold(ui.ctx(), T[4]),
        color(palette.text),
    );
    let mut job = LayoutJob::simple(
        panel.text.clone(),
        fonts::regular(T[2]),
        color(palette.text_dim),
        room,
    );
    job.halign = Align::Center;
    let text = painter.layout_job(job);

    let labels: Vec<f32> = (panel.buttons.iter())
        .map(|button| {
            painter
                .layout_no_wrap(
                    button.label().to_owned(),
                    fonts::regular(T[2]),
                    color(palette.text),
                )
                .size()
                .x
                .ceil()
                + 2.0 * S[3]
        })
        .collect();
    let row = labels.iter().sum::<f32>() + S[1] * labels.len().saturating_sub(1) as f32;

    let height = title.size().y + S[2] + text.size().y + S[2] + BUTTON_HEIGHT;
    let middle = place.center().x;
    let mut top = place.center().y - height / 2.0;

    painter.galley(
        pos2(middle - title.size().x / 2.0, top),
        title.clone(),
        color(palette.text),
    );
    top += title.size().y + S[2];
    // A galley set centred is drawn about the point it is given.
    let text_height = text.size().y;
    painter.galley(pos2(middle, top), text, color(palette.text_dim));
    top += text_height + S[2];

    let mut pressed = None;
    let mut left = middle - row / 2.0;
    for (button, width) in panel.buttons.iter().zip(labels) {
        let rect = Rect::from_min_size(pos2(left, top), vec2(width, BUTTON_HEIGHT));
        left = rect.right() + S[1];
        let works = button.works();
        let sense = if works {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.id().with(("empty-library", *button)), sense);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, works, button.label()));
        let (fill, tint) = if button.primary() {
            (palette.accent, palette.on_accent)
        } else if works && response.hovered() {
            (palette.field_hover, palette.text)
        } else {
            (palette.field, palette.text)
        };
        // What cannot be pressed yet is there, and fainter than what can.
        let faint = if works { 1.0 } else { 0.5 };
        ui.painter()
            .rect_filled(rect, R[2], color(fill).gamma_multiply(faint));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            button.label(),
            fonts::regular(T[2]),
            color(tint).gamma_multiply(faint),
        );
        // One that does nothing yet senses no click.
        if response.clicked() {
            pressed = Some(*button);
        }
    }
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    const PLACE: Rect = Rect {
        min: pos2(260.0, 44.0),
        max: pos2(1280.0, 777.0),
    };

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    struct Fixture {
        ctx: egui::Context,
        place: Rect,
        time: f64,
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, egui::Color32)>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                place: PLACE,
                time: 0.0,
                texts: Vec::new(),
                fills: Vec::new(),
            }
        }

        fn frame(&mut self, panel: &Panel, events: Vec<Event>) -> Option<PanelButton> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let place = self.place;
            let mut pressed = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| pressed = show(ui, place, panel));
            });
            full.textures_delta.clear();
            self.texts.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                    }
                    egui::Shape::Rect(rect) => self.fills.push((rect.rect, rect.fill)),
                    _ => {}
                });
            }
            pressed
        }

        fn click(&mut self, panel: &Panel, at: Pos2) -> Option<PanelButton> {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let mut pressed = self.frame(panel, vec![Event::PointerMoved(at)]);
            pressed = pressed.or(self.frame(panel, vec![button(true)]));
            pressed = pressed.or(self.frame(panel, vec![button(false)]));
            pressed.or(self.frame(panel, Vec::new()))
        }

        fn drew(&self, text: &str) -> Rect {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
                .unwrap_or_else(|| panic!("{text} is not drawn: {:?}", self.texts))
        }

        /// The filled rectangle the words `label` stand in.
        fn button(&self, label: &str) -> (Rect, egui::Color32) {
            let words = self.drew(label);
            (self.fills.iter())
                .copied()
                .find(|(rect, _)| rect.contains_rect(words) && rect.height() == BUTTON_HEIGHT)
                .unwrap_or_else(|| panic!("{label} has no button: {:?}", self.fills))
        }
    }

    const CHOICES: &[PanelButton] = &[PanelButton::AddFolder, PanelButton::WatchedFolders];

    fn looking() -> Panel {
        Panel {
            title: "No photos yet",
            text: "Looking for photos…".to_owned(),
            buttons: CHOICES,
        }
    }

    fn hidden() -> Panel {
        Panel {
            title: "Every photo is hidden",
            text: "The library has no photo to show here: all of them are in Hidden, in the \
                   sidebar."
                .to_owned(),
            buttons: &[PanelButton::ShowHidden],
        }
    }

    #[test]
    fn the_panel_is_a_title_a_sentence_and_its_buttons_in_the_middle_of_its_place() {
        let mut f = Fixture::new();
        f.frame(&looking(), Vec::new());
        let title = f.drew("No photos yet");
        let text = f.drew("Looking for photos…");
        let (add, _) = f.button("Add folder…");
        let (watched, _) = f.button("Watched folders…");
        // One under the other, each about the middle of the place.
        assert!(title.bottom() < text.top() && text.bottom() < add.top());
        for rect in [title, text] {
            assert!((rect.center().x - PLACE.center().x).abs() < 2.0, "{rect:?}");
        }
        // The two buttons side by side, as a pair about the middle.
        assert_eq!(add.top(), watched.top());
        assert!(add.right() < watched.left());
        let pair = (add.left() + watched.right()) / 2.0;
        assert!((pair - PLACE.center().x).abs() < 1.0, "{pair}");
        // And the whole block about the middle of the place, top to bottom.
        let block = (title.top() + add.bottom()) / 2.0;
        assert!((block - PLACE.center().y).abs() < 6.0, "{block}");
    }

    // 26rem: a sentence with a path in it is set in lines, not across the window.
    #[test]
    fn a_long_sentence_is_wrapped_and_stays_centred() {
        let panel = Panel {
            title: "No photos yet",
            text: "photon watches /home/ada/Pictures and has found no photos or videos there. \
                   Add the folder your photos are in: photon never moves, changes or deletes \
                   them."
                .to_owned(),
            buttons: CHOICES,
        };
        let mut f = Fixture::new();
        f.frame(&panel, Vec::new());
        let text = f.drew(&panel.text);
        assert!(text.width() <= TEXT_WIDTH + 1.0, "{text:?}");
        assert!(text.height() > 30.0, "more than one line: {text:?}");
        assert!((text.center().x - PLACE.center().x).abs() < 6.0, "{text:?}");
        let (add, _) = f.button("Add folder…");
        assert!(add.top() > text.bottom());

        // In a place narrower than that it is as wide as the place allows.
        f.place = Rect::from_min_max(pos2(260.0, 44.0), pos2(560.0, 777.0));
        f.frame(&panel, Vec::new());
        let narrow = f.drew(&panel.text);
        assert!(narrow.left() >= 260.0 + S[4] - 1.0 && narrow.right() <= 560.0 - S[4] + 1.0);
        assert!(narrow.height() > text.height());
    }

    #[test]
    fn the_button_that_works_is_pressed_and_the_ones_to_come_take_no_press() {
        let mut f = Fixture::new();
        f.frame(&hidden(), Vec::new());
        let (show_hidden, _) = f.button("Show hidden photos");
        assert_eq!(
            f.click(&hidden(), show_hidden.center()),
            Some(PanelButton::ShowHidden)
        );
        assert_eq!(f.click(&hidden(), PLACE.center()), None, "beside it");

        f.frame(&looking(), Vec::new());
        let (add, _) = f.button("Add folder…");
        let (watched, _) = f.button("Watched folders…");
        assert_eq!(f.click(&looking(), add.center()), None);
        assert_eq!(f.click(&looking(), watched.center()), None);
    }

    // Drawn where they will be, and fainter than what can be pressed: the one that will
    // be the way forward in its colour all the same.
    #[test]
    fn the_buttons_to_come_are_fainter_and_the_one_put_forward_is_in_the_accent() {
        let mut f = Fixture::new();
        let palette = palette(&f.ctx);
        f.frame(&looking(), Vec::new());
        let (_, add) = f.button("Add folder…");
        let (_, watched) = f.button("Watched folders…");
        assert_eq!(add, color(palette.accent).gamma_multiply(0.5));
        assert_eq!(watched, color(palette.field).gamma_multiply(0.5));
        f.frame(&hidden(), Vec::new());
        let (_, show_hidden) = f.button("Show hidden photos");
        assert_eq!(show_hidden, color(palette.field));
    }
}
