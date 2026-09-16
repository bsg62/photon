//! The keywords embedded in a photo file, from XMP and IPTC.
//!
//! One bounded read serves both parsers: the XMP packet and the IPTC block both sit in the
//! leading segments of every container photon reads, so `xmp::MAX_PREFIX` bytes is enough
//! and a 20 MB photo is never pulled through memory for a tag list. photon only reads;
//! nothing here writes to a file.

use std::{fs::File, io::Read, path::Path};

/// The distinct keywords in `path`, XMP `dc:subject` first and then IPTC 2:25, each in its
/// own order. Picasa and Lightroom write the same list to both places, so the second
/// source normally adds nothing; when they differ (a tool that wrote only one of them),
/// the union is what the person tagged. Never fails: an unreadable file has no keywords.
///
/// Exact de-duplication, not case-folded: "Berlin" and "berlin" are two keywords to every
/// tool that wrote them, and folding them here would show one spelling and hide the other.
pub fn read_keywords(path: &Path) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    if file
        .by_ref()
        .take(crate::xmp::MAX_PREFIX as u64)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Vec::new();
    }
    keywords_in(&buf)
}

/// [`read_keywords`] over bytes already read.
pub fn keywords_in(prefix: &[u8]) -> Vec<String> {
    let mut keywords: Vec<String> = Vec::new();
    let xmp = crate::xmp::packet_in(prefix)
        .map(|packet| crate::xmp::subjects_from_xml(&packet))
        .unwrap_or_default();
    for keyword in xmp.into_iter().chain(crate::iptc::keywords_in(prefix)) {
        if !keywords.contains(&keyword) {
            keywords.push(keyword);
        }
    }
    keywords
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        iptc_app13, jpeg_bytes, jpeg_with_segments, jpeg_with_xmp_packet, png_bytes, write_file,
        xmp_packet_with_subjects,
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
}
