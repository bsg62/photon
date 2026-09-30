use crate::Result;
use crate::avif;
use crate::jpeg;
use crate::turbo;
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageFormat, ImageReader};
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// How much of a filled buffer counts as the file's head when telling an AVIF from anything
/// else: enough for any `ftyp` box a still-image writer produces, brands and all. Sniffing
/// never costs its own read: a `BufReader`'s first fill (its capacity, several KiB) already
/// holds far more than this, so the check only slices the buffer `fill_buf` already has.
const SNIFF_BYTES: u64 = 256;

/// Whether the head of `buf` (capped at [`SNIFF_BYTES`]) is an AVIF.
fn sniffed_avif(buf: &[u8]) -> bool {
    avif::is_avif(&buf[..buf.len().min(SNIFF_BYTES as usize)])
}

/// Decodes any photo photon indexes, recognising the format from its bytes. AVIF is photon's
/// own decoder (`avif`), since `image` reads it only through a C library; everything else is
/// `image`. Every full decode goes through here, so a new format is one branch in one place.
pub fn decode_image(path: &Path) -> Result<DynamicImage> {
    decode_from(BufReader::new(File::open(path)?), path)
}

/// [`decode_image`] over a reader already open on `path`'s contents: the file itself, or the
/// bytes [`preview_decode`] read to try libjpeg-turbo first and then handed back.
fn decode_from<R: BufRead + Seek>(mut reader: R, path: &Path) -> Result<DynamicImage> {
    // `fill_buf` peeks without consuming: the sniff costs no seek and no re-read, unlike a
    // take-and-rewind, which is what `read_header`'s "one open file, few syscalls" reasoning
    // (metadata.rs) needs from this on every describe().
    let is_avif = sniffed_avif(reader.fill_buf()?);
    if is_avif {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        return Ok(avif::decode_avif(&bytes)?);
    }
    let mut image = ImageReader::new(reader);
    // The extension is only the fallback when the bytes say nothing, as with
    // `ImageReader::open`, which this replaces so the file is opened once.
    if let Ok(format) = ImageFormat::from_path(path) {
        image.set_format(format);
    }
    Ok(image.with_guessed_format()?.decode()?)
}

/// Stored dimensions from a reader at any position (it is rewound first), and whether the
/// file is an AVIF, whose orientation lives in its container rather than its EXIF. A header
/// read, not a decode, as `describe()` needs - except for an AVIF, whose container is
/// read to the end before it is parsed.
///
/// A JPEG's size comes from `jpeg::dimensions`, not from `image`, whose JPEG decoder reads
/// the whole file into memory before parsing a byte of it (`JpegDecoder::new` in image
/// 0.25 starts with `read_to_end`). For a 20 MB photo on a network share or a spinning
/// disk that read was nearly all an import paid per photo, and a metadata backfill paid it
/// for dimensions it does not even store.
pub(crate) fn dimensions<R: BufRead + Seek>(reader: &mut R) -> (Option<(u32, u32)>, bool) {
    if reader.seek(SeekFrom::Start(0)).is_err() {
        return (None, false);
    }
    // Peeked, not consumed, for the same reason `decode_image` peeks: this runs once per
    // new or changed file, so an extra seek-and-reread here is the whole scan paying for it.
    let is_avif = reader.fill_buf().is_ok_and(sniffed_avif);
    if is_avif {
        let mut bytes = Vec::new();
        let dims = reader
            .read_to_end(&mut bytes)
            .ok()
            .and_then(|_| avif::avif_dimensions(&bytes));
        return (dims, true);
    }
    if reader.fill_buf().is_ok_and(jpeg::is_jpeg) {
        if let Some(dims) = jpeg::dimensions(reader) {
            return (Some(dims), false);
        }
        // The header walk gives up on some streams zune reads anyway - stray bytes between
        // segments, which zune steps over, are one - so a JPEG it refuses still goes to
        // `image` below: an odd file gets its size at the old cost of a full read, rather
        // than no size at all.
        if reader.seek(SeekFrom::Start(0)).is_err() {
            return (None, false);
        }
    }
    let dims = ImageReader::new(reader)
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok());
    (dims, false)
}

