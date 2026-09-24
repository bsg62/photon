//! AVIF, decoded in pure Rust: `zenavif-parse` for the container, `rav1d` for the AV1
//! inside it. `image` decodes AVIF only through dav1d, a C library, and photon takes no
//! native dependencies (`docs/superpowers/specs/2026-09-24-photon-avif-design.md`).

mod av1;
mod yuv;

use std::borrow::Cow;

use image::error::{DecodingError, ImageFormatHint};
use image::{DynamicImage, ImageError, ImageFormat, ImageResult, RgbImage, imageops};
use zenavif_parse::{
    AV1Metadata, AvifParser, ColorInformation, DecodeConfig, GridConfig, Unstoppable,
};

/// The bound `image`'s default limits put on every other format's decode, which
/// `decode::decode_oriented`'s memory reasoning relies on.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// Whether `head`, the first bytes of a file, is an AVIF: an `ftyp` box whose major *or
/// compatible* brands include `avif` (a still) or `avis` (a sequence). The compatible list
/// matters: a file whose major brand is the generic `mif1` is still an AVIF.
pub(crate) fn is_avif(head: &[u8]) -> bool {
    brands(head).is_some_and(|(major, compatible)| {
        is_avif_brand(&major) || compatible.iter().any(is_avif_brand)
    })
}

/// The major and compatible brands of the `ftyp` box `head` starts with, if it starts with
/// one. Sizes 0 ("to the end") and 1 (64-bit) are legal but no still-image writer uses them
/// for an `ftyp`; anything under 16 cannot hold a major brand and a version.
fn brands(head: &[u8]) -> Option<([u8; 4], &[[u8; 4]])> {
    if head.len() < 16 || &head[4..8] != b"ftyp" {
        return None;
    }
    let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if size < 16 {
        return None;
    }
    let major = [head[8], head[9], head[10], head[11]];
    let compatible = head.get(16..size.min(head.len())).unwrap_or(&[]);
    Some((major, compatible.as_chunks::<4>().0))
}

fn is_avif_brand(brand: &[u8; 4]) -> bool {
    matches!(brand, b"avif" | b"avis")
}

/// Decodes an AVIF to the picture as it is meant to be shown: grid tiles stitched, alpha
/// attached, and the container's crop, rotation and mirror applied.
pub fn decode_avif(bytes: &[u8]) -> ImageResult<DynamicImage> {
    let bytes = with_avif_major(bytes);
    let parser = parse(&bytes)?;
    let img = if parser.grid_tile_count() > 0 {
        // A grid's alpha would be a second grid; photon shows such a photo opaque.
        DynamicImage::ImageRgb8(grid(&parser)?)
    } else {
        let planes = av1::decode(&parser.primary_data().map_err(parse_failed)?).map_err(failed)?;
        let (matrix, full_range) = colour(&parser, &planes);
        let rgb = yuv::to_rgb(&planes, matrix, full_range);
        match parser.alpha_data() {
            None => DynamicImage::ImageRgb8(rgb),
            Some(alpha) => {
                let alpha = av1::decode(&alpha.map_err(parse_failed)?).map_err(failed)?;
                let rgba =
                    yuv::attach_alpha(rgb, &alpha, parser.premultiplied_alpha()).map_err(failed)?;
                DynamicImage::ImageRgba8(rgba)
            }
        }
    };
    Ok(transform(&parser, img))
}

/// The dimensions `decode_avif` would return, read from the container and the AV1 sequence
/// headers alone - no pixel is decoded.
pub fn avif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let bytes = with_avif_major(bytes);
    let parser = parse(&bytes).ok()?;
    let (w, h) = if parser.grid_tile_count() > 0 {
        // A grid item's own payload is the grid description, not AV1: the size comes from
        // the layout, or from a tile when the layout leaves it at zero.
        let tile = AV1Metadata::parse_av1_bitstream(&parser.tile_data(0).ok()?).ok()?;
        grid_size(
            parser.grid_config()?,
            (tile.max_frame_width.get(), tile.max_frame_height.get()),
        )
    } else {
        let meta = parser.primary_metadata().ok()?;
        (meta.max_frame_width.get(), meta.max_frame_height.get())
    };
    let (w, h) = clean_aperture(&parser, w, h).map_or((w, h), |(_, _, cw, ch)| (cw, ch));
    Some(match parser.rotation().map(|r| r.angle) {
        Some(90 | 270) => (h, w),
        _ => (w, h),
    })
}

