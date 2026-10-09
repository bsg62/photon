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
//! bidirectional algorithm finds, and inside a right-to-left run its words and the spaces
//! between them, each piece in the place it is read at. A word in one script has one
//! direction and, nearly always, one face.

use eframe::egui::{self, Color32, FontId, Pos2, TextWrapMode, WidgetText, pos2};
use unicode_bidi::BidiInfo;

/// A piece of a line that is laid out by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub text: &'a str,
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
            // Read from the right: the last word of the run is its leftmost piece.
            let words = words_and_spaces(text);
            pieces.extend(words.into_iter().rev().map(|text| Run { text, rtl }));
        } else {
            pieces.push(Run { text, rtl });
        }
    }
    pieces
}

/// `text` cut where whitespace begins and ends: "ab  cd " is "ab", "  ", "cd", " ".
fn words_and_spaces(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut space = None;
    for (at, letter) in text.char_indices() {
        let is_space = letter.is_whitespace();
        if space.is_some_and(|was| was != is_space) {
            pieces.push(&text[start..at]);
            start = at;
        }
        space = Some(is_space);
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
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
        let galley = WidgetText::from(run.text).into_galley(
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

    fn runs(text: &str) -> Vec<(&str, bool)> {
        visual_runs(text)
            .into_iter()
            .map(|run| (run.text, run.rtl))
            .collect()
    }

    #[test]
    fn left_to_right_text_is_one_piece() {
        assert_eq!(runs("2026-07 Coast"), [("2026-07 Coast", false)]);
        assert!(runs("").is_empty());
    }

    // Read from the right, so the first word is the rightmost piece. Words and not the
    // whole run, because epaint sets two words down left to right when the space between
    // them is drawn by another face.
    #[test]
    fn right_to_left_text_is_its_words_from_the_last_to_the_first() {
        assert_eq!(
            runs("שלום עולם"),
            [("עולם", true), (" ", true), ("שלום", true)]
        );
        assert_eq!(
            runs("رحلة الصيف"),
            [("الصيف", true), (" ", true), ("رحلة", true)]
        );
    }

    // The line starts right-to-left, so it is read from the right: the Latin word, last in
    // the text, is drawn first from the left, and its letters keep their order.
    #[test]
    fn a_latin_word_after_hebrew_is_a_piece_of_its_own_at_the_left() {
        assert_eq!(
            runs("שלום עולם abc"),
            [
                ("abc", false),
                (" ", true),
                ("עולם", true),
                (" ", true),
                ("שלום", true)
            ]
        );
    }

    // A year inside an Arabic name reads left to right, as digits do in any script.
    #[test]
    fn digits_inside_right_to_left_text_keep_their_order() {
        assert_eq!(
            runs("رحلة 2024 الصيف"),
            [
                ("الصيف", true),
                (" ", true),
                ("2024", false),
                (" ", true),
                ("رحلة", true)
            ]
        );
        assert_eq!(runs("אב 12"), [("12", false), (" ", true), ("אב", true)]);
    }

    // A path is left-to-right with a right-to-left folder name in it.
    #[test]
    fn a_right_to_left_name_in_a_path_is_a_piece_in_its_place() {
        assert_eq!(
            runs("/home/Pictures/שלום/a.jpg"),
            [
                ("/home/Pictures/", false),
                ("שלום", true),
                ("/a.jpg", false)
            ]
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
                    run.text.to_owned(),
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
    }

    #[test]
    fn text_is_cut_where_whitespace_begins_and_ends() {
        assert_eq!(words_and_spaces("ab  cd "), ["ab", "  ", "cd", " "]);
        assert_eq!(words_and_spaces(" x"), [" ", "x"]);
        assert_eq!(words_and_spaces("x"), ["x"]);
        assert!(words_and_spaces("").is_empty());
    }
}