/// Rotates/flips `img` so an image with EXIF `orientation` displays upright.
pub fn apply_orientation(img: DynamicImage, orientation: u8) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Decodes `path` (format sniffed from its bytes), fits it within `max_edge` and orients it.
///
/// The decode runs under `image`'s default limits, which cap one decode's allocations at
/// 512 MiB - roughly a 134 MP photo at RGBA8, so well clear of any camera photon will meet,
/// while a header claiming absurd dimensions is refused before anything is allocated. That
/// is a per-decode bound, not a total: what keeps the sum of the workers' decode buffers
/// bounded is `MAX_WORKERS`, so the two belong together - and AVIF's own peak is roughly 3x a
/// JPEG's for the same pixel count (a rav1d frame, then its planes widened to 16-bit samples,
/// then the RGB8 output), which makes that per-worker bound matter more, not less, for AVIF.
/// An AVIF is bounded differently: `zenavif_parse::DecodeConfig`'s `peak_memory_limit` and
/// `total_megapixels_limit` are inert on the lazy parse path photon uses (see
/// `avif::MAX_DECODE_BYTES`'s doc), so what actually bounds a decode there is rav1d's own
/// per-frame pixel cap (`avif::av1::MAX_FRAME_PIXELS`) plus, for a grid, `avif::grid`'s own
/// check of the declared canvas against `avif::MAX_DECODE_BYTES` before allocating it. The
/// file is still read to the end first (the container has to be parsed before any pixel is
/// decoded), so unlike every other format here, nothing bounds how much of an AVIF file is
/// read into memory before decoding starts.
///
/// The shrink that follows adds to the decode: [`fit_within`] works in the source's width at
/// the preview's height, in the decode's own 8-bit pixels - about a quarter of a 24 MP decode,
/// an eighth of a 134 MP one. `image`'s resize, before it, did the same in 32-bit float RGBA,
/// five times as much and more than the decode itself at 24 MP. At the 512 MiB limit that
/// takes one worker's worst case from about 770 MiB to about 560. `MAX_WORKERS` stays where it
/// is all the same: the cap is there because more workers than the disk can feed buy nothing,
/// and a smaller peak per worker does not change that.
///
/// A JPEG is decoded by libjpeg-turbo at a reduced scale when it can be, and by `image` as
/// above otherwise: see [`preview_decode`], and `turbo` for why the scaled decode keeps the
/// same bound.
pub fn decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage> {
    let (img, _) = preview_decode(path, max_edge)?;
    Ok(apply_orientation(img, orientation))
}

/// Which decoder made a preview. Only the tests read it: it is how they tell the fast path
/// from the fallback, whose pictures `same_picture` cannot tell apart, by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewDecoder {
    /// libjpeg-turbo's scaled decode (`turbo`).
    Turbo,
    /// [`decode_image`] and [`fit_within`], as every preview was made before.
    Image,
}

/// `path` decoded and fitted within `max_edge`, not yet oriented, with which decoder made it.
/// It is [`decode_oriented`] without its last step.
///
/// A JPEG larger than `max_edge` goes to libjpeg-turbo first, which decodes it straight at the
/// smallest n/8 scale that covers the preview. Everything else, and every JPEG libjpeg-turbo
/// hands back, goes to [`decode_image`] and [`fit_within`] as before.
///
/// The file is read once either way. A JPEG is read whole, as `image`'s JPEG decoder reads it
/// anyway, and a handed-back JPEG is decoded from those same bytes. Any other format streams
/// from the file as it always did.
pub(crate) fn preview_decode(path: &Path, max_edge: u32) -> Result<(DynamicImage, PreviewDecoder)> {
    let mut reader = BufReader::new(File::open(path)?);
    if !jpeg::is_jpeg(reader.fill_buf()?) {
        let img = fit_within(decode_from(reader, path)?, max_edge);
        return Ok((img, PreviewDecoder::Image));
    }
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    // The target is the photo's own `fitted` size, not the scaled decode's: n/8 rounds up, so
    // fitting the scaled picture can land a pixel off the size every cached thumbnail of the
    // photo has.
    if let Some((width, height)) = jpeg::dimensions(&mut Cursor::new(bytes.as_slice()))
        && width.max(height) > max_edge
    {
        let target = fitted(width, height, max_edge);
        // The span is what gives turbo's own debug events their path, for the smoke
        // checklist's count of fallbacks: a warning late in a file costs nearly a whole
        // libjpeg decode before zune starts, and that count is how it is noticed.
        let scaled = {
            let _preview = tracing::debug_span!("preview", path = %path.display()).entered();
            turbo::scaled_decode(&bytes, target)
        };
        if let Some(img) = scaled {
            return Ok((resize_to(img, target), PreviewDecoder::Turbo));
        }
    }
    let img = fit_within(decode_from(Cursor::new(bytes), path)?, max_edge);
    Ok((img, PreviewDecoder::Image))
}

