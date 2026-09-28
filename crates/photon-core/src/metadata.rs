use crate::keywords::Embedded;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

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

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(i64::MAX - FUTURE_SLACK_S, |d| d.as_secs() as i64)
}

/// Reads dimensions and EXIF data. Never fails: missing data falls back to defaults.
pub fn read_image_meta(path: &Path) -> ImageMeta {
    read_image_meta_at(path, unix_now())
}

/// [`read_image_meta`] with the clock passed in, so the future bound can be tested.
pub(crate) fn read_image_meta_at(path: &Path, now: i64) -> ImageMeta {
    let (dims, exif, avif) = read_header(path);
    meta_from(dims, exif, avif, now)
}

/// Everything `describe()` reads out of a photo file: what [`read_image_meta`] and
/// [`crate::keywords::read_embedded`] answer, answered the same, from one open file and one
/// read of its leading bytes. Never fails, as neither of those does.
///
/// The keywords and caption need the first `xmp::MAX_PREFIX` bytes whatever the format, and
/// the header of every format but a large AVIF lies inside them, so the header is parsed out
/// of the prefix already in memory ([`Prefixed`]) and the file is read again only past it.
/// Read separately, the two opened the file twice and read its head twice - once in the
/// header parser's small buffered reads, once in one go - and on a network share or a
/// spinning archive drive, per-file syscalls are what an import costs.
pub fn read_image(path: &Path) -> (ImageMeta, Embedded) {
    let Ok(file) = File::open(path) else {
        return (
            meta_from(None, None, false, unix_now()),
            Embedded::default(),
        );
    };
    read_image_from(file, unix_now())
}

/// [`read_image`] over an open file, with the clock passed in, so a test can count what it
/// reads.
fn read_image_from<R: Read + Seek>(mut file: R, now: i64) -> (ImageMeta, Embedded) {
    let mut prefix = Vec::new();
    let (embedded, prefix, inner_pos) = match crate::xmp::read_prefix(&mut file, &mut prefix) {
        Ok(_) => {
            let end = prefix.len() as u64;
            (crate::keywords::embedded_in(&prefix), prefix, Some(end))
        }
        // `read_embedded` answers nothing for a failed read. The header is read from the
        // start of the file, as it was when it had an open file of its own, rather than from
        // what a failed read left behind, with the cursor wherever it gave up.
        Err(_) => (Embedded::default(), Vec::new(), None),
    };
    let mut reader = BufReader::new(Prefixed {
        prefix,
        inner: file,
        pos: 0,
        inner_pos,
    });
    let (dims, exif, avif) = read_header_from(&mut reader);
    (meta_from(dims, exif, avif, now), embedded)
}

/// A file whose leading bytes were already read into `prefix`: reads there are served from
/// memory and reads beyond it from the file, so a header parser can seek anywhere in the
/// file without its head being read twice.
struct Prefixed<R> {
    prefix: Vec<u8>,
    inner: R,
    /// Where the next read starts, in the file.
    pos: u64,
    /// Where `inner`'s own cursor is, when that is known. A read past the prefix seeks
    /// `inner` only when it is somewhere else: after the prefix read it sits at the prefix's
    /// end, which is where a parser reading on out of the prefix continues from.
    inner_pos: Option<u64>,
}

impl<R: Read + Seek> Read for Prefixed<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        let rest = usize::try_from(self.pos)
            .ok()
            .and_then(|pos| self.prefix.get(pos..))
            .unwrap_or_default();
        if !rest.is_empty() {
            n = rest.len().min(buf.len());
            buf[..n].copy_from_slice(&rest[..n]);
            self.pos += n as u64;
            if n == buf.len() {
                return Ok(n);
            }
        }
        // A read that runs off the end of the prefix goes on into the file, as one read of
        // the file would have. A short read here is within `Read`'s contract and no parser
        // in the tree is known to mind one, but the promise is that the header parsers see
        // what a plain file shows them, and a file never cut a read at 256 KiB.
        let read = if self.inner_pos == Some(self.pos) {
            self.inner.read(&mut buf[n..])
        } else {
            self.inner
                .seek(SeekFrom::Start(self.pos))
                .and_then(|_| self.inner.read(&mut buf[n..]))
        };
        match read {
            Ok(m) => {
                self.pos += m as u64;
                self.inner_pos = Some(self.pos);
                Ok(n + m)
            }
            Err(e) => {
                self.inner_pos = None;
                // What came out of the prefix was read; the error surfaces on the next call.
                if n > 0 { Ok(n) } else { Err(e) }
            }
        }
    }
}

