//! One line of text in any mix of scripts.
//!
//! epaint 0.36 shapes text - Arabic joins, and a stretch of one direction is laid out in
//! it - but it does not run the Unicode bidirectional algorithm. It cuts a line into
//! stretches of one font face each, shapes every stretch in the direction of its first
//! strong letter, and sets the stretches down left to right in the order of the text.
//! Measured 2026-10-09 by reading glyph positions out of its galleys:
//!
//! - "אב 12" came out as "21 בא": the digits were shaped inside the right-to-left stretch
//!   and reversed with it.
//! - "رحلة الصيف" came out with its first word on the left: the space is drawn by another
//!   face than the Arabic letters, so the two words were two stretches, set down left to
//!   right.
//! - "שלום עולם abc" came out with "cba".
//!
//! So a line is cut here first, into pieces epaint is right about: the runs the
//! bidirectional algorithm finds, and inside a right-to-left run its words and whatever
//! stands between them, each piece in the place it is read at. A word in one script has
//! one direction and, nearly always, one face.
//!
//! Cut at everything that is not a letter, not only at spaces: a hyphen or a bracket is
//! drawn by another face than the letters as readily as a space is. A review found that
//! by shaping the pieces with real faces (Noto Sans beside Noto Sans Hebrew): cut at
//! spaces alone, "שלום-עולם" still had its first word on the left. The tests here lay
//! text out with egui's own fonts, which have no Hebrew and so draw everything with one
//! face; what they hold is the cutting, not what a second face does to an uncut run.
//!
//! What this does not do: cut a line short at its beginning when it is right-to-left
//! (`paint_line` cuts at the right, which is such a name's start), or anything for a text
//! field, whose caret and typing in mixed-direction text are egui's own.

use eframe::egui::{self, Color32, FontId, Pos2, TextWrapMode, WidgetText, pos2};
use std::borrow::Cow;
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};

/// A piece of a line that is laid out by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    /// The piece as it is handed to egui. Borrowed from the line, but for what stands
    /// between two right-to-left words, which is turned round and mirrored first.
    pub text: Cow<'a, str>,
    pub rtl: bool,
}

/// The pieces of `text`, left to right as they are drawn. Only the first paragraph: a name
/// or a caption on one line has no other.
pub fn visual_runs(text: &str) -> Vec<Run<'_>> {
    let info = BidiInfo::new(text, None);
    let Some(paragraph) = info.paragraphs.first() else {
        return Vec::new();
    };
    let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
    let mut pieces = Vec::new();
    for run in runs.into_iter().filter(|run| !run.is_empty()) {
        let rtl = levels[run.start].is_rtl();
        let text = &text[run];
        if rtl {
            // Read from the right: the last piece of the run is its leftmost.
            for (piece, word) in words_and_between(text).into_iter().rev() {
                let text = if word {
                    Cow::Borrowed(piece)
                } else {
                    turned_round(piece)
                };
                pieces.push(Run { text, rtl });
            }
        } else {
            pieces.push(Run {
                text: Cow::Borrowed(text),
                rtl,
            });
        }
    }
    pieces
}

/// Whether `letter` is part of a right-to-left word: a letter of such a script, a mark on
/// one (a Hebrew vowel point, an Arabic harakah), or a joiner inside one, which Persian
/// writes between two letters of one word. Cut at a mark or a joiner, the word would be
/// shaped in halves and its letters would not join.
fn in_word(letter: char) -> bool {
    matches!(
        bidi_class(letter),
        BidiClass::R | BidiClass::AL | BidiClass::NSM | BidiClass::BN
    )
}

/// A right-to-left run cut into its words and what stands between them, in the order of
/// the text, each with whether it is a word: "אב - גד" is "אב", " - ", "גד".
fn words_and_between(text: &str) -> Vec<(&str, bool)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut word = None;
    for (at, letter) in text.char_indices() {
        let is_word = in_word(letter);
        if let Some(was) = word
            && was != is_word
        {
            pieces.push((&text[start..at], was));
            start = at;
        }
        word = Some(is_word);
    }
    if let Some(word) = word {
        pieces.push((&text[start..], word));
    }
    pieces
}

