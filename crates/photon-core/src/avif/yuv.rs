//! YUV to RGB for decoded AV1 planes. Nearest chroma sample, no upsampling filter: this
//! feeds thumbnails and edits, and the viewer shows the webview's own decode of the file.

use super::av1::Planes;
use image::{Rgb, RgbImage, Rgba, RgbaImage};

/// H.273 matrix coefficients 0: the planes are G, B, R rather than luma and chroma.
pub(super) const IDENTITY: u16 = 0;

/// Luma weights (Kr, Kb) for an H.273 matrix. Unspecified (2), and every matrix photon does
/// not know, falls back to BT.601 - what libavif assumes in the same case.
fn weights(matrix: u16) -> (f32, f32) {
    match matrix {
        1 => (0.2126, 0.0722),
        // BT.2020 constant-luminance (10) is treated as its non-constant sibling: close
        // enough for a thumbnail, and no camera writes it.
        9 | 10 => (0.2627, 0.0593),
        _ => (0.299, 0.114),
    }
}

/// Maps samples of one bit depth and range onto 0..1 (luma) and -0.5..0.5 (chroma).
#[derive(Clone, Copy)]
struct Levels {
    max: f32,
    scale: f32,
    full: bool,
}

impl Levels {
    fn new(depth: u8, full: bool) -> Self {
        Self {
            max: ((1u32 << depth) - 1) as f32,
            scale: (1u32 << (depth - 8)) as f32,
            full,
        }
    }

    fn luma(self, s: u16) -> f32 {
        if self.full {
            f32::from(s) / self.max
        } else {
            (f32::from(s) - 16.0 * self.scale) / (219.0 * self.scale)
        }
    }

    fn chroma(self, s: u16) -> f32 {
        if self.full {
            (f32::from(s) - (self.max + 1.0) / 2.0) / self.max
        } else {
            (f32::from(s) - 128.0 * self.scale) / (224.0 * self.scale)
        }
    }
}

fn to8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(super) fn to_rgb(p: &Planes, matrix: u16, full_range: bool) -> RgbImage {
    let levels = Levels::new(p.depth, full_range);
    let (kr, kb) = weights(matrix);
    let kg = 1.0 - kr - kb;
    let chroma_width = p.chroma_width();
    RgbImage::from_fn(p.width as u32, p.height as u32, |x, y| {
        let (x, y) = (x as usize, y as usize);
        let luma = levels.luma(p.y[y * p.width + x]);
        if p.mono {
            let g = to8(luma);
            return Rgb([g, g, g]);
        }
        let i = (y >> p.shift.1) * chroma_width + (x >> p.shift.0);
        let (u, v) = (p.u[i], p.v[i]);
        let [r, g, b] = if matrix == IDENTITY {
            [levels.luma(v), luma, levels.luma(u)]
        } else {
            let (cb, cr) = (levels.chroma(u), levels.chroma(v));
            let r = luma + 2.0 * (1.0 - kr) * cr;
            let b = luma + 2.0 * (1.0 - kb) * cb;
            [r, (luma - kr * r - kb * b) / kg, b]
        };
        Rgb([to8(r), to8(g), to8(b)])
    })
}

