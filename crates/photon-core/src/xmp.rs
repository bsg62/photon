//! Reads `xmp:Rating` and `dc:subject` out of a photo's embedded XMP packet.
//!
//! The packet is plain text in every container photon supports — JPEG `APP1`, PNG
//! uncompressed `iTXt`, WebP `XMP ` chunk, GIF Application Extension — so a bounded scan
//! for the packet markers finds all of them without a parser per format. WebP is untested:
//! the design is container-agnostic but no WebP fixture exists yet. photon only ever
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
    rating_from_xml(&packet_in(&buf)?)
}

/// The XMP packet in the leading bytes of a file, as text, if one is complete there.
///
/// Decoded lossily: the packet is UTF-8 by specification, but the bytes around it are
/// whatever the container holds, and the slice starts at the packet marker so nothing
/// before it is decoded at all.
pub fn packet_in(prefix: &[u8]) -> Option<String> {
    let start = prefix
        .windows(PACKET_START.len())
        .position(|w| w == PACKET_START)?;
    let text = String::from_utf8_lossy(&prefix[start..]);
    let end = text.find(PACKET_END).map(|i| i + PACKET_END.len())?;
    Some(text[..end].to_string())
}

/// The keywords in an XMP packet: every `rdf:li` inside `dc:subject`, in document order,
/// trimmed, with empties dropped. Entities and character references are resolved, so
/// `Tom &amp; Jerry` comes back as written by the person, not by the encoder.
///
/// Malformed XML yields what was read up to the error rather than nothing: a keyword list
/// cut short is still a keyword list, and the alternative loses every tag in the file over
/// one stray byte at the end of it.
pub fn subjects_from_xml(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut subjects = Vec::new();
    let mut in_subject = false;
    let mut in_item = false;
    let mut current = String::new();
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "dc:subject" => in_subject = true,
                "rdf:li" if in_subject => {
                    in_item = true;
                    current.clear();
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                "dc:subject" => in_subject = false,
                "rdf:li" if in_item => {
                    in_item = false;
                    let keyword = current.trim();
                    if !keyword.is_empty() {
                        subjects.push(keyword.to_string());
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_item => current.push_str(t.as_ref()),
            Ok(Event::CData(t)) if in_item => current.push_str(&t),
            // quick-xml hands `&amp;` and `&#x263A;` over as their own events rather than
            // resolving them inside the text, so a keyword with an ampersand arrives in
            // three pieces.
            Ok(Event::GeneralRef(r)) if in_item => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    current.push(c);
                } else if let Some(text) = quick_xml::escape::resolve_predefined_entity(&r) {
                    current.push_str(text);
                }
            }
            _ => {}
        }
    }
    subjects
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
                // Set (not just raised) on every Start/Empty: an unrelated element between
                // an empty `<xmp:Rating/>` and its sibling text must not be misread as the
                // rating, so entering any other element clears the flag.
                in_rating = e.name().as_ref() == "xmp:Rating";
            }
            Ok(Event::End(_)) => {
                in_rating = false;
            }
            Ok(Event::Text(t)) if in_rating => {
                // Whitespace between the tag and its digits is not a failed parse: real XMP
                // is usually pretty-printed, so `<xmp:Rating>\n4\n</xmp:Rating>` arrives as
                // an indentation text node first. Clearing the flag on it would skip the
                // digit that follows and silently miss the rating.
                if t.as_ref().trim().is_empty() {
                    continue;
                }
                // Same reasoning as the attribute: no unescaping needed for a numeric value.
                if let Some(rating) = parse_rating(t.as_ref()) {
                    return Some(rating);
                }
                in_rating = false;
            }
            _ => {}
        }
    }
}