/// What stands between two right-to-left words, as it is drawn: read from the right like
/// the rest of the run, so its characters in reverse, and a bracket facing the other way
/// (the one that closes in the text is the leftmost drawn, where it has to open). egui
/// lays the piece out left to right as it is given, mirroring nothing.
fn turned_round(between: &str) -> Cow<'_, str> {
    let turned: String = between.chars().rev().map(mirrored).collect();
    if turned == between {
        Cow::Borrowed(between)
    } else {
        Cow::Owned(turned)
    }
}

/// The bracket that faces the other way; any other character as it is.
fn mirrored(letter: char) -> char {
    const PAIRS: [(char, char); 6] = [
        ('(', ')'),
        ('[', ']'),
        ('{', '}'),
        ('<', '>'),
        ('«', '»'),
        ('‹', '›'),
    ];
    PAIRS
        .iter()
        .find_map(|&(open, close)| match letter {
            l if l == open => Some(close),
            l if l == close => Some(open),
            _ => None,
        })
        .unwrap_or(letter)
}

/// Paints `text` on one line from `left`, centred on `middle`, in at most `room`, cut
/// short where the room ends. Answers the width it took.
pub fn paint_line(
    ui: &egui::Ui,
    painter: &egui::Painter,
    (left, middle): (f32, f32),
    room: f32,
    text: &str,
    font: FontId,
    tint: Color32,
) -> f32 {
    let mut used = 0.0;
    for run in visual_runs(text) {
        let left_over = room - used;
        if left_over <= 0.0 {
            break;
        }
        let galley = WidgetText::from(run.text.as_ref()).into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            left_over,
            font.clone(),
        );
        let size = galley.size();
        let at: Pos2 = pos2(left + used, middle - size.y / 2.0);
        painter.galley(at, galley, tint);
        used += size.x;
    }
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str) -> Vec<(String, bool)> {
        visual_runs(text)
            .into_iter()
            .map(|run| (run.text.to_string(), run.rtl))
            .collect()
    }

    fn pieces(expected: &[(&str, bool)]) -> Vec<(String, bool)> {
        expected
            .iter()
            .map(|(text, rtl)| ((*text).to_owned(), *rtl))
            .collect()
    }

    #[test]
    fn left_to_right_text_is_one_piece() {
        assert_eq!(runs("2026-07 Coast"), pieces(&[("2026-07 Coast", false)]));
        assert!(runs("").is_empty());
    }

    // Read from the right, so the first word is the rightmost piece. Words and not the
    // whole run, because epaint sets two words down left to right when what stands between
    // them is drawn by another face.
    #[test]
    fn right_to_left_text_is_its_words_from_the_last_to_the_first() {
        assert_eq!(
            runs("שלום עולם"),
            pieces(&[("עולם", true), (" ", true), ("שלום", true)])
        );
        assert_eq!(
            runs("رحلة الصيف"),
            pieces(&[("الصيف", true), (" ", true), ("رحلة", true)])
        );
    }

    // A hyphen, an underscore or a mark is drawn by another face than the letters as
    // readily as a space is, and a folder named "קיץ-חורף" is as common as one with a
    // space. Found in review, by shaping each piece with Noto's faces: cut at spaces only,
    // the two words of "שלום-עולם" came out with the first on the left.
    #[test]
    fn a_right_to_left_run_is_cut_at_everything_that_is_not_a_letter() {
        assert_eq!(
            runs("שלום-עולם"),
            pieces(&[("עולם", true), ("-", true), ("שלום", true)])
        );
        assert_eq!(
            runs("رحلة_الصيف"),
            pieces(&[("الصيف", true), ("_", true), ("رحلة", true)])
        );
        assert_eq!(runs("תמונות!"), pieces(&[("!", true), ("תמונות", true)]));
    }

    // Several marks in a row are read from the right like everything else in the run, and
    // a piece is laid out left to right, so its characters are turned round here.
    #[test]
    fn the_marks_between_two_words_are_turned_round() {
        assert_eq!(
            runs("אב!? גד"),
            pieces(&[("גד", true), (" ?!", true), ("אב", true)])
        );
    }

    // "(2)" after a Hebrew word is drawn "(2)", to the word's left: the bracket that
    // closes in the text is the leftmost thing drawn, and it has to open there.
    #[test]
    fn brackets_in_right_to_left_text_are_mirrored() {
        assert_eq!(
            runs("קיץ (2)"),
            pieces(&[("(", true), ("2", false), (") ", true), ("קיץ", true)])
        );
        assert_eq!(
            runs("קיץ [א]"),
            pieces(&[("[", true), ("א", true), ("] ", true), ("קיץ", true)])
        );
    }

    // A vowel point belongs to its letter and a joiner to its word: cut there, a pointed
    // Hebrew word would be drawn in pieces and a Persian one would lose its joining.
    #[test]
    fn marks_and_joiners_stay_inside_their_word() {
        let pointed = "שָׁלוֹם";
        assert_eq!(runs(pointed), pieces(&[(pointed, true)]));
        let persian = "می\u{200c}خواهم";
        assert_eq!(runs(persian), pieces(&[(persian, true)]));
    }

    // The line starts right-to-left, so it is read from the right: the Latin word, last in
    // the text, is drawn first from the left, and its letters keep their order.
    #[test]
    fn a_latin_word_after_hebrew_is_a_piece_of_its_own_at_the_left() {
        assert_eq!(
            runs("שלום עולם abc"),
            pieces(&[
                ("abc", false),
                (" ", true),
                ("עולם", true),
                (" ", true),
                ("שלום", true)
            ])
        );
    }

    // A year inside an Arabic name reads left to right, as digits do in any script.
    #[test]
    fn digits_inside_right_to_left_text_keep_their_order() {
        assert_eq!(
            runs("رحلة 2024 الصيف"),
            pieces(&[
                ("الصيف", true),
                (" ", true),
                ("2024", false),
                (" ", true),
                ("رحلة", true)
            ])
        );
        assert_eq!(
            runs("אב 12"),
            pieces(&[("12", false), (" ", true), ("אב", true)])
        );
    }

    // A path is left-to-right with a right-to-left folder name in it.
    #[test]
    fn a_right_to_left_name_in_a_path_is_a_piece_in_its_place() {
        assert_eq!(
            runs("/home/Pictures/שלום/a.jpg"),
            pieces(&[
                ("/home/Pictures/", false),
                ("שלום", true),
                ("/a.jpg", false)
            ])
        );
    }

    /// The letters of `text` from left to right as `paint_line` sets them down: each piece
    /// laid out by itself, the pieces side by side. With egui's own fonts, which every
    /// machine has, since they are compiled in: a letter they lack is still shaped in its
    /// script's direction.
    fn drawn(text: &str) -> String {
        let ctx = egui::Context::default();
        let mut drawn = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for run in visual_runs(text) {
                let galley = ui.painter().layout_no_wrap(
                    run.text.to_string(),
                    FontId::proportional(15.0),
                    Color32::WHITE,
                );
                let mut glyphs: Vec<(f32, char)> = galley
                    .rows
                    .iter()
                    .flat_map(|row| row.glyphs.iter())
                    // A cluster of several letters has one glyph that is drawn and
                    // zero-width ones standing for the rest.
                    .filter(|glyph| glyph.advance_width > 0.0)
                    .map(|glyph| (glyph.pos.x, glyph.chr))
                    .collect();
                glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
                drawn.extend(glyphs.into_iter().map(|(_, letter)| letter));
            }
        });
        output.textures_delta.clear();
        drawn
    }

    // The three lines that were drawn wrong, as a reader now sees them from the left. A
    // right-to-left word reads from its right end, so its letters appear here reversed.
    #[test]
    fn a_line_is_set_down_in_the_order_it_is_read() {
        assert_eq!(drawn("ab cd"), "ab cd");
        assert_eq!(drawn("אב 12"), "12 בא");
        assert_eq!(drawn("رحلة الصيف"), "فيصلا ةلحر");
        assert_eq!(drawn("שלום עולם abc"), "abc םלוע םולש");
        assert_eq!(drawn("/p/שלום/a.jpg"), "/p/םולש/a.jpg");
        assert_eq!(drawn("קיץ (2)"), "(2) ץיק");
        assert_eq!(drawn("שלום-עולם"), "םלוע-םולש");
    }
}
