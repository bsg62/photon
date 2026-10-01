//! Matching a typed search query against a photo's names.
//!
//! The whole of what "search matches this photo" means lives here, rather than inside the
//! row closure in `library::items`, so that the parts that are easy to get wrong - case
//! folding outside ASCII, wildcard characters, an empty query - are testable without
//! seeding a database and counting rows.

use crate::media::MediaKind;
use crate::metadata::Gps;

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
/// - `tag:`, `person:`, `album:` and `folder:` restrict a term to the photo's keywords, the
///   people Picasa named on it, the albums it is in, and its folder's name or alias, read
///   the way `camera:` reads its value. A photo with none of the thing never matches.
/// - `is:starred`, `is:edited`, `is:video` and `is:photo` ask about the photo rather than
///   its text. An `is:` photon does not know is dropped like a prefix with no value: `is:st`
///   is `is:starred` half typed.
/// - `has:gps` keeps the photos that record where they were taken, and `near:LAT,LON` those
///   taken within a kilometre of that point, in decimal degrees - or within another distance,
///   `near:48.137,11.575,25km`. The info panel's "Photos nearby" link writes one.
/// - A leading `-` turns a term round: `-tag:family`, `-is:starred`, `-lake`. A negated
///   value of several words (`-camera:"canon eos"`) excludes the photos the positive form
///   finds, so it is "not all of these words", not "none of them". A query of nothing but
///   negated terms is a query: `-is:starred` is every photo without a star. Quoted, the
///   hyphen is text (`"-1"`), and a lone `-` is dangling.
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
    Tag(String),
    Person(String),
    Album(String),
    Folder(String),
    /// `is:starred`.
    Starred,
    /// `is:edited`: turned or cropped in photon.
    Edited,
    /// `has:gps`.
    HasGps,
    /// `near:`: within `radius_m` metres of a point. Held in whole units - the point in
    /// 1e-7 degrees, about a centimetre - because a term is compared for equality, which a
    /// float is not.
    Near {
        lat_e7: i64,
        lon_e7: i64,
        radius_m: i64,
    },
    /// A `-` term: true unless every term inside matches. A list because one token can be
    /// several terms (`camera:"canon eos"`), and the negation is of the token.
    Not(Vec<Term>),
}

/// The side tables a query's terms read, so a search loads only the ones it was asked about:
/// most queries name neither a person nor an album.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    pub people: bool,
    pub albums: bool,
}

impl Term {
    fn matches(&self, haystacks: &Haystacks) -> bool {
        match self {
            Term::Any(needle) => haystacks.any_contains(needle),
            Term::Camera(needle) => haystacks.camera_contains(needle),
            Term::Lens(needle) => haystacks.lens_contains(needle),
            Term::From(start) => haystacks.taken.is_some_and(|t| t >= *start),
            Term::To(end) => haystacks.taken.is_some_and(|t| t < *end),
            Term::Kind(kind) => haystacks.kind == Some(*kind),
            Term::Tag(needle) => haystacks.tags.contains(needle),
            Term::Person(needle) => haystacks.people.contains(needle),
            Term::Album(needle) => haystacks.albums.contains(needle),
            Term::Folder(needle) => haystacks.folder.contains(needle),
            Term::Starred => haystacks.starred,
            Term::Edited => haystacks.edited,
            Term::HasGps => haystacks.gps.is_some(),
            Term::Near {
                lat_e7,
                lon_e7,
                radius_m,
            } => haystacks.gps.is_some_and(|gps| {
                let centre = Gps {
                    lat: *lat_e7 as f64 / 1e7,
                    lon: *lon_e7 as f64 / 1e7,
                };
                gps.distance_km(centre) * 1000.0 <= *radius_m as f64
            }),
            Term::Not(terms) => !terms.iter().all(|term| term.matches(haystacks)),
        }
    }
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
    /// The photo's keywords, joined by spaces, for `tag:`.
    pub tags: Option<&'a str>,
    /// The names of the people on the photo, joined by spaces, for `person:`.
    pub people: Option<&'a str>,
    /// The names of the albums the photo is in, joined by spaces, for `album:`.
    pub albums: Option<&'a str>,
    /// The folder's name and alias, joined by a space, for `folder:`.
    pub folder: Option<&'a str>,
    pub starred: bool,
    pub edited: bool,
    /// Where the photo was taken, for `has:gps` and `near:`.
    pub gps: Option<Gps>,
}

