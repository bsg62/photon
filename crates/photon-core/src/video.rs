//! What photon reads from a video file's container: its displayed size, running time,
//! capture date, and an iPhone's make and model. Never a frame - the webview draws the
//! poster frame and plays the video (spec `2026-09-26-photon-video-design.md`).
//!
//! Hand-rolled, like the XMP and INI readers: MP4 and QuickTime share ISO-BMFF's boxes, and
//! photon needs five of them. The walk follows fixed paths (`moov/mvhd`, `moov/trak/tkhd`,
//! `moov/trak/mdia/hdlr`, `moov/meta/keys|ilst`), so it never recurses deeper than those,
//! and a box that claims more than its parent holds ends the walk with whatever was read.

use crate::metadata::{naive_to_unix, plausible_taken_at};
use jiff::{Timestamp, tz::TimeZone};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoMeta {
    /// As displayed: a quarter-turn matrix swaps the stored size, so nothing downstream ever
    /// rotates a frame the webview has already rotated.
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<i64>,
    /// Naive local wall-clock seconds, the way `items.taken_at` holds a photo's EXIF date.
    pub taken_at: Option<i64>,
    pub make: Option<String>,
    pub model: Option<String>,
}

/// `moov` is read whole, so it is capped: a real one is kilobytes to a few megabytes.
const MAX_MOOV: u64 = 64 << 20;
/// A WebM's Info and Tracks come before its first Cluster, well inside this.
const WEBM_HEAD: u64 = 1 << 20;
const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];
/// Seconds from 1904-01-01 (`mvhd`'s epoch) to 1970-01-01.
const MAC_EPOCH: i64 = 2_082_844_800;
/// The longest running time taken at its word. Past it the header is lying, or holds a
/// placeholder - a live recording's WebM can say anything - and a tile reading "2378:12:05"
/// is worse than one reading nothing.
const MAX_DURATION_MS: i64 = 7 * 24 * 3600 * 1000;

/// A running time that means something: zero is a writer that did not know (a fragmented
/// MP4 leaves `mvhd` at 0, a WebM being recorded has no Duration yet), so it is `None`
/// rather than a tile badged "0:00".
fn plausible_duration(ms: i64) -> Option<i64> {
    (ms > 0 && ms <= MAX_DURATION_MS).then_some(ms)
}

pub fn read_meta(path: &Path) -> VideoMeta {
    read_meta_with(path, &TimeZone::system(), crate::now_ms() / 1000)
}

pub(crate) fn read_meta_with(path: &Path, tz: &TimeZone, now: i64) -> VideoMeta {
    let Ok(mut file) = File::open(path) else {
        return VideoMeta::default();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return VideoMeta::default();
    }
    if magic == EBML_MAGIC {
        let mut head = Vec::new();
        let _ = file.rewind();
        let _ = (&mut file).take(WEBM_HEAD).read_to_end(&mut head);
        return read_webm(&head).unwrap_or_default();
    }
    let Some(moov) = find_moov(&mut file, len) else {
        return VideoMeta::default();
    };
    read_moov(&moov, tz, now)
}

/// Walks the top-level boxes by seeking, not reading: a camera that does not "fast start"
/// writes `moov` after gigabytes of `mdat`.
fn find_moov<R: Read + Seek>(r: &mut R, len: u64) -> Option<Vec<u8>> {
    let mut pos = 0u64;
    while pos + 8 <= len {
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 8];
        r.read_exact(&mut h).ok()?;
        let (mut size, mut header) = (u64::from(u32::from_be_bytes(h[..4].try_into().ok()?)), 8);
        if size == 1 {
            let mut large = [0u8; 8];
            r.read_exact(&mut large).ok()?;
            (size, header) = (u64::from_be_bytes(large), 16);
        } else if size == 0 {
            size = len - pos;
        }
        if size < header || pos.checked_add(size)? > len {
            return None;
        }
        if &h[4..8] == b"moov" {
            if size > MAX_MOOV {
                return None;
            }
            let mut body = vec![0u8; (size - header) as usize];
            r.read_exact(&mut body).ok()?;
            return Some(body);
        }
        pos += size;
    }
    None
}

