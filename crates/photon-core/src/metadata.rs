use std::{fs::File, io::BufReader, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageMeta {
    /// Stored pixel dimensions, before applying `orientation`.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..=8 (1 = upright).
    pub orientation: u8,
    /// Capture time as naive local time interpreted as UTC seconds.
    pub taken_at: Option<i64>,
    /// `Some(0..=5)` once the file has been read; `None` only if it could not be opened.
    pub rating: Option<u8>,
}

/// Reads dimensions and EXIF data. Never fails: missing data falls back to defaults.
pub fn read_image_meta(path: &Path) -> ImageMeta {
    let (width, height) = image::image_dimensions(path).unwrap_or((0, 0));
    let mut meta = ImageMeta {
        width,
        height,
        orientation: 1,
        taken_at: None,
        rating: None,
    };
    // Zero rather than None when there is no packet: the file was read and had nothing to
    // say. None is reserved for rows no scan has ever looked at.
    meta.rating = Some(crate::xmp::read_rating(path).unwrap_or(0));
    if let Some(exif) = read_exif(path) {
        if let Some(o) = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            && (1..=8).contains(&o)
        {
            meta.orientation = o as u8;
        }
        meta.taken_at = [
            exif::Tag::DateTimeOriginal,
            exif::Tag::DateTimeDigitized,
            exif::Tag::DateTime,
        ]
        .iter()
        .find_map(|&tag| {
            exif.get_field(tag, exif::In::PRIMARY)
                .and_then(|f| parse_exif_datetime(&f.value))
        });
    }
    meta
}

/// Dimensions as displayed, after applying the EXIF orientation.
pub fn oriented_dims(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

fn read_exif(path: &Path) -> Option<exif::Exif> {
    let file = File::open(path).ok()?;
    exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()
}

fn parse_exif_datetime(value: &exif::Value) -> Option<i64> {
    let exif::Value::Ascii(parts) = value else {
        return None;
    };
    let dt = exif::DateTime::from_ascii(parts.first()?).ok()?;
    if dt.year == 0 || dt.month == 0 || dt.day == 0 {
        return None;
    }
    Some(naive_to_unix(
        dt.year as i64,
        dt.month as u32,
        dt.day as u32,
        dt.hour as u32,
        dt.minute as u32,
        dt.second as u32,
    ))
}

/// Civil date to Unix seconds (Howard Hinnant's days-from-civil algorithm).
pub(crate) fn naive_to_unix(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_with_exif, png_bytes, write_file};

    #[test]
    fn reads_the_xmp_rating_alongside_exif() {
        use crate::testutil::jpeg_with_xmp;
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "star.jpg", &jpeg_with_xmp(4, 2, 3));
        assert_eq!(read_image_meta(&path).rating, Some(3));
    }

    #[test]
    fn a_photo_without_xmp_reads_as_unrated_rather_than_unread() {
        // Zero, not None: the file was read and had nothing to say. None means "never
        // looked at", which after a scan would be a lie.
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "plain.png", &png_bytes(3, 5));
        assert_eq!(read_image_meta(&path).rating, Some(0));
    }

    #[test]
    fn converts_naive_datetime_to_unix_seconds() {
        assert_eq!(naive_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(naive_to_unix(2000, 3, 1, 0, 0, 0), 951_868_800);
        assert_eq!(naive_to_unix(2024, 6, 15, 12, 30, 45), 1_718_454_645);
    }

    #[test]
    fn reads_exif_orientation_and_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            dir.path(),
            "a.jpg",
            &jpeg_with_exif(4, 2, 6, "2024:06:15 12:30:45"),
        );
        assert_eq!(
            read_image_meta(&path),
            ImageMeta {
                width: 4,
                height: 2,
                orientation: 6,
                taken_at: Some(1_718_454_645),
                rating: Some(0)
            }
        );
    }

    #[test]
    fn png_without_exif_gets_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.png", &png_bytes(3, 5));
        assert_eq!(
            read_image_meta(&path),
            ImageMeta {
                width: 3,
                height: 5,
                orientation: 1,
                taken_at: None,
                rating: Some(0)
            }
        );
    }

    #[test]
    fn unreadable_file_yields_zeroed_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "bad.jpg", b"not an image");
        assert_eq!(
            read_image_meta(&path),
            ImageMeta {
                width: 0,
                height: 0,
                orientation: 1,
                taken_at: None,
                rating: Some(0)
            }
        );
    }

    #[test]
    fn rejects_zeroed_exif_dates() {
        let value = exif::Value::Ascii(vec![b"0000:00:00 00:00:00".to_vec()]);
        assert_eq!(parse_exif_datetime(&value), None);
    }

    #[test]
    fn oriented_dims_swap_for_quarter_turns() {
        assert_eq!(oriented_dims(4, 2, 1), (4, 2));
        assert_eq!(oriented_dims(4, 2, 3), (4, 2));
        for o in 5..=8 {
            assert_eq!(oriented_dims(4, 2, o), (2, 4));
        }
    }
}