/// Appends `text` lowercased to `out`: exactly what `str::to_lowercase` would give, without
/// its allocation when `text` is ASCII, which almost every file name, camera and lens is.
///
/// Only a wholly ASCII string takes the byte-wise path. Anything else goes through
/// `to_lowercase` whole, because its answer for a letter depends on the letters around it:
/// a capital sigma lowers to `ς` at the end of a word and to `σ` elsewhere, so folding a
/// string in pieces - or the ASCII runs of it apart from the rest - would change the answer.
pub fn fold_into(out: &mut String, text: &str) {
    if text.is_ascii() {
        let start = out.len();
        out.push_str(text);
        out[start..].make_ascii_lowercase();
    } else {
        out.push_str(&text.to_lowercase());
    }
}

/// What a photo offers the matcher, lowercased once and held in buffers a search reuses from
/// photo to photo, so a search over the whole library allocates per photo only for text
/// outside ASCII. [`Fields`] is the same thing for a caller holding plain strings;
/// [`Query::matches`] builds one of these from it.
///
/// **The `any` haystacks share one buffer, separated by `\0`,** so an unprefixed term is one
/// `contains` over the photo rather than one per haystack. A needle must still be found
/// within a single haystack, never across two: a match that does not contain the separator
/// cannot span one, and a needle that does contain it (a query can hold any character) is
/// looked for in each haystack in turn, by the recorded ends. A haystack that itself holds a
/// `\0` needs nothing extra: it only splits into pieces a separator-free needle cannot span
/// either.
///
/// Haystacks can be dropped from the end back to a [`Haystacks::mark`], which is how a search
/// folds a folder's name and alias once for all its photos: they go in first, and each photo
/// truncates back to them.
#[derive(Debug, Default)]
pub struct Haystacks {
    any: String,
    /// Where each haystack in `any` ends, its separator excluded.
    ends: Vec<usize>,
    /// The `camera:` field, meaningful only while `has_camera`: a flag beside the text
    /// rather than an `Option`, so a photo without a camera does not free the buffer the
    /// next photo's camera is written into.
    camera: String,
    has_camera: bool,
    lens: String,
    has_lens: bool,
    /// The capture time, as [`Fields::taken`].
    pub taken: Option<i64>,
    /// What the file is, as [`Fields::kind`].
    pub kind: Option<MediaKind>,
    tags: Field,
    people: Field,
    albums: Field,
    /// The folder's, so it outlives [`Haystacks::truncate`] as the folder's haystacks do.
    folder: Field,
    pub starred: bool,
    pub edited: bool,
    /// Where the photo was taken, as [`Fields::gps`].
    pub gps: Option<Gps>,
    scratch: String,
}

/// The text a prefixed term is confined to, lowercased. A flag beside the text for the
/// reason on `Haystacks::camera`.
#[derive(Debug, Default)]
struct Field {
    text: String,
    present: bool,
}

impl Field {
    fn clear(&mut self) {
        self.present = false;
        self.text.clear();
    }

    /// Sets the field from text that is lowercase already.
    fn set_folded(&mut self, folded: Option<&str>) {
        self.clear();
        if let Some(folded) = folded {
            self.present = true;
            self.text.push_str(folded);
        }
    }

    fn set(&mut self, text: Option<&str>) {
        self.clear();
        if let Some(text) = text {
            self.present = true;
            fold_into(&mut self.text, text);
        }
    }

    fn contains(&self, needle: &str) -> bool {
        self.present && self.text.contains(needle)
    }
}

/// A point in a [`Haystacks`] to truncate back to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mark {
    len: usize,
    count: usize,
}

const SEPARATOR: char = '\0';

impl Haystacks {
    /// The position after the last haystack pushed.
    pub fn mark(&self) -> Mark {
        Mark {
            len: self.any.len(),
            count: self.ends.len(),
        }
    }

    /// Drops every haystack pushed after `mark`, and everything else that belongs to one
    /// photo: the camera, lens, date, kind, keywords, people, albums, star and edit. The
    /// `folder:` field stays, as the folder's haystacks before the mark do.
    pub fn truncate(&mut self, mark: Mark) {
        self.any.truncate(mark.len);
        self.ends.truncate(mark.count);
        self.has_camera = false;
        self.has_lens = false;
        self.taken = None;
        self.kind = None;
        self.tags.clear();
        self.people.clear();
        self.albums.clear();
        self.starred = false;
        self.edited = false;
        self.gps = None;
    }

