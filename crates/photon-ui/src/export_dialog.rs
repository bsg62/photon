//! The export dialog: which photos, where to, whether edits are rendered into the copies
//! and how large they may be, and what is said when the export is over.
//! `createExportDialog` in Rust, with the words of `ExportDialog.svelte`.
//!
//! The photos are taken when the dialog opens: it has the keyboard, and a scan landing
//! while it is open can change what the grid has selected.
//!
//! Where the Svelte dialog asks the system for a folder, this one has a field: the path
//! is typed, or a folder is dropped on the window, until the picker of a later
//! sub-project. A path is asked about as it changes, off the UI thread, and the answer
//! counts only for the text it was asked for: the next letter is another question.
//!
//! There is no `busy` here: the dialog is closed in the frame it answers, and a closed
//! dialog reads no second Enter. One export at a time is the application's to keep.

use crate::grid::labels::{grouped, photo_count};
use std::path::{Path, PathBuf};

/// The sizes the dialog offers: the longest edge a copy may have, in pixels, or none for
/// the photo's own size. Screens' widths, since a copy made smaller is one to look at or
/// to send.
pub const SIZES: [(Option<u32>, &str); 5] = [
    (None, "Original size"),
    (Some(3840), "3840 px"),
    (Some(2560), "2560 px"),
    (Some(1920), "1920 px"),
    (Some(1280), "1280 px"),
];

pub const APPLY_EDITS: &str = "Apply edits to the copies";
pub const LONGEST_EDGE: &str = "Longest edge";
/// Always shown, because it is the one surprise in the feature: photon reads camera
/// information and never writes it, so a re-encoded copy cannot carry it.
pub const NOTE: &str = "A photo you have turned or cropped, or one made smaller, is \
re-encoded, and the copy carries no camera information. Every other photo is copied \
exactly as it is: one already within the size, and every video and GIF.";
pub const EXPORT: &str = "Export";
pub const CANCEL: &str = "Cancel";
/// The field's name, for whoever reads the window without seeing it.
pub const FIELD: &str = "Folder to export to";
pub const PLACEHOLDER: &str = "Path of a folder";
/// The picker's button, which waits for the picker.
pub const CHOOSE: &str = "Choose…";
/// What is said under the field of a path that is not one to ask the engine about.
pub const NOT_WHOLE: &str = "Type the whole path of a folder";
pub const NO_FOLDER: &str = "There is no folder at this path";

/// What is known of the destination the field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dest {
    /// Nothing is typed.
    Empty,
    /// The path has been asked about, and the answer is on its way.
    Checking,
    /// Copies may be written there: the folder, with a `~` written out.
    Passed(String),
    /// They may not, and why.
    Refused(String),
}

/// An export to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub ids: Vec<i64>,
    pub dest: String,
    pub apply_edits: bool,
    pub max_edge: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportDialog {
    /// The photos the export will write, as they were selected when the dialog opened.
    ids: Vec<i64>,
    /// What the field holds.
    path: String,
    dest: Dest,
    /// What the last answer said against a path: kept while the next path is asked
    /// about, so that the line under the field does not come and go at every letter.
    said: Option<String>,
    apply_edits: bool,
    /// Whether the checkbox has been ticked or unticked since the dialog opened: the
    /// stored choice, read a moment later, does not undo what the user has just said.
    ticked: bool,
    max_edge: Option<u32>,
}

impl ExportDialog {
    /// The dialog as it opens, for `ids`: nothing typed, edits applied until the stored
    /// choice is read (which is also what a failed read leaves), and the photo's own
    /// size - never the size of the export before: left on, it would quietly shrink the
    /// next one, and that one might be the archive.
    pub fn new(ids: Vec<i64>) -> Self {
        Self {
            ids,
            path: String::new(),
            dest: Dest::Empty,
            said: None,
            apply_edits: true,
            ticked: false,
            max_edge: None,
        }
    }

    /// How many photos the export will write.
    pub fn count(&self) -> usize {
        self.ids.len()
    }

