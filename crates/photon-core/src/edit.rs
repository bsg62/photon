//! Non-destructive edits: quarter turns and a crop, held in `library.db` and applied
//! wherever photon draws a photo. The file is never touched.
//!
//! One definition, used by everything that renders: **orient** (EXIF), then **turn**, then
//! **crop**. The crop is expressed in the turned image, which is the image the user was
//! looking at when they drew it. Thumbnails, the viewer's full image and the face outlines
//! all go through this module, so they cannot disagree about what an edit means.

use crate::{Result, decode::apply_orientation};
use image::{DynamicImage, ImageReader};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

/// The denominator of a crop coordinate: an edge is a fraction `n / 65535` of the turned
/// image. Integers rather than floats so an edit is exact in the database, hashes stably
/// into the thumbnail key, and survives the trip through JSON unchanged. (It is also how
/// Picasa stores its own `rect64` crops.)
pub const CROP_UNIT: u32 = 65_535;

/// A crop narrower or shorter than this fraction of the image is refused: it is a slip of
/// the pointer, and it would render as a smear of a few upscaled pixels.
const MIN_CROP_UNITS: u16 = 656; // ~1%

/// The kept rectangle, as fractions of the turned image in [`CROP_UNIT`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

/// What the user has done to a photo. `Default` is the untouched photo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    /// Clockwise quarter turns, 0..=3, applied after the EXIF orientation.
    pub turns: u8,
    pub crop: Option<Crop>,
}

impl Crop {
    const FULL: Crop = Crop {
        left: 0,
        top: 0,
        right: CROP_UNIT as u16,
        bottom: CROP_UNIT as u16,
    };

    /// One integer for the database column, packed as Picasa packs a `rect64`.
    pub fn to_db(self) -> i64 {
        ((self.left as u64) << 48
            | (self.top as u64) << 32
            | (self.right as u64) << 16
            | self.bottom as u64) as i64
    }

    pub fn from_db(value: i64) -> Self {
        let v = value as u64;
        Crop {
            left: (v >> 48) as u16,
            top: (v >> 32) as u16,
            right: (v >> 16) as u16,
            bottom: v as u16,
        }
    }

    fn is_valid(self) -> bool {
        self.right > self.left
            && self.bottom > self.top
            && self.right - self.left >= MIN_CROP_UNITS
            && self.bottom - self.top >= MIN_CROP_UNITS
    }

    /// The same rectangle after the image under it is turned a quarter clockwise: what was
    /// the bottom edge is now the left one.
    fn turned_cw(self) -> Self {
        let flip = |v: u16| CROP_UNIT as u16 - v;
        Crop {
            left: flip(self.bottom),
            top: self.left,
            right: flip(self.top),
            bottom: self.right,
        }
    }
}

impl Edit {
    /// Builds an edit from untrusted parts (the IPC boundary, a database row). Turns wrap;
    /// a crop that is inverted or too small is an error rather than something to repair;
    /// a crop of the whole frame is no crop, so that "untouched" has one representation and
    /// an untouched photo keeps the thumbnail key it always had.
    pub fn new(turns: u8, crop: Option<Crop>) -> Option<Self> {
        let crop = match crop {
            Some(c) if !c.is_valid() => return None,
            Some(Crop::FULL) => None,
            other => other,
        };
        Some(Edit {
            turns: turns % 4,
            crop,
        })
    }

    pub fn is_identity(self) -> bool {
        self == Edit::default()
    }

    /// This edit with the picture turned a further quarter, the crop carried round with it
    /// so it still frames the same part of the photo.
    pub fn turned(self, clockwise: bool) -> Self {
        let quarter = if clockwise { 1 } else { 3 };
        let mut crop = self.crop;
        for _ in 0..quarter {
            crop = crop.map(Crop::turned_cw);
        }
        Edit {
            turns: (self.turns + quarter) % 4,
            crop,
        }
    }