    /// Empties everything.
    pub fn clear(&mut self) {
        self.truncate(Mark::default());
        self.folder.clear();
    }

    /// Sets the field `tag:` terms are confined to, from keywords lowercased already and
    /// joined by spaces. Not a haystack of `any`: the caller pushes the keywords there too.
    pub fn set_tags_folded(&mut self, tags: Option<&str>) {
        self.tags.set_folded(tags);
    }

    /// Sets the field `person:` terms are confined to, lowercased already. People are in
    /// no haystack of `any`: an unprefixed word does not find them.
    pub fn set_people_folded(&mut self, people: Option<&str>) {
        self.people.set_folded(people);
    }

    /// Sets the field `album:` terms are confined to, lowercased already; in no haystack of
    /// `any` either.
    pub fn set_albums_folded(&mut self, albums: Option<&str>) {
        self.albums.set_folded(albums);
    }

    /// Sets the field `folder:` terms are confined to: the folder's name and its alias, so
    /// the term finds the folder by either, as an unprefixed word does.
    pub fn set_folder(&mut self, name: &str, alias: Option<&str>) {
        self.folder.set(Some(name));
        if let Some(alias) = alias {
            self.folder.text.push(' ');
            fold_into(&mut self.folder.text, alias);
        }
    }

    /// Adds a haystack, lowercasing it.
    pub fn push(&mut self, text: &str) {
        self.push_with(|out| fold_into(out, text));
    }

    /// Adds a haystack `write` appends to the buffer, which it must write already lowercased:
    /// for text lowercase by construction (`50mm`, `iso400`, a date), which would otherwise
    /// be formatted into a string of its own only to be lowercased into another.
    pub fn push_with(&mut self, write: impl FnOnce(&mut String)) {
        if !self.ends.is_empty() {
            self.any.push(SEPARATOR);
        }
        write(&mut self.any);
        self.ends.push(self.any.len());
    }

    /// Adds a caption: lowercased, with runs of whitespace collapsed to one space. A caption
    /// keeps its line breaks in storage, for the info panel to show whole; a quoted phrase
    /// must still match across one. Collapsed first and lowercased whole, as one string,
    /// for the reason on [`fold_into`].
    pub fn push_caption(&mut self, caption: &str) {
        let mut collapsed = std::mem::take(&mut self.scratch);
        collapsed.clear();
        for (n, word) in caption.split_whitespace().enumerate() {
            if n > 0 {
                collapsed.push(' ');
            }
            collapsed.push_str(word);
        }
        self.push(&collapsed);
        self.scratch = collapsed;
    }

    /// Sets the field `camera:` terms are confined to: make and model, lowercased, as one
    /// string with a space between, so a term can match either or a word of each. `None` for
    /// both leaves the photo with no camera, which no camera term matches. It is not a
    /// haystack of `any`: the caller pushes make and model there separately.
    pub fn set_camera(&mut self, make: Option<&str>, model: Option<&str>) {
        self.has_camera = make.is_some() || model.is_some();
        self.camera.clear();
        if self.has_camera {
            // Folded apart and joined, where `Query::matches` folds `Fields::camera` joined:
            // the same answer, because a space is neither a letter nor ignorable to the
            // sigma rule, so neither side of it can change how the other lowercases.
            fold_into(&mut self.camera, make.unwrap_or(""));
            self.camera.push(' ');
            fold_into(&mut self.camera, model.unwrap_or(""));
        }
    }

    /// Sets the field `lens:` terms are confined to. Not a haystack of `any` either.
    pub fn set_lens(&mut self, lens: Option<&str>) {
        self.has_lens = lens.is_some();
        self.lens.clear();
        if let Some(lens) = lens {
            fold_into(&mut self.lens, lens);
        }
    }

    /// Whether some haystack of `any` contains `needle`.
    fn any_contains(&self, needle: &str) -> bool {
        if !needle.contains(SEPARATOR) {
            return self.any.contains(needle);
        }
        let mut start = 0;
        self.ends.iter().any(|&end| {
            let found = self.any[start..end].contains(needle);
            start = end + SEPARATOR.len_utf8();
            found
        })
    }