/// `img` shrunk to fit within `max_edge` on its long side, with a bilinear filter; an image
/// already that small is returned as it is, never enlarged.
///
/// Not `image`'s own `resize`, which is what this replaced: that is scalar code, and it
/// resamples through a 32-bit float RGBA intermediate as wide as the *source* - ~100 MB for
/// a 24 MP photo shrunk to a preview, on top of the decode, per worker. `fast_image_resize`
/// is pure Rust, picks its SIMD path at run time, and keeps its intermediate in the source's
/// own 8-bit pixels at the *destination's* height (~19 MB for the same photo). The filter is
/// the same tent `image` called `Triangle`, so a thumbnail rendered now and one cached before
/// are the same picture to `similar::same_picture` (a test in this module holds that).
pub fn fit_within(img: DynamicImage, max_edge: u32) -> DynamicImage {
    fit_within_by(img, max_edge, FilterType::Bilinear)
}

/// [`fit_within`] with the caller's filter, for a picture made to be looked at rather than
/// tiled (the clipboard's Lanczos).
pub(crate) fn fit_within_by(img: DynamicImage, max_edge: u32, filter: FilterType) -> DynamicImage {
    if img.width().max(img.height()) <= max_edge {
        return img;
    }
    let size = fitted(img.width(), img.height(), max_edge);
    resize_by(&img, size, filter)
}

/// `img` shrunk to fit within `max_edge`, each pixel the average of the box of source pixels
/// it covers; an image already that small is copied as it is. For a small picture made from
/// one a few times larger that is kept as well - the grid thumbnail from the preview - so it
/// borrows rather than takes the source.
///
/// The box is what `image`'s `thumbnail` averaged over, which made every grid thumbnail
/// cached before this, so the two are the same picture to `similar::same_picture` (a test in
/// `thumbs::cache` holds that). `thumbnail` is scalar; this takes `fast_image_resize`'s SIMD
/// path, about 0.2 ms for a 1600 px preview where `thumbnail` took 3.2.
pub(crate) fn shrink_within(img: &DynamicImage, max_edge: u32) -> DynamicImage {
    if img.width().max(img.height()) <= max_edge {
        return img.clone();
    }
    resize_by(
        img,
        fitted(img.width(), img.height(), max_edge),
        FilterType::Box,
    )
}

/// `img` resized to exactly `size` with [`fit_within`]'s filter. It is for a picture decoded
/// at a scale near the size it must end up at (`turbo`), whose own `fitted` size could be a
/// pixel off the photo's.
fn resize_to(img: DynamicImage, size: (u32, u32)) -> DynamicImage {
    if (img.width(), img.height()) == size {
        return img;
    }
    resize_by(&img, size, FilterType::Bilinear)
}

fn resize_by(img: &DynamicImage, (width, height): (u32, u32), filter: FilterType) -> DynamicImage {
    let mut out = DynamicImage::new(width, height, img.color());
    // Alpha is resampled as it is stored, as `image` did, rather than premultiplied: that
    // would take a premultiplied copy of the whole source first, a second full-size buffer
    // for every transparent PNG, to change nothing for the JPEGs that are nearly every photo.
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(filter))
        .use_alpha(false);
    match Resizer::new().resize(img, &mut out, &options) {
        Ok(()) => out,
        // Only a pixel layout the crate does not know refuses (`DynamicImage` is
        // non-exhaustive, so a future `image` can add one): slower, but still a thumbnail.
        Err(_) => img.resize(width, height, image::imageops::FilterType::Triangle),
    }
}