/// Joins the colour picture with its alpha item's luma, dividing premultiplied colour back
/// out so the result is straight alpha like every other RGBA image `image` hands around.
pub(super) fn attach_alpha(
    rgb: RgbImage,
    alpha: &Planes,
    premultiplied: bool,
) -> Result<RgbaImage, String> {
    if (alpha.width, alpha.height) != (rgb.width() as usize, rgb.height() as usize) {
        return Err("the alpha plane does not match the picture's size".into());
    }
    let levels = Levels::new(alpha.depth, alpha.full_range);
    Ok(RgbaImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        let a = to8(levels.luma(alpha.y[y as usize * alpha.width + x as usize]));
        let straight = |c: u8| {
            if premultiplied && a > 0 {
                ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
            } else {
                c
            }
        };
        let [r, g, b] = rgb.get_pixel(x, y).0;
        Rgba([straight(r), straight(g), straight(b), a])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A width×height picture of one YUV colour, at `depth` bits and `shift` subsampling.
    fn flat(width: usize, height: usize, depth: u8, shift: (u32, u32), yuv: [u16; 3]) -> Planes {
        let mut p = Planes {
            width,
            height,
            depth,
            mono: false,
            shift,
            y: vec![yuv[0]; width * height],
            u: Vec::new(),
            v: Vec::new(),
            matrix: 2,
            full_range: true,
        };
        let chroma = p.chroma_width() * p.chroma_height();
        p.u = vec![yuv[1]; chroma];
        p.v = vec![yuv[2]; chroma];
        p
    }

    fn close(got: [u8; 3], want: [u8; 3]) -> bool {
        got.iter()
            .zip(want)
            .all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= 2)
    }

    /// Pure red coded with BT.709's weights, limited range: Y 63, Cb 102, Cr 240.
    const RED_709_LIMITED: [u16; 3] = [63, 102, 240];

    #[test]
    fn converts_bt709_limited_range() {
        let rgb = to_rgb(&flat(2, 2, 8, (1, 1), RED_709_LIMITED), 1, false);
        let px = rgb.get_pixel(1, 1).0;
        assert!(close(px, [255, 0, 0]), "{px:?}");
    }

    /// The same samples read with BT.601's weights come out visibly wrong: this is what a
    /// matrix mix-up looks like, and why the matrix is read from the file.
    #[test]
    fn the_matrix_changes_the_colour() {
        let rgb = to_rgb(&flat(2, 2, 8, (1, 1), RED_709_LIMITED), 6, false);
        let [r, g, _] = rgb.get_pixel(0, 0).0;
        assert!(
            r < 245 || g > 10,
            "BT.601 should not reproduce BT.709 red: {r} {g}"
        );
    }

    #[test]
    fn full_range_uses_the_whole_scale() {
        // Full-range black and white are 0 and 255, where limited range has 16 and 235.
        let white = to_rgb(&flat(1, 1, 8, (0, 0), [255, 128, 128]), 6, true);
        assert_eq!(white.get_pixel(0, 0).0, [255, 255, 255]);
        let limited_white = to_rgb(&flat(1, 1, 8, (0, 0), [235, 128, 128]), 6, false);
        assert_eq!(limited_white.get_pixel(0, 0).0, [255, 255, 255]);
        let read_as_full = to_rgb(&flat(1, 1, 8, (0, 0), [235, 128, 128]), 6, true);
        assert!(read_as_full.get_pixel(0, 0).0[0] < 240);
    }

    #[test]
    fn deeper_samples_scale_down_to_eight_bits() {
        let white = to_rgb(&flat(1, 1, 10, (0, 0), [1023, 512, 512]), 6, true);
        assert_eq!(white.get_pixel(0, 0).0, [255, 255, 255]);
        let grey = to_rgb(&flat(1, 1, 12, (0, 0), [2048, 2048, 2048]), 6, true);
        assert!(close(grey.get_pixel(0, 0).0, [128, 128, 128]));
    }

    /// Identity (matrix 0) stores G in the luma plane, B in U and R in V.
    #[test]
    fn identity_is_gbr() {
        let rgb = to_rgb(&flat(1, 1, 8, (0, 0), [0, 255, 128]), IDENTITY, true);
        assert_eq!(rgb.get_pixel(0, 0).0, [128, 0, 255]);
    }

    #[test]
    fn monochrome_is_grey() {
        let mut p = flat(2, 1, 8, (0, 0), [200, 0, 0]);
        p.mono = true;
        p.u.clear();
        p.v.clear();
        assert_eq!(to_rgb(&p, 6, true).get_pixel(1, 0).0, [200, 200, 200]);
    }

    /// Each pixel takes the chroma sample covering it: at 4:2:0 on an odd width the last
    /// column reads the rounded-up chroma column, and nothing indexes past the plane.
    #[test]
    fn subsampled_chroma_is_read_per_pixel() {
        let mut p = flat(3, 3, 8, (1, 1), [128, 128, 128]);
        // Chroma is 2×2; make its bottom-right sample red-ish.
        p.v[3] = 255;
        let rgb = to_rgb(&p, 6, true);
        assert!(rgb.get_pixel(2, 2).0[0] > 200, "(2,2) reads chroma (1,1)");
        assert!(rgb.get_pixel(1, 1).0[0] < 160, "(1,1) reads chroma (0,0)");
    }

    #[test]
    fn attaches_alpha_from_its_luma() {
        let rgb = RgbImage::from_pixel(2, 1, Rgb([200, 100, 0]));
        let mut alpha = flat(2, 1, 8, (0, 0), [0, 0, 0]);
        alpha.mono = true;
        alpha.y = vec![128, 255];
        let rgba = attach_alpha(rgb.clone(), &alpha, false).unwrap();
        assert_eq!(rgba.get_pixel(0, 0).0, [200, 100, 0, 128]);
        assert_eq!(rgba.get_pixel(1, 0).0, [200, 100, 0, 255]);
        // Premultiplied colour is divided back out: 100 at alpha 128 was 200 at full.
        let pre = RgbImage::from_pixel(2, 1, Rgb([100, 50, 0]));
        let rgba = attach_alpha(pre, &alpha, true).unwrap();
        assert!(close(
            [rgba.get_pixel(0, 0).0[0], rgba.get_pixel(0, 0).0[1], 0],
            [199, 100, 0]
        ));
        // A mismatched alpha plane is an error, not a panic.
        let mut small = alpha;
        small.width = 1;
        small.y.truncate(1);
        assert!(attach_alpha(rgb, &small, false).is_err());
    }
}
