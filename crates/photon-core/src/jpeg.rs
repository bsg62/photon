//! The marker segments at the head of a JPEG, walked without decoding anything: `iptc` reads
//! keywords out of them, `decode::dimensions` reads the frame size and `metadata` the frame
//! size and the EXIF block. One walker serves them all so that a quirk of the stream (fill
//! bytes, a marker with no length) is handled the same way wherever photon reads a JPEG's
//! headers.

use std::io::{BufRead, Read, Seek, SeekFrom};

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const TEM: u8 = 0x01;
const RST0: u8 = 0xD0;
const RST7: u8 = 0xD7;
const APP1: u8 = 0xE1;

/// What opens an APP1 segment holding EXIF, ahead of the TIFF structure inside it - the
/// same test kamadak-exif makes.
const EXIF_ID: &[u8] = b"Exif\0\0";

/// One marker segment ahead of the scan data: its marker code and how many body bytes
/// follow the two length bytes.
pub(crate) struct Segment {
    pub(crate) marker: u8,
    pub(crate) len: usize,
}

/// Whether a file's leading bytes are a JPEG's: SOI and the start of the next marker, the
/// same three bytes `image` guesses a JPEG from.
pub(crate) fn is_jpeg(head: &[u8]) -> bool {
    head.starts_with(&[0xFF, SOI, 0xFF])
}

/// Consumes SOI, the two bytes a JPEG starts with. False, having read them anyway, for a
/// stream that starts with anything else.
pub(crate) fn read_soi<R: Read>(r: &mut R) -> bool {
    let mut soi = [0; 2];
    r.read_exact(&mut soi).is_ok() && soi == [0xFF, SOI]
}

/// Reads up to the next segment that has a body and returns its header, leaving `r` at the
/// body's first byte; the caller reads or steps over the body before calling this again.
///
/// `None` ends the walk: at SOS or EOI, past which there are no more header segments; at
/// the end of the stream; and at anything that is not a marker where one has to be, or a
/// length too short to count its own two bytes. Every call consumes at least two bytes, so
/// a walk over any input ends.
pub(crate) fn next_segment<R: BufRead>(r: &mut R) -> Option<Segment> {
    match next(r) {
        Next::Segment(segment) => Some(segment),
        Next::End | Next::Broken => None,
    }
}

/// Where one step of the walk lands, for the caller that has to tell the two ends of a
/// walk apart; [`next_segment`] for the rest.
enum Next {
    Segment(Segment),
    /// SOS or EOI: every header segment has been seen.
    End,
    /// Something the walk cannot follow: no marker where one has to be, a length too short
    /// to count its own two bytes, or the end of the stream.
    Broken,
}

fn next<R: BufRead>(r: &mut R) -> Next {
    step(r).unwrap_or(Next::Broken)
}

/// [`next`], with the end of the stream as `None`.
fn step<R: BufRead>(r: &mut R) -> Option<Next> {
    loop {
        if read_u8(r)? != 0xFF {
            return Some(Next::Broken);
        }
        let mut marker = read_u8(r)?;
        // Any number of 0xFF fill bytes may precede a marker code (T.81 B.1.1.2).
        while marker == 0xFF {
            marker = read_u8(r)?;
        }
        match marker {
            // Stuffing inside entropy-coded data, never a marker here.
            0x00 => return Some(Next::Broken),
            // Standalone markers carry no length (T.81 B.1.1.4): reading one would take the
            // next marker's two bytes as a length and skip to somewhere arbitrary.
            TEM | RST0..=RST7 | SOI => continue,
            SOS | EOI => return Some(Next::End),
            _ => {}
        }
        let mut len = [0; 2];
        r.read_exact(&mut len).ok()?;
        let Some(len) = u16::from_be_bytes(len).checked_sub(2) else {
            return Some(Next::Broken);
        };
        return Some(Next::Segment(Segment {
            marker,
            len: usize::from(len),
        }));
    }
}

