use crate::Result;
use crate::avif;
use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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
    let mut reader = BufReader::new(File::open(path)?);
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
/// read, not a decode, as `describe()` needs.
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
/// bounded is `MAX_WORKERS`, so the two belong together. An AVIF is bounded the same way by
/// `avif::DecodeConfig`'s own 512 MiB limit instead, and the file is read to the end first
/// (the container has to be parsed before any pixel is decoded), so the bound there is on
/// the decode's allocations, not on how much of the file is read.
pub fn decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage> {
    let img = decode_image(path)?;
    let img = if img.width().max(img.height()) > max_edge {
        img.resize(max_edge, max_edge, FilterType::Triangle)
    } else {
        img
    };
    Ok(apply_orientation(img, orientation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::testutil::{avif_fixture, bmp_bytes, jpeg_bytes, tiff_bytes, write_file};
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
}
