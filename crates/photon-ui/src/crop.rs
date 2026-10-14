//! The geometry of the viewer's crop tool: what a drag makes of the rectangle, what a
//! ratio makes of it, and how it is written. `crop.ts`, with its tests' cases.
//!
//! A rectangle is in fractions of the picture the tool shows - the photo turned and not
//! cropped - which is the frame `photon_core::edit::Crop` is defined in. In `f64`, as
//! the Svelte tool's numbers are: a rectangle dragged and written must come back as the
//! same units.

use photon_core::edit::CROP_UNIT;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// The whole picture, which is how "no crop" is drawn.
pub const FULL: Rect = Rect {
    left: 0.0,
    top: 0.0,
    right: 1.0,
    bottom: 1.0,
};

impl Rect {
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }
}

/// What is being dragged: an edge, a corner, or the whole rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
    Move,
}

/// The eight that are drawn; the rectangle itself is the ninth.
pub const HANDLES: [Handle; 8] = [
    Handle::N,
    Handle::S,
    Handle::E,
    Handle::W,
    Handle::Ne,
    Handle::Nw,
    Handle::Se,
    Handle::Sw,
];

impl Handle {
    fn north(self) -> bool {
        matches!(self, Self::N | Self::Ne | Self::Nw)
    }

    fn south(self) -> bool {
        matches!(self, Self::S | Self::Se | Self::Sw)
    }

    fn east(self) -> bool {
        matches!(self, Self::E | Self::Ne | Self::Se)
    }

    fn west(self) -> bool {
        matches!(self, Self::W | Self::Nw | Self::Sw)
    }

    /// Where the handle stands on a rectangle, as fractions of its width and height.
    pub fn place(self) -> (f64, f64) {
        let x = if self.west() {
            0.0
        } else if self.east() {
            1.0
        } else {
            0.5
        };
        let y = if self.north() {
            0.0
        } else if self.south() {
            1.0
        } else {
            0.5
        };
        (x, y)
    }
}

/// The smallest side a rectangle can have, as a fraction: by a drag, and by a ratio
/// chosen on a small rectangle, which grows it to this (`fit_aspect`). Comfortably above
/// the library's refusal threshold (about 1%), so a rectangle the tool allows is never one
/// the write refuses.
pub const MIN_SIDE: f64 = 0.03;

/// A ratio a rectangle can be held to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ratio {
    Free,
    /// The picture's own.
    Original,
    /// Long side over short side: turned to face the way the picture does.
    Of(f64),
}

/// The ratios offered, each with its name.
pub const ASPECTS: [(&str, Ratio); 6] = [
    ("Free", Ratio::Free),
    ("Original", Ratio::Original),
    ("1:1", Ratio::Of(1.0)),
    ("4:3", Ratio::Of(4.0 / 3.0)),
    ("3:2", Ratio::Of(3.0 / 2.0)),
    ("16:9", Ratio::Of(16.0 / 9.0)),
];

/// The width-to-height ratio a locked rectangle must keep **in fractions**, or none for
/// a free one. A rectangle's shape in pixels is its shape in fractions times the
/// picture's own, so the picture's ratio is divided out: on any photo, "Original" is a
/// square of fractions. A named ratio is turned to face the way the picture does, so
/// "4:3" on a portrait photo offers 3:4.
pub fn fraction_ratio(ratio: Ratio, image_aspect: f64) -> Option<f64> {
    // (Not `image_aspect <= 0.0`: a picture with no size has no ratio, and neither has
    // one whose size is not a number.)
    if image_aspect.is_nan() || image_aspect <= 0.0 {
        return None;
    }
    match ratio {
        Ratio::Free => None,
        Ratio::Original => Some(1.0),
        Ratio::Of(ratio) => {
            let pixels = if image_aspect >= 1.0 {
                ratio
            } else {
                1.0 / ratio
            };
            Some(pixels / image_aspect)
        }
    }
}

