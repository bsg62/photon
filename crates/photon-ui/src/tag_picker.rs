//! The dialog that writes one keyword to a whole selection: which way it writes, what is
//! typed, which of the library's keywords that narrows to, and what is said afterwards.
//! `createTagPicker` in Rust, with the words of `TagPicker.svelte`.
//!
//! The photos are taken when the dialog opens and not read when it writes: the dialog
//! has the keyboard, and the selection behind it can change while it is up - a scan
//! taking a photo away - so what the title said it would act on is what it acts on.
//!
//! There is no `busy` here, where the Svelte picker has one against a second Enter
//! landing before the close has rendered: this dialog is closed by the application in
//! the frame it answers, and a closed dialog reads no key.

use crate::grid::labels::grouped;

/// Which way the dialog writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Add,
    Remove,
}

/// A keyword to write, and the photos to write it to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    pub mode: Mode,
    /// Trimmed. The library may store another name for it - one the user has renamed
    /// it to - and answers with the name stored.
    pub tag: String,
    pub ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagPicker {
    mode: Mode,
    /// The photos the write will reach, as they were selected when the dialog opened.
    ids: Vec<i64>,
    draft: String,
}

pub const NO_KEYWORDS: &str = "No keywords in the library yet.";
pub const NO_MATCH: &str = "No keyword matches what you typed.";
/// What the list of keywords says it is, to whoever reads the window without seeing it.
pub const LIST: &str = "Existing keywords";

/// "1 photo", or "12 photos": a count that a title or a message names.
fn counted(count: usize) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

impl Mode {
    /// The dialog's title: which way it writes, and to how many photos. The selection
    /// is behind the dialog and cannot be counted by eye once it has the keyboard.
    pub fn title(self, count: usize) -> String {
        let to = match self {
            Mode::Add => "Add a keyword to",
            Mode::Remove => "Remove a keyword from",
        };
        format!("{to} {}", counted(count))
    }

    /// What the empty field says it is for.
    pub fn placeholder(self) -> &'static str {
        match self {
            Mode::Add => "New or existing keyword",
            Mode::Remove => "Keyword to remove",
        }
    }

    /// The field's name, for whoever reads the window without seeing it.
    pub fn field(self) -> &'static str {
        match self {
            Mode::Add => "Keyword to add",
            Mode::Remove => "Keyword to remove",
        }
    }

    /// The button that writes what is typed.
    pub fn button(self) -> &'static str {
        match self {
            Mode::Add => "Add",
            Mode::Remove => "Remove",
        }
    }

    /// What is said once the keyword is written: the name the library stored, which a
    /// rename rule can change, and how many photos it reached. The library's count is
    /// the truth - a photo purged since the dialog opened is passed over, not refused -
    /// and the difference is said: swallowed, the user would believe twelve photos carry
    /// a keyword that ten of them do.
    pub fn written(self, stored: &str, done: usize, asked: usize) -> String {
        let (verb, preposition) = match self {
            Mode::Add => ("Added", "to"),
            Mode::Remove => ("Removed", "from"),
        };
        let reached = if done == asked {
            counted(done)
        } else {
            format!("{} of {}", grouped(done), counted(asked))
        };
        format!("{verb} “{stored}” {preposition} {reached}")
    }
}

