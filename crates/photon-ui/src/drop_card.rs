//! The card over the window while something with a path is held over it: it says what
//! letting go will do. Over everything - a menu, a dialog - since the drop is the
//! window's whatever is open.
//!
//! It takes no press: the drag is the system's, which reports the drop wherever it
//! lands. And it asks for no frame: the system says when something is held and when it
//! is not, and each of those draws one.

use crate::{
    icons::Icon,
    theme::{
        apply::{color, dialog_shadow, palette},
        fonts,
        tokens::{R, S, SCRIM, T},
    },
};
use eframe::egui::{self, Id, Rect, Sense, Stroke, StrokeKind, WidgetInfo, WidgetType, pos2, vec2};

/// The folder before the words.
const ICON: f32 = 20.0;

/// Draws the card in the middle of the window, over a ground that dims the rest.
pub fn show(ctx: &egui::Context, words: &str) {
    let palette = palette(ctx);
    let window = ctx.content_rect();
    egui::Area::new(Id::new("drop-card"))
        // Over a dialog, which is in the foreground: under the export's it says where
        // the folder goes.
        .order(egui::Order::Tooltip)
        .fixed_pos(window.min)
        .interactable(false)
        // There at once, and gone at once: egui fades a new area in over a few frames,
        // each of them asked for.
        .fade_in(false)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            painter.rect_filled(window, 0.0, color(SCRIM));
            let text =
                painter.layout_no_wrap(words.to_owned(), fonts::regular(T[3]), color(palette.text));
            let inner = vec2(ICON + S[1] + text.size().x.ceil(), ICON.max(text.size().y));
            let card = Rect::from_center_size(window.center(), inner + 2.0 * vec2(S[4], S[3]));
            painter.add(dialog_shadow().as_shape(card, R[3]));
            painter.rect_filled(card, R[3], color(palette.surface));
            painter.rect_stroke(
                card,
                R[3],
                Stroke::new(1.0, color(palette.line)),
                StrokeKind::Outside,
            );
            // Its words, for whoever reads the window without seeing it - which is also
            // how a test of the whole application reads them.
            ui.interact(card, Id::new("drop-card-words"), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, words));
            let content = Rect::from_center_size(card.center(), inner);
            let icon = Rect::from_min_size(
                pos2(content.left(), content.center().y - ICON / 2.0),
                vec2(ICON, ICON),
            );
            Icon::Folder.paint(ui, icon, ICON, false, color(palette.text));
            painter.galley(
                pos2(
                    icon.right() + S[1],
                    content.center().y - text.size().y / 2.0,
                ),
                text,
                color(palette.text),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder_drop::{CARD, CARD_EXPORT};
    use eframe::egui::{Color32, Event, PointerButton, Pos2, RawInput, Vec2};

    const WINDOW: Vec2 = vec2(1000.0, 700.0);

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    /// A window with a button in it, and the card over it while `words` is some.
    struct Fixture {
        ctx: egui::Context,
        time: f64,
        words: Option<&'static str>,
        /// Whether a dialog stands under the card.
        modal: bool,
        clicked: bool,
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, Color32)>,
        repaint: std::time::Duration,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                time: 0.0,
                words: None,
                modal: false,
                clicked: false,
                texts: Vec::new(),
                fills: Vec::new(),
                repaint: std::time::Duration::MAX,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let (words, modal) = (self.words, self.modal);
            let mut clicked = false;
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default().show(ui, |ui| {
                    let at = Rect::from_center_size(WINDOW.to_pos2() / 2.0, vec2(160.0, 40.0));
                    clicked = ui.put(at, egui::Button::new("behind")).clicked();
                });
                if modal {
                    egui::Modal::new(Id::new("dialog")).show(ui.ctx(), |ui| {
                        ui.label("the dialog");
                    });
                }
                if let Some(words) = words {
                    show(ui.ctx(), words);
                }
            });
            full.textures_delta.clear();
            self.clicked = clicked;
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
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }
    }

    #[test]
    fn the_card_says_what_letting_go_will_do_in_the_middle_of_a_dimmed_window() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.drew(CARD), None);
        f.words = Some(CARD);
        f.frame(Vec::new());
        f.frame(Vec::new());
        let words = f.drew(CARD).expect("the card's words");
        let whole = Rect::from_min_size(Pos2::ZERO, WINDOW);
        // The scrim over all of the window, and the card on it, about its middle.
        assert!(
            (f.fills.iter()).any(|(rect, fill)| *rect == whole && *fill == color(SCRIM)),
            "{:?}",
            f.fills
        );
        let surface = color(palette(&f.ctx).surface);
        let card = (f.fills.iter())
            .find(|(rect, fill)| *fill == surface && rect.contains_rect(words))
            .map(|(rect, _)| *rect)
            .expect("the card");
        assert!((card.center() - whole.center()).length() < 1.0, "{card:?}");
        assert!(card.width() < 500.0 && card.height() < 80.0, "{card:?}");
        // The folder stands before the words, inside the card.
        assert!(words.left() - card.left() > ICON + S[1]);
        // Under the export's dialog it says where the folder goes.
        f.words = Some(CARD_EXPORT);
        f.frame(Vec::new());
        assert!(f.drew(CARD_EXPORT).is_some());
        assert_eq!(f.drew(CARD), None);
    }

    // A drop is the window's whatever is open, and under the export's dialog the card
    // says where the folder goes: it is drawn over a dialog, not under its scrim.
    #[test]
    fn the_card_is_drawn_over_a_dialog() {
        let mut f = Fixture::new();
        f.modal = true;
        f.words = Some(CARD_EXPORT);
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        // What is painted later lies on top.
        let place = |text: &str| {
            (f.texts.iter())
                .position(|(drawn, _)| drawn == text)
                .unwrap_or_else(|| panic!("{text} is drawn: {:?}", f.texts))
        };
        assert!(place(CARD_EXPORT) > place("the dialog"));
    }

    // The system says when something is held over the window and when it is not, and
    // each of those is a frame: the card itself asks for none - not for a fade either.
    #[test]
    fn the_card_asks_for_no_frame() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        f.words = Some(CARD);
        // egui measures a new area in one frame and shows it in the next, and the
        // folder's picture is made a frame after it is first asked for: those two
        // frames are asked for, and none after them.
        f.frame(Vec::new());
        f.frame(Vec::new());
        for _ in 0..4 {
            f.frame(Vec::new());
            assert_eq!(f.repaint, std::time::Duration::MAX);
        }
    }

    // It is something to read and nothing to press: what lies under it is pressed
    // through it. (A drag from another program sends the window no pointer at all; this
    // is the pointer that was already there.)
    #[test]
    fn the_card_takes_no_press() {
        let mut f = Fixture::new();
        f.words = Some(CARD);
        f.frame(Vec::new());
        let at = WINDOW.to_pos2() / 2.0;
        f.frame(vec![Event::PointerMoved(at)]);
        for pressed in [true, false] {
            f.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        assert!(f.clicked);
    }
}