    /// The thumbnail key of a file with fingerprint `fingerprint` under this edit. The
    /// untouched photo keeps the bare fingerprint, so every thumbnail cached before edits
    /// existed is still the right one.
    pub fn thumb_key(self, fingerprint: u64) -> u64 {
        if self.is_identity() {
            return fingerprint;
        }
        let mut bytes = [0u8; 17];
        bytes[..8].copy_from_slice(&fingerprint.to_le_bytes());
        bytes[8] = self.turns;
        bytes[9..].copy_from_slice(&self.crop.map_or(-1, Crop::to_db).to_le_bytes());
        xxh3_64(&bytes)
    }

    /// The size of the edited picture, given the size of the oriented one.
    pub fn dims(self, width: u32, height: u32) -> (u32, u32) {
        let (w, h) = if self.turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        match self.crop {
            None => (w, h),
            Some(c) => {
                let (x0, y0, x1, y1) = c.pixels(w, h);
                (x1 - x0, y1 - y0)
            }
        }
    }

    /// Turns and crops an already oriented image.
    pub fn apply(self, img: DynamicImage) -> DynamicImage {
        let img = match self.turns {
            1 => img.rotate90(),
            2 => img.rotate180(),
            3 => img.rotate270(),
            _ => img,
        };
        match self.crop {
            None => img,
            Some(c) => {
                let (x0, y0, x1, y1) = c.pixels(img.width(), img.height());
                img.crop_imm(x0, y0, x1 - x0, y1 - y0)
            }
        }
    }

    /// Only the turn, for the crop tool: it shows the whole picture with the current crop
    /// drawn on it, so the user adjusts the rectangle rather than cropping a crop.
    pub fn without_crop(self) -> Self {
        Edit {
            turns: self.turns,
            crop: None,
        }
    }

    /// Where a rectangle of the oriented, unedited picture (fractions 0..1: left, top,
    /// right, bottom) lands in the edited one. `None` when its centre is cropped away, which
    /// is when a face outline would be naming someone no longer in the frame; otherwise
    /// clipped to the frame.
    pub fn map_rect(self, rect: (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)> {
        let (mut l, mut t, mut r, mut b) = rect;
        for _ in 0..self.turns {
            (l, t, r, b) = (1.0 - b, l, 1.0 - t, r);
        }
        let Some(c) = self.crop else {
            return Some((l, t, r, b));
        };
        let unit = CROP_UNIT as f64;
        let (cl, ct, cr, cb) = (
            c.left as f64 / unit,
            c.top as f64 / unit,
            c.right as f64 / unit,
            c.bottom as f64 / unit,
        );
        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
        if cx < cl || cx > cr || cy < ct || cy > cb {
            return None;
        }
        let x = |v: f64| ((v - cl) / (cr - cl)).clamp(0.0, 1.0);
        let y = |v: f64| ((v - ct) / (cb - ct)).clamp(0.0, 1.0);
        Some((x(l), y(t), x(r), y(b)))
    }
}

impl Crop {
    /// The pixel rectangle `(x0, y0, x1, y1)` in an image `w` by `h`, never empty.
    fn pixels(self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let at = |v: u16, extent: u32| {
            ((v as u64 * extent as u64 + CROP_UNIT as u64 / 2) / CROP_UNIT as u64) as u32
        };
        let (x0, y0) = (at(self.left, w), at(self.top, h));
        let x0 = x0.min(w.saturating_sub(1));
        let y0 = y0.min(h.saturating_sub(1));
        let x1 = at(self.right, w).clamp(x0 + 1, w.max(x0 + 1));
        let y1 = at(self.bottom, h).clamp(y0 + 1, h.max(y0 + 1));
        (x0, y0, x1, y1)
    }
}

/// The full-size edited picture, encoded: JPEG at `quality`, or PNG when the source has
/// transparency to keep. Returns the bytes and their MIME type.
///
/// Decoded at full resolution, under the same allocation limits as a thumbnail decode
/// (`decode::decode_oriented`). The caller serves the *file* for an untouched photo; this is
/// only ever asked for an edit, where there is no file that holds the picture.
///
/// `quality` is the caller's because the two callers want different things from it: the
/// viewer throws its render away after one look (`FULL_QUALITY`), and an export is the copy
/// the user keeps (`export::EXPORT_QUALITY`).
pub fn render_full(
    path: &Path,
    orientation: u8,
    edit: Edit,
    quality: u8,
) -> Result<(Vec<u8>, &'static str)> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()?;
    let img = edit.apply(apply_orientation(img, orientation));
    let mut bytes = Vec::new();
    let mime = if img.color().has_alpha() {
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )?;
        "image/png"
    } else {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
        // `into_rgb8` hands back the buffer of a photo that already is RGB8, which a JPEG
        // is; `to_rgb8` would copy all of it.
        img.into_rgb8().write_with_encoder(encoder)?;
        "image/jpeg"
    };
    Ok((bytes, mime))
}

