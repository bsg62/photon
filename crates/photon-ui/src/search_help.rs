//! Everything the search box understands, as the help beside it lists it:
//! `ui/src/lib/search-help.ts`, string for string, until the switch-over deletes that file.
//!
//! Written by hand from `Query::terms` in `photon-core`'s `search.rs` and held to it by the
//! tests below, which read that file: a prefix, an `is:` or a `has:` the parser knows that
//! is not here fails them, and so does one listed here that the parser does not know. What
//! they cannot check is the wording, or the examples inside it.
//!
//! No egui here.

/// One thing the search box understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The text as it is typed.
    pub text: &'static str,
    /// Whether a click puts it in the box: a whole term (`is:starred`), or a prefix the
    /// user finishes (`camera:`). Otherwise it is an example to read - a word, a phrase, a
    /// date - since the user's own is what belongs in the box.
    pub insert: bool,
    pub does: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpGroup {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

pub const SEARCH_HELP: &[HelpGroup] = &[
    HelpGroup {
        title: "Words",
        entries: &[
            Entry {
                text: "lisbon tram",
                insert: false,
                does: "Both words, wherever they are: file or folder name, camera, lens, keyword, caption",
            },
            Entry {
                text: "lake OR pond",
                insert: false,
                does: "Either one. OR is written in capitals",
            },
            Entry {
                text: "\"summer hike\"",
                insert: false,
                does: "The words together, in that order",
            },
            Entry {
                text: "-draft",
                insert: false,
                does: "Without it. A hyphen works before anything below too: -tag:family, -has:tag",
            },
        ],
    },
    HelpGroup {
        title: "When",
        entries: &[
            Entry {
                text: "2024-06",
                insert: false,
                does: "A year, month or day it was taken: 2024, 2024-06, 2024-06-14",
            },
            Entry {
                text: "from:",
                insert: true,
                does: "From then on: from:2019-06",
            },
            Entry {
                text: "to:",
                insert: true,
                does: "Up to and including then: to:2020",
            },
            Entry {
                text: "on:",
                insert: true,
                does: "That day in any year: on:07-14",
            },
        ],
    },
    HelpGroup {
        title: "In one place",
        entries: &[
            Entry {
                text: "camera:",
                insert: true,
                does: "The camera only: camera:canon",
            },
            Entry {
                text: "lens:",
                insert: true,
                does: "The lens only: lens:50mm",
            },
            Entry {
                text: "tag:",
                insert: true,
                does: "A keyword: tag:family",
            },
            Entry {
                text: "person:",
                insert: true,
                does: "Someone named on the photo: person:anna",
            },
            Entry {
                text: "album:",
                insert: true,
                does: "An album it is in: album:lisbon",
            },
            Entry {
                text: "folder:",
                insert: true,
                does: "Its folder: folder:2019",
            },
        ],
    },
    HelpGroup {
        title: "What it is",
        entries: &[
            Entry {
                text: "is:starred",
                insert: true,
                does: "Starred",
            },
            Entry {
                text: "is:edited",
                insert: true,
                does: "Turned or cropped in photon",
            },
            Entry {
                text: "is:photo",
                insert: true,
                does: "A photo, not a video",
            },
            Entry {
                text: "is:video",
                insert: true,
                does: "A video",
            },
            Entry {
                text: "is:duplicate",
                insert: true,
                does: "Has a copy or a look-alike in the library",
            },
            Entry {
                text: "is:portrait",
                insert: true,
                does: "Taller than wide",
            },
            Entry {
                text: "is:landscape",
                insert: true,
                does: "Wider than tall",
            },
            Entry {
                text: "is:square",
                insert: true,
                does: "As wide as tall",
            },
        ],
    },
    HelpGroup {
        title: "What it has",
        entries: &[
            Entry {
                text: "has:tag",
                insert: true,
                does: "Any keyword. -has:tag finds the untagged",
            },
            Entry {
                text: "has:caption",
                insert: true,
                does: "A caption",
            },
            Entry {
                text: "has:album",
                insert: true,
                does: "In at least one album",
            },
            Entry {
                text: "has:person",
                insert: true,
                does: "Someone named on it",
            },
            Entry {
                text: "has:face",
                insert: true,
                does: "A face, named or not",
            },
            Entry {
                text: "faces:",
                insert: true,
                does: "That many faces: faces:2, or faces:3+ for three or more",
            },
            Entry {
                text: "has:gps",
                insert: true,
                does: "Records where it was taken",
            },
            Entry {
                text: "near:",
                insert: true,
                does: "Within a kilometre of a place: near:46.54,12.14 - or near:46.54,12.14,5km",
            },
        ],
    },
    HelpGroup {
        title: "Numbers",
        entries: &[
            Entry {
                text: "50mm",
                insert: false,
                does: "A focal length as a word. So are f/1.8 and iso400",
            },
            Entry {
                text: "size:",
                insert: true,
                does: "File size, with kb, mb or gb: size:>10mb",
            },
            Entry {
                text: "mp:",
                insert: true,
                does: "Megapixels: mp:<2",
            },
            Entry {
                text: "iso:",
                insert: true,
                does: "ISO: iso:>=1600",
            },
            Entry {
                text: "aperture:",
                insert: true,
                does: "The f-number: aperture:<2",
            },
            Entry {
                text: "focal:",
                insert: true,
                does: "Focal length in millimetres: focal:>100",
            },
        ],
    },
];

/// The box's text after a click on an entry: the term after what is already there, one
/// space between. The search is every word at once, so adding narrows it - which is what a
/// click on "is:starred" under a typed "lisbon" is asking for.
///
/// A phrase left open is closed first. The parser reads an unclosed quote as running to
/// the end, so the term added after `"summer hike` would have become part of the phrase.
pub fn insert_term(query: &str, term: &str) -> String {
    let held = query.trim_end();
    if held.is_empty() {
        return term.to_owned();
    }
    let open = held.matches('"').count() % 2 == 1;
    format!("{held}{} {term}", if open { "\"" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const PARSER: &str = include_str!("../../photon-core/src/search.rs");
    /// The prefixes that take one of a fixed set of words rather than a value of the user's.
    const FIXED: [&str; 2] = ["is:", "has:"];

    /// `Query::terms`, the one function that reads a token: everything from its signature
    /// to the next one. The tests further down that file name prefixes too, and must not
    /// count. Cut by what the two lines begin with, never by a line ending: a Windows
    /// checkout has another.
    fn terms(parser: &str) -> &str {
        let from = parser.find("fn terms(").expect("the parser has `terms`");
        let to = parser.find("pub fn needs(").expect("and `needs` after it");
        &parser[from..to]
    }

    /// Every lowercase word in `text` that stands between `before` and `after`.
    fn between<'a>(text: &'a str, before: &str, after: &str) -> Vec<&'a str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find(after) {
            let head = &rest[..at];
            if let Some(start) = head.rfind(before) {
                let word = &head[start + before.len()..];
                let plain = word.bytes().all(|b| b.is_ascii_lowercase() || b == b':');
                if plain && !word.is_empty() {
                    found.push(word);
                }
            }
            rest = &rest[at + after.len()..];
        }
        found
    }

    /// Every prefix the parser knows, as it is typed: `camera:`, `is:`, `size:`.
    fn prefixes(terms: &str) -> Vec<&str> {
        between(terms, "prefixed(\"", "\")")
    }

    /// The values a prefix with a fixed set takes: the arms of the `match` that follows it.
    fn values_of<'a>(terms: &'a str, prefix: &str) -> Vec<&'a str> {
        let from = terms
            .find(&format!("prefixed(\"{prefix}\")"))
            .unwrap_or_else(|| panic!("the parser has no {prefix}"));
        let rest = &terms[from + 1..];
        let block = &rest[..rest.find("prefixed(\"").unwrap_or(rest.len())];
        between(block, "\"", "\" =>")
    }

    /// Everything the parser understands, as the list would name it.
    fn known(parser: &str) -> BTreeSet<String> {
        let terms = terms(parser);
        let mut known = BTreeSet::new();
        for prefix in prefixes(terms) {
            if FIXED.contains(&prefix) {
                known.extend(
                    values_of(terms, prefix)
                        .iter()
                        .map(|value| format!("{prefix}{value}")),
                );
            } else {
                known.insert(prefix.to_owned());
            }
        }
        known
    }

    fn listed() -> BTreeSet<String> {
        (SEARCH_HELP.iter().flat_map(|group| group.entries))
            .filter(|entry| entry.insert)
            .map(|entry| entry.text.to_owned())
            .collect()
    }

    // The two checks below pass on a parser they could read nothing from. These are what
    // it holds today; a new prefix or value belongs here and in the list.
    #[test]
    fn the_grammar_is_read_out_of_the_parser() {
        let terms = terms(PARSER);
        fn sorted(mut words: Vec<&str>) -> Vec<&str> {
            words.sort_unstable();
            words
        }
        assert_eq!(
            sorted(prefixes(terms)),
            [
                "album:",
                "aperture:",
                "camera:",
                "faces:",
                "focal:",
                "folder:",
                "from:",
                "has:",
                "is:",
                "iso:",
                "lens:",
                "mp:",
                "near:",
                "on:",
                "person:",
                "size:",
                "tag:",
                "to:"
            ]
        );
        assert_eq!(
            sorted(values_of(terms, "is:")),
            [
                "duplicate",
                "edited",
                "landscape",
                "photo",
                "portrait",
                "square",
                "starred",
                "video"
            ]
        );
        assert_eq!(
            sorted(values_of(terms, "has:")),
            ["album", "caption", "face", "gps", "person", "tag"]
        );
    }

    // A Windows checkout hands the parser over with other line endings, and a reader that
    // matched across a line would find nothing there, and nowhere else.
    #[test]
    fn the_parser_is_read_with_either_line_ending() {
        let unix = PARSER.replace("\r\n", "\n");
        let windows = unix.replace('\n', "\r\n");
        assert_eq!(known(&unix), known(&windows));
        assert!(known(&windows).contains("is:starred"));
    }

    #[test]
    fn everything_the_parser_understands_is_listed() {
        let missing: Vec<_> = known(PARSER).difference(&listed()).cloned().collect();
        assert_eq!(missing, Vec::<String>::new());
    }

    #[test]
    fn nothing_is_offered_that_the_parser_would_drop() {
        let unknown: Vec<_> = listed().difference(&known(PARSER)).cloned().collect();
        assert_eq!(unknown, Vec::<String>::new());
    }

    #[test]
    fn every_entry_says_what_it_finds_once() {
        let texts: Vec<_> = (SEARCH_HELP.iter().flat_map(|group| group.entries))
            .map(|entry| entry.text)
            .collect();
        let once: BTreeSet<_> = texts.iter().collect();
        assert_eq!(once.len(), texts.len());
        assert_eq!(SEARCH_HELP.len(), 6);
        for group in SEARCH_HELP {
            assert!(!group.title.is_empty());
            assert!(!group.entries.is_empty(), "{}", group.title);
            for entry in group.entries {
                assert!(!entry.does.is_empty(), "{}", entry.text);
            }
        }
    }

    // The list is the Svelte UI's until the switch-over: a term added there is added here.
    #[test]
    fn the_list_is_the_svelte_uis() {
        let svelte = include_str!("../../../ui/src/lib/search-help.ts");
        for entry in SEARCH_HELP.iter().flat_map(|group| group.entries) {
            let quoted = format!("text: '{}'", entry.text);
            assert!(
                svelte.contains(&quoted),
                "{} is not in search-help.ts",
                entry.text
            );
            assert!(
                svelte.contains(&format!("does: '{}'", entry.does)),
                "{} is worded otherwise in search-help.ts",
                entry.text
            );
        }
        let entries = svelte.matches("{ text: '").count();
        let here: usize = SEARCH_HELP.iter().map(|group| group.entries.len()).sum();
        assert_eq!(here, entries, "an entry of search-help.ts is not here");
    }

    #[test]
    fn a_term_is_alone_in_an_empty_box() {
        assert_eq!(insert_term("", "is:starred"), "is:starred");
        assert_eq!(insert_term("   ", "camera:"), "camera:");
    }

    #[test]
    fn a_term_goes_after_what_is_there_one_space_between() {
        assert_eq!(insert_term("lisbon", "is:starred"), "lisbon is:starred");
        // A space already typed is not doubled.
        assert_eq!(insert_term("lisbon ", "tag:"), "lisbon tag:");
        assert_eq!(
            insert_term("lake OR pond", "has:gps"),
            "lake OR pond has:gps"
        );
    }

    #[test]
    fn a_phrase_left_open_is_closed_so_the_term_is_not_read_as_part_of_it() {
        assert_eq!(
            insert_term("\"summer hike", "is:starred"),
            "\"summer hike\" is:starred"
        );
        assert_eq!(
            insert_term("lake \"summer hi ", "tag:"),
            "lake \"summer hi\" tag:"
        );
        // A closed phrase, and two of them, are left alone.
        assert_eq!(
            insert_term("\"summer hike\"", "is:starred"),
            "\"summer hike\" is:starred"
        );
        assert_eq!(insert_term("\"a b\" \"c d\"", "on:"), "\"a b\" \"c d\" on:");
    }

    // Leading space and inner spacing are the user's, a quoted phrase's included.
    #[test]
    fn what_was_typed_is_left_as_it_was() {
        assert_eq!(
            insert_term(" \"summer  hike\"", "on:"),
            " \"summer  hike\" on:"
        );
    }
}