/// The caption in an XMP packet: `dc:description`'s `rdf:Alt` entry whose `xml:lang` is
/// `x-default`, or else its first entry, trimmed. `None` when there is no non-empty one.
///
/// `x-default` first because that is the entry a tool writes when the user typed one caption;
/// the language-tagged ones are translations. Resolves references the way
/// `subjects_from_xml` does, so a caption with an ampersand arrives whole.
pub fn description_from_xml(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut in_description = false;
    // Some(whether this entry is x-default) while inside an rdf:li of dc:description.
    let mut item: Option<bool> = None;
    let mut current = String::new();
    let mut first: Option<String> = None;
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "dc:description" => in_description = true,
                "rdf:li" if in_description => {
                    let is_default = e.attributes().flatten().any(|attr| {
                        attr.key.as_ref() == "xml:lang" && attr.value.as_ref() == "x-default"
                    });
                    item = Some(is_default);
                    current.clear();
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                "dc:description" => in_description = false,
                "rdf:li" => {
                    if let Some(is_default) = item.take() {
                        let text = current.trim();
                        if !text.is_empty() {
                            if is_default {
                                return Some(text.to_string());
                            }
                            first.get_or_insert_with(|| text.to_string());
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if item.is_some() => current.push_str(t.as_ref()),
            Ok(Event::CData(t)) if item.is_some() => current.push_str(&t),
            Ok(Event::GeneralRef(r)) if item.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    current.push(c);
                } else if let Some(text) = quick_xml::escape::resolve_predefined_entity(&r) {
                    current.push_str(text);
                }
            }
            _ => {}
        }
    }
    first
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
        xmp_packet_with_description, xmp_packet_with_subjects,
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
    fn a_rating_split_across_text_events_is_still_read() {
        // A comment (or CDATA, or a processing instruction) splits the text run, so the
        // indentation arrives as its own Text event and the digits as a second one. Without
        // skipping the blank one, its failed parse clears the "inside a rating" flag and the
        // number that follows is never looked at — a silent miss of a rating that is there.
        //
        // This is the test that pins the guard: reverting it yields None instead of Some(4).
        assert_eq!(
            rating_from_xml("<xmp:Rating>\n<!-- c -->4</xmp:Rating>"),
            Some(4)
        );
    }

    #[test]
    fn a_pretty_printed_rating_element_is_read() {
        // Coverage, not proof: quick-xml emits one Text event for a contiguous run and
        // `parse_rating` already trims, so indented XMP was read correctly before the guard
        // above existed and is read correctly without it. Pinned because it is the shape
        // real files actually take.
        assert_eq!(
            rating_from_xml("<xmp:Rating>\n      4\n    </xmp:Rating>"),
            Some(4)
        );
    }

    #[test]
    fn an_unrelated_element_after_an_empty_rating_tag_is_not_misread_as_the_rating() {
        // `<xmp:Rating/>` sets `in_rating`; without clearing it on the next Start for a
        // different element, the Urgency text below would be misread as the rating.
        let xml = r#"<xmp:Rating/><photoshop:Urgency>3</photoshop:Urgency>"#;
        assert_eq!(rating_from_xml(xml), None);
    }

    #[test]
    fn subjects_are_read_from_the_dc_bag_in_order() {
        let xml =
            crate::testutil::xmp_packet_with_subjects(&["beach", " summer ", "", "Tom & Jerry"]);
        assert_eq!(
            subjects_from_xml(&xml),
            ["beach", "summer", "Tom & Jerry"],
            "trimmed, empties dropped, the ampersand entity resolved"
        );
    }

    #[test]
    fn a_character_reference_in_a_subject_is_resolved() {
        let xml = "<dc:subject><rdf:Bag><rdf:li>caf&#xe9;</rdf:li></rdf:Bag></dc:subject>";
        assert_eq!(subjects_from_xml(xml), ["café"]);
    }

    #[test]
    fn list_items_outside_the_subject_bag_are_not_keywords() {
        // `dc:creator` is an rdf:Seq of rdf:li too; only the subject bag holds keywords.
        let xml = r#"<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
            <dc:creator><rdf:Seq><rdf:li>Ada</rdf:li></rdf:Seq></dc:creator>
            <dc:subject><rdf:Bag><rdf:li>lake</rdf:li></rdf:Bag></dc:subject>
        </rdf:Description>"#;
        assert_eq!(subjects_from_xml(xml), ["lake"]);
        assert!(subjects_from_xml("<x:xmpmeta/>").is_empty());
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

    #[test]
    fn a_packet_with_a_start_marker_but_no_closing_tag_yields_none() {
        // The packet is truncated (e.g. a partially-written or corrupted file): the start
        // marker is found but `text.find(PACKET_END)` never matches, so `?` bails to None.
        let dir = tempfile::tempdir().unwrap();
        // The rating sits on the start tag on purpose: without it, an implementation that
        // treated truncation as "scan to the end of the buffer" would also return None here
        // and the test could not tell the two apart. With it, correct code still returns
        // None — the packet is incomplete — while that regression would return Some(4).
        let mut bytes = PACKET_START.to_vec();
        bytes.extend_from_slice(br#" xmlns:x="adobe:ns:meta/" xmp:Rating="4"><rdf:RDF>"#);
        let path = write_file(dir.path(), "truncated.jpg", &bytes);
        assert_eq!(read_rating(&path), None);
    }

    #[test]
    fn the_default_language_description_is_the_caption() {
        let xml =
            xmp_packet_with_description(&[(Some("de"), "Oma"), (Some("x-default"), "Grandma")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Grandma"));
    }

    #[test]
    fn without_a_default_language_the_first_entry_is_the_caption() {
        let xml = xmp_packet_with_description(&[(Some("de"), "  Oma  "), (Some("fr"), "Mamie")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Oma"));
    }

    #[test]
    fn a_description_with_an_entity_reference_is_read_whole() {
        let xml = xmp_packet_with_description(&[(Some("x-default"), "Tom & Jerry")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Tom & Jerry"));
    }

    #[test]
    fn a_description_with_a_numeric_character_reference_is_read_whole() {
        // `xmp_packet_with_description` escapes `&`, which would double-escape a numeric
        // reference, so this packet is written out directly.
        let xml = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:description><rdf:Alt><rdf:li xml:lang="x-default">Caf&#233; Lisboa</rdf:li></rdf:Alt></dc:description></rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;
        assert_eq!(description_from_xml(xml).as_deref(), Some("Café Lisboa"));
    }

    #[test]
    fn keywords_and_empty_descriptions_are_not_captions() {
        assert_eq!(
            description_from_xml(&xmp_packet_with_subjects(&["beach"])),
            None
        );
        let blank = xmp_packet_with_description(&[(Some("x-default"), "   ")]);
        assert_eq!(description_from_xml(&blank), None);
    }
}