/// A JPEG's stored width and height (before any EXIF orientation), from its frame header.
///
/// Everything ahead of the frame header is stepped over by its declared length, so what
/// this reads is the few KiB of headers and never the scan data, however large the file.
/// `None` for a stream with no usable frame header before its first scan, which
/// `decode::dimensions` hands to `image` instead.
///
/// Every frame type is sized, including the ones zune cannot decode (12-bit, lossless,
/// arithmetic-coded), for which `image` answered no size at all. Such a photo now stores
/// its real size instead of 0x0; it still fails to render either way.
pub(crate) fn dimensions<R: BufRead + Seek>(r: &mut R) -> Option<(u32, u32)> {
    if !read_soi(r) {
        return None;
    }
    loop {
        let segment = next_segment(r)?;
        if is_sof(segment.marker) {
            return frame_size(r, segment.len);
        }
        skip(r, segment.len)?;
    }
}

/// What `describe()` reads from a JPEG's headers.
pub(crate) struct Head {
    /// Stored width and height, as [`dimensions`] reads them.
    pub(crate) dims: (u32, u32),
    /// The TIFF structure inside the first `Exif` APP1 segment, for
    /// `exif::Reader::read_raw`.
    pub(crate) exif: Option<Vec<u8>>,
}

/// A JPEG's frame size and EXIF block, from its headers alone.
///
/// Not kamadak-exif's `read_from_container`: its search for the EXIF block does not stop at
/// the first scan, so in a JPEG with no EXIF - an export, a download, a messenger's copy -
/// it looks through the compressed data to the end of the file, and sizing the photo from
/// its headers would still leave a whole-file read behind it. This takes the first `Exif`
/// APP1 segment as that search does, but stops at the scan: a block after it, which that
/// search would still reach, is not found. EXIF belongs at the head of the file, and
/// reading past the scan is the whole-file read this is here to avoid.
///
/// The walk goes on past the frame header until it has the EXIF block or reaches the scan:
/// EXIF belongs straight after SOI, but kamadak-exif also finds one misplaced after the
/// frame header, and a photo's date is worth the table segments between the two. `None`
/// when there is no usable frame header, as for [`dimensions`] - and when a defect stops
/// the walk before the EXIF block has turned up, since kamadak-exif's search steps over
/// bytes this refuses and may find one behind it.
pub(crate) fn head<R: BufRead + Seek>(r: &mut R) -> Option<Head> {
    if !read_soi(r) {
        return None;
    }
    let mut dims = None;
    let mut exif = None;
    loop {
        let segment = match next(r) {
            Next::Segment(segment) => segment,
            Next::End => return dims.map(|dims| Head { dims, exif }),
            Next::Broken => return None,
        };
        let mut rest = segment.len;
        if dims.is_none() && is_sof(segment.marker) {
            dims = Some(frame_size(r, segment.len)?);
            rest -= FRAME_SIZE_BYTES;
        } else if exif.is_none() && segment.marker == APP1 && rest >= EXIF_ID.len() {
            // XMP travels in APP1 segments too, so the identifier is read before the body.
            let mut id = [0; EXIF_ID.len()];
            r.read_exact(&mut id).ok()?;
            rest -= id.len();
            if id == EXIF_ID {
                let mut tiff = vec![0; rest];
                r.read_exact(&mut tiff).ok()?;
                exif = Some(tiff);
                rest = 0;
            }
        }
        if let (Some(dims), Some(_)) = (dims, &exif) {
            return Some(Head { dims, exif });
        }
        skip(r, rest)?;
    }
}

