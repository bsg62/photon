//! AVIF, decoded in pure Rust: `zenavif-parse` for the container, `rav1d` for the AV1
//! inside it. `image` decodes AVIF only through dav1d, a C library, and photon takes no
//! native dependencies (`docs/superpowers/specs/2026-09-24-photon-avif-design.md`).

mod av1;
mod yuv;

use std::borrow::Cow;

use image::error::{DecodingError, ImageFormatHint};
use image::{DynamicImage, ImageError, ImageFormat, ImageResult, RgbImage, imageops};
use zenavif_parse::{AvifParser, ColorInformation, DecodeConfig, Unstoppable};

/// The bound `image`'s default limits put on every other format's decode, which
/// `decode::decode_oriented`'s memory reasoning relies on. AVIF is *not* bounded by
/// `zenavif_parse::DecodeConfig` at this size: in zenavif-parse 0.6.2 `peak_memory_limit` and
/// `total_megapixels_limit` are enforced only by the eager, deprecated
/// `read_avif`/`AvifData` path (behind the crate's `eager` feature, which photon does not
/// enable); the lazy `AvifParser` photon uses applies only `max_grid_tiles` and the animation
/// frame cap. This constant is reused here as the cap `grid()` checks a declared grid canvas
/// against before allocating it - the actual bound on an AVIF decode is that check plus
/// rav1d's own per-frame pixel limit (`av1::MAX_FRAME_PIXELS`), not anything zenavif-parse
/// enforces.
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
        // A grid item's own payload (`primary_data()` on a grid item) is the ImageGrid
        // description, not AV1 - see `grid_layout`'s doc for why this is read directly rather
        // than through zenavif-parse's `grid_config()`.
        let layout = grid_layout(&parser.primary_data().ok()?)?;
        (layout.output_width, layout.output_height)
    } else {
        // The AV1 sequence header's `max_frame_width`/`max_frame_height`, not the container's
        // `ispe` property: equivalent for a still, since the two differ only when the
        // bitstream sets `frame_size_override_flag`, which a still-image encoder does not.
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
    // `DecodeConfig::default()`'s tile and animation-frame caps do apply on this (lazy) parse
    // path. `with_peak_memory_limit`/`with_total_megapixels_limit` used to be set here too, but
    // in zenavif-parse 0.6.2 those two are enforced only by the eager path this crate does not
    // use (see `MAX_DECODE_BYTES`'s doc), so they were dead weight that read as a real bound;
    // removed rather than kept with a comment, so there is nothing here to mistake for one.
    let config = DecodeConfig::default();
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
    let payload = parser.primary_data().map_err(parse_failed)?;
    let layout = grid_layout(&payload).ok_or_else(|| failed("grid without a layout"))?;
    let columns = layout.columns;
    let count = parser.grid_tile_count();
    if count != (layout.rows * layout.columns) as usize {
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
                let (w, h) = (layout.output_width, layout.output_height);
                // Checked before the tile-extent check below (and before allocating): each
                // AV1 frame is bounded by rav1d's own `frame_size_limit`
                // (`av1::MAX_FRAME_PIXELS`), but the *canvas* `RgbImage::new` allocates here is
                // sized from the container-declared `output_width`/`output_height`, which
                // nothing else bounds - zenavif-parse's `peak_memory_limit` and
                // `total_megapixels_limit` are inert on this parse path (see
                // `MAX_DECODE_BYTES`'s doc). An oversized declaration must be refused here,
                // before the `RgbImage::new` below, or allocation failure aborts the process.
                if u64::from(w) * u64::from(h) * 4 > MAX_DECODE_BYTES {
                    return Err(failed("grid larger than the decode bound"));
                }
                if w > columns * tile_size.0 || h > layout.rows * tile_size.1 {
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

/// A grid item's layout, read from the payload `parser.primary_data()` returns for a grid
/// item (its own item data, not a property box).
struct GridLayout {
    rows: u32,
    columns: u32,
    output_width: u32,
    output_height: u32,
}

/// Parses a grid item's ImageGrid payload (ISO/IEC 23008-12 `ImageGrid`):
/// `version` (u8, must be 0), `flags` (u8, bit 0 set means `output_width`/`output_height` are
/// u32, else u16), `rows_minus_one` (u8), `columns_minus_one` (u8), then `output_width` and
/// `output_height` big-endian.
///
/// photon reads this itself rather than using zenavif-parse's `AvifParser::grid_config()`:
/// in zenavif-parse 0.6.2, `calculate_grid_config` only looks for an `ImageGrid` *property*
/// box in `ipco`, which no real file carries - HEIF stores the ImageGrid as the grid item's
/// own data, as read here, not a property of it. Lacking that box, zenavif-parse falls back to
/// dividing the primary item's `ispe` by a tile's `ispe`, and only when that division is exact;
/// otherwise it guesses `rows = tile_count, columns = 1, output 0x0` - so a grid padded to the
/// tile size (the normal phone layout, whose declared output is not an exact multiple of the
/// tile size) is silently decoded as a vertical stack of full, unpadded tiles instead of being
/// trimmed to its real, padded-out dimensions.
fn grid_layout(payload: &[u8]) -> Option<GridLayout> {
    if payload.len() < 4 {
        return None;
    }
    let (version, flags, rows_minus_one, columns_minus_one) =
        (payload[0], payload[1], payload[2], payload[3]);
    if version != 0 {
        return None;
    }
    let rest = &payload[4..];
    let field_size = if flags & 1 != 0 { 4 } else { 2 };
    if rest.len() < field_size * 2 {
        return None;
    }
    let read = |bytes: &[u8]| -> u32 {
        if field_size == 4 {
            u32::from_be_bytes(bytes.try_into().unwrap())
        } else {
            u32::from(u16::from_be_bytes(bytes.try_into().unwrap()))
        }
    };
    Some(GridLayout {
        rows: u32::from(rows_minus_one) + 1,
        columns: u32::from(columns_minus_one) + 1,
        output_width: read(&rest[..field_size]),
        output_height: read(&rest[field_size..field_size * 2]),
    })
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

    /// A progressive (layered) AVIF must decode at its full resolution, not its low-resolution
    /// base layer: `dav1d_default_settings`'s `all_layers = 1` returns only the first spatial
    /// layer, which is half the size on each axis for this fixture.
    #[test]
    fn decodes_a_progressive_avif_at_full_size() {
        check(
            "progressive.avif",
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

    /// A container-declared grid canvas larger than the decode bound must be refused before
    /// `grid()` allocates it, not after: an oversized `RgbImage::new` aborts the process
    /// instead of returning an error. `grid_10bit.avif`'s ImageGrid payload declares a 2x2
    /// grid of 64x64 tiles with a 128x128 output (`[0, 0, 1, 1, 0, 128, 0, 128]`, found in the
    /// item data `grid()` now reads directly via `grid_layout`). This test patches that
    /// payload's `output_width`/`output_height` fields (bytes 4..8) from 128 to 65535 each,
    /// leaving `rows_minus_one`/`columns_minus_one` untouched so the tile count (4, the file's
    /// real physical AV1 tiles) still matches the declared 2x2 layout and the loop reaches the
    /// canvas-size check. 65535 fits the payload's 16-bit field (flags stay 0), and
    /// 65535 * 65535 * 4 bytes is about 17 GB, comfortably past `MAX_DECODE_BYTES` - no need to
    /// widen the fields to 32-bit via the flags byte. `grid()` checks that cap *before* the
    /// pre-existing `w > columns * tile_w` guard, and this test relies on that order: the
    /// patched declaration (65535) also exceeds `columns * tile_w` (2 * 64 = 128, from the
    /// *real* decoded AV1 tile size, unaffected by the payload patch), so without the new check
    /// this fixture would still be refused, just by the older guard - with a different message.
    /// Asserting the message pins that the new check is what actually fired.
    #[test]
    fn refuses_a_grid_canvas_larger_than_the_decode_bound() {
        let bytes = avif_fixture("grid_10bit.avif");
        let bytes = patch_grid_payload_output(&bytes, (128, 128), (65535, 65535));
        let err = decode_avif(&bytes).unwrap_err();
        match &err {
            ImageError::Decoding(e) => assert!(
                e.to_string().contains("grid larger than the decode bound"),
                "wrong guard fired: {err}"
            ),
            other => panic!("expected a Decoding error, got {other:?}"),
        }
    }

    /// Rewrites a grid item's ImageGrid payload's `output_width`/`output_height` (16-bit,
    /// big-endian, right after the 4-byte version/flags/rows-1/columns-1 header) from `old` to
    /// `new`. Asserts exactly one payload in `bytes` declares `old` (as
    /// `[0, 0, rows-1, columns-1, ...old]` is not searched for directly; callers pass the
    /// `output_width`/`output_height` pair, found via the fixed `rows_minus_one = 1,
    /// columns_minus_one = 1` header this crate's grid fixtures share), so the patch cannot
    /// silently hit the wrong bytes or (if the fixture ever changes) do nothing.
    fn patch_grid_payload_output(bytes: &[u8], old: (u16, u16), new: (u16, u16)) -> Vec<u8> {
        let mut needle = vec![0u8, 0, 1, 1];
        needle.extend_from_slice(&old.0.to_be_bytes());
        needle.extend_from_slice(&old.1.to_be_bytes());
        let matches: Vec<_> = bytes
            .windows(needle.len())
            .enumerate()
            .filter(|(_, w)| *w == needle.as_slice())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one ImageGrid payload declaring {old:?}, found {}",
            matches.len()
        );
        let mut out = bytes.to_vec();
        let at = matches[0] + 4; // past version/flags/rows_minus_one/columns_minus_one
        out[at..at + 2].copy_from_slice(&new.0.to_be_bytes());
        out[at + 2..at + 4].copy_from_slice(&new.1.to_be_bytes());
        out
    }

    /// `avifenc --grid 2x2` on a 129x129 four-quadrant source pads each tile to 65x65 (130x130
    /// physical canvas) and declares a 129x129 output in the ImageGrid payload - the normal
    /// phone-camera shape, where the output is not an exact multiple of the tile size. Before
    /// the fix, zenavif-parse's `ispe`-division fallback (129 not divisible by 65) gave up and
    /// reported `rows = 4, columns = 1, output = 0x0`, which `grid()`/`avif_dimensions` decoded
    /// and reported as a 65x260 vertical stack of the four unpadded tiles instead of a trimmed
    /// 129x129 picture.
    #[test]
    fn stitches_a_padded_grid() {
        check(
            "grid_padded.avif",
            (129, 129),
            &[
                ((32, 32), RED),
                ((96, 32), GREEN),
                ((32, 96), BLUE),
                ((96, 96), WHITE),
            ],
        );
    }

    #[test]
    fn grid_layout_reads_16_bit_fields() {
        // version 0, flags 0 (16-bit), rows-1 = 1 (2 rows), columns-1 = 2 (3 columns),
        // output_width = 300, output_height = 200.
        let payload = [0, 0, 1, 2, 0x01, 0x2C, 0x00, 0xC8];
        let layout = grid_layout(&payload).unwrap();
        assert_eq!(layout.rows, 2);
        assert_eq!(layout.columns, 3);
        assert_eq!(layout.output_width, 300);
        assert_eq!(layout.output_height, 200);
    }

    #[test]
    fn grid_layout_reads_32_bit_fields_when_flags_bit_0_is_set() {
        // flags bit 0 set: output_width/output_height are u32, not u16. Using a value that
        // does not fit 16 bits (70000) proves the 32-bit path was actually taken, not just
        // permitted: reading it as two u16s would give the wrong number instead of failing.
        let payload = [0, 1, 0, 0, 0x00, 0x01, 0x11, 0x70, 0x00, 0x00, 0x00, 0x64];
        let layout = grid_layout(&payload).unwrap();
        assert_eq!(layout.rows, 1);
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.output_width, 70_000);
        assert_eq!(layout.output_height, 100);
    }

    #[test]
    fn grid_layout_rejects_a_short_payload() {
        assert!(grid_layout(&[]).is_none());
        assert!(grid_layout(&[0, 0, 0]).is_none());
        // Header present, but only 3 of the required 4 output bytes.
        assert!(grid_layout(&[0, 0, 1, 1, 0, 128, 0]).is_none());
    }

    #[test]
    fn grid_layout_rejects_an_unknown_version() {
        assert!(grid_layout(&[1, 0, 1, 1, 0, 128, 0, 128]).is_none());
    }
}