impl<R: Seek> Seek for Prefixed<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(pos) => pos,
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "seek before the start")
            })?,
            // Only the file knows where it ends.
            SeekFrom::End(_) => {
                let pos = self.inner.seek(to)?;
                self.inner_pos = Some(pos);
                pos
            }
        };
        Ok(self.pos)
    }
}

/// The [`ImageMeta`] a header read yields.
fn meta_from(
    dims: Option<(u32, u32)>,
    exif: Option<exif::Exif>,
    avif: bool,
    now: i64,
) -> ImageMeta {
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
/// reads to the end of the file (see `decode::dimensions`); either way both come from one
/// already-open file, not two: opening and header-parsing a file twice doubled the syscalls
/// of an import for nothing, which on a network share or a spinning archive drive is what
/// the import costs. `describe()` reads the header through [`read_image`], which shares that
/// one open file with the keyword and caption read as well.
fn read_header(path: &Path) -> (Option<(u32, u32)>, Option<exif::Exif>, bool) {
    let Ok(file) = File::open(path) else {
        return (None, None, false);
    };
    read_header_from(&mut BufReader::new(file))
}

/// [`read_header`] over an open file, so a test can count what it reads.
///
/// A JPEG is read by `jpeg::head`, one walk of its headers for both answers; see there for
/// why kamadak-exif's own search is not used for one. Anything else, and a JPEG whose
/// headers that walk cannot follow, goes the general way.
fn read_header_from<R: BufRead + Seek>(
    reader: &mut R,
) -> (Option<(u32, u32)>, Option<exif::Exif>, bool) {
    if reader.fill_buf().is_ok_and(crate::jpeg::is_jpeg) {
        if let Some(head) = crate::jpeg::head(reader) {
            let exif = head
                .exif
                .and_then(|tiff| exif::Reader::new().read_raw(tiff).ok());
            return (Some(head.dims), exif, false);
        }
        if reader.seek(SeekFrom::Start(0)).is_err() {
            return (None, None, false);
        }
    }
    let exif = exif::Reader::new().read_from_container(reader).ok();
    // `dimensions` rewinds first: the EXIF read consumed an unspecified amount, and a file
    // with no EXIF at all leaves the cursor wherever the attempt gave up.
    let (dims, avif) = crate::decode::dimensions(reader);
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
    let mut out = String::with_capacity(10);
    write_date_text(&mut out, secs);
    out
}

/// [`date_text`] appended to `out`, for search, which writes it once per photo into a
/// buffer it reuses rather than allocating a string each time. One spelling, so the two
/// cannot drift apart.
pub fn write_date_text(out: &mut String, secs: i64) {
    use std::fmt::Write;
    let (y, m, d) = civil_from_unix(secs);
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{y:04}-{m:02}-{d:02}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        ExifSpec, avif_fixture, counted, gif_with_xmp, iptc_app13_datasets, jpeg_bytes,
        jpeg_with_exif, jpeg_with_exif_spec, jpeg_with_segments, png_bytes, png_with_xmp,
        write_file, xmp_packet_with_subjects,
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

    /// The whole of what `describe()` reads for a JPEG's size and EXIF is its head, with
    /// EXIF or without: a megabyte of scan data goes unread. Without is the case
    /// `jpeg::head` is for - kamadak-exif's own search for the block goes on through the
    /// scan data to the end-of-image marker, which in a photo is nearly the whole file.
    #[test]
    fn a_jpeg_header_read_stops_at_its_headers() {
        let dir = tempfile::tempdir().unwrap();
        for (name, mut bytes, has_exif) in [
            (
                "camera.jpg",
                jpeg_with_exif(40, 20, 6, "2024:06:15 12:30:45"),
                true,
            ),
            ("export.jpg", jpeg_bytes(40, 20), false),
        ] {
            // Ahead of the end-of-image marker, where a photo's bulk is: bytes appended after
            // it would never be reached by a search that stops there.
            let eoi = bytes.len() - 2;
            assert_eq!(bytes[eoi..], [0xFF, 0xD9]);
            bytes.splice(eoi..eoi, std::iter::repeat_n(0x5A, 1024 * 1024));
            let path = write_file(dir.path(), name, &bytes);
            let mut reader = counted(&path);
            let (dims, exif, avif) = read_header_from(&mut reader);
            assert_eq!(
                (dims, exif.is_some(), avif),
                (Some((40, 20)), has_exif, false),
                "{name}"
            );
            let read = reader.get_ref().read;
            assert!(
                read < 64 * 1024,
                "{name}: read {read} of {} bytes",
                bytes.len()
            );
        }
    }

    /// A defect between the frame header and a misplaced EXIF block does not cost the
    /// photo its EXIF: kamadak-exif's search steps over stray bytes the header walk refuses,
    /// so the walk hands such a file back to it instead of reading it as having none.
    #[test]
    fn exif_behind_a_defect_after_the_frame_header_is_still_read() {
        let camera = jpeg_with_exif(40, 20, 6, "2024:06:15 12:30:45");
        let len = usize::from(u16::from_be_bytes([camera[4], camera[5]]));
        let app1 = &camera[2..4 + len]; // the whole segment, marker included
        let plain = jpeg_bytes(40, 20);
        // Past the fixture's baseline frame header: its marker (2) and 17-byte segment.
        let after = plain
            .windows(4)
            .position(|w| w == [0xFF, 0xC0, 0x00, 0x11])
            .unwrap()
            + 2
            + 0x11;
        let bytes = [&plain[..after], &[0x00, 0x00], app1, &plain[after..]].concat();
        let (dims, exif, _) = read_header_from(&mut std::io::Cursor::new(&bytes));
        assert_eq!(dims, Some((40, 20)));
        let exif = exif.expect("the EXIF behind the stray bytes");
        assert_eq!(
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0)),
            Some(6)
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

    /// A JPEG as a camera and a tagging tool leave it: EXIF, then XMP keywords and an IPTC
    /// caption, then `scan` bytes of scan data ahead of the end-of-image marker.
    fn tagged_jpeg(scan: usize) -> Vec<u8> {
        let spec = ExifSpec {
            orientation: Some(6),
            datetime: Some("2024:06:15 12:30:45"),
            ..ExifSpec::default()
        };
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&crate::testutil::exif_tiff(&spec));
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(xmp_packet_with_subjects(&["beach", "東京"]).as_bytes());
        let iptc = iptc_app13_datasets(&[(120, b"From Picasa"), (25, b"family")]);
        let mut bytes = jpeg_with_segments(40, 20, &[(0xE1, &exif), (0xE1, &xmp), (0xED, &iptc)]);
        let eoi = bytes.len() - 2;
        bytes.splice(eoi..eoi, std::iter::repeat_n(0x5A, scan));
        bytes
    }

    /// `describe()`'s read takes a file's head off the disk once: the keyword read's prefix,
    /// and nothing more, since the header parser reads the same bytes out of memory. Read
    /// on their own, the header walk's buffered reads came on top of the prefix.
    #[test]
    fn describe_reads_a_file_s_head_once() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("large.jpg", tagged_jpeg(1024 * 1024)),
            ("small.jpg", tagged_jpeg(0)),
        ] {
            let path = write_file(dir.path(), name, &bytes);
            let mut file = crate::testutil::Counting::new(File::open(&path).unwrap());
            let (meta, embedded) = read_image_from(&mut file, 1_800_000_000);
            assert_eq!(
                (meta.width, meta.height, meta.orientation, meta.taken_at),
                (40, 20, 6, Some(1_718_454_645)),
                "{name}"
            );
            assert_eq!(embedded.keywords, ["beach", "東京", "family"], "{name}");
            assert_eq!(embedded.caption.as_deref(), Some("From Picasa"), "{name}");
            assert_eq!(
                file.read,
                bytes.len().min(crate::xmp::MAX_PREFIX) as u64,
                "{name}: of {} bytes",
                bytes.len()
            );
        }
    }

    /// An AVIF's size is read from the whole file, so a phone's multi-megabyte AVIF is read
    /// through past the prefix: on from where the prefix read left the file, every byte
    /// read once and no seek spent getting back to where the file already is.
    #[test]
    fn a_large_avif_is_read_once_straight_through() {
        let mut bytes = avif_fixture("exif_orientation6.avif");
        // A top-level `free` box is how an encoder pads a file; the parser steps over it.
        bytes.extend_from_slice(&crate::testutil::mp4_box(b"free", &vec![0; 300_000]));
        assert!(bytes.len() > crate::xmp::MAX_PREFIX);
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "phone.avif", &bytes);
        let mut file = crate::testutil::Counting::new(File::open(&path).unwrap());
        let (meta, _) = read_image_from(&mut file, 1_800_000_000);
        assert_eq!((meta.width, meta.height, meta.orientation), (32, 64, 1));
        assert_eq!((file.read, file.seeks), (bytes.len() as u64, 0));
    }

    /// The one read answers what the two separate reads it replaced answer, for every kind
    /// of file `describe()` sees - including a header that lies past the prefix, which is
    /// read from the file beyond it, and an AVIF, whose size is read to the end of the file.
    #[test]
    fn read_image_answers_what_the_separate_reads_answer() {
        let dir = tempfile::tempdir().unwrap();
        // Five 65,000-byte APP2 segments ahead of the frame header put it at ~325 KiB.
        let filler = vec![0x11; 65_000];
        let far: Vec<(u8, &[u8])> = (0..5).map(|_| (0xE2, filler.as_slice())).collect();
        let far_header = jpeg_with_segments(30, 10, &far);
        assert!(far_header.len() > crate::xmp::MAX_PREFIX);
        let mut files = vec![
            ("tagged.jpg", tagged_jpeg(1024 * 1024)),
            ("small.jpg", tagged_jpeg(0)),
            ("far.jpg", far_header),
            ("plain.jpg", jpeg_bytes(8, 8)),
            ("a.png", png_with_xmp(8, 8, 4)),
            ("a.gif", gif_with_xmp(8, 8, 2)),
            ("a.tif", crate::testutil::tiff_bytes(8, 8)),
            ("empty.jpg", Vec::new()),
        ];
        for avif in ["exif_orientation6.avif", "irot90.avif", "grid_padded.avif"] {
            files.push((avif, avif_fixture(avif)));
        }
        let mut padded = avif_fixture("irot90.avif");
        padded.extend_from_slice(&crate::testutil::mp4_box(b"free", &vec![0; 300_000]));
        files.push(("padded.avif", padded));
        for (name, bytes) in &files {
            let path = write_file(dir.path(), name, bytes);
            assert_eq!(
                read_image(&path),
                (
                    read_image_meta(&path),
                    crate::keywords::read_embedded(&path)
                ),
                "{name}"
            );
        }
        let far = read_image(&dir.path().join("far.jpg")).0;
        assert_eq!((far.width, far.height), (30, 10), "found past the prefix");
        let missing = dir.path().join("missing.jpg");
        assert_eq!(
            read_image(&missing),
            (read_image_meta(&missing), Embedded::default())
        );
    }

    /// Every read and seek a header parser makes lands on the file's own bytes, wherever it
    /// falls against the prefix: inside it, across its end, past it, and back into it.
    #[test]
    fn a_prefixed_file_reads_as_the_file() {
        let file: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut r = Prefixed {
            prefix: file[..300].to_vec(),
            inner: std::io::Cursor::new(file.clone()),
            pos: 0,
            inner_pos: None,
        };
        // Start in the prefix and cross its end in one read, which comes back whole rather
        // than cut at a boundary the caller cannot see.
        let mut buf = [0; 100];
        r.seek(SeekFrom::Start(250)).unwrap();
        assert_eq!(r.read(&mut buf).unwrap(), 100);
        assert_eq!(buf, file[250..350]);
        // On past it, then back inside it, then past it again from a different place than
        // the file's cursor was left at.
        assert_eq!(r.read(&mut buf).unwrap(), 100);
        assert_eq!(buf, file[350..450]);
        r.seek(SeekFrom::Current(-400)).unwrap();
        assert_eq!(r.read(&mut buf).unwrap(), 100);
        assert_eq!(buf, file[50..150]);
        r.seek(SeekFrom::Start(700)).unwrap();
        assert_eq!(r.read(&mut buf).unwrap(), 100);
        assert_eq!(buf, file[700..800]);
        r.seek(SeekFrom::End(-50)).unwrap();
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, file[950..]);
        assert!(r.seek(SeekFrom::Current(-2000)).is_err());
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