/// High enough that a second generation is not visible at 100%, for a render the viewer
/// shows once and throws away. A render the user *keeps* is encoded at
/// `export::EXPORT_QUALITY` instead.
pub const FULL_QUALITY: u8 = 92;

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// 4 wide, 2 tall, every pixel a different colour, so any wrong turn or offset shows.
    fn numbered() -> DynamicImage {
        let mut img = RgbaImage::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([(x * 60) as u8, (y * 200) as u8, 7, 255]));
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    fn crop(left: f64, top: f64, right: f64, bottom: f64) -> Crop {
        let u = |v: f64| (v * CROP_UNIT as f64).round() as u16;
        Crop {
            left: u(left),
            top: u(top),
            right: u(right),
            bottom: u(bottom),
        }
    }

    #[test]
    fn a_crop_survives_the_database_even_with_the_top_bit_set() {
        let c = crop(0.75, 0.5, 1.0, 1.0);
        assert!(
            c.to_db() < 0,
            "left >= 0.5 sets the sign bit of the packed i64"
        );
        assert_eq!(Crop::from_db(c.to_db()), c);
    }

    #[test]
    fn the_untouched_photo_has_one_representation_and_its_old_thumbnail_key() {
        assert_eq!(Edit::new(4, Some(Crop::FULL)), Some(Edit::default()));
        assert_eq!(Edit::default().thumb_key(42), 42);
        let turned = Edit::new(1, None).unwrap();
        let cropped = Edit::new(0, Some(crop(0.0, 0.0, 0.5, 1.0))).unwrap();
        assert_ne!(turned.thumb_key(42), 42);
        assert_ne!(cropped.thumb_key(42), turned.thumb_key(42));
        assert_ne!(turned.thumb_key(42), turned.thumb_key(43));
    }

    #[test]
    fn a_crop_that_is_inverted_or_a_sliver_is_refused() {
        assert_eq!(Edit::new(0, Some(crop(0.6, 0.0, 0.4, 1.0))), None);
        assert_eq!(Edit::new(0, Some(crop(0.0, 0.5, 1.0, 0.5))), None);
        assert_eq!(Edit::new(0, Some(crop(0.0, 0.0, 0.005, 1.0))), None);
        assert!(Edit::new(0, Some(crop(0.0, 0.0, 0.02, 0.02))).is_some());
    }

    #[test]
    fn the_crop_is_taken_from_the_turned_picture() {
        // Turned clockwise the 4x2 picture is 2x4, its left column the old bottom row read
        // left to right going down. The right half of that is the old *top* row.
        let edit = Edit::new(1, Some(crop(0.5, 0.0, 1.0, 1.0))).unwrap();
        let out = edit.apply(numbered()).to_rgba8();
        assert_eq!((out.width(), out.height()), (1, 4));
        assert_eq!(edit.dims(4, 2), (1, 4));
        let src = numbered().to_rgba8();
        for y in 0..4 {
            assert_eq!(out.get_pixel(0, y), src.get_pixel(y, 0), "row {y}");
        }
    }

    #[test]
    fn turning_an_edit_is_turning_its_picture() {
        // The property the rotate button rests on: the crop goes round with the photo, so
        // the same part of it stays framed. An implementation that only bumps `turns`
        // crops a different region after the turn and fails here.
        let start = Edit::new(0, Some(crop(0.25, 0.0, 0.75, 0.5))).unwrap();
        let mut edit = start;
        for step in 1..=4 {
            let before = edit.apply(numbered());
            edit = edit.turned(true);
            assert_eq!(
                edit.apply(numbered()).to_rgba8(),
                before.rotate90().to_rgba8(),
                "after {step} clockwise turns"
            );
        }
        assert_eq!(edit, start, "four quarter turns come back round");
        assert_eq!(start.turned(true).turned(false), start);
        assert_eq!(
            start.turned(false).apply(numbered()).to_rgba8(),
            start.apply(numbered()).rotate270().to_rgba8()
        );
    }

    #[test]
    fn dims_agree_with_the_rendered_picture() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(1000, 667));
        for turns in 0..4 {
            for c in [
                None,
                Some(crop(0.1, 0.2, 0.9, 0.7)),
                Some(crop(0.0, 0.0, 0.333, 1.0)),
            ] {
                let edit = Edit::new(turns, c).unwrap();
                let out = edit.apply(img.clone());
                assert_eq!(
                    edit.dims(1000, 667),
                    (out.width(), out.height()),
                    "{edit:?}"
                );
            }
        }
    }

    #[test]
    fn a_crop_of_a_tiny_picture_is_never_empty() {
        let edit = Edit::new(0, Some(crop(0.9, 0.9, 1.0, 1.0))).unwrap();
        let out = edit.apply(numbered());
        assert_eq!((out.width(), out.height()), (1, 1));
        assert_eq!(edit.dims(4, 2), (1, 1));
    }

    #[test]
    fn a_face_follows_the_edit_or_leaves_with_the_crop() {
        let face = (0.1, 0.2, 0.3, 0.4);
        assert_eq!(Edit::default().map_rect(face), Some(face));

        // A quarter turn clockwise: the top-left region moves to the top right.
        let (l, t, r, b) = Edit::new(1, None).unwrap().map_rect(face).unwrap();
        let close = |a: f64, e: f64| (a - e).abs() < 1e-9;
        assert!(close(l, 0.6) && close(t, 0.1) && close(r, 0.8) && close(b, 0.3));

        // Cropped to the left half: x doubles, y is untouched.
        let left_half = Edit::new(0, Some(crop(0.0, 0.0, 0.5, 1.0))).unwrap();
        let (l, t, r, b) = left_half.map_rect(face).unwrap();
        assert!((l - 0.2).abs() < 1e-4 && (r - 0.6).abs() < 1e-4);
        assert!(close(t, 0.2) && close(b, 0.4));

        // Cropped to the right half: the face's centre is gone, and so is the face.
        let right_half = Edit::new(0, Some(crop(0.5, 0.0, 1.0, 1.0))).unwrap();
        assert_eq!(right_half.map_rect(face), None);

        // Straddling the edge with its centre inside: kept, clipped to the frame.
        let (l, ..) = Edit::new(0, Some(crop(0.15, 0.0, 1.0, 1.0)))
            .unwrap()
            .map_rect(face)
            .unwrap();
        assert_eq!(l, 0.0);
    }

    #[test]
    fn the_full_render_is_the_edited_picture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        numbered().save(&path).unwrap();
        let edit = Edit::new(1, Some(crop(0.0, 0.0, 1.0, 0.5))).unwrap();
        let (bytes, mime) = render_full(&path, 1, edit, FULL_QUALITY).unwrap();
        // The fixture is RGBA, so the render stays lossless and can be compared exactly.
        assert_eq!(mime, "image/png");
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!(out.to_rgba8(), edit.apply(numbered()).to_rgba8());

        let opaque = dir.path().join("b.jpg");
        numbered().to_rgb8().save(&opaque).unwrap();
        let (bytes, mime) = render_full(&opaque, 1, edit, FULL_QUALITY).unwrap();
        assert_eq!(mime, "image/jpeg");
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), edit.dims(4, 2));
    }
}
