//! Matching a typed search query against a photo's names.
//!
//! The whole of what "search matches this photo" means lives here, rather than inside the
//! row closure in `library::items`, so that the parts that are easy to get wrong - case
//! folding outside ASCII, wildcard characters, an empty query - are testable without
//! seeding a database and counting rows.

use crate::media::MediaKind;

/// A parsed search query: alternatives separated by `OR`, each a list of terms that must
/// all match.
///
/// **Words narrow.** `lake bell` finds photos matching both words, each wherever it likes:
/// `italy lake` finds `lake.jpg` in the folder `2019 Italy`. Until 2026-09-18 words were
/// OR-ed, for the "I remember a lake and a bell" query, and that was reversed (spec
/// `2026-09-18-photon-search-grammar-design.md`) because it cannot coexist with any term
/// meant as a filter: under OR, adding `2019` or `camera:x100` to a query returned more
/// photos, not fewer. Widening is still there, but asked for: `lake OR bell`.
///
/// **The grammar** is deliberately small:
/// - `AND` and `OR` are operators only in capitals, so `salt and pepper` still searches for
///   the word "and". `AND` binds tighter and is what adjacency already means; there are no
///   parentheses.
/// - `"double quotes"` make one term of several words and make an operator or a prefix
///   literal.
/// - `from:` and `to:` take `YYYY`, `YYYY-MM` or `YYYY-MM-DD` and keep photos taken within
///   or after, or within or before, the whole period named: `from:2019-06 to:2019-08` is
///   June to August inclusive. Naming both ends inclusively leaves nothing to guess, which
///   `after:`/`before:` would (is `after:2019` in 2019?).
/// - `camera:` and `lens:` restrict a term to that field. A bare `canon` matches a folder
///   named Canon as readily as the camera; `camera:canon` does not. A quoted value with
///   several words (`camera:"canon eos 5d"`, which is what the info panel's links send)
///   asks for every word in the field rather than the exact phrase, so the link does not
///   depend on how the maker spaced its own name.
/// - Anything dangling is ignored rather than searched for: an operator with nothing on one
///   side, a prefix with no value. They are what a query looks like halfway through being
///   typed, and treating `lake OR` as "lake AND the word or" would flash an empty grid
///   between two keystrokes.
/// - `video` and `photo`, unquoted, filter on what the file is rather than searching for the
///   word: quoted (`"video"`) they are the word, for a folder actually named Videos.
///
/// **Matching is done here rather than with SQL `LIKE`** for two reasons, both of which
/// bite real libraries. SQLite folds case for ASCII only, so `MÜNCHEN` would never find
/// `München`, and `lower()` has the same limit without the ICU extension - a native
/// dependency this project does not take. And `LIKE` reads `%` and `_` in the user's text
/// as wildcards unless every one is escaped, so a search for `50%` would return
/// everything. `contains` has no metacharacters to escape and cannot get that wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    alternatives: Vec<Vec<Term>>,
}

/// One lowercased needle and where it may be found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Any(String),
    Camera(String),
    Lens(String),
    /// Taken at or after this instant: the first second of the period `from:` named.
    From(i64),
    /// Taken before this instant: the first second *after* the period `to:` named, so the
    /// whole of that period is in and `to:2019` ends exactly where `from:2020` begins.
    To(i64),
    /// `video` or `photo`, unquoted: what the file is.
    Kind(MediaKind),
}

/// What a photo offers the matcher. `any` is every searchable text, the camera and lens
/// included, so an unprefixed word still finds them; `camera` and `lens` are what the
/// prefixed terms are confined to.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fields<'a> {
    pub any: &'a [&'a str],
    /// Make and model as one string, so `camera:` can match either or a word of each.
    pub camera: Option<&'a str>,
    pub lens: Option<&'a str>,
    /// The capture time, in the naive seconds `taken_at` holds, for `from:` and `to:`.
    pub taken: Option<i64>,
    /// What the file is, for `video` and `photo`.
    pub kind: Option<MediaKind>,
}

/// A whitespace-separated piece of the raw query, quotes removed.
struct Token {
    text: String,
    /// Byte length of `text` that came before the first quote, or `None` if no part was
    /// quoted. An operator must be wholly unquoted and a prefix must end before the quote.
    unquoted_prefix: Option<usize>,
}

