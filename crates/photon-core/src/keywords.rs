//! The keywords and caption embedded in a photo file, from XMP and IPTC.
//!
//! One bounded read serves both parsers: the XMP packet and the IPTC block both sit in the
//! leading segments of every container photon reads, so `xmp::MAX_PREFIX` bytes is enough
//! and a 20 MB photo is never pulled through memory for a tag list. photon only reads;
//! nothing here writes to a file.

use std::{fs::File, io::Read, path::Path};

/// A caption longer than this is cut, by characters: it is one more search haystack in every
/// search, and no caption a person types is anywhere near it.
pub const MAX_CAPTION_CHARS: usize = 2000;

/// What a photo file says about itself in text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Embedded {
    pub keywords: Vec<String>,
    /// XMP `dc:description`, else IPTC 2:120. Never EXIF `ImageDescription`: cameras fill it
    /// with their own name, and trusting it would caption every photo from that camera.
    pub caption: Option<String>,
}

/// The distinct keywords in `path`, XMP `dc:subject` first and then IPTC 2:25, each in its
/// own order. Picasa and Lightroom write the same list to both places, so the second
/// source normally adds nothing; when they differ (a tool that wrote only one of them),
/// the union is what the person tagged. Never fails: an unreadable file has no keywords.
///
/// Exact de-duplication, not case-folded: "Berlin" and "berlin" are two keywords to every
/// tool that wrote them, and folding them here would show one spelling and hide the other.
pub fn read_keywords(path: &Path) -> Vec<String> {
    read_embedded(path).keywords
}

/// [`read_keywords`] over bytes already read.
pub fn keywords_in(prefix: &[u8]) -> Vec<String> {
    keywords_from(crate::xmp::packet_in(prefix).as_deref(), prefix)
}

fn keywords_from(packet: Option<&str>, prefix: &[u8]) -> Vec<String> {
    let mut keywords: Vec<String> = Vec::new();
    let xmp = packet
        .map(crate::xmp::subjects_from_xml)
        .unwrap_or_default();
    for keyword in xmp.into_iter().chain(crate::iptc::keywords_in(prefix)) {
        if !keywords.contains(&keyword) {
            keywords.push(keyword);
        }
    }
    keywords
}

/// The keywords and caption in `path`, from one bounded read. Never fails: an unreadable
/// file has neither.
pub fn read_embedded(path: &Path) -> Embedded {
    let Ok(mut file) = File::open(path) else {
        return Embedded::default();
    };
    let mut buf = Vec::new();
    if file
        .by_ref()
        .take(crate::xmp::MAX_PREFIX as u64)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Embedded::default();
    }
    embedded_in(&buf)
}