/// The size `image`'s `resize(max_edge, max_edge, _)` gives `width` by `height`: the aspect
/// kept, each edge rounded to the nearest pixel and never below one. Every thumbnail already
/// in a cache was made at this size, so a photo rendered now matches the ones beside it
/// rather than coming out a pixel narrower.
fn fitted(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let ratio =
        (f64::from(max_edge) / f64::from(width)).min(f64::from(max_edge) / f64::from(height));
    let edge = |v: u32| ((f64::from(v) * ratio).round() as u32).max(1);
    (edge(width), edge(height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::testutil::{
        avif_fixture, bmp_bytes, counted, encode, jpeg_bytes, jpeg_with_segments, noisy_jpeg,
        noisy_rgb, tiff_bytes, write_file,
    };
    use image::{Rgba, RgbaImage};
    use std::fs::File;
    use std::io::BufReader;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

    /// 2×1 image: red on the left, blue on the right.
    fn red_blue() -> DynamicImage {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, RED);
        img.put_pixel(1, 0, BLUE);
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn orientation_changes_dimensions() {
        for o in 1..=8u8 {
            let img = apply_orientation(red_blue(), o);
            let expected = if o >= 5 { (1, 2) } else { (2, 1) };
            assert_eq!((img.width(), img.height()), expected, "orientation {o}");
        }
    }

    #[test]
    fn orientation_moves_pixels() {
        let flipped = apply_orientation(red_blue(), 2).to_rgba8();
        assert_eq!(*flipped.get_pixel(0, 0), BLUE);
        let cw = apply_orientation(red_blue(), 6).to_rgba8();
        assert_eq!((*cw.get_pixel(0, 0), *cw.get_pixel(0, 1)), (RED, BLUE));
        let ccw = apply_orientation(red_blue(), 8).to_rgba8();
        assert_eq!((*ccw.get_pixel(0, 0), *ccw.get_pixel(0, 1)), (BLUE, RED));
    }

    #[test]
    fn decode_downscales_and_orients() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg_bytes(400, 200));
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        let img = decode_oriented(&path, 6, 100).unwrap();
        assert_eq!((img.width(), img.height()), (50, 100));
    }

    /// `fit_within` computes its own output size rather than asking `image`, so this holds it
    /// to `image`'s rounding: across every width from just over the edge to four times it,
    /// on strips of several aspect ratios, both ways round.
    #[test]
    fn a_fitted_picture_is_the_size_images_resize_made_it() {
        let edge = 100;
        for long in edge + 1..=4 * edge {
            for short in [1, 2, 3, 7, 33, 67, 99] {
                for (w, h) in [(long, short), (short, long)] {
                    let img = DynamicImage::new_rgb8(w, h);
                    let theirs = img.resize(edge, edge, image::imageops::FilterType::Nearest);
                    let ours = fit_within(img, edge);
                    assert_eq!(
                        (ours.width(), ours.height()),
                        (theirs.width(), theirs.height()),
                        "{w}x{h}"
                    );
                }
            }
        }
    }

    /// A library's cache mixes thumbnails made before `fit_within` replaced `image`'s resize
    /// with ones made after, and the look-alike check compares them with each other. The two
    /// resamplers must therefore make the same picture as far as `same_picture` can tell.
    ///
    /// The picture is stripes finer than the preview can hold - 0.7975 cycles a pixel, just
    /// under the 0.8 samples a pixel of a 2000-to-1600 shrink - over a gradient and a block.
    /// A filter removes the stripes; a resampler that merely samples (nearest neighbour, say)
    /// folds them into a wave 400 pixels long, which survives down to the 32-pixel comparison.
    #[test]
    fn a_preview_is_the_same_picture_as_the_one_images_resize_made() {
        let (w, h) = (2000, 1000);
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            let wave = (x as f64 * 0.7975 * std::f64::consts::TAU).sin();
            let stripe = (127.0 + 100.0 * wave) as u8;
            let block = if (500..900).contains(&x) && (300..700).contains(&y) {
                40
            } else {
                (x * 255 / w) as u8
            };
            image::Rgb([stripe, block, (y * 255 / h) as u8])
        }));
        let grid = |preview: DynamicImage| crate::similar::reduce(&preview.thumbnail(256, 256));
        let before = grid(img.resize(1600, 1600, image::imageops::FilterType::Triangle));
        let after = grid(fit_within(img, 1600));
        let difference = crate::similar::picture_difference(&before, &after);
        assert!(
            difference < crate::similar::SAME_PICTURE_MAX_DIFFERENCE / 4.0,
            "{difference}"
        );
    }

    #[test]
    fn decode_never_upscales() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg_bytes(40, 20));
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (40, 20));
    }

    #[test]
    fn decode_reports_corrupt_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let bad = write_file(dir.path(), "bad.jpg", b"definitely not a jpeg");
        assert!(matches!(
            decode_oriented(&bad, 1, 100),
            Err(Error::Image(_))
        ));
        let missing = dir.path().join("missing.jpg");
        assert!(matches!(
            decode_oriented(&missing, 1, 100),
            Err(Error::Io(_))
        ));
    }

    /// The fixtures are hand-built byte streams, so this fails on the decode rather than
    /// on a missing encoder when the crate's `tiff`/`bmp` features are off.
    #[test]
    fn decodes_tiff_and_bmp() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [("a.tif", tiff_bytes(40, 20)), ("a.bmp", bmp_bytes(40, 20))] {
            let path = write_file(dir.path(), name, &bytes);
            let img = decode_oriented(&path, 1, 100).expect(name);
            assert_eq!((img.width(), img.height()), (40, 20), "{name}");
        }
    }

    /// AVIF goes through photon's own decoder, whichever function the caller used.
    #[test]
    fn decodes_avif() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.avif", &avif_fixture("irot90.avif"));
        // Upright already: the container's rotation is part of the decode.
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (32, 64));
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&path).unwrap()));
        assert_eq!((dims, avif), (Some((32, 64)), true));
    }

    /// Routing is by content, as for every other format: a JPEG named `.avif` is a JPEG,
    /// and an AVIF named `.jpg` is an AVIF.
    #[test]
    fn routes_by_content_not_extension() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = write_file(dir.path(), "really-a-jpeg.avif", &jpeg_bytes(40, 20));
        assert_eq!(decode_image(&jpeg).unwrap().width(), 40);
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&jpeg).unwrap()));
        assert_eq!((dims, avif), (Some((40, 20)), false));
        let avif = write_file(dir.path(), "really-an-avif.jpg", &avif_fixture("mono.avif"));
        assert_eq!(decode_image(&avif).unwrap().width(), 64);
    }

    /// Sizing a JPEG reads its headers, not the file: behind a megabyte appended after the
    /// end-of-image marker, `image`'s decoder read every byte of it to learn two numbers.
    #[test]
    fn a_jpeg_is_sized_from_its_headers_without_reading_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = jpeg_bytes(40, 20);
        bytes.resize(bytes.len() + 1024 * 1024, 0x5A);
        let path = write_file(dir.path(), "a.jpg", &bytes);
        let mut reader = counted(&path);
        assert_eq!(dimensions(&mut reader), (Some((40, 20)), false));
        let read = reader.get_ref().read;
        assert!(read < 64 * 1024, "read {read} of {} bytes", bytes.len());
    }

    /// A JPEG the header walk gives up on still gets its size from `image`: zune steps over
    /// stray bytes between segments, where the walk stops. A truncated one gets none from
    /// either, and says so rather than failing.
    #[test]
    fn a_jpeg_the_header_walk_refuses_is_sized_by_image() {
        let dir = tempfile::tempdir().unwrap();
        // Two stray bytes after a comment segment (SOI, then FF FE 00 03 'x'), not straight
        // after SOI, where they would stop the file starting the way a JPEG is recognised.
        let jpeg = jpeg_with_segments(40, 20, &[(0xFE, b"x")]);
        let stray = [&jpeg[..7], &[0x00, 0x00], &jpeg[7..]].concat();
        assert_eq!(
            jpeg::dimensions(&mut std::io::Cursor::new(&stray)),
            None,
            "the walk refuses it"
        );
        let path = write_file(dir.path(), "stray.jpg", &stray);
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&path).unwrap()));
        assert_eq!((dims, avif), (Some((40, 20)), false));

        // Cut inside the JFIF segment, ahead of the frame header.
        let path = write_file(dir.path(), "cut.jpg", &jpeg[..20]);
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&path).unwrap()));
        assert_eq!((dims, avif), (None, false));
    }

    /// A broken AVIF is a source defect (the failed-thumbnail placeholder), not an I/O error
    /// the thumbnail service would retry: `is_source_defect` (thumbs/service.rs) treats
    /// `ImageError::IoError` as retryable, so this pins the specific variant `decode_avif`
    /// promises (`Decoding`), not merely `Error::Image(_)`, which an `Unsupported` would also
    /// satisfy without proving the corrupt-file path at all.
    #[test]
    fn decode_reports_a_corrupt_avif_as_an_image_error() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = avif_fixture("red_blue_709_limited.avif");
        let path = write_file(dir.path(), "cut.avif", &bytes[..bytes.len() / 2]);
        assert!(matches!(
            decode_oriented(&path, 1, 100),
            Err(Error::Image(image::ImageError::Decoding(_)))
        ));
    }

    #[test]
    fn a_jpeg_larger_than_the_preview_is_decoded_by_libjpeg_turbo() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "wide.jpg", &noisy_jpeg(3300, 40));
        let (img, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        assert_eq!((img.width(), img.height()), fitted(3300, 40, 1600));
    }

    /// Everything but a large YCbCr JPEG is `image`'s, as every preview was before.
    #[test]
    fn everything_else_is_decoded_by_image() {
        let dir = tempfile::tempdir().unwrap();
        let whole = noisy_jpeg(3300, 40);
        let cases = [
            ("small.jpg", noisy_jpeg(400, 40)),
            // A photo already the preview's size is the one small input the size gate alone
            // decides: its target is the photo itself, which 8/8 covers, so without the gate
            // it would be decoded by libjpeg-turbo for nothing. A smaller one (`small.jpg`)
            // is upscaled by `fitted`, which `scale_for` refuses either way.
            ("edge.jpg", noisy_jpeg(1600, 40)),
            ("wide.png", encode(&noisy_rgb(3300, 40), ImageFormat::Png)),
            (
                "grey.jpg",
                encode(&noisy_rgb(3300, 40).grayscale(), ImageFormat::Jpeg),
            ),
            ("cut.jpg", whole[..whole.len() / 2].to_vec()),
            ("a.avif", avif_fixture("irot90.avif")),
        ];
        for (name, bytes) in cases {
            let path = write_file(dir.path(), name, &bytes);
            // A cut JPEG may be an error from zune: still zune's answer, not the fast path's.
            let decoder = preview_decode(&path, 1600).map(|(_, decoder)| decoder);
            assert!(!matches!(decoder, Ok(PreviewDecoder::Turbo)), "{name}");
        }
    }

    /// At 7/8, 1828x77 comes out 1600x68, a pixel taller than the photo's 1600x67 preview,
    /// which every cached thumbnail of it has. Fitting the scaled picture would keep the 68.
    #[test]
    fn a_preview_decoded_at_a_scale_is_the_photos_own_fitted_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "odd.jpg", &noisy_jpeg(1828, 77));
        let (img, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        assert_eq!((img.width(), img.height()), (1600, 67));
    }

    /// A library's cache mixes thumbnails made by zune with ones made by libjpeg-turbo, and
    /// the look-alike check compares them with each other, so the two must make the same
    /// picture as far as `same_picture` can tell. That is the same bar, and the same reason,
    /// as `a_preview_is_the_same_picture_as_the_one_images_resize_made`.
    #[test]
    fn libjpeg_turbos_preview_is_the_same_picture_as_zunes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "photo.jpg", &noisy_jpeg(1800, 300));
        let (ours, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        let theirs = fit_within(decode_image(&path).unwrap(), 1600);
        assert_eq!(
            (ours.width(), ours.height(), ours.color()),
            (theirs.width(), theirs.height(), theirs.color())
        );
        let grid = |preview: &DynamicImage| crate::similar::reduce(&preview.thumbnail(256, 256));
        let difference = crate::similar::picture_difference(&grid(&ours), &grid(&theirs));
        assert!(
            difference < crate::similar::SAME_PICTURE_MAX_DIFFERENCE / 4.0,
            "{difference}"
        );
    }

    /// Damaged data keeps today's answer exactly: zune's picture, or the `Error::Image` that
    /// marks the photo Failed rather than retrying it (`is_source_defect`, thumbs/service.rs).
    #[test]
    fn a_damaged_jpeg_previews_as_zune_decides_it() {
        let dir = tempfile::tempdir().unwrap();
        let whole = noisy_jpeg(3300, 40);
        let mut corrupt = whole.clone();
        let middle = corrupt.len() / 2;
        corrupt[middle..middle + 64].fill(0xFF);
        for (name, bytes) in [
            ("cut.jpg", whole[..whole.len() / 2].to_vec()),
            ("corrupt.jpg", corrupt),
        ] {
            let path = write_file(dir.path(), name, &bytes);
            let ours = preview_decode(&path, 1600);
            let theirs = decode_image(&path).map(|img| fit_within(img, 1600));
            match (ours, theirs) {
                (Ok((ours, decoder)), Ok(theirs)) => {
                    assert_eq!(decoder, PreviewDecoder::Image, "{name}");
                    assert!(ours.as_bytes() == theirs.as_bytes(), "{name}");
                }
                (Err(Error::Image(_)), Err(Error::Image(_))) => {}
                (ours, theirs) => panic!(
                    "{name}: {:?} against {:?}",
                    ours.map(|(_, decoder)| decoder),
                    theirs.map(|_| ())
                ),
            }
        }
    }
}