/// The child boxes of one box's body. Stops, rather than guessing, at the first box whose
/// size does not fit.
struct Boxes<'a>(&'a [u8]);

impl<'a> Iterator for Boxes<'a> {
    type Item = ([u8; 4], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let d = self.0;
        let size32 = be32(d, 0)?;
        let kind: [u8; 4] = d.get(4..8)?.try_into().ok()?;
        let (size, header) = match size32 {
            0 => (d.len() as u64, 8),
            1 => (be64(d, 8)?, 16),
            n => (u64::from(n), 8),
        };
        if size < header as u64 || size > d.len() as u64 {
            self.0 = &[];
            return None;
        }
        let (this, rest) = d.split_at(size as usize);
        self.0 = rest;
        Some((kind, &this[header..]))
    }
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    Boxes(data).find(|(k, _)| k == kind).map(|(_, body)| body)
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

fn read_moov(moov: &[u8], tz: &TimeZone, now: i64) -> VideoMeta {
    let (created, duration_ms) = child(moov, b"mvhd")
        .and_then(read_mvhd)
        .unwrap_or((None, None));
    let (width, height) = Boxes(moov)
        .filter(|(k, _)| k == b"trak")
        .find(|(_, trak)| {
            child(trak, b"mdia")
                .and_then(|m| child(m, b"hdlr"))
                .and_then(|h| h.get(8..12))
                == Some(b"vide")
        })
        .and_then(|(_, trak)| child(trak, b"tkhd"))
        .and_then(read_tkhd)
        .unwrap_or((0, 0));
    let apple = apple_keys(moov);
    let get = |key: &str| apple.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    VideoMeta {
        width,
        height,
        duration_ms,
        taken_at: capture_date(
            get("com.apple.quicktime.creationdate").as_deref(),
            created,
            tz,
            now,
        ),
        make: get("com.apple.quicktime.make"),
        model: get("com.apple.quicktime.model"),
    }
}

/// (creation time in 1904-epoch seconds, running time in ms). A zero creation time, and a
/// zero or all-ones duration, mean the writer did not say, and come back `None`.
fn read_mvhd(b: &[u8]) -> Option<(Option<u64>, Option<i64>)> {
    let (created, timescale, duration, unknown) = match *b.first()? {
        1 => (be64(b, 4)?, be32(b, 20)?, be64(b, 24)?, u64::MAX),
        _ => (
            u64::from(be32(b, 4)?),
            be32(b, 12)?,
            u64::from(be32(b, 16)?),
            u64::from(u32::MAX),
        ),
    };
    let duration_ms = (timescale > 0 && duration != unknown)
        .then(|| i64::try_from(u128::from(duration) * 1000 / u128::from(timescale)).ok())
        .flatten()
        .and_then(plausible_duration);
    Some(((created != 0).then_some(created), duration_ms))
}

/// The displayed size: the stored one, swapped when the matrix turns a quarter.
fn read_tkhd(b: &[u8]) -> Option<(u32, u32)> {
    let (matrix, size) = if *b.first()? == 1 { (52, 88) } else { (40, 76) };
    let m = |i: usize| be32(b, matrix + 4 * i).map(|v| v as i32);
    let (a, bb, c, d) = (m(0)?, m(1)?, m(3)?, m(4)?);
    let (w, h) = (be32(b, size)? >> 16, be32(b, size + 4)? >> 16);
    let quarter = a == 0 && d == 0 && bb != 0 && c != 0;
    Some(if quarter { (h, w) } else { (w, h) })
}

/// QuickTime's `moov/meta` key list, UTF-8 values only.
fn apple_keys(moov: &[u8]) -> Vec<(String, String)> {
    let Some(meta) = child(moov, b"meta") else {
        return Vec::new();
    };
    // QuickTime writes `meta` as a plain box, ISO-BMFF as a full box with four bytes of
    // version and flags first; the first child's type tells the two apart.
    let meta = if meta.get(4..8) == Some(b"hdlr") {
        meta
    } else {
        meta.get(4..).unwrap_or_default()
    };
    let mut names = Vec::new();
    if let Some(keys) = child(meta, b"keys") {
        let mut at = 8;
        while let Some(size) = be32(keys, at).map(|s| s as usize) {
            let Some(name) = keys.get(at + 8..at + size.max(8)) else {
                break;
            };
            names.push(String::from_utf8_lossy(name).into_owned());
            at += size.max(8);
        }
    }
    let Some(ilst) = child(meta, b"ilst") else {
        return Vec::new();
    };
    Boxes(ilst)
        .filter_map(|(index, item)| {
            let name = names.get((u32::from_be_bytes(index) as usize).checked_sub(1)?)?;
            let data = child(item, b"data")?;
            if be32(data, 0)? != 1 {
                return None;
            }
            let text = data.get(8..)?;
            Some((name.clone(), String::from_utf8_lossy(text).into_owned()))
        })
        .collect()
}

/// Apple's date, then `mvhd`'s, each only if believable (`plausible_taken_at`).
fn capture_date(apple: Option<&str>, mvhd: Option<u64>, tz: &TimeZone, now: i64) -> Option<i64> {
    let apple = apple.and_then(parse_apple_date);
    let mvhd = mvhd.and_then(|t| wall_clock(i64::try_from(t).ok()? - MAC_EPOCH, tz));
    [apple, mvhd]
        .into_iter()
        .flatten()
        .find(|&t| plausible_taken_at(t, now))
}

/// `2024-06-15T12:30:45+0200`: the wall clock as written. Its offset is dropped on purpose -
/// `taken_at` is the camera's local time, and this *is* the camera's local time.
fn parse_apple_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
    let (y, mo, d, h, mi, se) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    ((1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && se < 61)
        .then(|| naive_to_unix(i64::from(y), mo, d, h, mi, se))
}

/// `mvhd` is UTC; photos are the camera's wall clock. The zone in force *at that instant*
/// (DST included) is the best the file allows - the camera's own zone is not recorded, so
/// a video shot abroad is placed by the home zone.
fn wall_clock(unix: i64, tz: &TimeZone) -> Option<i64> {
    let at = Timestamp::from_second(unix).ok()?;
    Some(unix + i64::from(tz.to_offset(at).seconds()))
}

fn read_webm(buf: &[u8]) -> Option<VideoMeta> {
    let (_, segment) = ebml_children(buf).find(|(id, _)| *id == 0x1853_8067)?;
    let mut meta = VideoMeta::default();
    let (mut scale, mut ticks) = (1_000_000u64, None::<f64>);
    for (id, body) in ebml_children(segment) {
        match id {
            0x1549_A966 => {
                for (id, v) in ebml_children(body) {
                    match id {
                        0x2A_D7B1 => scale = ebml_uint(v).unwrap_or(scale),
                        0x4489 => ticks = ebml_float(v),
                        _ => {}
                    }
                }
            }
            0x1654_AE6B => {
                for (_, entry) in ebml_children(body).filter(|(id, _)| *id == 0xAE) {
                    let is_video =
                        ebml_children(entry).any(|(id, v)| id == 0x83 && ebml_uint(v) == Some(1));
                    let Some((_, video)) = ebml_children(entry).find(|(id, _)| *id == 0xE0) else {
                        continue;
                    };
                    if !is_video {
                        continue;
                    }
                    for (id, v) in ebml_children(video) {
                        match id {
                            0xB0 => {
                                meta.width = ebml_uint(v)
                                    .and_then(|n| u32::try_from(n).ok())
                                    .unwrap_or(0)
                            }
                            0xBA => {
                                meta.height = ebml_uint(v)
                                    .and_then(|n| u32::try_from(n).ok())
                                    .unwrap_or(0)
                            }
                            _ => {}
                        }
                    }
                    break;
                }
            }
            0x1F43_B675 => break, // the first Cluster: Info and Tracks are behind us
            _ => {}
        }
    }
    // No separate finiteness check: `as i64` turns NaN into 0 and saturates an infinity or
    // an enormous float at an i64 extreme, and the bound refuses all three.
    meta.duration_ms = ticks
        .map(|t| (t * scale as f64 / 1e6) as i64)
        .and_then(plausible_duration);
    Some(meta)
}

/// An EBML variable-length integer: (value, length). An ID keeps its marker bit; a size
/// drops it, and a size of all ones means "unknown", returned as `None`.
fn vint(d: &[u8], keep_marker: bool) -> Option<(Option<u64>, usize)> {
    let first = *d.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || d.len() < len {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu64 >> len };
    let mut v = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & mask
    };
    for &b in &d[1..len] {
        v = (v << 8) | u64::from(b);
    }
    let unknown = !keep_marker && v == (1u64 << (7 * len)) - 1;
    Some(((!unknown).then_some(v), len))
}