/// `start` after `handle` has been dragged by (`dx`, `dy`), both fractions of the
/// picture.
///
/// Always from the rectangle as it was when the button went down, never from the last
/// result: adding up each move lets a rectangle that hit the edge drift, because the
/// part of each move the edge swallowed is gone for good.
///
/// `k` is `fraction_ratio`'s answer. Locked, a corner follows whichever axis the pointer
/// moved further along; an edge grows the other dimension about the rectangle's middle.
/// When the picture's edge stops one dimension, it stops both.
pub fn drag_rect(start: Rect, handle: Handle, dx: f64, dy: f64, k: Option<f64>) -> Rect {
    if handle == Handle::Move {
        let (w, h) = (start.width(), start.height());
        let left = (start.left + dx).clamp(0.0, 1.0 - w);
        let top = (start.top + dy).clamp(0.0, 1.0 - h);
        return Rect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        };
    }
    let (east, west, south, north) = (handle.east(), handle.west(), handle.south(), handle.north());
    let Some(k) = k else {
        return Rect {
            left: if west {
                (start.left + dx).clamp(0.0, start.right - MIN_SIDE)
            } else {
                start.left
            },
            right: if east {
                (start.right + dx).clamp(start.left + MIN_SIDE, 1.0)
            } else {
                start.right
            },
            top: if north {
                (start.top + dy).clamp(0.0, start.bottom - MIN_SIDE)
            } else {
                start.top
            },
            bottom: if south {
                (start.bottom + dy).clamp(start.top + MIN_SIDE, 1.0)
            } else {
                start.bottom
            },
        };
    };

    let min_w = MIN_SIDE.max(MIN_SIDE * k);
    if (east || west) && (north || south) {
        // A corner: the opposite corner stays put.
        let ax = if east { start.left } else { start.right };
        let ay = if south { start.top } else { start.bottom };
        let want_w = if east {
            start.right + dx - ax
        } else {
            ax - (start.left + dx)
        };
        let want_h = if south {
            start.bottom + dy - ay
        } else {
            ay - (start.top + dy)
        };
        let room_w = if east { 1.0 - ax } else { ax };
        let room_h = if south { 1.0 - ay } else { ay };
        let w = want_w
            .max(want_h * k)
            .max(min_w)
            .min(room_w)
            .min(room_h * k);
        let h = w / k;
        return Rect {
            left: if east { ax } else { ax - w },
            right: if east { ax + w } else { ax },
            top: if south { ay } else { ay - h },
            bottom: if south { ay + h } else { ay },
        };
    }
    if east || west {
        let ax = if east { start.left } else { start.right };
        let cy = (start.top + start.bottom) / 2.0;
        let want_w = if east {
            start.right + dx - ax
        } else {
            ax - (start.left + dx)
        };
        let room = if east { 1.0 - ax } else { ax };
        let w = want_w.max(min_w).min(room).min(2.0 * cy.min(1.0 - cy) * k);
        let h = w / k;
        return Rect {
            left: if east { ax } else { ax - w },
            right: if east { ax + w } else { ax },
            top: cy - h / 2.0,
            bottom: cy + h / 2.0,
        };
    }
    let ay = if south { start.top } else { start.bottom };
    let cx = (start.left + start.right) / 2.0;
    let want_h = if south {
        start.bottom + dy - ay
    } else {
        ay - (start.top + dy)
    };
    let room = if south { 1.0 - ay } else { ay };
    let h = want_h
        .max(min_w / k)
        .min(room)
        .min(2.0 * cx.min(1.0 - cx) / k);
    let w = h * k;
    Rect {
        left: cx - w / 2.0,
        right: cx + w / 2.0,
        top: if south { ay } else { ay - h },
        bottom: if south { ay + h } else { ay },
    }
}