    fn camera_contains(&self, needle: &str) -> bool {
        self.has_camera && self.camera.contains(needle)
    }

    fn lens_contains(&self, needle: &str) -> bool {
        self.has_lens && self.lens.contains(needle)
    }
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

/// How far `near:` reaches when the query names no distance.
const NEAR_DEFAULT_M: i64 = 1_000;

/// The term a `near:` value names: `LAT,LON` in decimal degrees, and optionally a distance
/// as `,5km` or `,500m`. `None` for anything else, which drops the term like a date that is
/// not one: `near:48.1,` is a point half typed.
fn near(value: &str) -> Option<Term> {
    let mut parts = value.split(',');
    let mut degrees = |limit: f64| {
        let part = parts.next()?;
        // `parse` reads "nan", "inf" and "1e5" too; a coordinate is digits, a sign and a point.
        let plain = part
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-');
        let value: f64 = plain.then(|| part.parse().ok()).flatten()?;
        (value.abs() <= limit).then_some((value * 1e7).round() as i64)
    };
    let lat_e7 = degrees(90.0)?;
    let lon_e7 = degrees(180.0)?;
    let radius_m = match parts.next() {
        None => NEAR_DEFAULT_M,
        Some(distance) => {
            let (number, unit_m) = match distance.strip_suffix("km") {
                Some(number) => (number, 1000.0),
                None => (distance.strip_suffix('m')?, 1.0),
            };
            let plain = number.bytes().all(|b| b.is_ascii_digit() || b == b'.');
            let number: f64 = plain.then(|| number.parse().ok()).flatten()?;
            // Anything past half the Earth's circumference is everywhere.
            let metres = (number * unit_m).round();
            (metres > 0.0 && metres <= 20_100_000.0).then_some(metres as i64)?
        }
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Term::Near {
        lat_e7,
        lon_e7,
        radius_m,
    })
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
            // A leading hyphen outside quotes negates the token. Lowercasing an ASCII hyphen
            // keeps its length, so the offsets recorded against the raw text still apply.
            let negated = text.starts_with('-') && token.unquoted_prefix.is_none_or(|at| at >= 1);
            let (text, unquoted_prefix) = if negated {
                (&text[1..], token.unquoted_prefix.map(|at| at - 1))
            } else {
                (text.as_str(), token.unquoted_prefix)
            };
            let terms = if text.is_empty() && unquoted_prefix.is_none() {
                // A lone `-`: a negation with nothing typed after it yet.
                Vec::new()
            } else {
                Self::terms(text, unquoted_prefix)
            };
            let terms = if negated && !terms.is_empty() {
                vec![Term::Not(terms)]
            } else {
                terms
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

    /// The terms one token stands for, its text lowercased and any negating hyphen removed.
    /// `unquoted_prefix` is [`Token::unquoted_prefix`] against that text. None at all for a
    /// token that is dangling: a prefix with no value, a date that is not one, an `is:`
    /// photon does not know.
    fn terms(text: &str, unquoted_prefix: Option<usize>) -> Vec<Term> {
        // `video` and `photo` filter on what the file is. Unquoted only: `"video"` is
        // still the word, for a folder called Videos.
        if unquoted_prefix.is_none() {
            match text {
                "video" => return vec![Term::Kind(MediaKind::Video)],
                "photo" => return vec![Term::Kind(MediaKind::Image)],
                _ => {}
            }
        }
        let prefixed = |prefix: &str| {
            // Lowercasing never changes the length of these ASCII prefixes, so the
            // offset recorded against the raw text still applies.
            let outside_quotes = unquoted_prefix.is_none_or(|at| at >= prefix.len());
            (outside_quotes && text.starts_with(prefix)).then(|| &text[prefix.len()..])
        };
        let words = |value: &str, term: fn(String) -> Term| -> Vec<Term> {
            value
                .split_whitespace()
                .map(|w| term(w.to_string()))
                .collect()
        };
        if let Some(value) = prefixed("camera:") {
            words(value, Term::Camera)
        } else if let Some(value) = prefixed("lens:") {
            words(value, Term::Lens)
        } else if let Some(value) = prefixed("tag:") {
            words(value, Term::Tag)
        } else if let Some(value) = prefixed("person:") {
            words(value, Term::Person)
        } else if let Some(value) = prefixed("album:") {
            words(value, Term::Album)
        } else if let Some(value) = prefixed("folder:") {
            words(value, Term::Folder)
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
        } else if let Some(value) = prefixed("is:") {
            match value {
                "starred" => vec![Term::Starred],
                "edited" => vec![Term::Edited],
                "video" => vec![Term::Kind(MediaKind::Video)],
                "photo" => vec![Term::Kind(MediaKind::Image)],
                _ => Vec::new(),
            }
        } else if let Some(value) = prefixed("has:") {
            match value {
                "gps" => vec![Term::HasGps],
                _ => Vec::new(),
            }
        } else if let Some(value) = prefixed("near:") {
            near(value).into_iter().collect()
        } else {
            vec![Term::Any(text.to_string())]
        }
    }

    /// Which side tables the query's terms read.
    pub fn needs(&self) -> Needs {
        fn visit(terms: &[Term], needs: &mut Needs) {
            for term in terms {
                match term {
                    Term::Person(_) => needs.people = true,
                    Term::Album(_) => needs.albums = true,
                    Term::Not(inner) => visit(inner, needs),
                    _ => {}
                }
            }
        }
        let mut needs = Needs::default();
        for terms in &self.alternatives {
            visit(terms, &mut needs);
        }
        needs
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
    /// Folds `fields` into a [`Haystacks`] and asks [`Query::matches_folded`], so every test
    /// written against this entry point runs the matcher a library search runs.
    pub fn matches(&self, fields: &Fields<'_>) -> bool {
        let mut haystacks = Haystacks::default();
        for text in fields.any {
            haystacks.push(text);
        }
        // `Fields::camera` is make and model already joined, so it is folded whole.
        haystacks.has_camera = fields.camera.is_some();
        fold_into(&mut haystacks.camera, fields.camera.unwrap_or(""));
        haystacks.set_lens(fields.lens);
        haystacks.taken = fields.taken;
        haystacks.kind = fields.kind;
        haystacks.tags.set(fields.tags);
        haystacks.people.set(fields.people);
        haystacks.albums.set(fields.albums);
        haystacks.folder.set(fields.folder);
        haystacks.starred = fields.starred;
        haystacks.edited = fields.edited;
        haystacks.gps = fields.gps;
        self.matches_folded(&haystacks)
    }

    /// Whether some alternative has every one of its terms found in `haystacks`, which are
    /// lowercased already. They are folded once, up front, because with AND every term of
    /// an alternative is tried and most photos fail on the first: folding lazily per term
    /// would redo the same work for each.
    pub fn matches_folded(&self, haystacks: &Haystacks) -> bool {
        self.alternatives
            .iter()
            .any(|terms| terms.iter().all(|term| term.matches(haystacks)))
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
            ..Fields::default()
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
            ..Fields::default()
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

    /// A photo with something in every confined field, each a word no other field has.
    fn rich() -> Fields<'static> {
        Fields {
            any: &["a.jpg", "zoo family"],
            tags: Some("Zoo family"),
            people: Some("Anna Schmidt Peter Braun"),
            albums: Some("Best of 2019"),
            folder: Some("2019 Italy Holiday"),
            starred: true,
            edited: true,
            kind: Some(MediaKind::Image),
            ..Fields::default()
        }
    }

    #[test]
    fn tag_person_album_and_folder_terms_are_confined_to_their_fields() {
        let photo = rich();
        for hit in [
            "tag:zoo",
            "TAG:Family",
            "person:anna",
            "person:\"anna schmidt\"",
            "album:best",
            "folder:italy",
            "folder:holiday",
        ] {
            assert!(Query::parse(hit).matches(&photo), "{hit}");
        }
        // Each value sits in another field, so only confinement makes these miss.
        for miss in [
            "tag:anna",
            "tag:italy",
            "person:zoo",
            "person:best",
            "album:italy",
            "album:zoo",
            "folder:best",
            "folder:anna",
        ] {
            assert!(!Query::parse(miss).matches(&photo), "{miss}");
        }
        // People and albums are in no unprefixed haystack.
        assert!(!Query::parse("anna").matches(&photo));
        // A photo with none of the thing never matches, an empty needle aside.
        let bare = Fields {
            any: &["zoo.jpg"],
            ..Fields::default()
        };
        for miss in ["tag:zoo", "person:zoo", "album:zoo", "folder:zoo"] {
            assert!(!Query::parse(miss).matches(&bare), "{miss}");
        }
    }

    #[test]
    fn is_terms_ask_about_the_photo() {
        let photo = rich();
        let plain = Fields {
            any: &["starred edited.jpg"],
            kind: Some(MediaKind::Video),
            ..Fields::default()
        };
        for q in ["is:starred", "is:edited", "is:photo", "IS:Starred"] {
            assert!(Query::parse(q).matches(&photo), "{q}");
            assert!(!Query::parse(q).matches(&plain), "{q}");
        }
        assert!(Query::parse("is:video").matches(&plain));
        assert_eq!(Query::parse("is:video"), Query::parse("video"));
        // Half typed, or unknown: dropped, not searched for.
        assert_eq!(Query::parse("lake is:st"), Query::parse("lake"));
        assert!(Query::parse("is:").is_empty());
        // Quoted, it is text.
        assert!(names("\"is:starred\"", &["is:starred.jpg"]));
    }

    #[test]
    fn a_leading_hyphen_negates_a_term() {
        let photo = rich();
        let plain = Fields {
            any: &["b.jpg"],
            kind: Some(MediaKind::Image),
            ..Fields::default()
        };
        for q in ["-is:starred", "-tag:zoo", "-person:anna", "-zoo", "-video"] {
            let negative = Query::parse(q);
            assert!(!negative.is_empty(), "{q} is a query on its own");
            let positive = Query::parse(&q[1..]);
            for fields in [&photo, &plain] {
                assert_eq!(negative.matches(fields), !positive.matches(fields), "{q}");
            }
        }
        // It narrows like any other term, and binds to its own token only.
        assert!(Query::parse("zoo -is:video").matches(&photo));
        assert!(!Query::parse("zoo -is:starred").matches(&photo));
        assert!(Query::parse("-is:starred OR zoo").matches(&photo));
    }

    #[test]
    fn a_negated_value_of_several_words_excludes_what_the_positive_finds() {
        // "anna" is there and "zzz" is not: the positive form misses, so the negative hits.
        // Negating each word on its own would exclude the photo for its "anna".
        let photo = rich();
        assert!(Query::parse("-person:\"anna zzz\"").matches(&photo));
        assert!(!Query::parse("-person:\"anna schmidt\"").matches(&photo));
    }

    #[test]
    fn a_hyphen_is_text_inside_quotes_and_dangling_alone() {
        assert!(names("\"-1\"", &["img-1.jpg"]));
        assert!(!names("\"-1\"", &["img1.jpg"]));
        assert_eq!(Query::parse("lake -"), Query::parse("lake"));
        assert_eq!(Query::parse("lake -tag:"), Query::parse("lake"));
        assert!(Query::parse("-").is_empty());
        // A hyphen inside a word is the word.
        assert!(names("img-1", &["img-1.jpg"]));
        // A quoted value after the hyphen is still negated: the hyphen is outside.
        assert!(!names("-\"img 1\"", &["img 1.jpg"]));
    }

    #[test]
    fn has_gps_and_near_ask_where_a_photo_was_taken() {
        let marienplatz = Fields {
            gps: Some(Gps {
                lat: 48.137_4,
                lon: 11.575_5,
            }),
            ..Fields::default()
        };
        let nowhere = Fields {
            any: &["gps near.jpg"],
            ..Fields::default()
        };
        let m = |q: &str, f: &Fields<'_>| Query::parse(q).matches(f);
        assert!(m("has:gps", &marienplatz));
        assert!(!m("has:gps", &nowhere));
        assert!(m("-has:gps", &nowhere));
        // A kilometre unless told otherwise. Odeonsplatz is about 500 m north, Nymphenburg
        // about 9 km west.
        assert!(m("near:48.1420,11.5775", &marienplatz));
        assert!(!m("near:48.1583,11.5033", &marienplatz));
        assert!(m("near:48.1583,11.5033,10km", &marienplatz));
        assert!(!m("near:48.1420,11.5775,300m", &marienplatz));
        assert!(m("near:48.1420,11.5775,0.6km", &marienplatz));
        // A photo with no position is near nothing.
        assert!(!m("near:48.1420,11.5775,20000km", &nowhere));
        // The southern and western halves.
        let sydney = Fields {
            gps: Some(Gps {
                lat: -33.868_8,
                lon: 151.209_3,
            }),
            ..Fields::default()
        };
        assert!(m("near:-33.8688,151.2093", &sydney));
        assert!(!m("near:33.8688,151.2093", &sydney));
    }

    #[test]
    fn a_near_that_names_no_point_is_dropped() {
        for half_typed in [
            "near:",
            "near:48",
            "near:48.1,",
            "near:48.1,11.5,",
            "near:48.1,11.5,5",
            "near:48.1,11.5,km",
            "near:48.1,11.5,0km",
            "near:48.1,11.5,5km,1",
            "near:91,11.5",
            "near:48.1,181",
            "near:nan,11.5",
            "near:1e1,11.5",
            "has:",
            "has:gp",
        ] {
            assert_eq!(
                Query::parse(&format!("lake {half_typed}")),
                Query::parse("lake"),
                "{half_typed}"
            );
        }
        assert_eq!(
            Query::parse("near:48.1,11.5"),
            Query::parse("near:48.1,11.5,1km")
        );
        assert_eq!(
            Query::parse("near:48.1,11.5,1km"),
            Query::parse("near:48.1,11.5,1000m")
        );
    }

    #[test]
    fn needs_names_the_side_tables_a_query_reads() {
        assert_eq!(Query::parse("lake tag:zoo").needs(), Needs::default());
        assert_eq!(
            Query::parse("lake OR person:anna").needs(),
            Needs {
                people: true,
                albums: false
            }
        );
        assert_eq!(
            Query::parse("-album:best").needs(),
            Needs {
                people: false,
                albums: true
            }
        );
    }

    #[test]
    fn truncating_keeps_the_folder_field_and_drops_the_photos() {
        let mut haystacks = Haystacks::default();
        haystacks.set_folder("Italy", Some("Holiday"));
        let mark = haystacks.mark();
        haystacks.set_tags_folded(Some("zoo"));
        haystacks.set_people_folded(Some("anna"));
        haystacks.set_albums_folded(Some("best"));
        haystacks.starred = true;
        haystacks.edited = true;
        haystacks.gps = Some(Gps { lat: 1.0, lon: 1.0 });
        haystacks.truncate(mark);
        for gone in [
            "has:gps",
            "tag:zoo",
            "person:anna",
            "album:best",
            "is:starred",
            "is:edited",
        ] {
            assert!(!Query::parse(gone).matches_folded(&haystacks), "{gone}");
        }
        assert!(Query::parse("folder:italy").matches_folded(&haystacks));
        assert!(Query::parse("folder:holiday").matches_folded(&haystacks));
        haystacks.clear();
        assert!(!Query::parse("folder:italy").matches_folded(&haystacks));
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
    fn folding_agrees_with_to_lowercase_on_ascii_and_everything_else() {
        // `fold_into` takes a byte-wise path for ASCII only. Each non-ASCII case here is
        // one that path, or folding in pieces, gets wrong: the capitals it cannot lower,
        // a letter that lowers to two chars, and the sigma whose lowercase depends on
        // whether a letter follows it - which is why a mixed string is folded whole.
        for text in [
            "IMG_0001.JPG",
            "Canon EOS 5D Mark IV",
            "50% OFF_[x]",
            "",
            "MÜNCHEN 2019",
            "Straße ÅLESUND",
            "İSTANBUL",
            "ΟΔΟΣ ΣΑΣ",
            "ΣΑΣ.jpg",
            "Kraków ZAKOPANE",
        ] {
            let mut folded = String::from("kept ");
            fold_into(&mut folded, text);
            assert_eq!(folded, format!("kept {}", text.to_lowercase()), "{text:?}");
        }
    }

    #[test]
    fn a_needle_is_found_within_one_haystack_never_across_two() {
        // The haystacks share one buffer, separated by a NUL. A query can hold a NUL too,
        // and such a needle must still be looked for haystack by haystack, not in the
        // joined buffer, where it would find the seam between two of them.
        let photo = &["lake.jpg", "Trips"];
        assert!(!names("jpg\0trips", photo), "spans two haystacks");
        assert!(names("a\0b", &["x.jpg", "a\0b"]), "within one haystack");
        assert!(!names("a\0b", &["a", "b"]), "spans two haystacks");
        // A haystack holding a NUL of its own changes nothing for a needle without one.
        assert!(names("b.jpg", &["a\0b.jpg"]));
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