impl TagPicker {
    /// The dialog as it opens: for `ids`, the photos selected now, with nothing typed.
    pub fn new(mode: Mode, ids: Vec<i64>) -> Self {
        Self {
            mode,
            ids,
            draft: String::new(),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// How many photos the write will reach.
    pub fn count(&self) -> usize {
        self.ids.len()
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn type_in(&mut self, text: String) {
        self.draft = text;
    }

    /// The known keywords the draft narrows to, best first: the name itself, whatever
    /// its case, then those that begin with it, then the rest that contain it - each in
    /// the library's own order. The first row is what Enter writes when the library has
    /// the name (`submit`), or the two disagree. With nothing typed, all of them.
    ///
    /// Without regard to case, and in Rust, as every other match in photon: there is no
    /// `COLLATE NOCASE` in the library, and SQLite's `lower()` knows ASCII alone.
    pub fn suggestions<'a>(&self, all: &'a [String]) -> Vec<&'a str> {
        let typed = self.draft.trim().to_lowercase();
        if typed.is_empty() {
            return all.iter().map(String::as_str).collect();
        }
        let rank = |tag: &str| {
            let name = tag.to_lowercase();
            if name == typed {
                Some(0)
            } else if name.starts_with(&typed) {
                Some(1)
            } else if name.contains(&typed) {
                Some(2)
            } else {
                None
            }
        };
        let mut matching: Vec<(u8, &str)> = (all.iter())
            .filter_map(|tag| Some((rank(tag)?, tag.as_str())))
            .collect();
        // A stable sort: within a rank the library's order stands.
        matching.sort_by_key(|(rank, _)| *rank);
        matching.into_iter().map(|(_, tag)| tag).collect()
    }

    /// What to write: `tag` - a row of the list that was clicked - or what is typed.
    /// None when there is nothing to write: a blank name, or no photos.
    ///
    /// **What is typed is written as the library has it**, when it has the name under
    /// another case: "lisbon" typed where the library has "Lisbon" is the first row of
    /// the list, and Enter writes that row's keyword. Written as typed - which is what
    /// the Svelte picker does - Enter made a second keyword beside the first, the one
    /// thing the list is there to prevent, and removed nothing where the row would have
    /// removed it. A name the library has exactly is written exactly, so two keywords
    /// that differ only by case can each still be reached.
    pub fn submit(&self, tag: Option<&str>, known: &[String]) -> Option<Submit> {
        let name = tag.unwrap_or(&self.draft).trim();
        if name.is_empty() || self.ids.is_empty() {
            return None;
        }
        let folded = name.to_lowercase();
        let had = (known.iter().find(|tag| *tag == name))
            .or_else(|| known.iter().find(|tag| tag.to_lowercase() == folded));
        Some(Submit {
            mode: self.mode,
            tag: had.map_or(name, String::as_str).to_owned(),
            ids: self.ids.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|tag| (*tag).to_owned()).collect()
    }

    fn typed(mode: Mode, ids: &[i64], draft: &str) -> TagPicker {
        let mut picker = TagPicker::new(mode, ids.to_vec());
        picker.type_in(draft.to_owned());
        picker
    }

    // tag-picker.svelte.test.ts, case for case.

    #[test]
    fn the_known_keywords_are_narrowed_without_regard_to_case() {
        let picker = typed(Mode::Add, &[1, 2], "BE");
        let known = all(&["Beach", "sunset", "beer", "Umbrella"]);
        assert_eq!(picker.suggestions(&known), ["Beach", "beer"]);
        // Beyond ASCII too, which SQLite's own `lower()` would not manage.
        let picker = typed(Mode::Add, &[1], "ärz");
        let known = all(&["Berge", "Die Ärzte", "ÄRZTIN"]);
        assert_eq!(picker.suggestions(&known), ["ÄRZTIN", "Die Ärzte"]);
    }

    // What was typed is what was meant: a name that merely contains it and happens to
    // come earlier in the library must not stand above it, or Enter and the first row
    // disagree.
    #[test]
    fn an_exact_match_is_first_then_the_names_that_begin_with_what_was_typed() {
        let picker = typed(Mode::Add, &[1], "sun");
        let known = all(&["sunset", "risingsun", "sun", "sunny", "shotgun"]);
        assert_eq!(
            picker.suggestions(&known),
            ["sun", "sunset", "sunny", "risingsun"]
        );
        // What is typed is trimmed before it is matched.
        let picker = typed(Mode::Add, &[1], "  sun ");
        assert_eq!(picker.suggestions(&known)[0], "sun");
    }

    #[test]
    fn every_keyword_is_shown_while_nothing_is_typed() {
        let known = all(&["beach", "sun"]);
        assert_eq!(
            TagPicker::new(Mode::Add, vec![1]).suggestions(&known),
            ["beach", "sun"]
        );
        assert_eq!(
            typed(Mode::Add, &[1], "   ").suggestions(&known),
            ["beach", "sun"]
        );
    }

    #[test]
    fn a_blank_name_writes_nothing() {
        assert_eq!(typed(Mode::Add, &[1, 2], "   ").submit(None, &[]), None);
        assert_eq!(
            TagPicker::new(Mode::Remove, vec![1]).submit(None, &[]),
            None
        );
        assert_eq!(
            typed(Mode::Add, &[1, 2], "beach").submit(Some("  "), &[]),
            None
        );
    }

    #[test]
    fn nothing_is_written_when_no_photo_is_selected() {
        assert_eq!(typed(Mode::Add, &[], "beach").submit(None, &[]), None);
    }

    // The first row of the list is the library's keyword of that name, whatever its
    // case, and Enter writes what the first row would: not a second keyword beside it.
    #[test]
    fn what_is_typed_is_written_as_the_library_has_it_under_another_case() {
        let known = all(&["Lisbon", "sunset", "Ärzte"]);
        for (mode, draft, written) in [
            (Mode::Add, "lisbon", "Lisbon"),
            (Mode::Remove, " LISBON ", "Lisbon"),
            (Mode::Add, "ärzte", "Ärzte"),
            // A name the library does not have is written as it is typed.
            (Mode::Add, "Porto", "Porto"),
            (Mode::Add, "lis", "lis"),
        ] {
            let picker = typed(mode, &[1], draft);
            assert_eq!(picker.submit(None, &known).unwrap().tag, written, "{draft}");
            if written != draft.trim() {
                assert_eq!(picker.suggestions(&known)[0], written, "{draft}");
            }
        }
        // Two that differ only by case: each is reached by typing it exactly, and
        // another spelling writes the one the library lists first.
        let both = all(&["Beach", "beach"]);
        assert_eq!(
            typed(Mode::Add, &[1], "beach")
                .submit(None, &both)
                .unwrap()
                .tag,
            "beach"
        );
        assert_eq!(
            typed(Mode::Add, &[1], "BEACH")
                .submit(None, &both)
                .unwrap()
                .tag,
            "Beach"
        );
    }

    #[test]
    fn what_is_typed_is_written_trimmed_to_the_photos_the_dialog_opened_for() {
        let picker = typed(Mode::Remove, &[4, 9], "  beach ");
        assert_eq!(picker.count(), 2);
        assert_eq!(
            picker.submit(None, &[]),
            Some(Submit {
                mode: Mode::Remove,
                tag: "beach".to_owned(),
                ids: vec![4, 9],
            })
        );
    }

    #[test]
    fn a_keyword_handed_over_is_written_and_not_the_draft() {
        let picker = typed(Mode::Add, &[7], "be");
        assert_eq!(
            picker.submit(Some("Beach"), &all(&["beach", "Beach"])),
            Some(Submit {
                mode: Mode::Add,
                tag: "Beach".to_owned(),
                ids: vec![7],
            })
        );
    }

    // The message it reports.

    #[test]
    fn the_message_names_the_keyword_stored_which_a_rename_rule_can_change() {
        assert_eq!(
            Mode::Add.written("vacation", 12, 12),
            "Added “vacation” to 12 photos"
        );
        assert_eq!(
            Mode::Add.written("beach", 1_200, 1_200),
            "Added “beach” to 1,200 photos"
        );
    }

    #[test]
    fn it_says_from_when_removing_and_counts_one_photo_singly() {
        assert_eq!(
            Mode::Remove.written("beach", 1, 1),
            "Removed “beach” from 1 photo"
        );
    }

    // A selection can outlive its photos, so the library's count is the truth, and the
    // difference is worth saying.
    #[test]
    fn it_says_how_many_of_the_selection_it_reached_when_some_had_gone() {
        assert_eq!(
            Mode::Add.written("beach", 10, 12),
            "Added “beach” to 10 of 12 photos"
        );
        assert_eq!(
            Mode::Remove.written("beach", 0, 1),
            "Removed “beach” from 0 of 1 photo"
        );
    }

    #[test]
    fn the_title_says_which_way_it_writes_and_to_how_many_photos() {
        assert_eq!(Mode::Add.title(1), "Add a keyword to 1 photo");
        assert_eq!(Mode::Add.title(12), "Add a keyword to 12 photos");
        assert_eq!(
            Mode::Remove.title(3_000),
            "Remove a keyword from 3,000 photos"
        );
    }

    // The words are the Svelte dialog's: read out of the two files that hold them.
    #[test]
    fn the_words_are_the_svelte_dialogs() {
        let dialog = include_str!("../../../ui/src/components/TagPicker.svelte");
        for words in [
            "'Add a keyword to' : 'Remove a keyword from'",
            "'New or existing keyword' : 'Keyword to remove'",
            "'Keyword to add' : 'Keyword to remove'",
            "'Add' : 'Remove'",
            NO_KEYWORDS,
            NO_MATCH,
            LIST,
            "aria-label=\"Cancel\"",
        ] {
            assert!(
                dialog.lines().any(|line| line.contains(words)),
                "TagPicker.svelte no longer says {words:?}"
            );
        }
        for mode in [Mode::Add, Mode::Remove] {
            for words in [mode.placeholder(), mode.field(), mode.button()] {
                assert!(dialog.contains(&format!("'{words}'")), "{words:?}");
            }
        }
        let state = include_str!("../../../ui/src/lib/tag-picker.svelte.ts");
        for words in [
            "mode === 'add' ? 'Added' : 'Removed'",
            "mode === 'add' ? 'to' : 'from'",
            "`${verb} “${done.tag}” ${preposition} ${reached}`",
            "of ${counted(asked)}",
        ] {
            assert!(
                state.lines().any(|line| line.contains(words)),
                "tag-picker.svelte.ts no longer says {words:?}"
            );
        }
    }
}