/// The largest rectangle of ratio `k` inside `rect`, on the same centre: what choosing
/// a ratio does to the rectangle already drawn.
pub fn fit_aspect(rect: Rect, k: Option<f64>) -> Rect {
    let Some(k) = k else {
        return rect;
    };
    let (w, h) = (rect.width(), rect.height());
    let cx = (rect.left + rect.right) / 2.0;
    let cy = (rect.top + rect.bottom) / 2.0;
    let fit_w = w.min(h * k);
    let fit_h = fit_w / k;
    // A ratio far from the rectangle's own shrinks one side with no floor: on a small
    // rectangle of a panorama, held to 1:1, under the least the library takes. Grown back
    // to `MIN_SIDE`, keeping the ratio, and kept inside the picture.
    let grow = (MIN_SIDE / fit_w).max(MIN_SIDE / fit_h).max(1.0);
    let (fit_w, fit_h) = (fit_w * grow, fit_h * grow);
    let cx = cx.clamp(fit_w / 2.0, 1.0 - fit_w / 2.0);
    let cy = cy.clamp(fit_h / 2.0, 1.0 - fit_h / 2.0);
    Rect {
        left: cx - fit_w / 2.0,
        right: cx + fit_w / 2.0,
        top: cy - fit_h / 2.0,
        bottom: cy + fit_h / 2.0,
    }
}

/// `[left, top, right, bottom]` for `set_item_edit`, or none for the whole picture,
/// which is how "no crop" is said.
pub fn to_wire(rect: Rect) -> Option<[u16; 4]> {
    let unit = f64::from(CROP_UNIT);
    // (Rounded half away from nought, as `Math.round` rounds what is not negative.)
    let to = |v: f64| (v.clamp(0.0, 1.0) * unit).round() as u16;
    let wire = [to(rect.left), to(rect.top), to(rect.right), to(rect.bottom)];
    let whole = [0, 0, CROP_UNIT as u16, CROP_UNIT as u16];
    (wire != whole).then_some(wire)
}