/// `bytes`, or a copy whose major brand is the `avif`/`avis` it lists as compatible.
/// zenavif-parse refuses any other major brand, even leniently, while libavif and the
/// browsers accept a generic `mif1` file that declares AVIF compatibility. Only this copy
/// is changed - never the file.
fn with_avif_major(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Some((major, compatible)) = brands(bytes) else {
        return Cow::Borrowed(bytes);
    };
    match compatible.iter().find(|brand| is_avif_brand(brand)) {
        Some(brand) if !is_avif_brand(&major) => {
            let mut copy = bytes.to_vec();
            copy[8..12].copy_from_slice(brand);
            Cow::Owned(copy)
        }
        _ => Cow::Borrowed(bytes),
    }
}

fn parse(bytes: &[u8]) -> ImageResult<AvifParser<'_>> {
    let config = DecodeConfig::default()
        .with_peak_memory_limit(MAX_DECODE_BYTES)
        .with_total_megapixels_limit((MAX_DECODE_BYTES / 4 / 1_000_000) as u32);
    AvifParser::from_bytes_with_config(bytes, &config, &Unstoppable).map_err(parse_failed)
}

/// The container's `colr` description where it has one, else the AV1 sequence header's -
/// libavif's order. An ICC-only `colr` says nothing about the matrix, so it falls through.
fn colour(parser: &AvifParser<'_>, planes: &av1::Planes) -> (u16, bool) {
    match parser.color_info() {
        Some(ColorInformation::Nclx {
            matrix_coefficients,
            full_range,
            ..
        }) => (*matrix_coefficients, *full_range),
        _ => (planes.matrix, planes.full_range),
    }
}

fn grid(parser: &AvifParser<'_>) -> ImageResult<RgbImage> {
    let config = parser
        .grid_config()
        .ok_or_else(|| failed("grid without a layout"))?;
    let columns = u32::from(config.columns);
    let count = parser.grid_tile_count();
    if count != usize::from(config.rows) * usize::from(config.columns) {
        return Err(failed("grid tile count does not match its layout"));
    }
    let mut canvas: Option<RgbImage> = None;
    let mut tile_size = (0, 0);
    for index in 0..count {
        let planes =
            av1::decode(&parser.tile_data(index).map_err(parse_failed)?).map_err(failed)?;
        let (matrix, full_range) = colour(parser, &planes);
        let tile = yuv::to_rgb(&planes, matrix, full_range);
        let canvas = match &mut canvas {
            Some(canvas) => canvas,
            None => {
                tile_size = tile.dimensions();
                let (w, h) = grid_size(config, tile_size);
                if w > columns * tile_size.0 || h > u32::from(config.rows) * tile_size.1 {
                    return Err(failed("grid larger than its tiles"));
                }
                canvas.insert(RgbImage::new(w, h))
            }
        };
        if tile.dimensions() != tile_size {
            return Err(failed("grid tiles differ in size"));
        }
        let (row, column) = (index as u32 / columns, index as u32 % columns);
        // `replace` clips at the canvas edge, which is the trim to the declared output size.
        imageops::replace(
            canvas,
            &tile,
            i64::from(column * tile_size.0),
            i64::from(row * tile_size.1),
        );
    }
    canvas.ok_or_else(|| failed("grid with no tiles"))
}

/// A grid's output size, or the tiles' extent when the file leaves it at zero.
fn grid_size(config: &GridConfig, (tile_w, tile_h): (u32, u32)) -> (u32, u32) {
    if config.output_width == 0 || config.output_height == 0 {
        (
            u32::from(config.columns) * tile_w,
            u32::from(config.rows) * tile_h,
        )
    } else {
        (config.output_width, config.output_height)
    }
}