/// Child elements. A master element whose size runs past the buffer (a Segment, read only
/// as far as `WEBM_HEAD`) is cut at the buffer's end; an unknown size runs to it.
fn ebml_children(mut d: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    std::iter::from_fn(move || {
        let (id, id_len) = vint(d, true)?;
        let (size, size_len) = vint(d.get(id_len..)?, false)?;
        let start = id_len + size_len;
        let end = size.map_or(d.len(), |s| start.saturating_add(s as usize).min(d.len()));
        let body = d.get(start..end)?;
        d = &d[end..];
        Some((id?, body))
    })
}

fn ebml_uint(d: &[u8]) -> Option<u64> {
    (d.len() <= 8).then(|| d.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
}

fn ebml_float(d: &[u8]) -> Option<f64> {
    match d.len() {
        4 => Some(f64::from(f32::from_be_bytes(d.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(d.try_into().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::naive_to_unix;
    use crate::testutil::{Mp4Spec, mp4_bytes, write_file};
    use jiff::tz::{self, TimeZone};

    const NOW: i64 = 1_800_000_000; // 2027
    /// 2024-06-15 10:30:45 UTC in `mvhd`'s 1904 epoch.
    const MVHD_2024_06_15_1030_UTC: u32 = 3_801_292_245;

    fn read(bytes: &[u8], tz: &TimeZone) -> VideoMeta {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "clip.mov", bytes);
        read_meta_with(&path, tz, NOW)
    }

    fn plus_two() -> TimeZone {
        TimeZone::fixed(tz::offset(2))
    }

    fn base() -> Mp4Spec<'static> {
        Mp4Spec {
            width: 1920,
            height: 1080,
            timescale: 600,
            duration: 600 * 83,
            ..Mp4Spec::default()
        }
    }

    #[test]
    fn reads_size_and_running_time() {
        let meta = read(&mp4_bytes(&base()), &plus_two());
        assert_eq!((meta.width, meta.height), (1920, 1080));
        assert_eq!(meta.duration_ms, Some(83_000));
    }

    #[test]
    fn a_quarter_turn_swaps_the_displayed_size_and_a_half_turn_does_not() {
        for (rotation, size) in [(90, (1080, 1920)), (270, (1080, 1920)), (180, (1920, 1080))] {
            let meta = read(&mp4_bytes(&Mp4Spec { rotation, ..base() }), &plus_two());
            assert_eq!((meta.width, meta.height), size, "{rotation}°");
        }
    }

    #[test]
    fn the_video_track_is_found_behind_a_sound_track() {
        let meta = read(
            &mp4_bytes(&Mp4Spec {
                audio_first: true,
                ..base()
            }),
            &plus_two(),
        );
        assert_eq!((meta.width, meta.height), (1920, 1080));
    }

    #[test]
    fn apples_date_is_taken_as_wall_clock_and_wins_over_mvhd() {
        let spec = Mp4Spec {
            apple_date: Some("2024-06-15T12:30:45+0200"),
            mvhd_created: MVHD_2024_06_15_1030_UTC - 3600, // an hour off, to tell them apart
            ..base()
        };
        // Read in a zone that is *not* the one the offset names: the wall clock must be
        // taken as written, not converted.
        let meta = read(&mp4_bytes(&spec), &TimeZone::fixed(tz::offset(-5)));
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn an_mvhd_time_is_converted_to_the_zones_wall_clock() {
        let spec = Mp4Spec {
            mvhd_created: MVHD_2024_06_15_1030_UTC,
            ..base()
        };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn a_zero_mvhd_time_is_no_date() {
        assert_eq!(read(&mp4_bytes(&base()), &plus_two()).taken_at, None);
    }

    #[test]
    fn an_implausible_apple_date_falls_through_to_mvhd() {
        let spec = Mp4Spec {
            apple_date: Some("1904-01-01T00:00:00Z"),
            mvhd_created: MVHD_2024_06_15_1030_UTC,
            ..base()
        };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn reads_apples_make_and_model() {
        let spec = Mp4Spec {
            make: Some("Apple"),
            model: Some("iPhone 15 Pro"),
            ..base()
        };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.make.as_deref(), Some("Apple"));
        assert_eq!(meta.model.as_deref(), Some("iPhone 15 Pro"));
    }

    #[test]
    fn finds_moov_after_a_large_mdat() {
        let spec = Mp4Spec {
            mdat_before: 3 << 20,
            ..base()
        };
        assert_eq!(
            read(&mp4_bytes(&spec), &plus_two()).duration_ms,
            Some(83_000)
        );
    }

    #[test]
    fn garbage_and_truncated_files_read_as_nothing() {
        let whole = mp4_bytes(&base());
        for bytes in [&[][..], b"not a video at all", &whole[..whole.len() / 2]] {
            assert_eq!(read(bytes, &plus_two()), VideoMeta::default());
        }
        // A box claiming more than its parent holds ends the walk rather than reading past it.
        let mut lying = whole.clone();
        let moov = lying.windows(4).position(|w| w == b"moov").unwrap() - 4;
        lying[moov..moov + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read(&lying, &plus_two()), VideoMeta::default());
    }

    /// An EBML element with an 8-byte size, which every reader must accept.
    fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.push(0x01);
        out.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        out.extend_from_slice(body);
        out
    }

    fn webm(duration: f64) -> Vec<u8> {
        let info = el(
            &[0x15, 0x49, 0xA9, 0x66],
            &[
                el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]), // 1 ms per tick
                el(&[0x44, 0x89], &duration.to_be_bytes()),
            ]
            .concat(),
        );
        let video = el(
            &[0xE0],
            &[
                el(&[0xB0], &1280u16.to_be_bytes()),
                el(&[0xBA], &720u16.to_be_bytes()),
            ]
            .concat(),
        );
        let tracks = el(
            &[0x16, 0x54, 0xAE, 0x6B],
            &el(&[0xAE], &[el(&[0x83], &[1]), video].concat()),
        );
        let mut segment = vec![
            0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ]; // unknown size
        segment.extend(info);
        segment.extend(tracks);
        segment.extend(el(&[0x1F, 0x43, 0xB6, 0x75], &[0u8; 32])); // a Cluster: the walk stops here
        [
            el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"webm")),
            segment,
        ]
        .concat()
    }

    #[test]
    fn a_zero_mvhd_duration_is_unknown_not_zero() {
        let spec = Mp4Spec {
            duration: 0,
            ..base()
        };
        assert_eq!(read(&mp4_bytes(&spec), &plus_two()).duration_ms, None);
    }

    #[test]
    fn a_webm_duration_that_is_zero_absurd_or_not_a_number_is_unknown() {
        let eight_days = 8.0 * 24.0 * 3600.0 * 1000.0;
        for ticks in [0.0, -5.0, f64::NAN, f64::INFINITY, eight_days] {
            let meta = read(&webm(ticks), &plus_two());
            assert_eq!(meta.duration_ms, None, "{ticks}");
            assert_eq!(
                (meta.width, meta.height),
                (1280, 720),
                "the rest still reads"
            );
        }
        let six_days = 6.0 * 24.0 * 3600.0 * 1000.0;
        assert_eq!(
            read(&webm(six_days), &plus_two()).duration_ms,
            Some(six_days as i64)
        );
    }

    #[test]
    fn reads_a_webms_size_and_running_time_but_no_date() {
        let meta = read(&webm(83_000.0), &plus_two());
        assert_eq!((meta.width, meta.height), (1280, 720));
        assert_eq!(meta.duration_ms, Some(83_000));
        assert_eq!(meta.taken_at, None);
    }
}
