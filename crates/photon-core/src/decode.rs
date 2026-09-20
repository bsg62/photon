use crate::Result;
use image::{DynamicImage, ImageReader, imageops::FilterType};
use std::path::Path;

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
/// bounded is `MAX_WORKERS`, so the two belong together.
pub fn decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()?;
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
    use crate::testutil::{bmp_bytes, jpeg_bytes, tiff_bytes, write_file};
    use image::{Rgba, RgbaImage};

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
}
