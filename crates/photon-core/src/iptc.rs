//! Reads IPTC-IIM keywords out of a JPEG's APP13 segment.
//!
//! Picasa, Bridge and most tools before XMP wrote keywords as IIM dataset 2:25 inside the
//! "Photoshop 3.0" APP13 segment, resource `0x0404`. That is a three-level container —
//! JPEG segments, then Photoshop image resources, then IIM records — and each level is a
//! few dozen lines of bounds-checked slicing, so it is hand-rolled rather than taken as a
//! dependency. Nothing here writes.

/// The keywords in the leading bytes of a JPEG, in file order. Empty for a file that is
/// not a JPEG, has no APP13 segment, or has no keywords; never fails.
pub fn keywords_in(prefix: &[u8]) -> Vec<String> {
    let mut keywords = Vec::new();
    for payload in app13_segments(prefix) {
        for resource in photoshop_resources(payload) {
            iim_values(resource, KEYWORDS_DATASET, &mut keywords);
        }
    }
    keywords
}

/// The first IPTC caption (2:120, Caption-Abstract) in the leading bytes of a JPEG, decoded
/// like keywords. Picasa writes a caption typed under a photo here.
pub fn caption_in(prefix: &[u8]) -> Option<String> {
    let mut captions = Vec::new();
    for payload in app13_segments(prefix) {
        for resource in photoshop_resources(payload) {
            iim_values(resource, CAPTION_DATASET, &mut captions);
        }
    }
    captions.into_iter().next()
}

const SOI: [u8; 2] = [0xFF, 0xD8];
const APP13: u8 = 0xED;
const SOS: u8 = 0xDA;
const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";
const IPTC_RESOURCE: u16 = 0x0404;
const IIM_MARKER: u8 = 0x1C;
const KEYWORDS_RECORD: u8 = 2;
const KEYWORDS_DATASET: u8 = 25;
/// Caption-Abstract, where Picasa writes the caption.
const CAPTION_DATASET: u8 = 120;

/// Every APP13 "Photoshop 3.0" segment's resource bytes, walking the marker stream from
/// SOI up to the scan data. A file that does not start with SOI is not a JPEG and yields
/// nothing; a truncated prefix ends the walk quietly.
fn app13_segments(prefix: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    if prefix.len() < 2 || prefix[..2] != SOI {
        return found;
    }
    let mut at = 2;
    while at + 4 <= prefix.len() {
        if prefix[at] != 0xFF {
            break;
        }
        let marker = prefix[at + 1];
        // Fill bytes between segments are legal.
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if marker == SOS {
            break;
        }
        let len = u16::from_be_bytes([prefix[at + 2], prefix[at + 3]]) as usize;
        if len < 2 {
            break;
        }
        let body_start = at + 4;
        let body_end = at + 2 + len;
        if body_end > prefix.len() {
            break;
        }
        let body = &prefix[body_start..body_end];
        if marker == APP13
            && let Some(resources) = body.strip_prefix(PHOTOSHOP_HEADER)
        {
            found.push(resources);
        }
        at = body_end;
    }
    found
}

/// The data of every `8BIM` resource whose id is the IPTC-NAA one.
///
/// A resource is `8BIM`, a two-byte id, a Pascal string name padded to an even length, a
/// four-byte size and that many data bytes, padded to an even length.
fn photoshop_resources(bytes: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 12 <= bytes.len() {
        if &bytes[at..at + 4] != b"8BIM" {
            break;
        }
        let id = u16::from_be_bytes([bytes[at + 4], bytes[at + 5]]);
        let name_len = bytes[at + 6] as usize;
        // The name's length byte and the name itself, together padded to even.
        let name_total = (1 + name_len).div_ceil(2) * 2;
        let size_at = at + 6 + name_total;
        if size_at + 4 > bytes.len() {
            break;
        }
        let size = u32::from_be_bytes([
            bytes[size_at],
            bytes[size_at + 1],
            bytes[size_at + 2],
            bytes[size_at + 3],
        ]) as usize;
        let data_start = size_at + 4;
        let data_end = data_start + size;
        if data_end > bytes.len() {
            break;
        }
        if id == IPTC_RESOURCE {
            found.push(&bytes[data_start..data_end]);
        }
        at = data_end + (size % 2);
    }
    found
}

/// Appends every record-2 value of the given dataset in an IIM block. A record is `0x1C`,
/// record number, dataset number, a two-byte size and the data; a size with its top bit set
/// is the extended form, which neither a keyword nor a caption uses, and ends the walk since
/// its length cannot be read here.
fn iim_values(bytes: &[u8], dataset: u8, out: &mut Vec<String>) {
    let mut at = 0;
    while at + 5 <= bytes.len() {
        if bytes[at] != IIM_MARKER {
            break;
        }
        let record = bytes[at + 1];
        let dataset_here = bytes[at + 2];
        let size = u16::from_be_bytes([bytes[at + 3], bytes[at + 4]]) as usize;
        if size & 0x8000 != 0 {
            break;
        }
        let data_start = at + 5;
        let data_end = data_start + size;
        if data_end > bytes.len() {
            break;
        }
        if record == KEYWORDS_RECORD && dataset_here == dataset {
            let value = decode(&bytes[data_start..data_end]);
            let value = value.trim();
            if !value.is_empty() {
                out.push(value.to_string());
            }
        }
        at = data_end;
    }
}

