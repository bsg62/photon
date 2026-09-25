use std::{fs::File, io::BufReader, path::Path};

/// The generation of [`read_image_meta`] a row was last read with, stored in
/// `items.exif_version`. The scanner re-describes an unchanged file whose stored version is
/// behind this one, which is how a library indexed before a field existed acquires it: bump
/// this when a new field is read, and every folder's next scan re-reads its files once.
/// A header read per file, not a decode.
///
/// 0 is reserved for rows that predate the camera columns (the migration's default).
/// 2 is the first generation that refuses an implausible capture date (see
/// [`plausible_taken_at`]); the bump is what re-dates photos indexed under 1.
/// 3 is the first generation that reads the photo's caption (`keywords::read_embedded`);
/// the bump is what captions photos indexed under 2.
pub const EXIF_VERSION: i64 = 3;

/// Capture dates earlier than this are refused: 1970-01-01, in naive-as-UTC seconds. A
/// camera whose clock was never set writes 0000 or 1900-something, and no digital camera
/// predates the bound. The cost is a scan deliberately back-dated in EXIF to before 1970,
/// which falls back to the file's mtime like a photo with no EXIF at all - accepted when
/// the bound was chosen (spec `2026-09-18-photon-exif-date-sanity-design.md`).
const EARLIEST_TAKEN_AT: i64 = 0;

/// How far past "now" a capture date may lie and still be believed. `taken_at` is the
/// camera's naive local time read as UTC, so an honest photo taken this minute in UTC+14
/// reads fourteen hours ahead; a day covers every timezone with room to spare.
const FUTURE_SLACK_S: i64 = 24 * 60 * 60;

/// Whether a capture date can be believed, given the current time in Unix seconds.
///
/// One file with a broken date is not a local problem: a folder is filed in the sidebar
/// and placed in the grid by its photos' dates, so a single photo dated 4501 drags its
/// whole folder to the top of the library. A refused date makes the caller fall back to
/// the file's mtime, the same path a photo without EXIF takes.
pub(crate) fn plausible_taken_at(taken_at: i64, now: i64) -> bool {
    (EARLIEST_TAKEN_AT..=now + FUTURE_SLACK_S).contains(&taken_at)
}

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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(i64::MAX - FUTURE_SLACK_S, |d| d.as_secs() as i64);
    read_image_meta_at(path, now)
}

/// [`read_image_meta`] with the clock passed in, so the future bound can be tested.
pub(crate) fn read_image_meta_at(path: &Path, now: i64) -> ImageMeta {
    let (dims, exif, avif) = read_header(path);
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
        // An AVIF is turned by its container's `irot`/`imir`, which the decoder applies and
        // `dims` already reflects. libavif keeps a phone's EXIF orientation beside the
        // `irot` it derives from it, so honouring both would turn the photo twice.
        if !avif
            && let Some(o) = exif
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
        // The plausibility check sits inside the search, not after it: a file whose
        // DateTimeOriginal is garbage often still carries a sane DateTime, and that beats
        // falling all the way back to the mtime.
        .find_map(|&tag| {
            exif.get_field(tag, exif::In::PRIMARY)
                .and_then(|f| parse_exif_datetime(&f.value))
                .filter(|&t| plausible_taken_at(t, now))
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

/// Dimensions, EXIF and whether the file is an AVIF, from one open file.
///
/// Both are in the same leading bytes for every format but AVIF, whose `avif_dimensions`
/// reads to the end of the file (see `decode::dimensions`); either way `describe()` runs
/// this for every new or changed photo from one already-open file, not two: opening and
/// header-parsing a file twice doubled the syscalls of an import for nothing, which on a
/// network share or a spinning archive drive is what the import costs.
fn read_header(path: &Path) -> (Option<(u32, u32)>, Option<exif::Exif>, bool) {
    let Ok(file) = File::open(path) else {
        return (None, None, false);
    };
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok();
    // `dimensions` rewinds first: the EXIF read consumed an unspecified amount, and a file
    // with no EXIF at all leaves the cursor wherever the attempt gave up.
    let (dims, avif) = crate::decode::dimensions(&mut reader);
    (dims, exif, avif)
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
    use crate::testutil::{
        ExifSpec, avif_fixture, jpeg_with_exif, jpeg_with_exif_spec, png_bytes, write_file,
    };

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
    fn a_capture_date_is_believed_only_between_1970_and_tomorrow() {
        let now = 1_718_454_645; // 2024-06-15
        assert!(plausible_taken_at(0, now));
        assert!(!plausible_taken_at(-1, now), "1969 predates the bound");
        assert!(
            plausible_taken_at(now + FUTURE_SLACK_S, now),
            "UTC+14 today"
        );
        assert!(!plausible_taken_at(now + FUTURE_SLACK_S + 1, now));
    }

    #[test]
    fn an_implausible_exif_date_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let now = 1_718_454_645; // 2024-06-15
        let future = write_file(
            dir.path(),
            "future.jpg",
            &jpeg_with_exif(4, 2, 1, "4501:01:01 00:00:00"),
        );
        assert_eq!(read_image_meta_at(&future, now).taken_at, None);
        let ancient = write_file(
            dir.path(),
            "ancient.jpg",
            &jpeg_with_exif(4, 2, 1, "1900:01:01 00:00:00"),
        );
        assert_eq!(read_image_meta_at(&ancient, now).taken_at, None);
        // The same file read by a clock past its date is believed: the bound is relative
        // to now, not a constant.
        let sane = write_file(
            dir.path(),
            "sane.jpg",
            &jpeg_with_exif(4, 2, 1, "2024:06:15 12:30:45"),
        );
        assert_eq!(read_image_meta_at(&sane, now).taken_at, Some(1_718_454_645));
        assert_eq!(read_image_meta_at(&sane, 1_000_000_000).taken_at, None);
    }

    #[test]
    fn a_sane_later_date_tag_beats_an_implausible_earlier_one() {
        let dir = tempfile::tempdir().unwrap();
        let spec = ExifSpec {
            datetime: Some("4501:01:01 00:00:00"),
            modified: Some("2024:06:15 12:30:45"),
            ..ExifSpec::default()
        };
        let path = write_file(dir.path(), "a.jpg", &jpeg_with_exif_spec(4, 2, &spec));
        assert_eq!(
            read_image_meta_at(&path, 1_800_000_000).taken_at,
            Some(1_718_454_645)
        );
    }

    /// libavif writes a phone's EXIF orientation into `irot` and keeps the EXIF as it was, so
    /// this file says "rotate" twice. The decoder already applies `irot`; honouring the EXIF
    /// as well would turn the photo a second time. Dimensions are the displayed ones, and
    /// the date still comes from the EXIF.
    #[test]
    fn an_avif_is_oriented_by_its_container_not_its_exif() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            dir.path(),
            "phone.avif",
            &avif_fixture("exif_orientation6.avif"),
        );
        let meta = read_image_meta(&path);
        assert_eq!((meta.width, meta.height, meta.orientation), (32, 64, 1));
        assert_eq!(meta.taken_at, Some(1_718_454_645));
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
