use std::{
    fs::File,
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

/// The generation of [`read_image_meta`] a row was last read with, stored in
/// `items.exif_version`. The scanner re-describes an unchanged file whose stored version is
/// behind this one, which is how a library indexed before a field existed acquires it: bump
/// this when a new field is read, and every folder's next scan re-reads its files once.
/// A header read per file, not a decode.
///
/// 0 is reserved for rows that predate the camera columns (the migration's default).
pub const EXIF_VERSION: i64 = 1;

/// What the camera wrote about itself and the exposure. Every field is optional because
/// every field is: a phone omits the lens, a scan omits everything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraMeta {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    /// Focal length in millimetres, as shot (no 35 mm equivalent).
    pub focal_mm: Option<f64>,
    /// The f-number.
    pub aperture: Option<f64>,
    /// Exposure time in seconds.
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageMeta {
    /// Stored pixel dimensions, before applying `orientation`.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..=8 (1 = upright).
    pub orientation: u8,
    /// Capture time as naive local time interpreted as UTC seconds.
    pub taken_at: Option<i64>,
    /// Always `None`: ratings come from Picasa's per-directory INI, applied by the scanner
    /// after the walk (see `scanner::apply_picasa`), not from anything in the file itself.
    /// Kept on the struct because `NewItem` still carries the column.
    pub rating: Option<u8>,
    pub camera: CameraMeta,
}

/// Reads dimensions and EXIF data. Never fails: missing data falls back to defaults.
pub fn read_image_meta(path: &Path) -> ImageMeta {
    let (dims, exif) = read_header(path);
    let (width, height) = dims.unwrap_or((0, 0));
    let mut meta = ImageMeta {
        width,
        height,
        orientation: 1,
        taken_at: None,
        rating: None,
        camera: CameraMeta::default(),
    };
    if let Some(exif) = exif {
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
        meta.camera = read_camera(&exif);
    }
    meta
}

/// The camera fields, each `None` when absent or unusable. Fields in the Exif sub-IFD
/// (lens, exposure) belong to the primary image as far as `In` is concerned, the same way
/// `DateTimeOriginal` does above.
fn read_camera(exif: &exif::Exif) -> CameraMeta {
    let text = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| ascii_text(&f.value))
    };
    let rational = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| rational_f64(&f.value))
    };
    CameraMeta {
        make: text(exif::Tag::Make),
        model: text(exif::Tag::Model),
        lens: text(exif::Tag::LensModel),
        focal_mm: rational(exif::Tag::FocalLength),
        aperture: rational(exif::Tag::FNumber),
        exposure_s: rational(exif::Tag::ExposureTime),
        iso: exif
            .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .filter(|&iso| iso > 0)
            .map(i64::from),
    }
}

