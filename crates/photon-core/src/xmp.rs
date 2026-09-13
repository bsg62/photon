//! Reads `xmp:Rating` out of a photo's embedded XMP packet.
//!
//! The packet is plain text in every container photon supports — JPEG `APP1`, PNG
//! uncompressed `iTXt`, WebP `XMP ` chunk, GIF Application Extension — so a bounded scan
//! for the packet markers finds all of them without a parser per format. photon only ever
//! reads: nothing here writes to a file.

use quick_xml::Reader;
use quick_xml::events::Event;
use std::{fs::File, io::Read, path::Path};

/// How much of a file is searched for the packet. XMP sits near the start of every
/// container above; this stops a rating lookup pulling a 20MB photo through memory, and
/// keeps the per-photo cost of a first scan predictable rather than scaling with size.
pub const MAX_PREFIX: usize = 256 * 1024;

const PACKET_START: &[u8] = b"<x:xmpmeta";
const PACKET_END: &str = "</x:xmpmeta>";

/// The rating in a photo's embedded XMP, if it has one. Never fails: an unreadable file,
/// a truncated packet or malformed XML all yield `None`, because one bad photo must not
/// fail a scan of a hundred thousand.
pub fn read_rating(path: &Path) -> Option<u8> {
    let mut file = File::open(path).ok()?;
    let mut buf = Vec::new();
    file.by_ref()
        .take(MAX_PREFIX as u64)
        .read_to_end(&mut buf)
        .ok()?;
    let start = buf
        .windows(PACKET_START.len())
        .position(|w| w == PACKET_START)?;
    let text = String::from_utf8_lossy(&buf[start..]);
    let end = text.find(PACKET_END).map(|i| i + PACKET_END.len())?;
    rating_from_xml(&text[..end])
}

/// Reads `xmp:Rating` from an XMP packet, in either the attribute or the child-element
/// spelling. Returns `None` for a missing, malformed or out-of-range value — including
/// `-1`, which means "rejected" rather than a star.
pub fn rating_from_xml(xml: &str) -> Option<u8> {
    let mut reader = Reader::from_str(xml);
    let mut in_rating = false;
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => return None,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                for attr in e.attributes().flatten() {
                    // No unescaping: a rating is digits, so there is nothing an entity
                    // could encode here, and the attribute value is already valid UTF-8.
                    if attr.key.as_ref() == "xmp:Rating"
                        && let Some(rating) = parse_rating(&attr.value)
                    {
                        return Some(rating);
                    }
                }
                if e.name().as_ref() == "xmp:Rating" {
                    in_rating = true;
                }
            }
            Ok(Event::Text(t)) if in_rating => {
                // Same reasoning: no unescaping needed for a purely numeric value.
                if let Some(rating) = parse_rating(t.as_ref()) {
                    return Some(rating);
                }
                in_rating = false;
            }
            _ => {}
        }
    }
}

fn parse_rating(value: &str) -> Option<u8> {
    let n: i32 = value.trim().parse().ok()?;
    (0..=5).contains(&n).then_some(n as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        gif_with_xmp, jpeg_bytes, jpeg_with_xmp, png_with_xmp, write_file, xmp_packet,
    };

    #[test]
    fn reads_the_rating_from_an_attribute() {
        assert_eq!(rating_from_xml(&xmp_packet(3)), Some(3));
        assert_eq!(rating_from_xml(&xmp_packet(1)), Some(1));
        assert_eq!(rating_from_xml(&xmp_packet(0)), Some(0));
        assert_eq!(rating_from_xml(&xmp_packet(5)), Some(5));
    }

    #[test]
    fn reads_the_rating_from_a_child_element() {
        // Both spellings are legal XMP; some tools write the element form.
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
            xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <xmp:Rating>4</xmp:Rating>
            </rdf:Description></rdf:RDF></x:xmpmeta>"#;
        assert_eq!(rating_from_xml(xml), Some(4));
    }

    #[test]
    fn a_rejected_or_out_of_range_rating_is_not_a_rating() {
        // -1 means "rejected" in XMP, which is not a star. 6 is out of range.
        assert_eq!(rating_from_xml(&xmp_packet(-1)), None);
        assert_eq!(rating_from_xml(&xmp_packet(6)), None);
    }

    #[test]
    fn xml_without_a_rating_or_that_is_malformed_yields_none() {
        assert_eq!(
            rating_from_xml(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"/>"#),
            None
        );
        assert_eq!(rating_from_xml("<not xml at all"), None);
        assert_eq!(rating_from_xml(""), None);
    }

    #[test]
    fn the_gif_fixture_is_a_decodable_gif() {
        // Otherwise the "reads a rating out of each container" test proves only that a
        // packet can be found in arbitrary bytes — which the cap test already covers.
        let bytes = gif_with_xmp(8, 8, 3);
        assert!(
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Gif).is_ok(),
            "the fixture must be a valid GIF, not merely GIF-shaped"
        );
    }

    #[test]
    fn reads_a_rating_out_of_each_container() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = write_file(dir.path(), "a.jpg", &jpeg_with_xmp(8, 8, 2));
        let png = write_file(dir.path(), "a.png", &png_with_xmp(8, 8, 5));
        let gif = write_file(dir.path(), "a.gif", &gif_with_xmp(8, 8, 1));
        assert_eq!(read_rating(&jpg), Some(2));
        assert_eq!(read_rating(&png), Some(5));
        assert_eq!(read_rating(&gif), Some(1));
    }

    #[test]
    fn a_file_without_xmp_or_that_cannot_be_read_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write_file(dir.path(), "plain.jpg", &jpeg_bytes(8, 8));
        assert_eq!(read_rating(&plain), None);
        assert_eq!(read_rating(&dir.path().join("missing.jpg")), None);
    }

    #[test]
    fn a_packet_beyond_the_read_cap_is_not_found() {
        // The cap is what stops a rating lookup pulling a 20MB photo through memory, so it
        // has to actually bound the read rather than being advisory.
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![b'\0'; MAX_PREFIX];
        bytes.extend_from_slice(xmp_packet(4).as_bytes());
        let path = write_file(dir.path(), "late.jpg", &bytes);
        assert_eq!(read_rating(&path), None);

        // ...but one that ends just inside the cap is found.
        let packet = xmp_packet(4);
        let mut early = vec![b'\0'; MAX_PREFIX - packet.len()];
        early.extend_from_slice(packet.as_bytes());
        let path = write_file(dir.path(), "early.jpg", &early);
        assert_eq!(read_rating(&path), Some(4));
    }
}