/// [`read_embedded`] over bytes already read.
pub fn embedded_in(prefix: &[u8]) -> Embedded {
    let packet = crate::xmp::packet_in(prefix);
    let caption = packet
        .as_deref()
        .and_then(crate::xmp::description_from_xml)
        .or_else(|| crate::iptc::caption_in(prefix))
        // Both current sources already trim and drop empties, so this holds `Embedded`'s
        // contract (trimmed, never empty) for a future source that does not; it changes
        // nothing for xmp::description_from_xml or iptc::caption_in today.
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .map(|c| c.chars().take(MAX_CAPTION_CHARS).collect());
    Embedded {
        keywords: keywords_from(packet.as_deref(), prefix),
        caption,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        ExifSpec, iptc_app13, iptc_app13_datasets, jpeg_bytes, jpeg_with_exif_spec,
        jpeg_with_segments, jpeg_with_xmp_packet, png_bytes, write_file,
        xmp_packet_with_description, xmp_packet_with_subjects,
    };

    #[test]
    fn keywords_come_from_xmp_and_iptc_and_are_not_duplicated() {
        // The usual file: the same list in both places, plus one keyword only the IPTC
        // block carries. The union is three, not five.
        let xmp = xmp_packet_with_subjects(&["beach", "summer"]);
        let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        app1.extend_from_slice(xmp.as_bytes());
        let app13 = iptc_app13(&[b"beach", b"summer", b"family"]);
        let jpeg = jpeg_with_segments(8, 8, &[(0xE1, &app1), (0xED, &app13)]);
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg);
        assert_eq!(read_keywords(&path), ["beach", "summer", "family"]);
    }

    #[test]
    fn xmp_alone_is_enough_and_case_is_not_folded() {
        let jpeg = jpeg_with_xmp_packet(8, 8, &xmp_packet_with_subjects(&["Berlin", "berlin"]));
        assert_eq!(keywords_in(&jpeg), ["Berlin", "berlin"]);
    }

    #[test]
    fn files_without_keywords_or_that_cannot_be_read_have_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_keywords(&write_file(dir.path(), "a.jpg", &jpeg_bytes(8, 8))).is_empty());
        assert!(read_keywords(&write_file(dir.path(), "a.png", &png_bytes(8, 8))).is_empty());
        assert!(read_keywords(&dir.path().join("missing.jpg")).is_empty());
    }

    fn jpeg_with(xmp: Option<String>, iptc: &[(u8, &[u8])]) -> Vec<u8> {
        let mut segments: Vec<(u8, Vec<u8>)> = Vec::new();
        if let Some(packet) = xmp {
            let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            app1.extend_from_slice(packet.as_bytes());
            segments.push((0xE1, app1));
        }
        if !iptc.is_empty() {
            segments.push((0xED, iptc_app13_datasets(iptc)));
        }
        let refs: Vec<(u8, &[u8])> = segments.iter().map(|(m, b)| (*m, b.as_slice())).collect();
        jpeg_with_segments(8, 8, &refs)
    }

    #[test]
    fn xmp_wins_over_iptc_and_iptc_alone_is_enough() {
        let both = jpeg_with(
            Some(xmp_packet_with_description(&[(
                Some("x-default"),
                "From Lightroom",
            )])),
            &[(120, b"From Picasa")],
        );
        assert_eq!(
            embedded_in(&both).caption.as_deref(),
            Some("From Lightroom")
        );
        let iptc_only = jpeg_with(None, &[(120, b"From Picasa")]);
        assert_eq!(
            embedded_in(&iptc_only).caption.as_deref(),
            Some("From Picasa")
        );
    }

    #[test]
    fn a_whitespace_caption_is_none_and_keywords_still_come_through() {
        let jpeg = jpeg_with(None, &[(120, b" \n\t "), (25, b"lake")]);
        let embedded = embedded_in(&jpeg);
        assert_eq!(embedded.caption, None);
        assert_eq!(embedded.keywords, ["lake"]);
    }

    #[test]
    fn a_long_caption_is_cut_on_a_character_boundary() {
        // 1,999 ASCII characters, then a two-byte `é` as character 2,000, then more: a cut by
        // bytes would land inside the `é` and panic; the cut is by characters.
        let text = format!("{}é tail", "a".repeat(MAX_CAPTION_CHARS - 1));
        let jpeg = jpeg_with(
            Some(xmp_packet_with_description(&[(Some("x-default"), &text)])),
            &[],
        );
        let caption = embedded_in(&jpeg).caption.unwrap();
        assert_eq!(caption.chars().count(), MAX_CAPTION_CHARS);
        assert!(caption.ends_with('é'));
    }

    #[test]
    fn exif_image_description_is_not_a_caption() {
        // Cameras write their own name there; a file with nothing else has no caption.
        let spec = ExifSpec {
            description: Some("OLYMPUS DIGITAL CAMERA"),
            ..ExifSpec::default()
        };
        let jpeg = jpeg_with_exif_spec(8, 8, &spec);
        assert!(
            jpeg.windows(22).any(|w| w == b"OLYMPUS DIGITAL CAMERA"),
            "the fixture really carries the EXIF text"
        );
        assert_eq!(embedded_in(&jpeg).caption, None);
    }
}
