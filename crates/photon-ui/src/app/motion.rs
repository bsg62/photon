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
//! grid is covered, and the afterimage holds its own handles to them.
//!
//! **The closing is drawn over the live grid.** The viewer is closed at the press, as it
//! always was: the grid is drawn as the grid - its keys, its presses, its wheel - with the
//! tile the photo goes back to drawn empty (`GridData::hollow`), and over it, on a layer of
//! its own that takes no press, the black going and the photo shrinking into where the
//! tile is drawn in each frame (`Leaving`). The picture it is drawn from is the viewer's,
//! held here for the 200 ms: the viewer lets go of its own as it closes.

use super::App;
use crate::{
    afterimage::{Afterimage, of_layers_and},
    animate::DURATION_MS,
    theme::{self, apply::color, tokens::R},
    viewer_layout::{self, Frame},
    viewer_motion::{Passage, Uv, Way},
};
use eframe::egui;

/// The passage running, and what it is drawn over.
pub(super) struct Motion {
    pub(super) passage: Passage,
    /// The window as it stood at the press.
    frozen: Afterimage,
    /// The tile the photo grew out of, drawn as an empty place while it is out of it.
    tile: Option<egui::Rect>,
    /// The part of the window the grid's photos were seen in at the press (`seen_area`):
    /// the empty tile is cut to it, and is not drawn over the top bar or a pinned header.
    seen: Option<egui::Rect>,
    /// What the growing photo is cut to as it starts: the part of its tile that was seen,
    /// or what a closing turned round was cut to. It opens out to the whole window as the
    /// photo lands. None for a fade, which is never cut.
    clip_from: Option<egui::Rect>,
    /// The picture of a closing turned round, drawn on the way until the viewer has its
    /// own again: the viewer let go of its pictures as it closed.
    stand_in: Option<egui::TextureHandle>,
}

impl Motion {
    /// What the growing photo is cut to at `now`; none for a fade.
    pub(super) fn clip_at(&self, window: egui::Rect, now: f64) -> Option<egui::Rect> {
        let clip = self.clip_from?;
        Some(lerp_rect(clip, window, self.passage.progress(now)))
    }

    pub(super) fn stand_in(&self) -> Option<&egui::TextureHandle> {
        self.stand_in.as_ref()
    }
}

/// The viewer's photo on its way back into its tile, or fading where it stood, after the
/// viewer has closed.
pub(super) struct Leaving {
    passage: Passage,
    /// The picture it is drawn from: the viewer's, held for the passage alone.
    texture: egui::TextureHandle,
    /// The picture's size, in pixels.
    image: (f32, f32),
    /// What it was cut to at the press: the window, or what an opening turned round was
    /// still cut to. It closes in to the part of the window the grid's photos are seen in,
    /// so that a tile part under the top bar or a pinned header is landed in as it shows.
    clip_from: egui::Rect,
    /// What the last frame drew: where a fade stays when its tile is gone, and where an
    /// opening turned round starts from.
    drawn: Drawn,
}

/// The photo as a frame of a passage drew it.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    at: Frame,
    uv: Uv,
    ground: f32,
    clip: egui::Rect,
}

fn lerp_rect(a: egui::Rect, b: egui::Rect, k: f32) -> egui::Rect {
    egui::Rect::from_min_max(a.min.lerp(b.min, k), a.max.lerp(b.max, k))
}

fn uv_rect((left, top, right, bottom): Uv) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom))
}

