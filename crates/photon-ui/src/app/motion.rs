//! The viewer's passage on the application's side: the photo opening out of its tile over
//! a frozen picture of the window it was opened from (`viewer_motion.rs` is the
//! arithmetic, `viewer_view.rs` draws the photo and the ground). Named egui types, so a
//! view module and not a state module.
//!
//! **The frozen picture is the window as it stood at the press, drawn and nothing more.**
//! The viewer is open from the press - its keys are its own, and the grid is not drawn
//! under it, so its keys cannot be answered twice - and what is seen of the grid behind
//! the growing photo is the shapes its last frame drew (`afterimage::of_layers`), which
//! take no press, no hover and no key. They are taken in the frame the viewer is opened
//! in, after the grid has drawn: egui empties the shapes at the end of the frame. The
//! thumbnails they draw are the grid's last frame's, which `Thumbs::rest` keeps while the
//! grid is covered.

use super::App;
use crate::{
    afterimage::{Afterimage, of_layers},
    animate::DURATION_MS,
    theme::{self, apply::color, tokens::R},
    viewer_layout::{self, Frame},
    viewer_motion::{Passage, Way},
};
use eframe::egui;

/// The passage running, and what it is drawn over.
pub(super) struct Motion {
    pub(super) passage: Passage,
    /// The window as it stood at the press.
    frozen: Afterimage,
    /// The tile the photo grew out of, drawn as an empty place while it is out of it.
    tile: Option<egui::Rect>,
}

/// What the last frame drew of a passage: what a test of it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSeen {
    /// Where the photo was drawn.
    pub photo: egui::Rect,
    /// How much of the viewer's black ground was there, 0 to 1.
    pub ground: f32,
    /// How much of the controls showed, 0 to 1.
    pub controls: f32,
    pub opening: bool,
    /// The photo travelled between its tile and its place, and did not merely fade.
    pub zoom: bool,
}

/// The layers of the frozen picture: everything drawn in the frame but the motions' own
/// painters, which are not the window - a menu fading out, and the viewer's photo on its
/// way back to its tile.
fn of_the_window(layer: egui::LayerId) -> bool {
    layer.id != egui::Id::new("fading") && layer.id != egui::Id::new("viewer-leaving")
}

pub(super) fn rect(frame: Frame) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(frame.left, frame.top),
        egui::vec2(frame.width, frame.height),
    )
}

pub(super) fn frame(rect: egui::Rect) -> Frame {
    Frame {
        left: rect.left(),
        top: rect.top(),
        width: rect.width(),
        height: rect.height(),
    }
}

impl App {
    /// Begins the viewer's opening on the photo `id`, just opened: out of `tile`, where
    /// the grid drew it, when its thumbnail is there to grow; a fade where it lands
    /// otherwise - opened from compare or a face, or from a tile with nothing to grow.
    /// With the animations off, or opened before anything was drawn in the frame (a face
    /// whose photo a step had to find), the viewer is simply there, as it always was.
    pub(super) fn begin_opening(&mut self, id: i64, tile: Option<egui::Rect>) {
        self.motion = None;
        if !self.animations {
            return;
        }
        let ctx = self.ctx.clone();
        let now = ctx.input(|input| input.time) * 1000.0;
        let frozen = of_layers(&ctx, of_the_window, now, DURATION_MS);
        if frozen.is_empty() {
            return;
        }
        let Some(entry) = self.entry(id).copied() else {
            return;
        };
        let key = entry.thumb_key;
        let grows = !self.thumbs.failed(key) && self.thumbs.holds(key);
        let (way, from, tile) = match tile.filter(|_| grows) {
            Some(tile) => (Way::Zoom, frame(tile), Some(tile)),
            // Where the viewer will fit it, by the shape the grid's index gives it: the
            // panel is shut at every opening.
            None => {
                let window = frame(ctx.content_rect());
                let area = viewer_layout::photo_area(window, 0.0);
                let fit = viewer_layout::contained(entry.aspect, 1.0, area);
                (Way::Fade, fit, None)
            }
        };
        self.motion = Some(Motion {
            passage: Passage::opening(id, way, from, None, 0.0, now),
            frozen,
            tile,
        });
        ctx.request_repaint();
    }

    /// Paints what the viewer opens over, under everything the viewer draws: the frozen
    /// window, and the tile the photo has left, empty.
    pub(super) fn paint_frozen(&self, ui: &egui::Ui) {
        let Some(motion) = &self.motion else {
            return;
        };
        let ctx = ui.ctx();
        let painter = ctx.layer_painter(ui.layer_id());
        motion.frozen.paint(&painter, 1.0);
        if let Some(tile) = motion.tile.filter(|_| motion.passage.way() == Way::Zoom) {
            let palette = theme::apply::palette(ctx);
            painter.rect_filled(tile, R[1], color(palette.field));
        }
    }

    /// Says what the viewer's frame drew of the opening, and ends it once nothing moves:
    /// frames are asked while it runs and none after, and the window it was drawn over is
    /// let go of.
    pub(super) fn opening_drawn(&mut self, ctx: &egui::Context, now: f64) {
        let Some(motion) = &self.motion else {
            return;
        };
        let passage = motion.passage;
        // Where the photo was drawn on its way; where it stands, for a fade, or for a zoom
        // with no picture to draw yet (still at its tile).
        let still = passage.photo_at(frame(motion.tile.unwrap_or(ctx.content_rect())), now);
        let photo = match passage.way() {
            Way::Zoom => self.viewer_view.arrival(),
            Way::Fade => None,
        };
        self.motion_seen = Some(MotionSeen {
            photo: photo.unwrap_or(rect(still)),
            ground: passage.ground(now),
            controls: passage.controls(now),
            opening: passage.is_opening(),
            zoom: passage.way() == Way::Zoom,
        });
        if passage.running(now) {
            ctx.request_repaint();
        } else {
            self.motion = None;
        }
    }

    /// What the last frame drew of a passage; none when no passage ran in it.
    pub fn viewer_motion(&self) -> Option<MotionSeen> {
        self.motion_seen
    }
}