pub fn from_wire(crop: Option<[u16; 4]>) -> Rect {
    let unit = f64::from(CROP_UNIT);
    crop.map_or(FULL, |[left, top, right, bottom]| Rect {
        left: f64::from(left) / unit,
        top: f64::from(top) / unit,
        right: f64::from(right) / unit,
        bottom: f64::from(bottom) / unit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MID: Rect = Rect {
        left: 0.25,
        top: 0.25,
        right: 0.75,
        bottom: 0.75,
    };

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 5e-10
    }

    fn assert_rect(actual: Rect, expected: Rect) {
        for (side, a, b) in [
            ("left", actual.left, expected.left),
            ("top", actual.top, expected.top),
            ("right", actual.right, expected.right),
            ("bottom", actual.bottom, expected.bottom),
        ] {
            assert!(close(a, b), "{side}: {a} is not {b} ({actual:?})");
        }
    }

    fn shape(r: Rect) -> f64 {
        r.width() / r.height()
    }

    fn inside(r: Rect) -> bool {
        r.left >= -1e-12 && r.top >= -1e-12 && r.right <= 1.0 + 1e-12 && r.bottom <= 1.0 + 1e-12
    }

    // ---- dragRect, free ----

    #[test]
    fn a_free_drag_moves_only_the_sides_the_handle_names() {
        assert_rect(
            drag_rect(MID, Handle::E, 0.1, 0.4, None),
            Rect { right: 0.85, ..MID },
        );
        assert_rect(
            drag_rect(MID, Handle::Nw, -0.1, 0.05, None),
            Rect {
                left: 0.15,
                top: 0.3,
                ..MID
            },
        );
    }

    #[test]
    fn a_free_drag_stops_at_the_picture_and_at_the_least_size() {
        assert_rect(
            drag_rect(MID, Handle::Se, 9.0, 9.0, None),
            Rect {
                right: 1.0,
                bottom: 1.0,
                ..MID
            },
        );
        assert_rect(
            drag_rect(MID, Handle::W, 9.0, 0.0, None),
            Rect {
                left: 0.75 - MIN_SIDE,
                ..MID
            },
        );
        assert_rect(
            drag_rect(MID, Handle::N, 0.0, -9.0, None),
            Rect { top: 0.0, ..MID },
        );
    }

    #[test]
    fn the_whole_rectangle_is_moved_without_being_resized_and_stays_on_the_picture() {
        assert_rect(
            drag_rect(MID, Handle::Move, 0.1, -0.1, None),
            rect(0.35, 0.15, 0.85, 0.65),
        );
        assert_rect(
            drag_rect(MID, Handle::Move, 9.0, 9.0, None),
            rect(0.5, 0.5, 1.0, 1.0),
        );
        // Locked or not: a move keeps the shape it has.
        assert_rect(
            drag_rect(MID, Handle::Move, -9.0, 0.0, Some(2.0)),
            rect(0.0, 0.25, 0.5, 0.75),
        );
    }

    // ---- dragRect, locked ----
    //
    // A locked drag starts from a rectangle that already has the ratio: choosing a ratio
    // runs `fit_aspect` first. 2:1 about the middle is 0.5 wide and 0.25 tall.

    fn wide() -> Rect {
        fit_aspect(MID, Some(2.0))
    }

    #[test]
    fn a_locked_corner_keeps_the_ratio_and_the_opposite_corner_stays() {
        let r = drag_rect(wide(), Handle::Se, 0.1, 0.0, Some(2.0));
        assert!(close(shape(r), 2.0));
        assert_eq!((r.left, r.top), (0.25, 0.375));
        assert!(close(r.right, 0.85));
    }

    // Down by 0.1 asks for a height of 0.35, which at 2:1 is 0.7 wide: more than the 0.5
    // the pointer's x asks for, so the height leads.
    #[test]
    fn a_locked_corner_follows_the_axis_the_pointer_moved_further_along() {
        let r = drag_rect(wide(), Handle::Se, 0.0, 0.1, Some(2.0));
        assert!(close(shape(r), 2.0));
        assert!(close(r.bottom, 0.725));
        assert!(close(r.right, 0.95));
    }

    #[test]
    fn the_pictures_edge_stops_both_dimensions_when_it_stops_one() {
        let r = drag_rect(MID, Handle::Ne, 9.0, -9.0, Some(1.0));
        assert!(close(shape(r), 1.0));
        assert!(inside(r));
        assert_eq!((r.left, r.bottom), (0.25, 0.75));
        assert!(close(r.top, 0.0));
    }

    #[test]
    fn a_locked_edge_grows_the_other_dimension_about_its_middle() {
        let r = drag_rect(MID, Handle::E, 0.1, 0.0, Some(1.0));
        assert!(close(shape(r), 1.0));
        assert_eq!(r.left, 0.25);
        assert!(close((r.top + r.bottom) / 2.0, 0.5));
        let s = drag_rect(MID, Handle::S, 0.0, -9.0, Some(2.0));
        assert!(close(shape(s), 2.0));
        assert!(s.height() >= MIN_SIDE - 1e-12);
        assert!(close((s.left + s.right) / 2.0, 0.5));
    }

    #[test]
    fn a_locked_drag_never_leaves_the_picture_whatever_the_drag() {
        let starts = [MID, rect(0.0, 0.0, 0.3, 0.9), rect(0.6, 0.1, 1.0, 0.4)];
        for start in starts {
            for handle in HANDLES {
                for k in [0.5, 1.0, 2.5] {
                    for (dx, dy) in [(9.0, 9.0), (-9.0, -9.0), (9.0, -9.0), (0.01, -0.3)] {
                        let r = drag_rect(fit_aspect(start, Some(k)), handle, dx, dy, Some(k));
                        assert!(inside(r), "{handle:?} k={k} d={dx},{dy}: {r:?}");
                        assert!((shape(r) - k).abs() < 5e-7, "{handle:?} k={k}: {r:?}");
                    }
                }
            }
        }
    }

    // ---- fractionRatio ----

    // A 3:2 photo cropped to 1:1 keeps two thirds of its width for all of its height.
    #[test]
    fn the_pictures_own_ratio_is_divided_out_of_a_ratio_in_pixels() {
        assert!(close(
            fraction_ratio(Ratio::Of(1.0), 1.5).unwrap(),
            1.0 / 1.5
        ));
        assert_eq!(fraction_ratio(Ratio::Original, 1.5), Some(1.0));
        assert_eq!(fraction_ratio(Ratio::Free, 1.5), None);
    }

    // 4:3 on a portrait photo means 3:4 pixels.
    #[test]
    fn a_ratio_is_turned_to_face_the_way_the_picture_does() {
        let k = fraction_ratio(Ratio::Of(4.0 / 3.0), 2.0 / 3.0).unwrap();
        assert!(close(k, 3.0 / 4.0 / (2.0 / 3.0)));
    }

    #[test]
    fn a_picture_with_no_size_has_no_ratio_to_hold() {
        for aspect in [0.0, -1.0, f64::NAN] {
            assert_eq!(fraction_ratio(Ratio::Of(1.0), aspect), None);
            assert_eq!(fraction_ratio(Ratio::Original, aspect), None);
        }
    }

    // ---- fitAspect ----

    #[test]
    fn a_ratio_shrinks_one_dimension_about_the_centre() {
        assert_rect(fit_aspect(FULL, Some(0.5)), rect(0.25, 0.0, 0.75, 1.0));
        assert_rect(fit_aspect(FULL, Some(2.0)), rect(0.0, 0.25, 1.0, 0.75));
        assert_eq!(fit_aspect(MID, None), MID);
    }

    // ---- the wire form ----

    #[test]
    fn the_wire_form_comes_back_as_it_went_and_the_whole_picture_is_no_crop() {
        assert_eq!(to_wire(FULL), None);
        assert_eq!(
            to_wire(rect(0.5, 0.0, 1.0, 0.5)),
            Some([32768, 0, 65535, 32768])
        );
        assert_rect(from_wire(Some([0, 0, 65535, 65535])), FULL);
        assert_eq!(from_wire(None), FULL);
        let wire = to_wire(MID).unwrap();
        assert_eq!(to_wire(from_wire(Some(wire))), Some(wire));
    }

    #[test]
    fn no_coordinate_outside_the_unit_can_be_written() {
        assert_eq!(
            to_wire(rect(-0.2, 0.1, 1.4, 0.9)),
            Some([0, 6554, 65535, 58982])
        );
    }

    // The tool's least side is above the library's own, so what a drag can make is
    // never refused: the smallest free rectangle is a crop the engine takes.
    #[test]
    fn the_smallest_rectangle_a_drag_makes_is_one_the_library_takes() {
        let smallest = drag_rect(MID, Handle::Se, -9.0, -9.0, None);
        assert!(close(smallest.width(), MIN_SIDE) && close(smallest.height(), MIN_SIDE));
        let [left, top, right, bottom] = to_wire(smallest).unwrap();
        let crop = photon_core::edit::Crop {
            left,
            top,
            right,
            bottom,
        };
        assert!(photon_core::edit::Edit::new(0, Some(crop)).is_some());
    }

    #[test]
    fn each_handle_stands_at_its_edge_or_corner() {
        assert_eq!(Handle::N.place(), (0.5, 0.0));
        assert_eq!(Handle::Se.place(), (1.0, 1.0));
        assert_eq!(Handle::W.place(), (0.0, 0.5));
        assert_eq!(Handle::Ne.place(), (1.0, 0.0));
        assert_eq!(Handle::Move.place(), (0.5, 0.5));
    }

    // A ratio chosen on the least rectangle a drag makes, on a panorama, shrinks one side
    // with no floor: under the least crop the library takes. It is grown back to the
    // least side, keeps its ratio and stays inside the picture.
    #[test]
    fn a_ratio_on_the_least_rectangle_keeps_the_least_side() {
        // A panorama, whose square grows tall, and an upright strip, whose grows wide -
        // each at the picture's far edges, where growing would leave it.
        for picture in [4.0, 0.25] {
            let k = fraction_ratio(Ratio::Of(1.0), picture).unwrap();
            for at in [0.0, 0.5, 1.0 - MIN_SIDE] {
                let least = Rect {
                    left: at,
                    top: at,
                    right: at + MIN_SIDE,
                    bottom: at + MIN_SIDE,
                };
                let held = fit_aspect(least, Some(k));
                assert!(held.width() >= MIN_SIDE - 1e-12 && held.height() >= MIN_SIDE - 1e-12);
                assert!((held.width() / held.height() - k).abs() < 1e-9, "{held:?}");
                assert!(
                    held.left >= 0.0 && held.right <= 1.0 && held.top >= 0.0 && held.bottom <= 1.0
                );
            }
        }
    }
}