/// `clap`, then `irot`, then `imir`: the order MIAF gives them.
fn transform(parser: &AvifParser<'_>, img: DynamicImage) -> DynamicImage {
    let img = match clean_aperture(parser, img.width(), img.height()) {
        Some((x, y, w, h)) => img.crop_imm(x, y, w, h),
        None => img,
    };
    // `irot` turns anticlockwise; `image` turns clockwise.
    let img = match parser.rotation().map(|r| r.angle) {
        Some(90) => img.rotate270(),
        Some(180) => img.rotate180(),
        Some(270) => img.rotate90(),
        _ => img,
    };
    // Axis 1 flips left-to-right and axis 0 top-to-bottom: libavif's reading, and so the
    // browsers'. zenavif-parse's doc comment on `ImageMirror` says the opposite; the
    // `imir.avif` fixture pins which one the webview will agree with.
    match parser.mirror().map(|m| m.axis) {
        Some(0) => img.flipv(),
        Some(1) => img.fliph(),
        _ => img,
    }
}

/// The `clap` crop as (x, y, width, height), or `None` when there is none, or when it is
/// not a whole-pixel rectangle inside the picture - libavif ignores such a crop too.
fn clean_aperture(parser: &AvifParser<'_>, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let c = parser.clean_aperture()?;
    let ratio = |n: f64, d: u32| (d != 0).then(|| n / f64::from(d));
    let crop_w = ratio(f64::from(c.width_n), c.width_d)?;
    let crop_h = ratio(f64::from(c.height_n), c.height_d)?;
    // The offsets are of the crop's centre from the picture's centre.
    let x = ratio(f64::from(c.horiz_off_n), c.horiz_off_d)? + (f64::from(w) - crop_w) / 2.0;
    let y = ratio(f64::from(c.vert_off_n), c.vert_off_d)? + (f64::from(h) - crop_h) / 2.0;
    let whole =
        |v: f64| (v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32);
    let (x, y, crop_w, crop_h) = (whole(x)?, whole(y)?, whole(crop_w)?, whole(crop_h)?);
    let fits =
        |start: u32, len: u32, total: u32| start.checked_add(len).is_some_and(|end| end <= total);
    let inside = crop_w > 0 && crop_h > 0 && fits(x, crop_w, w) && fits(y, crop_h, h);
    inside.then_some((x, y, crop_w, crop_h))
}

fn failed(message: impl Into<String>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Avif),
        message.into(),
    ))
}

