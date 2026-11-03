//! The photo's passage between its tile in the grid and its place in the viewer, as
//! arithmetic: where it is drawn, which part of it, how dark the ground is and how much
//! of the controls show, at a moment on the window's clock in milliseconds. The view draws
//! from it each frame; nothing here knows how. A passage turned round in the middle is a
//! new passage started from where the old one was drawn, so there is no reverse.

use crate::animate::{CONTROLS_MS, DURATION_MS, eased};
use crate::viewer_layout::Frame;

/// A part of a picture: left, top, right, bottom, each in 0 to 1.
pub type Uv = (f32, f32, f32, f32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    /// The photo travels between its tile and its place.
    Zoom,
    /// The photo stays where it stands and the rest comes or goes.
    Fade,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Passage {
    id: i64,
    way: Way,
    opening: bool,
    from: Frame,
    from_uv: Option<Uv>,
    from_ground: f32,
    started_ms: f64,
}

impl Passage {
    /// A photo opening from `from` (its tile; for a fade, its landing place); `from_uv`
    /// is the part of it that was drawn when that was not the crop of its frame - a
    /// closing from a zoomed photo, turned round.
    pub fn opening(
        id: i64,
        way: Way,
        from: Frame,
        from_uv: Option<Uv>,
        from_ground: f32,
        now_ms: f64,
    ) -> Self {
        Passage {
            id,
            way,
            opening: true,
            from,
            from_uv,
            from_ground,
            started_ms: now_ms,
        }
    }

    /// A photo closing from where it was drawn; `from_uv` is the part of it that was
    /// drawn when that was not the crop of its frame.
    pub fn closing(
        id: i64,
        way: Way,
        from: Frame,
        from_uv: Option<Uv>,
        from_ground: f32,
        now_ms: f64,
    ) -> Self {
        Passage {
            id,
            way,
            opening: false,
            from,
            from_uv,
            from_ground,
            started_ms: now_ms,
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn way(&self) -> Way {
        self.way
    }

    pub fn is_opening(&self) -> bool {
        self.opening
    }

    fn since(&self, now_ms: f64) -> f64 {
        now_ms - self.started_ms
    }

    fn progress(&self, now_ms: f64) -> f32 {
        eased(self.since(now_ms), DURATION_MS) as f32
    }

    /// Where the photo is drawn. `to` is the landing place when opening and the tile when
    /// closing, which the caller reads anew each frame: the grid may have scrolled.
    pub fn photo_at(&self, to: Frame, now_ms: f64) -> Frame {
        match self.way {
            Way::Zoom => lerp_frame(self.from, to, self.progress(now_ms)),
            Way::Fade => self.from,
        }
    }

    /// Where the photo is drawn at `now_ms`, given where it was drawn at `drawn_ms`: each
    /// frame covers the share of what was left of the way that the curve covers of what
    /// was left of it. Towards a landing that stays, that is `photo_at`; a landing that
    /// moves on the way - a window resized, a picture of another shape arrived - is
    /// followed from where the photo stands, where `photo_at` would jump by as much of
    /// the move as the curve has come.
    pub fn follow(&self, drawn: Frame, drawn_ms: f64, to: Frame, now_ms: f64) -> Frame {
        if self.way == Way::Fade {
            return self.from;
        }
        let (was, now) = (self.progress(drawn_ms), self.progress(now_ms));
        if was >= 1.0 || now >= 1.0 {
            return to;
        }
        lerp_frame(drawn, to, (now - was) / (1.0 - was))
    }

    /// The part of the picture drawn into `drawn`.
    pub fn uv_at(&self, image: (f32, f32), drawn: Frame, now_ms: f64) -> Uv {
        let crop = crop_uv(image, (drawn.width, drawn.height));
        match self.from_uv {
            Some(from) => {
                let k = self.progress(now_ms);
                (
                    lerp(from.0, crop.0, k),
                    lerp(from.1, crop.1, k),
                    lerp(from.2, crop.2, k),
                    lerp(from.3, crop.3, k),
                )
            }
            None => crop,
        }
    }

    /// How much of the viewer's ground is there, 0 to 1.
    pub fn ground(&self, now_ms: f64) -> f32 {
        let to = if self.opening { 1.0 } else { 0.0 };
        lerp(self.from_ground, to, self.progress(now_ms))
    }

    /// How much of the controls show: they come once the photo has landed and go at once.
    pub fn controls(&self, now_ms: f64) -> f32 {
        if !self.opening {
            return 0.0;
        }
        match self.way {
            Way::Fade => self.ground(now_ms),
            Way::Zoom => eased(self.since(now_ms) - DURATION_MS, CONTROLS_MS) as f32,
        }
    }

    /// The photo has come to its place.
    pub fn landed(&self, now_ms: f64) -> bool {
        self.opening && self.progress(now_ms) >= 1.0
    }

    /// Something still moves: frames are asked while this holds.
    pub fn running(&self, now_ms: f64) -> bool {
        self.progress(now_ms) < 1.0 || (self.opening && self.controls(now_ms) < 1.0)
    }

    /// The tile it was going to is gone: the photo stays at `at` and goes with the ground.
    /// Time and ground go on unbroken.
    pub fn fade_instead(&mut self, at: Frame) {
        self.way = Way::Fade;
        self.from = at;
    }
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

pub fn lerp_frame(a: Frame, b: Frame, k: f32) -> Frame {
    Frame {
        left: lerp(a.left, b.left, k),
        top: lerp(a.top, b.top, k),
        width: lerp(a.width, b.width, k),
        height: lerp(a.height, b.height, k),
    }
}

/// The centred cut of a picture of `image` size to the shape of `frame`: what
/// `object-fit: cover` shows. A size of nothing shows the whole picture.
pub fn crop_uv(image: (f32, f32), frame: (f32, f32)) -> Uv {
    let (w, h) = image;
    let (fw, fh) = frame;
    if w <= 0.0 || h <= 0.0 || fw <= 0.0 || fh <= 0.0 {
        return (0.0, 0.0, 1.0, 1.0);
    }
    let (picture, shape) = (w / h, fw / fh);
    if picture > shape {
        let shown = shape / picture;
        ((1.0 - shown) / 2.0, 0.0, (1.0 + shown) / 2.0, 1.0)
    } else {
        let shown = picture / shape;
        (0.0, (1.0 - shown) / 2.0, 1.0, (1.0 + shown) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(left: f32, top: f32, width: f32, height: f32) -> Frame {
        Frame {
            left,
            top,
            width,
            height,
        }
    }

    const TILE: Frame = Frame {
        left: 10.0,
        top: 10.0,
        width: 100.0,
        height: 100.0,
    };
    const LANDING: Frame = Frame {
        left: 0.0,
        top: 0.0,
        width: 1000.0,
        height: 750.0,
    };

    fn opening() -> Passage {
        Passage::opening(1, Way::Zoom, TILE, None, 0.0, 1000.0)
    }

    #[test]
    fn crop_uv_is_the_whole_picture_when_the_shapes_match() {
        assert_eq!(crop_uv((400.0, 300.0), (4.0, 3.0)), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_wide_picture_in_a_square_is_cut_at_its_sides() {
        assert_eq!(crop_uv((256.0, 128.0), (1.0, 1.0)), (0.25, 0.0, 0.75, 1.0));
        assert_eq!(crop_uv((128.0, 256.0), (1.0, 1.0)), (0.0, 0.25, 1.0, 0.75));
    }

    #[test]
    fn an_opening_runs_from_the_tile_to_where_it_lands() {
        let p = opening();
        assert_eq!(p.photo_at(LANDING, 1000.0), TILE);
        assert_eq!(
            p.photo_at(LANDING, 1100.0),
            lerp_frame(TILE, LANDING, 0.875)
        );
        assert_eq!(p.photo_at(LANDING, 1200.0), LANDING);
        assert_eq!(p.ground(1000.0), 0.0);
        assert_eq!(p.ground(1100.0), 0.875);
        assert_eq!(p.ground(1200.0), 1.0);
    }

    #[test]
    fn the_controls_fade_in_once_the_photo_has_landed() {
        let p = opening();
        assert_eq!(p.controls(1199.0), 0.0);
        assert_eq!(p.controls(1250.0), 0.875);
        assert_eq!(p.controls(1300.0), 1.0);
        assert!(p.running(1299.0));
        assert!(!p.running(1300.0));
        assert!(!p.landed(1199.0));
        assert!(p.landed(1200.0));
    }

    #[test]
    fn a_fade_stays_where_it_lands() {
        let p = Passage::opening(1, Way::Fade, LANDING, None, 0.0, 1000.0);
        for now in [1000.0, 1050.0, 1100.0, 1200.0, 1500.0] {
            assert_eq!(p.photo_at(TILE, now), LANDING);
            assert_eq!(p.controls(now), p.ground(now));
        }
        assert!(p.running(1199.0));
        assert!(!p.running(1200.0));
    }

    #[test]
    fn a_closing_runs_to_the_tile_and_its_ground_goes() {
        let p = Passage::closing(1, Way::Zoom, LANDING, None, 1.0, 1000.0);
        assert!(!p.is_opening());
        assert_eq!(p.photo_at(TILE, 1000.0), LANDING);
        assert_eq!(p.photo_at(TILE, 1200.0), TILE);
        assert_eq!(p.ground(1000.0), 1.0);
        assert_eq!(p.ground(1100.0), 0.125);
        assert_eq!(p.ground(1200.0), 0.0);
        assert_eq!(p.controls(1000.0), 0.0);
        assert_eq!(p.controls(1100.0), 0.0);
        assert!(p.running(1199.0));
        assert!(!p.running(1200.0));
        assert!(!p.landed(1200.0));
    }

    #[test]
    fn a_zoomed_photo_closes_from_the_part_that_was_drawn() {
        let zoomed = (0.2, 0.3, 0.4, 0.5);
        let p = Passage::closing(1, Way::Zoom, LANDING, Some(zoomed), 1.0, 1000.0);
        let image = (256.0, 128.0);
        assert_eq!(p.uv_at(image, LANDING, 1000.0), zoomed);
        assert_eq!(p.uv_at(image, TILE, 1200.0), (0.25, 0.0, 0.75, 1.0));
    }

    // Followed frame by frame towards a landing that stays where it is, the photo is
    // where the curve puts it.
    #[test]
    fn followed_to_a_landing_that_stays_the_photo_is_where_the_curve_puts_it() {
        let p = opening();
        let mut drawn = (TILE, 1000.0);
        for now in [1016.0, 1050.0, 1100.0, 1150.0, 1199.0] {
            let at = p.follow(drawn.0, drawn.1, LANDING, now);
            let curve = p.photo_at(LANDING, now);
            for (a, b) in [
                (at.left, curve.left),
                (at.top, curve.top),
                (at.width, curve.width),
                (at.height, curve.height),
            ] {
                assert!((a - b).abs() < 1e-3, "{at:?} against {curve:?} at {now}");
            }
            drawn = (at, now);
        }
        assert_eq!(p.follow(drawn.0, drawn.1, LANDING, 1200.0), LANDING);
    }

    // A landing that moves - a window resized, a picture of another shape arrived - is
    // followed from where the photo was last drawn: no jump, the curve's share of what is
    // left of the way in each frame, and the new place reached on time.
    #[test]
    fn a_landing_that_moves_is_followed_from_where_the_photo_was_drawn() {
        let p = opening();
        let at = p.photo_at(LANDING, 1100.0);
        let moved = frame(100.0, 50.0, 800.0, 600.0);
        assert_eq!(p.follow(at, 1100.0, moved, 1100.0), at);
        // 0.875 of the way at 100 ms, 0.984375 at 150: seven eighths of what was left.
        let later = p.follow(at, 1100.0, moved, 1150.0);
        assert_eq!(later, lerp_frame(at, moved, 0.875));
        assert_eq!(p.follow(later, 1150.0, moved, 1200.0), moved);
        assert_eq!(p.follow(later, 1150.0, moved, 1300.0), moved);
    }

    // A fade does not travel, whatever it is told of a landing.
    #[test]
    fn a_fade_followed_stays_where_it_stands() {
        let p = Passage::opening(1, Way::Fade, LANDING, None, 0.0, 1000.0);
        assert_eq!(p.follow(TILE, 1050.0, TILE, 1100.0), LANDING);
    }

    // A passage turned round in the middle is a new one started from what the old one
    // drew at that moment: its place, the part of the picture and the ground go on from
    // there with no jump - a zoomed photo's closing opened again as much as an opening
    // closed.
    #[test]
    fn a_passage_turned_round_goes_on_from_what_was_drawn() {
        let image = (256.0, 128.0);
        type Turn = fn(Frame, Option<Uv>, f32, f64) -> Passage;
        let turned = |old: Passage, old_to: Frame, turn: Turn, new_to: Frame| {
            let at = 1100.0;
            let drawn = old.photo_at(old_to, at);
            let uv = old.uv_at(image, drawn, at);
            let ground = old.ground(at);
            let new = turn(drawn, Some(uv), ground, at);
            assert_eq!(new.photo_at(new_to, at), drawn);
            assert_eq!(new.uv_at(image, drawn, at), uv);
            assert_eq!(new.ground(at), ground);
        };
        let zoomed = (0.2, 0.3, 0.4, 0.5);
        turned(
            Passage::closing(1, Way::Zoom, LANDING, Some(zoomed), 1.0, 1000.0),
            TILE,
            |from, uv, ground, now| Passage::opening(1, Way::Zoom, from, uv, ground, now),
            LANDING,
        );
        turned(
            opening(),
            LANDING,
            |from, uv, ground, now| Passage::closing(1, Way::Zoom, from, uv, ground, now),
            TILE,
        );
    }

    #[test]
    fn a_passage_that_loses_its_tile_fades_from_where_it_is() {
        let mut p = Passage::closing(1, Way::Zoom, LANDING, None, 1.0, 1000.0);
        let ground = p.ground(1100.0);
        let at = p.photo_at(TILE, 1100.0);
        p.fade_instead(at);
        assert_eq!(p.way(), Way::Fade);
        assert_eq!(p.photo_at(frame(500.0, 500.0, 1.0, 1.0), 1150.0), at);
        assert_eq!(p.photo_at(TILE, 1200.0), at);
        assert_eq!(p.ground(1100.0), ground);
        assert_eq!(p.controls(1150.0), 0.0);
        assert_eq!(p.ground(1200.0), 0.0);
    }
}