fn tokenize(raw: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    let mut unquoted_prefix = None;
    let mut in_quotes = false;
    let mut flush = |text: &mut String, unquoted_prefix: &mut Option<usize>| {
        if !text.is_empty() {
            tokens.push(Token {
                text: std::mem::take(text),
                unquoted_prefix: unquoted_prefix.take(),
            });
        }
        *unquoted_prefix = None;
    };
    for c in raw.chars() {
        if c == '"' {
            unquoted_prefix.get_or_insert(text.len());
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            flush(&mut text, &mut unquoted_prefix);
        } else {
            text.push(c);
        }
    }
    // An unclosed quote runs to the end of the input: the query is still being typed.
    flush(&mut text, &mut unquoted_prefix);
    tokens
}

/// The period a `from:` or `to:` value names - `YYYY`, `YYYY-MM` or `YYYY-MM-DD` - as its
/// first second and the first second after it, in the naive wall-clock seconds `taken_at`
/// holds (the same reading `metadata::date_text` makes, so `from:2019-06` and the typed
/// `2019-06` agree about every photo).
///
/// Anything else is `None` and the term is dropped, like a prefix with no value: `2019-1`
/// is how `2019-12` looks one keystroke early, and reading it as January would flash the
/// wrong photos. The digits must be exactly the widths `date_text` writes for the same
/// reason. A date that does not exist (`2019-02-30`, and month 0 or 13, or day 0) is
/// refused by the round trip through `civil_from_unix`, which only ever answers a real
/// date, rather than by range checks and a month-length table of its own.
fn period(value: &str) -> Option<(i64, i64)> {
    use crate::metadata::{civil_from_unix, naive_to_unix};
    let number = |part: &str, width: usize| {
        (part.len() == width && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let parts: Vec<&str> = value.split('-').collect();
    let year = i64::from(number(parts.first()?, 4)?);
    let month = parts.get(1).map(|p| number(p, 2)).unwrap_or(Some(1))?;
    let day = parts.get(2).map(|p| number(p, 2)).unwrap_or(Some(1))?;
    if parts.len() > 3 {
        return None;
    }
    let start = naive_to_unix(year, month, day, 0, 0, 0);
    if civil_from_unix(start) != (year, month, day) {
        return None;
    }
    let end = match parts.len() {
        1 => naive_to_unix(year + 1, 1, 1, 0, 0, 0),
        // December needs no case of its own: days-from-civil counts months from March, so
        // month 13 is January of the next year. `to:2019-12` is pinned by a test.
        2 => naive_to_unix(year, month + 1, 1, 0, 0, 0),
        _ => start + 86_400,
    };
    Some((start, end))
}

impl Query {
    /// Parses `raw` by the grammar on [`Query`].
    ///
    /// Splitting on whitespace ignores leading, trailing and repeated whitespace,
    /// including non-ASCII whitespace, and yields nothing at all for a blank query. That
    /// keeps this in step with `Engine::set_search_query`, which decides a query is blank
    /// with `trim().is_empty()`: everything the engine calls blank is empty here. The
    /// reverse does not hold - a lone `OR` is not blank and parses empty - and that is
    /// fine: the engine stays in Search showing no matches for the text in the box.
    ///
    /// Duplicate terms within an alternative are dropped because re-scanning the same
    /// needle on every row cannot change the answer.
    pub fn parse(raw: &str) -> Self {
        let mut alternatives: Vec<Vec<Term>> = vec![Vec::new()];
        for token in tokenize(raw) {
            if token.unquoted_prefix.is_none() {
                match token.text.as_str() {
                    "OR" => {
                        alternatives.push(Vec::new());
                        continue;
                    }
                    "AND" => continue,
                    _ => {}
                }
            }
            let text = token.text.to_lowercase();
            // `video` and `photo` filter on what the file is. Unquoted only: `"video"` is
            // still the word, for a folder called Videos.
            if token.unquoted_prefix.is_none() && (text == "video" || text == "photo") {
                let kind = if text == "video" {
                    MediaKind::Video
                } else {
                    MediaKind::Image
                };
                let current = alternatives
                    .last_mut()
                    .expect("starts with one alternative");
                if !current.contains(&Term::Kind(kind)) {
                    current.push(Term::Kind(kind));
                }
                continue;
            }
            let prefixed = |prefix: &str| {
                // Lowercasing never changes the length of these ASCII prefixes, so the
                // offset recorded against the raw text still applies.
                let outside_quotes = token.unquoted_prefix.is_none_or(|at| at >= prefix.len());
                (outside_quotes && text.starts_with(prefix)).then(|| &text[prefix.len()..])
            };
            let terms: Vec<Term> = if let Some(value) = prefixed("camera:") {
                value
                    .split_whitespace()
                    .map(|w| Term::Camera(w.to_string()))
                    .collect()
            } else if let Some(value) = prefixed("lens:") {
                value
                    .split_whitespace()
                    .map(|w| Term::Lens(w.to_string()))
                    .collect()
            } else if let Some(value) = prefixed("from:") {
                period(value)
                    .map(|(start, _)| Term::From(start))
                    .into_iter()
                    .collect()
            } else if let Some(value) = prefixed("to:") {
                period(value)
                    .map(|(_, end)| Term::To(end))
                    .into_iter()
                    .collect()
            } else {
                vec![Term::Any(text.clone())]
            };
            let current = alternatives
                .last_mut()
                .expect("starts with one alternative");
            for term in terms {
                if !current.contains(&term) {
                    current.push(term);
                }
            }
        }
        alternatives.retain(|terms| !terms.is_empty());
        Self { alternatives }
    }

    /// Whether the query has no terms: a blank query, or one holding only operators.
    ///
    /// The caller returns no rows for this rather than every row: a "search" matching the
    /// whole library is indistinguishable from the library.
    pub fn is_empty(&self) -> bool {
        self.alternatives.is_empty()
    }

    /// Whether some alternative has every one of its terms found, ignoring case.
    ///
    /// The haystacks are lowercased once, up front, because with AND every term of an
    /// alternative is tried and most rows fail on the first: lowercasing lazily per term
    /// would redo the same allocation for each.
    pub fn matches(&self, fields: &Fields<'_>) -> bool {
        let any: Vec<String> = fields.any.iter().map(|h| h.to_lowercase()).collect();
        let camera = fields.camera.map(str::to_lowercase);
        let lens = fields.lens.map(str::to_lowercase);
        let within = |field: &Option<String>, needle: &str| {
            field.as_deref().is_some_and(|text| text.contains(needle))
        };
        self.alternatives.iter().any(|terms| {
            terms.iter().all(|term| match term {
                Term::Any(needle) => any.iter().any(|h| h.contains(needle.as_str())),
                Term::Camera(needle) => within(&camera, needle),
                Term::Lens(needle) => within(&lens, needle),
                Term::From(start) => fields.taken.is_some_and(|t| t >= *start),
                Term::To(end) => fields.taken.is_some_and(|t| t < *end),
                Term::Kind(kind) => fields.kind == Some(*kind),
            })
        })
    }

    /// How many terms the query holds across its alternatives. Test-only: the count is
    /// not part of what callers need, but `duplicate_terms_collapse` has to see that
    /// de-duplication happened, since a variant that kept duplicates would still pass
    /// that test's match assertion and differ only in the count.
    #[cfg(test)]
    fn term_count(&self) -> usize {
        self.alternatives.iter().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A photo with only names, which is all most of these tests are about.
    fn names(q: &str, any: &[&str]) -> bool {
        Query::parse(q).matches(&Fields {
            any,
            ..Fields::default()
        })
    }

    const LAKE_BELL: &[&str] = &["lake_bell.jpg", "Trips"];

    #[test]
    fn a_single_term_matches_a_substring() {
        assert!(names("bell", LAKE_BELL));
        assert!(names("lake", LAKE_BELL));
    }

    #[test]
    fn every_word_must_match_but_each_may_match_anywhere() {
        // As one substring, "lake bell" is absent from "lake_bell.jpg" - the separator is
        // an underscore - and "trips lake" spans two haystacks.
        assert!(names("lake bell", LAKE_BELL));
        assert!(names("trips lake", LAKE_BELL));
    }

    #[test]
    fn a_word_that_matches_nothing_empties_the_result() {
        // Words narrow. Under the OR this replaced, both of these were hits.
        assert!(!names("lake zzz", LAKE_BELL));
        assert!(!names("zzz bell", LAKE_BELL));
    }

    #[test]
    fn or_widens_and_and_binds_tighter() {
        assert!(names("zzz OR bell", LAKE_BELL));
        assert!(!names("zzz OR qqq", LAKE_BELL));
        // AND binds tighter. Read the other way round - lake OR zzz first, then AND qqq -
        // each of these would be false, since "qqq" and "zzz" match nothing.
        assert!(names("lake OR zzz qqq", LAKE_BELL));
        assert!(names("zzz qqq OR lake", LAKE_BELL));
    }

    #[test]
    fn an_explicit_and_is_what_adjacency_already_means() {
        assert_eq!(Query::parse("lake AND bell"), Query::parse("lake bell"));
    }

    #[test]
    fn operators_are_capitals_only() {
        // "salt and pepper.jpg" stays findable by its own name: a lowercase "and" is a
        // word, and must itself be found.
        assert!(names("salt and pepper", &["salt and pepper.jpg"]));
        assert!(!names("salt and pepper", &["salt pepper.jpg"]));
        assert!(!names("zzz or bell", LAKE_BELL));
    }

    #[test]
    fn dangling_operators_are_ignored() {
        // What a query looks like between two keystrokes.
        assert_eq!(Query::parse("lake OR"), Query::parse("lake"));
        assert_eq!(Query::parse("OR lake"), Query::parse("lake"));
        assert_eq!(Query::parse("lake AND"), Query::parse("lake"));
        assert_eq!(
            Query::parse("lake OR OR bell"),
            Query::parse("lake OR bell")
        );
        assert!(Query::parse("OR").is_empty());
        assert!(Query::parse("AND OR AND").is_empty());
    }

    #[test]
    fn quotes_make_a_phrase_and_a_literal() {
        assert!(names("\"lake bell\"", &["lake bell.jpg"]));
        assert!(!names("\"lake bell\"", LAKE_BELL), "the phrase has a space");
        // A quoted operator is a word, not an operator.
        assert!(!names("zzz \"OR\" bell", LAKE_BELL));
        assert!(names("\"OR\"", &["floor.jpg"]));
        // An unclosed quote runs to the end.
        assert!(names("\"lake be", &["lake bell.jpg"]));
        assert!(Query::parse("\"\"").is_empty());
    }

    #[test]
    fn a_prefixed_term_is_confined_to_its_field() {
        let photo = Fields {
            any: &[
                "a.jpg",
                "Canon outing",
                "NIKON CORPORATION",
                "NIKON D750",
                "50mm f/1.8",
            ],
            camera: Some("NIKON CORPORATION NIKON D750"),
            lens: Some("50mm f/1.8"),
            taken: None,
            kind: None,
        };
        assert!(Query::parse("canon").matches(&photo), "the folder name");
        assert!(!Query::parse("camera:canon").matches(&photo));
        assert!(Query::parse("camera:d750").matches(&photo));
        assert!(Query::parse("CAMERA:D750").matches(&photo));
        assert!(Query::parse("lens:50mm").matches(&photo));
        assert!(!Query::parse("lens:d750").matches(&photo));
        assert!(!Query::parse("camera:50mm").matches(&photo));
        // A photo with no camera data never matches a camera term.
        assert!(!Query::parse("camera:d750").matches(&Fields {
            any: &["d750.jpg"],
            ..Fields::default()
        }));
    }

    #[test]
    fn a_quoted_field_value_asks_for_every_word() {
        // What the info panel sends. "nikon d750" is not a substring of the field - the
        // make sits between - so an exact-phrase reading would miss the very photo the
        // link was clicked on.
        let photo = Fields {
            any: &[],
            camera: Some("NIKON CORPORATION NIKON D750"),
            lens: None,
            taken: None,
            kind: None,
        };
        assert!(Query::parse("camera:\"corporation d750\"").matches(&photo));
        assert!(!Query::parse("camera:\"nikon d850\"").matches(&photo));
    }

    #[test]
    fn a_prefix_is_literal_inside_quotes_and_ignored_without_a_value() {
        assert!(names("\"camera:x\"", &["camera:x.jpg"]));
        assert_eq!(Query::parse("lake camera:"), Query::parse("lake"));
        assert!(Query::parse("lens:").is_empty());
    }

    #[test]
    fn blank_queries_parse_empty_and_match_nothing() {
        // Agrees with `Engine::set_search_query`'s `trim().is_empty()` on what is blank.
        for raw in ["", "   ", "\t", "\n  \t "] {
            let q = Query::parse(raw);
            assert!(q.is_empty(), "{raw:?} should parse to no terms");
            assert!(!names(raw, LAKE_BELL));
        }
    }

    #[test]
    fn a_non_blank_query_is_not_empty() {
        assert!(!Query::parse("lake").is_empty());
    }

    #[test]
    fn case_folds_outside_ascii() {
        // This is the test that pins "match in Rust, not with SQL LIKE": SQLite folds
        // case for ASCII only, so 'München' LIKE '%MÜNCHEN%' is false. The lowercase
        // query matches under both and so proves nothing - it is the all-caps one that
        // discriminates.
        assert!(names("MÜNCHEN", &["a.jpg", "München"]));
        assert!(names("münchen", &["a.jpg", "MÜNCHEN"]));
    }

    #[test]
    fn eszett_and_ss_are_not_the_same_letter() {
        // A limit of `to_lowercase`, recorded as a decision rather than left as a
        // surprise: closing it needs full case folding, which is more than this warrants.
        assert!(!names("strasse", &["Straße.jpg", "Trips"]));
    }

    #[test]
    fn sql_wildcards_are_literal_characters() {
        // `contains` has no metacharacters, so a user searching for "50%" gets the
        // photos named "50%", not every photo. Punctuation-only terms are kept for
        // exactly this reason - dropping them would break these two queries.
        assert!(names("%", &["50%.jpg", "Trips"]));
        assert!(!names("%", &["50.jpg", "Trips"]));
        assert!(names("_", LAKE_BELL));
        assert!(!names("_", &["lake-bell.jpg", "Trips"]));
    }

    #[test]
    fn duplicate_terms_collapse() {
        // Re-scanning the same needle per row cannot change the answer, so parse drops
        // repeats - including ones that differ only by case.
        assert_eq!(Query::parse("lake lake LAKE").term_count(), 1);
        assert_eq!(Query::parse("lake OR lake").term_count(), 2);
        assert!(names("lake lake", LAKE_BELL));
    }

    /// A photo taken at `y-m-d h:mi:s`, wall-clock time, carrying no names at all, so only a
    /// date term can match it.
    fn taken(y: i64, m: u32, d: u32, h: u32, mi: u32, s: u32) -> Fields<'static> {
        Fields {
            taken: Some(crate::metadata::naive_to_unix(y, m, d, h, mi, s)),
            ..Fields::default()
        }
    }

    fn dated(q: &str, photo: &Fields<'_>) -> bool {
        Query::parse(q).matches(photo)
    }

    #[test]
    fn from_and_to_take_in_the_whole_period_they_name() {
        let photo = taken(2019, 6, 15, 12, 0, 0);
        for q in [
            "from:2019",
            "from:2019-06",
            "from:2019-06-15",
            "to:2019-06-15",
            "to:2019-06",
            "to:2019",
        ] {
            assert!(dated(q, &photo), "{q} takes in 2019-06-15");
        }
        for q in [
            "from:2019-06-16",
            "from:2019-07",
            "from:2020",
            "to:2019-06-14",
            "to:2019-05",
            "to:2018",
        ] {
            assert!(!dated(q, &photo), "{q} leaves out 2019-06-15");
        }
    }

    #[test]
    fn a_period_ends_on_its_last_second_and_the_next_begins_on_its_first() {
        // The last second of a day, a month and a year, and the first second after each:
        // `to:` must end where the next `from:` begins, with nothing in both or in neither.
        let edges = [
            (
                taken(2019, 6, 14, 23, 59, 59),
                taken(2019, 6, 15, 0, 0, 0),
                "2019-06-14",
                "2019-06-15",
            ),
            (
                taken(2019, 6, 30, 23, 59, 59),
                taken(2019, 7, 1, 0, 0, 0),
                "2019-06",
                "2019-07",
            ),
            (
                taken(2019, 12, 31, 23, 59, 59),
                taken(2020, 1, 1, 0, 0, 0),
                "2019",
                "2020",
            ),
        ];
        for (last, first, period, next) in edges {
            assert!(
                dated(&format!("to:{period}"), &last),
                "to:{period} holds its last second"
            );
            assert!(
                !dated(&format!("to:{period}"), &first),
                "to:{period} stops before {next}"
            );
            assert!(
                dated(&format!("from:{next}"), &first),
                "from:{next} holds its first second"
            );
            assert!(
                !dated(&format!("from:{next}"), &last),
                "from:{next} starts after {period}"
            );
        }
    }

    #[test]
    fn from_and_to_together_make_a_range() {
        let q = "from:2019-06 to:2019-08";
        assert!(dated(q, &taken(2019, 6, 1, 0, 0, 0)));
        assert!(dated(q, &taken(2019, 8, 31, 23, 59, 59)));
        assert!(!dated(q, &taken(2019, 5, 31, 23, 59, 59)));
        assert!(!dated(q, &taken(2019, 9, 1, 0, 0, 0)));
        // February's end moves with leap years.
        assert!(dated("to:2020-02", &taken(2020, 2, 29, 12, 0, 0)));
        assert!(!dated("to:2019-02", &taken(2019, 3, 1, 0, 0, 0)));
        assert!(
            dated("FROM:2019 TO:2019", &taken(2019, 3, 1, 0, 0, 0)),
            "any case"
        );
    }

    #[test]
    fn a_date_term_narrows_and_widens_like_any_other() {
        let photo = Fields {
            any: &["lake.jpg"],
            taken: Some(crate::metadata::naive_to_unix(2019, 6, 15, 12, 0, 0)),
            ..Fields::default()
        };
        assert!(dated("lake from:2019", &photo));
        assert!(!dated("lake from:2020", &photo));
        assert!(!dated("pond from:2019", &photo));
        assert!(dated("pond OR from:2019", &photo));
        assert!(dated("from:2020 OR lake", &photo));
    }

    #[test]
    fn a_photo_without_a_date_matches_no_date_term() {
        let photo = Fields {
            any: &["2019.jpg"],
            ..Fields::default()
        };
        assert!(!dated("from:2019", &photo));
        assert!(!dated("to:2019", &photo));
    }

    #[test]
    fn a_value_that_is_not_a_date_yet_is_ignored() {
        // What a date looks like halfway through being typed, and dates that do not exist,
        // search for nothing rather than for the literal text or an empty grid.
        for q in [
            "lake from:",
            "lake from:20",
            "lake from:2019-",
            "lake from:2019-1",
            "lake from:2019-13",
            "lake from:2019-00",
            "lake to:2019-02-30",
            "lake to:2019-06-1",
            "lake to:2019-06-15x",
            "lake to:2019-06-15-01",
            "lake to:2019-06-00",
            "lake from:abcd",
            "lake from:\"2019 06\"",
        ] {
            assert_eq!(Query::parse(q), Query::parse("lake"), "{q}");
        }
        assert!(Query::parse("from:2019-1").is_empty());
    }

    #[test]
    fn video_and_photo_filter_on_kind_and_quotes_make_them_words() {
        let clip = Fields {
            any: &["clip.mp4", "Trips"],
            kind: Some(MediaKind::Video),
            ..Fields::default()
        };
        let shot = Fields {
            any: &["video night.jpg"],
            kind: Some(MediaKind::Image),
            ..Fields::default()
        };
        assert!(Query::parse("video").matches(&clip));
        assert!(
            !Query::parse("video").matches(&shot),
            "a file named 'video' is not a video"
        );
        assert!(
            Query::parse("\"video\"").matches(&shot),
            "quoted, it is the word"
        );
        assert!(Query::parse("photo").matches(&shot));
        assert!(!Query::parse("photo").matches(&clip));
        assert!(Query::parse("video trips").matches(&clip));
    }

    #[test]
    fn a_date_prefix_is_literal_inside_quotes() {
        assert!(names("\"from:2019\"", &["from:2019.jpg"]));
        assert!(!dated("\"from:2019\"", &taken(2019, 6, 15, 12, 0, 0)));
    }
}