fn parse_failed(err: zenavif_parse::Error) -> ImageError {
    failed(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::avif_fixture;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    /// Decodes `name` and checks its size (both as decoded and as `avif_dimensions`
    /// reports it) and the colour at each point, within ±12 of the source colour: lossy
    /// AV1 moves flat blue to about 243. A matrix mix-up moves red by 22.
    fn check(name: &str, size: (u32, u32), points: &[((u32, u32), [u8; 4])]) {
        let bytes = avif_fixture(name);
        let img = decode_avif(&bytes)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .to_rgba8();
        assert_eq!(img.dimensions(), size, "{name}: decoded size");
        assert_eq!(
            avif_dimensions(&bytes),
            Some(size),
            "{name}: avif_dimensions"
        );
        for &((x, y), want) in points {
            let got = img.get_pixel(x, y).0;
            let near = got
                .iter()
                .zip(want)
                .all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= 12);
            assert!(near, "{name} at ({x},{y}): got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn decodes_bt709_limited_range_from_the_colr_box() {
        check(
            "red_blue_709_limited.avif",
            (64, 32),
            &[((16, 16), RED), ((48, 16), BLUE)],
        );
    }

    /// `colr` is an ICC profile here, so the matrix and range come from the AV1 sequence
    /// header. Defaulting to BT.601 full range instead turns red to about 233.
    #[test]
    fn falls_back_to_the_sequence_header_colour() {
        check(
            "icc_709_limited.avif",
            (64, 32),
            &[((16, 16), RED), ((48, 16), BLUE)],
        );
    }

    #[test]
    fn decodes_full_range_444() {
        check(
            "red_blue_444_full.avif",
            (64, 32),
            &[((16, 16), RED), ((48, 16), BLUE)],
        );
    }

    /// Tiles are laid out row by row: red, green / blue, white.
    #[test]
    fn stitches_a_ten_bit_grid() {
        check(
            "grid_10bit.avif",
            (128, 128),
            &[
                ((32, 32), RED),
                ((96, 32), GREEN),
                ((32, 96), BLUE),
                ((96, 96), WHITE),
            ],
        );
    }

    /// `irot` 1 is 90° anticlockwise: red|blue becomes blue over red.
    #[test]
    fn applies_irot() {
        check("irot90.avif", (32, 64), &[((16, 8), BLUE), ((16, 56), RED)]);
    }

    /// `imir` 1 mirrors left-to-right, as libavif and the browsers read it.
    #[test]
    fn applies_imir() {
        check("imir.avif", (64, 32), &[((8, 16), BLUE), ((56, 16), RED)]);
    }

    #[test]
    fn applies_clap() {
        check("clap.avif", (32, 32), &[((2, 16), RED), ((29, 16), RED)]);
    }

    #[test]
    fn keeps_alpha() {
        check(
            "alpha.avif",
            (32, 32),
            &[((8, 16), [255, 0, 0, 128]), ((24, 16), RED)],
        );
    }

    #[test]
    fn decodes_monochrome() {
        check(
            "mono.avif",
            (64, 32),
            &[((16, 16), BLACK), ((48, 16), WHITE)],
        );
    }

    #[test]
    fn decodes_an_odd_sized_420_picture() {
        check("odd_420.avif", (33, 17), &[((0, 0), RED), ((32, 16), RED)]);
    }

    /// 4:2:2 halves chroma horizontally only. Halving it vertically as well would read the
    /// blue bottom half's chroma from the red top half's rows.
    #[test]
    fn subsamples_422_horizontally_only() {
        check(
            "top_bottom_422.avif",
            (32, 32),
            &[((16, 4), RED), ((16, 28), BLUE)],
        );
    }

    /// A generic `mif1` major brand with `avif` among the compatible ones is an AVIF.
    /// zenavif-parse alone refuses it.
    #[test]
    fn accepts_a_mif1_major_brand() {
        let mut bytes = avif_fixture("red_blue_709_limited.avif");
        assert_eq!(&bytes[8..12], b"avif");
        bytes[8..12].copy_from_slice(b"mif1");
        assert!(is_avif(&bytes));
        let img = decode_avif(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (64, 32));
        assert_eq!(avif_dimensions(&bytes), Some((64, 32)));
    }

    #[test]
    fn sniffs_only_avif_brands() {
        assert!(is_avif(&avif_fixture("mono.avif")));
        // HEIC is the same container with other brands: not ours.
        let mut heic = avif_fixture("mono.avif");
        heic[8..12].copy_from_slice(b"heic");
        let size = u32::from_be_bytes([heic[0], heic[1], heic[2], heic[3]]) as usize;
        for brand in heic[16..size].chunks_mut(4) {
            if brand == b"avif" {
                brand.copy_from_slice(b"heic");
            }
        }
        assert!(!is_avif(&heic));
        assert!(!is_avif(b"\xFF\xD8\xFF\xE0 a jpeg"));
        assert!(!is_avif(b""));
        assert!(
            !is_avif(b"\0\0\0\x08ftypavif"),
            "a box too small to hold its brand"
        );
    }

    #[test]
    fn a_truncated_avif_is_an_error_not_a_panic() {
        let bytes = avif_fixture("red_blue_709_limited.avif");
        for cut in [16, bytes.len() / 2, bytes.len() - 1] {
            let err = decode_avif(&bytes[..cut]).unwrap_err();
            assert!(
                matches!(err, ImageError::Decoding(_)),
                "cut at {cut}: {err:?}"
            );
            assert_eq!(avif_dimensions(&bytes[..cut]), None, "cut at {cut}");
        }
    }
}