/// The layer the closing is drawn on: over the window, and no area's, so it takes no
/// press; left out of a frozen picture, which is the window's.
fn leaving_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("viewer-leaving"))
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
/// way back to its tile. (Being no area's, egui lists neither among the layers this is
/// asked of today - a layer a painter alone drew on is taken only when named, as
/// `begin_opening` names the closing it ends - and they are kept out by name all the same.)
fn of_the_window(layer: egui::LayerId) -> bool {
    layer.id != egui::Id::new("fading") && layer.id != leaving_layer().id
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
    ///
    /// Opened again on the photo that is on its way back to its tile, the passage turns
    /// round where the last frame drew it - its place, its part of the picture, its black,
    /// its cut - and the picture it was drawn from stands in until the viewer has its own.
    /// Opened on another, the closing is dropped.
    pub(super) fn begin_opening(&mut self, id: i64, tile: Option<egui::Rect>) {
        let left = self.leaving.take();
        let ends = (left.as_ref()).is_some_and(|leaving| leaving.passage.id() != id);
        let turning = left.filter(|leaving| leaving.passage.id() == id);
        self.motion = None;
        if !self.animations {
            return;
        }
        let ctx = self.ctx.clone();
        let now = ctx.input(|input| input.time) * 1000.0;
        // A photo on its way back into another tile than this one: that closing ends here,
        // and is part of what the window showed - its tile is not left empty under the
        // viewer. (A layer only a painter drew on, which egui lists among no areas.)
        let painted = if ends {
            vec![leaving_layer()]
        } else {
            Vec::new()
        };
        let frozen = of_layers_and(&ctx, of_the_window, &painted, now, DURATION_MS);
        if frozen.is_empty() {
            return;
        }
        let seen = tile.and(self.view.seen_area());
        if let Some(leaving) = turning {
            let (way, drawn) = (leaving.passage.way(), leaving.drawn);
            let zoom = way == Way::Zoom;
            self.motion = Some(Motion {
                passage: Passage::opening(id, way, drawn.at, Some(drawn.uv), drawn.ground, now),
                frozen,
                tile: tile.filter(|_| zoom),
                seen,
                clip_from: zoom.then_some(drawn.clip),
                stand_in: Some(leaving.texture),
            });
            ctx.request_repaint();
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
            seen,
            clip_from: tile.and(seen),
            stand_in: None,
        });
        ctx.request_repaint();
    }

    /// Begins the viewer's closing, as it closes: the photo shrinks from where the viewer
    /// drew it into the tile of `to` - the photo handed back to the grid - or, where there
    /// is none to go to, fades out where it stands. Nothing moves with the animations off,
    /// or where the viewer drew no picture to move.
    ///
    /// A fade where there is no grid to land in or no place in it known: no photo handed
    /// back (a link that leaves for another view, a grid of other results), the People
    /// page under the viewer, and a show, whose window leaves fullscreen at this moment
    /// and changes its size.
    pub(super) fn begin_closing(&mut self, to: Option<i64>, showing: bool) {
        self.leaving = None;
        if !self.animations {
            return;
        }
        let Some(open) = &self.viewer else {
            return;
        };
        let Some(photo) = self.viewer_view.photo_drawn() else {
            return;
        };
        let ctx = self.ctx.clone();
        let now = ctx.input(|input| input.time) * 1000.0;
        let window = ctx.content_rect();
        // Closed while it still opens, it goes back from the black and the cut it has.
        let (ground, clip_from) = match &self.motion {
            Some(motion) => (
                motion.passage.ground(now),
                motion.clip_at(window, now).unwrap_or(window),
            ),
            None => (1.0, window),
        };
        let way = match to {
            Some(_) if !showing && self.page.is_none() => Way::Zoom,
            _ => Way::Fade,
        };
        let uv = (
            photo.uv.min.x,
            photo.uv.min.y,
            photo.uv.max.x,
            photo.uv.max.y,
        );
        // The whole picture, fitted, is the cut of its own shape: told so, the part drawn
        // would be pulled to the tile's cut in a straight line, and squeezed on the way.
        let whole = [uv.0, uv.1, 1.0 - uv.2, 1.0 - uv.3]
            .iter()
            .all(|edge| edge.abs() < 1e-4);
        let at = frame(photo.at);
        let passage = Passage::closing(
            to.unwrap_or(open.viewer.id()),
            way,
            at,
            (!whole).then_some(uv),
            ground,
            now,
        );
        self.leaving = Some(Leaving {
            passage,
            texture: photo.texture.clone(),
            image: (photo.image.x, photo.image.y),
            clip_from,
            drawn: Drawn {
                at,
                uv,
                ground,
                clip: clip_from,
            },
        });
        ctx.request_repaint();
    }

    /// The photo whose tile the grid draws empty: the one shrinking back into it.
    pub(super) fn hollow(&self) -> Option<i64> {
        (self.leaving.as_ref())
            .filter(|leaving| leaving.passage.way() == Way::Zoom)
            .map(|leaving| leaving.passage.id())
    }

    /// Draws the closing over the window as this frame has drawn it: the black at its
    /// ground, and the photo on its way to where the grid drew its tile in this frame
    /// (`grid` says the grid was drawn) - a scroll moves where it is going - cut in to the
    /// part of the window the grid's photos are seen in. Its tile not drawn - scrolled out
    /// of sight, or the grid gone for the People page - it fades out where it is. Frames
    /// are asked while it runs, and one after, in which the tile is the photo's again.
    pub(super) fn leaving_drawn(&mut self, ctx: &egui::Context, now: f64, grid: bool) {
        let Some(leaving) = &mut self.leaving else {
            return;
        };
        let window = ctx.content_rect();
        let passage = &mut leaving.passage;
        let (mut to, mut clip) = (leaving.drawn.at, leaving.drawn.clip);
        if passage.way() == Way::Zoom {
            match grid.then(|| self.view.drawn_place(passage.id())).flatten() {
                Some(tile) => {
                    to = frame(tile);
                    let seen = self.view.seen_area().unwrap_or(window);
                    clip = lerp_rect(leaving.clip_from, seen, passage.progress(now));
                }
                None => passage.fade_instead(leaving.drawn.at),
            }
        }
        let at = passage.photo_at(to, now);
        let uv = passage.uv_at(leaving.image, at, now);
        let ground = passage.ground(now);
        let painter = ctx.layer_painter(leaving_layer());
        // The one colour in photon that no token holds, as in the viewer.
        painter.rect_filled(window, 0.0, egui::Color32::BLACK.gamma_multiply(ground));
        (painter.with_clip_rect(clip)).image(
            leaving.texture.id(),
            rect(at),
            uv_rect(uv),
            egui::Color32::WHITE,
        );
        leaving.drawn = Drawn {
            at,
            uv,
            ground,
            clip,
        };
        self.motion_seen = Some(MotionSeen {
            photo: rect(at),
            ground,
            controls: 0.0,
            opening: false,
            zoom: passage.way() == Way::Zoom,
        });
        ctx.request_repaint();
        if !passage.running(now) {
            self.leaving = None;
        }
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
            let seen = motion.seen.unwrap_or(tile);
            (painter.with_clip_rect(seen)).rect_filled(tile, R[1], color(palette.field));
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