/// The start-of-frame markers, SOF0 to SOF15, less the three codes inside that range that
/// are something else: DHT (0xC4), JPG (0xC8) and DAC (0xCC). A DHT ahead of the frame
/// header is legal, and read as a frame it would give the photo the size of a Huffman
/// table's first bytes.
fn is_sof(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

/// How much of a frame header's body [`frame_size`] reads.
const FRAME_SIZE_BYTES: usize = 5;

/// Width and height from a frame header's body: precision (1 byte), height, width (2 each).
fn frame_size<R: Read>(r: &mut R, len: usize) -> Option<(u32, u32)> {
    let mut body = [0; FRAME_SIZE_BYTES];
    if len < body.len() {
        return None;
    }
    r.read_exact(&mut body).ok()?;
    let height = u16::from_be_bytes([body[1], body[2]]);
    let width = u16::from_be_bytes([body[3], body[4]]);
    // A zero height is one a DNL marker defines after the first scan, which this does not
    // read, and a zero width is no picture at all. zune refuses both, so handing either
    // back to `image` answers as before.
    (height != 0 && width != 0).then(|| (u32::from(width), u32::from(height)))
}

/// Steps over `n` bytes: out of the buffer when they are already in it, by a seek past the
/// rest when they are not. Seeking is the point: an EXIF block carrying its own preview
/// JPEG or a Photoshop block can fill a 64 KiB segment, and a large ICC profile or extended
/// XMP is split across several, all ahead of the frame header. Not a bare `seek` either:
/// `BufReader` discards its buffer on every seek, so stepping over a 67-byte quantisation
/// table that way would re-read the next 8 KiB for each of the dozen small segments a
/// header holds.
fn skip<R: BufRead + Seek>(r: &mut R, n: usize) -> Option<()> {
    let buffered = r.fill_buf().ok()?.len();
    if n <= buffered {
        r.consume(n);
        return Some(());
    }
    r.consume(buffered);
    r.seek(SeekFrom::Current((n - buffered) as i64)).ok()?;
    Some(())
}

fn read_u8<R: Read>(r: &mut R) -> Option<u8> {
    let mut byte = [0];
    r.read_exact(&mut byte).ok()?;
    Some(byte[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        counted, jpeg_bytes, jpeg_with_exif, jpeg_with_iptc_keywords, jpeg_with_segments,
        jpeg_with_xmp, write_file,
    };
    use image::ImageReader;
    use std::io::Cursor;

    fn size(bytes: &[u8]) -> Option<(u32, u32)> {
        dimensions(&mut Cursor::new(bytes))
    }

    /// Where the fixture's frame header starts: `image` writes a baseline SOF0 for three
    /// components, whose length is 17.
    fn sof_at(bytes: &[u8]) -> usize {
        bytes
            .windows(4)
            .position(|w| w == [0xFF, 0xC0, 0x00, 0x11])
            .expect("a baseline frame header")
    }

    /// The stored size is whatever `image` (zune) reported before, width first and before
    /// any orientation - and the fixture's own size, so the two cannot agree on a wrong
    /// answer. Sizes past 255 on either axis pin both bytes of each field and their order.
    #[test]
    fn matches_image_on_the_fixtures() {
        let fixtures = [
            ((40, 20), jpeg_bytes(40, 20)),
            ((20, 40), jpeg_bytes(20, 40)),
            ((300, 2), jpeg_bytes(300, 2)),
            ((3, 257), jpeg_bytes(3, 257)),
            ((1, 1), jpeg_bytes(1, 1)),
            ((40, 20), jpeg_with_exif(40, 20, 6, "2024:06:15 12:30:45")),
            ((12, 30), jpeg_with_iptc_keywords(12, 30, &[b"beach"])),
            ((25, 9), jpeg_with_xmp(25, 9, 3)),
        ];
        for (expected, bytes) in fixtures {
            let image = ImageReader::new(Cursor::new(&bytes))
                .with_guessed_format()
                .unwrap()
                .into_dimensions()
                .unwrap();
            assert_eq!(image, expected, "image's answer for {expected:?}");
            assert_eq!(size(&bytes), Some(expected));
        }
    }

    /// The EXIF block `head` finds is the one kamadak-exif's own search finds, byte for
    /// byte: the first APP1 segment that opens `Exif\0\0` - past an XMP segment ahead of
    /// it, and past the frame header where a writer misplaced it - and none when there is
    /// none. The size alongside it is the frame header's.
    #[test]
    fn head_finds_the_exif_block_kamadak_exif_finds() {
        // An APP1 payload, "Exif\0\0" included, lifted from the segment after SOI.
        let exif_app1 = |bytes: &[u8]| {
            let len = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
            bytes[6..4 + len].to_vec()
        };
        let first = exif_app1(&jpeg_with_exif(40, 20, 6, "2024:06:15 12:30:45"));
        let second = exif_app1(&jpeg_with_exif(40, 20, 3, "2020:01:01 00:00:00"));
        let xmp = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>".to_vec();
        let plain = jpeg_bytes(40, 20);
        // Past the fixture's whole frame header: marker (2) and its 17-byte segment.
        let after = sof_at(&plain) + 2 + 0x11;
        let length = u16::try_from(2 + first.len()).unwrap().to_be_bytes();
        let misplaced = [
            &plain[..after],
            &[0xFF, APP1],
            &length,
            &first,
            &plain[after..],
        ]
        .concat();
        let cases = [
            ("after SOI", jpeg_with_segments(40, 20, &[(APP1, &first)])),
            (
                "behind XMP",
                jpeg_with_segments(40, 20, &[(APP1, &xmp), (APP1, &first)]),
            ),
            (
                "the first of two",
                jpeg_with_segments(40, 20, &[(APP1, &first), (APP1, &second)]),
            ),
            ("after the frame header", misplaced),
            ("none", plain),
        ];
        for (what, bytes) in cases {
            let kamadak = exif::Reader::new()
                .read_from_container(&mut Cursor::new(&bytes))
                .ok()
                .map(|exif| exif.buf().to_vec());
            let head = head(&mut Cursor::new(&bytes)).expect(what);
            assert_eq!(head.dims, (40, 20), "{what}");
            assert_eq!(head.exif.is_some(), what != "none", "{what}");
            assert_eq!(head.exif, kamadak, "{what}");
        }
    }

    /// Every SOF code is a frame header, whatever the coding (progressive photos are
    /// common), and the three codes inside the range that are not frames - DHT, JPG, DAC -
    /// are stepped over like any other segment: relabelled as one, the fixture's frame
    /// header is skipped and the walk reaches its scan with no size.
    #[test]
    fn every_start_of_frame_code_is_a_frame_and_the_codes_between_them_are_not() {
        let jpeg = jpeg_bytes(40, 20);
        let at = sof_at(&jpeg);
        for code in 0xC0..=0xCF {
            let mut relabelled = jpeg.clone();
            relabelled[at + 1] = code;
            let frame = !matches!(code, 0xC4 | 0xC8 | 0xCC);
            assert_eq!(
                size(&relabelled),
                frame.then_some((40, 20)),
                "marker {code:#04X}"
            );
        }
    }

    /// Segments ahead of the frame header are stepped over by their declared lengths, and
    /// by seeking: 200 KiB of them costs a few buffer fills, not 200 KiB of reads. Their
    /// bodies are full of bytes that read as a 1x1 frame header, which a walker that
    /// scanned for markers instead of trusting the lengths would stop at.
    #[test]
    fn steps_over_large_segments_by_seeking_past_them() {
        let decoy = |len: usize| -> Vec<u8> {
            [0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x01, 0x00, 0x01]
                .into_iter()
                .cycle()
                .take(len)
                .collect()
        };
        let (exif, icc, more_icc, photoshop) =
            (decoy(65_533), decoy(60_000), decoy(60_000), decoy(30_000));
        let bytes = jpeg_with_segments(
            40,
            20,
            &[
                (0xE1, &exif),
                (0xE2, &icc),
                (0xE2, &more_icc),
                (0xED, &photoshop),
            ],
        );
        let ahead = bytes.len() - jpeg_bytes(40, 20).len();
        assert!(
            ahead > 200 * 1024,
            "{ahead} bytes of segments ahead of the frame"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &bytes);
        let mut reader = counted(&path);
        assert_eq!(dimensions(&mut reader), Some((40, 20)));
        let read = reader.get_ref().read;
        assert!(
            read < 64 * 1024,
            "read {read} bytes to reach the frame header"
        );
    }

    /// Small segments are stepped over inside the buffer: twenty comments ahead of the
    /// frame header cost one fill, where a seek per segment would refill it twenty times.
    /// The tail makes every such refill a whole buffer, as it is in a real photo.
    #[test]
    fn small_segments_are_stepped_over_inside_the_buffer() {
        let comment = [b'c'; 50];
        let mut bytes = jpeg_with_segments(40, 20, &[(0xFE, &comment[..]); 20]);
        bytes.resize(bytes.len() + 256 * 1024, 0x5A);
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &bytes);
        let mut reader = counted(&path);
        assert_eq!(dimensions(&mut reader), Some((40, 20)));
        let read = reader.get_ref().read;
        assert!(
            read < 16 * 1024,
            "read {read} bytes to reach the frame header"
        );
    }

    /// Nothing after the first scan or the end of the image is a header of this picture.
    /// Each marker here carries two bytes that would read as an empty body, with an intact
    /// JPEG behind them.
    #[test]
    fn the_walk_ends_at_the_first_scan_or_the_end_of_the_image() {
        let jpeg = jpeg_bytes(40, 20);
        for marker in [0xDA, 0xD9] {
            let bytes = [&jpeg[..2], &[0xFF, marker, 0x00, 0x02], &jpeg[2..]].concat();
            assert_eq!(size(&bytes), None, "marker {marker:#04X}");
        }
    }

    /// 0xFF fill bytes may precede any marker, and TEM, RSTn and SOI have no length: read
    /// as if they had, the next marker's first two bytes become a length and the walk
    /// jumps somewhere arbitrary.
    #[test]
    fn fill_bytes_and_standalone_markers_are_stepped_over() {
        let jpeg = jpeg_bytes(40, 20);
        let mut bytes = jpeg[..2].to_vec(); // SOI
        bytes.extend_from_slice(&[0xFF, 0x01, 0xFF, 0xD0, 0xFF, 0xD7, 0xFF, 0xD8]);
        bytes.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // fill, then the fixture's next marker
        bytes.extend_from_slice(&jpeg[2..]);
        assert_eq!(size(&bytes), Some((40, 20)));
    }

    /// A height of zero is defined by a DNL marker after the first scan, which is not read
    /// here; zune refuses such a file too, so this must not claim a size for it.
    #[test]
    fn a_zero_height_or_width_is_no_size() {
        let jpeg = jpeg_bytes(40, 20);
        let at = sof_at(&jpeg);
        // After the marker (2), the length (2) and the precision (1): height, then width.
        for field in [at + 5, at + 7] {
            let mut zeroed = jpeg.clone();
            zeroed[field..field + 2].copy_from_slice(&[0, 0]);
            assert_eq!(size(&zeroed), None, "field at {field}");
        }
    }

    /// A stream cut anywhere has a size only once the frame header's width is complete,
    /// and no input can make the walk panic or spin.
    #[test]
    fn truncated_input_is_no_size() {
        let jpeg = jpeg_bytes(40, 20);
        let width_end = sof_at(&jpeg) + 9;
        for cut in 0..jpeg.len() {
            let expected = (cut >= width_end).then_some((40, 20));
            assert_eq!(size(&jpeg[..cut]), expected, "cut at {cut}");
        }
        assert_eq!(size(b""), None);
        assert_eq!(size(b"definitely not a jpeg"), None);
        assert_eq!(size(&[&[0xFF, 0xD8][..], &[0xFF; 100_000]].concat()), None);
    }

    /// A defect in the headers ends the walk with no size, for `image` to judge, rather than
    /// being read past on a guess: each defect here is followed by an intact JPEG, which a
    /// walker that tolerated it would go on to read a size from - one zune may not agree
    /// with, since zune tolerates some of these differently.
    #[test]
    fn a_defect_in_the_headers_is_no_size() {
        let jpeg = jpeg_bytes(40, 20);
        let defects: [(&str, &[u8]); 4] = [
            ("a byte where a marker belongs", &[0x00]),
            ("stuffing where a marker belongs", &[0xFF, 0x00, 0x00, 0x02]),
            ("a length below two", &[0xFF, 0xE0, 0x00, 0x01]),
            // Declares a two-byte body; the five bytes after it would read as 40x20.
            (
                "a frame header too short to hold a size",
                &[0xFF, 0xC0, 0x00, 0x04, 0x08, 0x00, 0x14, 0x00, 0x28],
            ),
        ];
        for (what, defect) in defects {
            let bytes = [&jpeg[..2], defect, &jpeg[2..]].concat();
            assert_eq!(size(&bytes), None, "{what}");
        }
    }
}