/// IIM text is UTF-8 when the file says so in dataset 1:90 and Latin-1 by default, and
/// most writers never set 1:90 either way. Valid UTF-8 is taken as UTF-8: a Latin-1 string
/// that happens to be valid UTF-8 is ASCII, so nothing is lost, and the other way round —
/// decoding real UTF-8 as Latin-1 — turns every accented letter into two wrong ones.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        iptc_app13, iptc_app13_datasets, jpeg_bytes, jpeg_with_iptc_keywords, jpeg_with_segments,
    };

    #[test]
    fn reads_keywords_from_the_app13_iptc_block() {
        let jpeg = jpeg_with_iptc_keywords(8, 8, &[b"beach", b"summer"]);
        assert_eq!(keywords_in(&jpeg), ["beach", "summer"]);
    }

    #[test]
    fn a_jpeg_without_iptc_or_a_non_jpeg_yields_nothing() {
        assert!(keywords_in(&jpeg_bytes(8, 8)).is_empty());
        assert!(keywords_in(b"\x89PNG\r\n\x1a\n").is_empty());
        assert!(keywords_in(&[]).is_empty());
    }

    #[test]
    fn utf8_is_kept_and_latin1_is_decoded() {
        // Picasa wrote Latin-1; Bridge writes UTF-8. `caf\xe9` is not valid UTF-8, so it is
        // read as Latin-1; the UTF-8 spelling is valid and kept as is.
        let jpeg = jpeg_with_iptc_keywords(8, 8, &[b"caf\xe9", "café".as_bytes()]);
        assert_eq!(keywords_in(&jpeg), ["café", "café"]);
    }

    #[test]
    fn other_datasets_and_other_resources_are_skipped() {
        // A caption (2:120) beside the keywords, and a resolution resource (0x03ED) before
        // the IPTC one, in the same segment. The walker has to step over both by their
        // declared lengths rather than by scanning for markers.
        let mut iim = vec![0x1C, 2, 120];
        iim.extend_from_slice(&5u16.to_be_bytes());
        iim.extend_from_slice(b"hello");
        iim.extend_from_slice(&[0x1C, 2, 25]);
        iim.extend_from_slice(&4u16.to_be_bytes());
        iim.extend_from_slice(b"lake");
        let mut payload = b"Photoshop 3.0\0".to_vec();
        payload.extend_from_slice(b"8BIM");
        payload.extend_from_slice(&0x03EDu16.to_be_bytes());
        payload.extend_from_slice(&[3, b'r', b'e', b's']); // a three-byte name, padded to 4
        payload.extend_from_slice(&3u32.to_be_bytes());
        payload.extend_from_slice(&[1, 2, 3, 0]); // three bytes of data, padded to even
        payload.extend_from_slice(b"8BIM");
        payload.extend_from_slice(&0x0404u16.to_be_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&(iim.len() as u32).to_be_bytes());
        payload.extend_from_slice(&iim);
        let jpeg = jpeg_with_segments(8, 8, &[(0xED, &payload)]);
        assert_eq!(keywords_in(&jpeg), ["lake"]);
    }

    #[test]
    fn keywords_are_found_behind_an_exif_segment_and_across_two_app13_segments() {
        let first = iptc_app13(&[b"one"]);
        let second = iptc_app13(&[b"two"]);
        let jpeg = jpeg_with_segments(
            8,
            8,
            &[(0xE1, b"Exif\0\0II*\0"), (0xED, &first), (0xED, &second)],
        );
        assert_eq!(keywords_in(&jpeg), ["one", "two"]);
    }

    #[test]
    fn a_truncated_segment_ends_the_walk_without_panicking() {
        let jpeg = jpeg_with_iptc_keywords(8, 8, &[b"beach"]);
        for cut in 0..jpeg.len() {
            let _ = keywords_in(&jpeg[..cut]);
        }
    }

    #[test]
    fn the_caption_is_dataset_2_120_beside_the_keywords() {
        let app13 =
            iptc_app13_datasets(&[(25, b"lake"), (120, b"caf\xe9 at dawn"), (120, b"second")]);
        let jpeg = jpeg_with_segments(8, 8, &[(0xED, &app13)]);
        assert_eq!(
            caption_in(&jpeg).as_deref(),
            Some("café at dawn"),
            "Latin-1, first record"
        );
        assert_eq!(keywords_in(&jpeg), ["lake"], "a caption is not a keyword");
    }

    #[test]
    fn no_caption_dataset_is_no_caption() {
        let jpeg = jpeg_with_segments(8, 8, &[(0xED, &iptc_app13_datasets(&[(25, b"lake")]))]);
        assert_eq!(caption_in(&jpeg), None);
        assert_eq!(caption_in(&jpeg_bytes(8, 8)), None);
    }
}