/// An ASCII field as text. Cameras pad these with spaces and NULs, and some write an empty
/// string rather than omitting the tag; both come back as `None` rather than as "" or " ".
/// Decoded lossily: the field is nominally ASCII, but a few firmwares write Latin-1 or
/// UTF-8 into it, and a make name with one bad byte is still a make name.
fn ascii_text(value: &exif::Value) -> Option<String> {
    let exif::Value::Ascii(parts) = value else {
        return None;
    };
    let text = String::from_utf8_lossy(parts.first()?);
    let text = text
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The first rational as a float, or `None` for a zero denominator (a value some cameras
/// write for "unknown") or a non-positive result, which no focal length, f-number or
/// exposure can have.
fn rational_f64(value: &exif::Value) -> Option<f64> {
    let exif::Value::Rational(parts) = value else {
        return None;
    };
    let r = parts.first()?;
    if r.denom == 0 {
        return None;
    }
    let f = r.to_f64();
    (f > 0.0).then_some(f)
}

/// Dimensions as displayed, after applying the EXIF orientation.
pub fn oriented_dims(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

/// Dimensions and EXIF from one open file.
///
/// Both are in the same leading bytes, and `describe()` runs this for every new or changed
/// photo: opening and header-parsing the file twice doubled the syscalls of an import for
/// nothing, which on a network share or a spinning archive drive is what the import costs.
fn read_header(path: &Path) -> (Option<(u32, u32)>, Option<exif::Exif>) {
    let Ok(file) = File::open(path) else {
        return (None, None);
    };
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok();
    // Back to the start: the EXIF read consumed an unspecified amount, and a file with no
    // EXIF at all leaves the cursor wherever the attempt gave up.
    let dims = reader
        .seek(SeekFrom::Start(0))
        .ok()
        .and_then(|_| {
            image::ImageReader::new(&mut reader)
                .with_guessed_format()
                .ok()
        })
        .and_then(|r| r.into_dimensions().ok());
    (dims, exif)
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

/// Unix seconds to a civil (year, month, day) in UTC: the inverse of [`naive_to_unix`],
/// same algorithm. Used to spell a capture date as `YYYY-MM-DD` for search, where the
/// stored value is the camera's wall-clock time and UTC is the right zone to read it in.
pub fn civil_from_unix(secs: i64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A capture time as `YYYY-MM-DD`, for search.
pub fn date_text(secs: i64) -> String {
    let (y, m, d) = civil_from_unix(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{ExifSpec, jpeg_with_exif, jpeg_with_exif_spec, png_bytes, write_file};

    #[test]
    fn converts_naive_datetime_to_unix_seconds() {
        assert_eq!(naive_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(naive_to_unix(2000, 3, 1, 0, 0, 0), 951_868_800);
        assert_eq!(naive_to_unix(2024, 6, 15, 12, 30, 45), 1_718_454_645);
    }

    #[test]
    fn civil_from_unix_inverts_naive_to_unix() {
        for (y, m, d) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 6, 15),
            (1999, 12, 31),
            (1969, 12, 31),
        ] {
            assert_eq!(
                civil_from_unix(naive_to_unix(y, m, d, 23, 59, 59)),
                (y, m, d)
            );
        }
        assert_eq!(date_text(1_718_454_645), "2024-06-15");
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
                rating: None,
                camera: CameraMeta::default(),
            }
        );
    }

    #[test]
    fn reads_the_camera_lens_and_exposure_fields() {
        let dir = tempfile::tempdir().unwrap();
        let spec = ExifSpec {
            make: Some("Canon"),
            model: Some("Canon EOS 5D Mark IV"),
            lens: Some("EF50mm f/1.8 STM"),
            focal: Some((50, 1)),
            fnumber: Some((18, 10)),
            exposure: Some((1, 250)),
            iso: Some(400),
            ..ExifSpec::default()
        };
        let path = write_file(dir.path(), "a.jpg", &jpeg_with_exif_spec(4, 2, &spec));
        assert_eq!(
            read_image_meta(&path).camera,
            CameraMeta {
                make: Some("Canon".into()),
                model: Some("Canon EOS 5D Mark IV".into()),
                lens: Some("EF50mm f/1.8 STM".into()),
                focal_mm: Some(50.0),
                aperture: Some(1.8),
                exposure_s: Some(0.004),
                iso: Some(400),
            }
        );
    }

    #[test]
    fn padded_strings_and_zero_denominators_read_as_absent() {
        // Cameras pad Make/Model with spaces or NULs to a fixed width, and write 0/0 for a
        // focal length they do not know. Neither is a value: an aperture of "inf" or a make
        // of "   " would render in the info panel and match every search.
        let dir = tempfile::tempdir().unwrap();
        let spec = ExifSpec {
            make: Some("NIKON CORPORATION   "),
            model: Some("   "),
            lens: Some("\0\0"),
            focal: Some((0, 0)),
            fnumber: Some((0, 10)),
            iso: Some(0),
            ..ExifSpec::default()
        };
        let path = write_file(dir.path(), "a.jpg", &jpeg_with_exif_spec(4, 2, &spec));
        assert_eq!(
            read_image_meta(&path).camera,
            CameraMeta {
                make: Some("NIKON CORPORATION".into()),
                ..CameraMeta::default()
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
                rating: None,
                camera: CameraMeta::default(),
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
                rating: None,
                camera: CameraMeta::default(),
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