    /// The dialog's title: the selection is behind the dialog and cannot be counted by
    /// eye once it has the keyboard.
    pub fn title(&self) -> String {
        format!("Export {}", photo_count(self.ids.len()))
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn dest(&self) -> &Dest {
        &self.dest
    }

    /// The field holds `text` now. Answers the text to ask about, which is none for an
    /// empty field: of that nothing is said.
    pub fn type_in(&mut self, text: String) -> Option<String> {
        self.path = text;
        if self.path.trim().is_empty() {
            self.dest = Dest::Empty;
            self.said = None;
            return None;
        }
        self.dest = Dest::Checking;
        Some(self.path.clone())
    }

    /// The answer about `asked`: the folder copies may be written into, or why not. It
    /// counts only for the text the field still holds - an answer about a path since
    /// typed over would pass, or refuse, a folder nobody is asking about.
    pub fn checked(&mut self, asked: &str, answer: Result<String, String>) {
        if asked != self.path || self.dest != Dest::Checking {
            return;
        }
        self.said = answer.as_ref().err().cloned();
        self.dest = match answer {
            Ok(folder) => Dest::Passed(folder),
            Err(why) => Dest::Refused(why),
        };
    }

    /// What is said under the field: why the path in it will not do - or, while the
    /// path is being asked about, why the last one would not. A path is asked about at
    /// every letter and answered a moment later: with the line taken away at each
    /// letter and put back at each answer it came and went as a path was typed, and
    /// the dialog, which stands in the middle of the window, moved up and down with it.
    pub fn problem(&self) -> Option<&str> {
        self.said.as_deref()
    }

    pub fn apply_edits(&self) -> bool {
        self.apply_edits
    }

    /// The stored choice has been read. Not over a tick made meanwhile.
    pub fn remembered(&mut self, apply: bool) {
        if !self.ticked {
            self.apply_edits = apply;
        }
    }

    /// The checkbox ticked or unticked: stored by the application as it is made rather
    /// than when the export runs - an export the user cancels still said how they want
    /// exports to work.
    pub fn tick(&mut self, apply: bool) {
        self.apply_edits = apply;
        self.ticked = true;
    }

    /// The library would not store the tick: the box goes back to what is stored, so
    /// that the two cannot disagree.
    pub fn put_back(&mut self, was: bool) {
        self.apply_edits = was;
    }

    pub fn max_edge(&self) -> Option<u32> {
        self.max_edge
    }

    pub fn size(&mut self, max_edge: Option<u32>) {
        self.max_edge = max_edge.filter(|edge| *edge > 0);
    }

    /// The export to run: none until a destination has passed, and none of no photos.
    pub fn job(&self) -> Option<Job> {
        let Dest::Passed(dest) = &self.dest else {
            return None;
        };
        (!self.ids.is_empty()).then(|| Job {
            ids: self.ids.clone(),
            dest: dest.clone(),
            apply_edits: self.apply_edits,
            max_edge: self.max_edge,
        })
    }
}

/// What the field's text is as a path to ask about: none for an empty field, and why
/// not for a text that is no whole path.
///
/// A `~` at its start is the home folder, as a shell has it: a typed path without it is
/// a long way round on the systems that have one. A path that is not absolute is
/// refused: it would be read from wherever photon happens to have been started.
pub fn typed_path(text: &str, home: Option<&Path>) -> Option<Result<PathBuf, &'static str>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let below_home = (text.strip_prefix('~'))
        .filter(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
        .map(|rest| rest.trim_start_matches(['/', '\\']));
    let path = match (below_home, home) {
        (Some(""), Some(home)) => home.to_owned(),
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    };
    Some(if path.is_absolute() {
        Ok(path)
    } else {
        Err(NOT_WHOLE)
    })
}

/// What is said when an export is over: how many photos landed and where, and when some
/// did not, how many of those asked for and the first reason. Nothing written at all is
/// an error, and the reason is the whole of what the user gets.
pub fn report(
    written: usize,
    failed: usize,
    reason: Option<&str>,
    asked: usize,
    dest: &str,
) -> Result<String, String> {
    if written == 0 {
        return Err(reason.map_or_else(
            || format!("Nothing could be exported to {dest}"),
            str::to_owned,
        ));
    }
    let landed = if failed == 0 {
        photo_count(written)
    } else {
        format!("{} of {}", grouped(written), photo_count(asked))
    };
    let why = match reason {
        Some(reason) if failed > 0 => format!(" — {reason}"),
        _ => String::new(),
    };
    Ok(format!("Exported {landed} to {dest}{why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole path on the system the test runs on, and the home folder beside it.
    fn whole() -> &'static str {
        if cfg!(windows) {
            "C:\\exports"
        } else {
            "/exports"
        }
    }

    fn home() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            "C:\\Users\\ada"
        } else {
            "/home/ada"
        })
    }

    /// A dialog whose destination has passed.
    fn passed(ids: &[i64]) -> ExportDialog {
        let mut dialog = ExportDialog::new(ids.to_vec());
        let asked = dialog.type_in(whole().to_owned()).unwrap();
        dialog.checked(&asked, Ok(whole().to_owned()));
        dialog
    }

    // export-dialog.svelte.test.ts, as far as its cases are this module's.

    #[test]
    fn it_opens_with_edits_applied_the_photos_own_size_and_the_count_it_will_write() {
        let dialog = ExportDialog::new(vec![1, 2, 3]);
        assert_eq!(dialog.count(), 3);
        assert_eq!(dialog.title(), "Export 3 photos");
        assert_eq!(ExportDialog::new(vec![7]).title(), "Export 1 photo");
        assert_eq!(
            ExportDialog::new((0..1200).collect()).title(),
            "Export 1,200 photos"
        );
        assert!(dialog.apply_edits());
        assert_eq!(dialog.max_edge(), None);
        assert_eq!(dialog.path(), "");
        assert_eq!(dialog.dest(), &Dest::Empty);
    }

    #[test]
    fn nothing_is_written_until_a_destination_has_passed() {
        let mut dialog = ExportDialog::new(vec![1, 2]);
        assert_eq!(dialog.job(), None);
        let asked = dialog.type_in(whole().to_owned()).unwrap();
        assert_eq!(asked, whole());
        assert_eq!(dialog.dest(), &Dest::Checking);
        assert_eq!(dialog.job(), None, "not while it is being asked about");
        dialog.checked(&asked, Err("no".to_owned()));
        assert_eq!(dialog.dest(), &Dest::Refused("no".to_owned()));
        assert_eq!(dialog.job(), None);
    }

    #[test]
    fn it_exports_what_it_was_opened_with_to_the_folder_that_passed() {
        let dialog = passed(&[4, 9]);
        assert_eq!(
            dialog.job(),
            Some(Job {
                ids: vec![4, 9],
                dest: whole().to_owned(),
                apply_edits: true,
                max_edge: None,
            })
        );
        // And nothing of no photos.
        assert_eq!(passed(&[]).job(), None);
    }

    #[test]
    fn it_exports_at_the_size_chosen_and_the_photos_own_size_is_no_size() {
        let mut dialog = passed(&[1]);
        dialog.size(Some(1920));
        assert_eq!(dialog.max_edge(), Some(1920));
        assert_eq!(dialog.job().unwrap().max_edge, Some(1920));
        dialog.size(None);
        assert_eq!(dialog.job().unwrap().max_edge, None);
        // A select's "original" is a nought in the Svelte dialog: no size either.
        dialog.size(Some(0));
        assert_eq!(dialog.max_edge(), None);
    }

    // A size left on from last time would quietly shrink the next export.
    #[test]
    fn the_size_is_forgotten_between_exports() {
        let mut dialog = passed(&[1]);
        dialog.size(Some(1280));
        assert_eq!(ExportDialog::new(vec![1]).max_edge(), None);
        // The sizes offered are the Svelte dialog's, the photo's own first.
        assert_eq!(SIZES[0], (None, "Original size"));
        assert_eq!(
            SIZES.map(|(edge, _)| edge.unwrap_or(0)),
            [0, 3840, 2560, 1920, 1280]
        );
    }

    #[test]
    fn a_refused_folder_is_said_against_the_field_and_cleared_by_one_that_passes() {
        let mut dialog = ExportDialog::new(vec![1]);
        let asked = dialog.type_in(whole().to_owned()).unwrap();
        dialog.checked(&asked, Err("inside a watched folder".to_owned()));
        assert_eq!(
            dialog.dest(),
            &Dest::Refused("inside a watched folder".to_owned())
        );
        // The next letter is another question: the refusal goes with the text it was of.
        let other = format!("{}2", whole());
        let asked = dialog.type_in(other.clone()).unwrap();
        assert_eq!(dialog.dest(), &Dest::Checking);
        dialog.checked(&asked, Ok(other.clone()));
        assert_eq!(dialog.dest(), &Dest::Passed(other));
    }

    // What is said under the field is of the last path that was answered about, until
    // the next answer: the line must not come and go at every letter.
    #[test]
    fn what_was_said_of_a_path_stands_until_the_next_answer() {
        let mut dialog = ExportDialog::new(vec![1]);
        assert_eq!(dialog.problem(), None);
        let asked = dialog.type_in("/nowhere".to_owned()).unwrap();
        assert_eq!(dialog.problem(), None, "nothing is known yet");
        dialog.checked(&asked, Err(NO_FOLDER.to_owned()));
        assert_eq!(dialog.problem(), Some(NO_FOLDER));
        // The next letter: asked about again, and the line stands meanwhile.
        let asked = dialog.type_in("/nowhere/else".to_owned()).unwrap();
        assert_eq!(dialog.dest(), &Dest::Checking);
        assert_eq!(dialog.problem(), Some(NO_FOLDER));
        assert_eq!(dialog.job(), None);
        dialog.checked(&asked, Err("another reason".to_owned()));
        assert_eq!(dialog.problem(), Some("another reason"));
        // A path that passes has nothing said of it, and neither has an empty field.
        let asked = dialog.type_in(whole().to_owned()).unwrap();
        dialog.checked(&asked, Ok(whole().to_owned()));
        assert_eq!(dialog.problem(), None);
        let asked = dialog.type_in("/nowhere".to_owned()).unwrap();
        dialog.checked(&asked, Err(NO_FOLDER.to_owned()));
        dialog.type_in(String::new());
        assert_eq!(dialog.problem(), None);
        // Nor is what was said of the path before the empty field said of the next.
        dialog.type_in("/n".to_owned());
        assert_eq!(dialog.problem(), None);
    }

    // The answer is from another thread, a moment behind the typing.
    #[test]
    fn an_answer_counts_only_for_the_text_the_field_still_holds() {
        let mut dialog = ExportDialog::new(vec![1]);
        let first = dialog.type_in(whole().to_owned()).unwrap();
        let second = dialog.type_in(format!("{}/inside", whole())).unwrap();
        // The first path's answer arrives after the second was typed: it passes nothing.
        dialog.checked(&first, Ok(first.clone()));
        assert_eq!(dialog.dest(), &Dest::Checking);
        assert_eq!(dialog.job(), None);
        dialog.checked(&second, Err("no".to_owned()));
        assert_eq!(dialog.dest(), &Dest::Refused("no".to_owned()));
        // An answer that comes twice changes nothing the second time.
        dialog.checked(&second, Ok(second.clone()));
        assert_eq!(dialog.dest(), &Dest::Refused("no".to_owned()));
        // Emptied, nothing is asked and nothing is said, whatever arrives.
        assert_eq!(dialog.type_in("  ".to_owned()), None);
        assert_eq!(dialog.dest(), &Dest::Empty);
        dialog.checked("  ", Ok("x".to_owned()));
        assert_eq!(dialog.dest(), &Dest::Empty);
    }

    #[test]
    fn the_stored_choice_is_taken_when_it_is_read_but_not_over_a_tick_made_meanwhile() {
        let mut dialog = passed(&[1]);
        dialog.remembered(false);
        assert!(!dialog.apply_edits());
        assert!(!dialog.job().unwrap().apply_edits);

        let mut dialog = ExportDialog::new(vec![1]);
        dialog.tick(false);
        dialog.remembered(true);
        assert!(!dialog.apply_edits(), "the read undid the tick");
    }

    #[test]
    fn the_checkbox_goes_back_when_the_setting_cannot_be_stored() {
        let mut dialog = passed(&[1]);
        dialog.tick(false);
        assert!(!dialog.job().unwrap().apply_edits);
        dialog.put_back(true);
        assert!(dialog.apply_edits());
        assert!(dialog.job().unwrap().apply_edits);
    }

    // The path typed.

    #[test]
    fn an_empty_field_is_no_path_and_no_problem() {
        assert_eq!(typed_path("", Some(&home())), None);
        assert_eq!(typed_path("   ", Some(&home())), None);
    }

    #[test]
    fn a_whole_path_is_asked_about_without_the_spaces_at_its_ends() {
        assert_eq!(
            typed_path(&format!("  {} ", whole()), Some(&home())),
            Some(Ok(PathBuf::from(whole())))
        );
    }

    #[test]
    fn a_path_that_is_not_whole_is_refused() {
        for text in ["exports", "./exports", "../exports", "~ada/exports"] {
            assert_eq!(
                typed_path(text, Some(&home())),
                Some(Err(NOT_WHOLE)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_tilde_is_the_home_folder() {
        assert_eq!(typed_path("~", Some(&home())), Some(Ok(home())));
        assert_eq!(typed_path("~/", Some(&home())), Some(Ok(home())));
        // As it is written, too: the message afterwards names the folder, and a home
        // folder joined with nothing would be named with a separator at its end.
        for text in ["~", "~/"] {
            let path = typed_path(text, Some(&home())).unwrap().unwrap();
            assert_eq!(path.to_str(), home().to_str(), "{text}");
        }
        assert_eq!(
            typed_path("~/Exports/2026", Some(&home())),
            Some(Ok(home().join("Exports/2026")))
        );
        assert_eq!(
            typed_path("~\\Exports", Some(&home())),
            Some(Ok(home().join("Exports")))
        );
        // With no home to be found it is a name like another, and no whole path.
        assert_eq!(typed_path("~/Exports", None), Some(Err(NOT_WHOLE)));
    }

    // The message it reports.

    #[test]
    fn one_photo_is_counted_singly_and_the_folder_named() {
        assert_eq!(
            report(1, 0, None, 1, "/home/ada/Desktop"),
            Ok("Exported 1 photo to /home/ada/Desktop".to_owned())
        );
        assert_eq!(
            report(1200, 0, None, 1200, "/x"),
            Ok("Exported 1,200 photos to /x".to_owned())
        );
    }

    #[test]
    fn it_says_how_many_of_the_selection_landed_and_why_when_some_did_not() {
        assert_eq!(
            report(10, 2, Some("item 4 not found"), 12, "/home/ada/Desktop"),
            Ok("Exported 10 of 12 photos to /home/ada/Desktop — item 4 not found".to_owned())
        );
        // A reason with nothing failed is not a reason to give.
        assert_eq!(
            report(3, 0, Some("x"), 3, "/d"),
            Ok("Exported 3 photos to /d".to_owned())
        );
        assert_eq!(
            report(1, 1, None, 2, "/d"),
            Ok("Exported 1 of 2 photos to /d".to_owned())
        );
    }

    // Nothing written is a failure, and the reason is the whole of what the user gets.
    #[test]
    fn nothing_written_is_an_error_with_the_reason_for_its_words() {
        assert_eq!(
            report(0, 2, Some("permission denied"), 2, "/d"),
            Err("permission denied".to_owned())
        );
        assert_eq!(
            report(0, 0, None, 0, "/d"),
            Err("Nothing could be exported to /d".to_owned())
        );
    }

    // The words are the Svelte dialog's: read out of the two files that hold them.
    #[test]
    fn the_words_are_the_svelte_dialogs() {
        let dialog = include_str!("../../../ui/src/components/ExportDialog.svelte");
        // The note is wrapped there: compared word for word, whatever the lines, and
        // with the tags it stands between - cut short, it is still in the whole one.
        let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let note = format!("<p class=\"note\"> {} </p>", words(NOTE));
        assert!(words(dialog).contains(&note), "the note");
        // Each with what stands on either side of it there: a label cut short is still
        // found inside the whole one.
        for text in [
            format!("<span>{APPLY_EDITS}</span>").as_str(),
            format!("<span>{LONGEST_EDGE}</span>").as_str(),
            format!(">{EXPORT}</button>").as_str(),
            format!(">{CANCEL}</button>").as_str(),
            "Export {dialog.count === 1 ? '1 photo' : `${dialog.count.toLocaleString()} photos`}",
        ] {
            assert!(
                dialog.lines().any(|line| line.contains(text)),
                "ExportDialog.svelte no longer says {text:?}"
            );
        }
        let state = include_str!("../../../ui/src/lib/export-dialog.svelte.ts");
        for (edge, label) in SIZES {
            let line = format!("{{ value: {}, label: '{label}' }}", edge.unwrap_or(0));
            assert!(state.lines().any(|at| at.contains(&line)), "{line}");
        }
        for text in [
            "`Nothing could be exported to ${to}`",
            "`${done.written.toLocaleString()} of ${counted(asked)}`",
            "` — ${done.reason}`",
            "`Exported ${landed} to ${to}${why}`",
        ] {
            assert!(
                state.lines().any(|line| line.contains(text)),
                "export-dialog.svelte.ts no longer says {text:?}"
            );
        }
    }
}
