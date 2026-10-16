//! Every key the native window answers, by where it works: what the `?` sheet and
//! Settings → Shortcuts both draw - `shortcuts.ts`, in this window's keys.
//!
//! Written by hand and held to the handlers by a test that reads their sources (`Key::…`
//! in the files `ANSWERED_IN` names): a key a handler answers that is not here fails it,
//! and so does a key listed here that no handler answers. It cannot check the wording - a
//! key whose meaning changes needs its row changed with it - nor a handler in a file the
//! test does not name: a new one is added there, with its group. Pure mouse gestures are
//! not keys and are not listed.
//!
//! The Svelte list less what this window does not answer yet: revealing a photo
//! (Ctrl+Shift+R) comes with the file manager in sub-project 7. Ctrl+C reaches the
//! handlers as `Event::Copy` and not as a key; the tripwire counts it by the `C` that
//! another key of the same groups names.

/// Where a group of keys works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupId {
    Everywhere,
    Grid,
    Compare,
    Viewer,
    Crop,
    Slideshow,
    Video,
}

/// The chords that do one thing, any one of them, each the keys pressed together. `Mod` is
/// Ctrl, or ⌘ on a Mac (`chord_label`); `click` and `wheel` are the mouse, for the gestures
/// that need a key held, and `arrow` is any of the four arrow keys.
pub struct Shortcut {
    pub keys: &'static [&'static [&'static str]],
    pub does: &'static str,
}

pub struct Group {
    pub id: GroupId,
    pub title: &'static str,
    pub rows: &'static [Shortcut],
}

const fn row(keys: &'static [&'static [&'static str]], does: &'static str) -> Shortcut {
    Shortcut { keys, does }
}

pub const SHORTCUTS: &[Group] = &[
    Group {
        id: GroupId::Everywhere,
        title: "Everywhere",
        rows: &[
            row(&[&["?"]], "Show this list"),
            row(&[&["F11"]], "Fullscreen on or off"),
            row(
                &[&["Mod", "F"], &["/"]],
                "Go to the search box, from the grid or the People page",
            ),
            row(&[&["Enter"]], "In the search box: go to the photos"),
            row(
                &[&["Esc"]],
                "In the search box: clear the search, or leave an empty box",
            ),
            row(&[&["Mod", "B"]], "Hide or show the sidebar"),
            row(
                &[&["←"], &["→"]],
                "On the sidebar's edge: resize the sidebar",
            ),
        ],
    },
    Group {
        id: GroupId::Grid,
        title: "Grid",
        rows: &[
            row(
                &[&["←"], &["→"], &["↑"], &["↓"]],
                "Move to the next photo that way",
            ),
            row(&[&["PgUp"], &["PgDn"]], "Move a screenful up or down"),
            row(&[&["Home"], &["End"]], "First or last photo"),
            row(&[&["Enter"]], "Open the photo in the viewer"),
            row(
                &[&["Mod", "click"]],
                "Add a photo to the selection, or take it out",
            ),
            row(&[&["Shift", "click"]], "Select every photo up to that one"),
            row(
                &[&["Shift", "arrow"]],
                "Select up to the next photo that way; also with Home, End, PgUp and PgDn",
            ),
            row(&[&["Mod", "A"]], "Select every photo"),
            row(&[&["Esc"]], "Clear the selection"),
            row(&[&["C"]], "Compare the two to four selected photos"),
            row(&[&["."]], "Star the selection, or take its stars off"),
            row(&[&["H"]], "Hide the selection; in Hidden, unhide it"),
            row(&[&["Mod", "C"]], "Copy the selected photo as a picture"),
        ],
    },
    Group {
        id: GroupId::Compare,
        title: "Compare",
        rows: &[
            row(&[&["1–4"]], "Focus that photo"),
            row(
                &[&["Tab"], &["Shift", "Tab"]],
                "Focus the next or the previous photo",
            ),
            row(
                &[&["S"], &["."]],
                "Star the focused photo, or take its star off",
            ),
            row(&[&["Enter"]], "Open the focused photo in the viewer"),
            row(&[&["Esc"]], "Back to the grid"),
        ],
    },
    Group {
        id: GroupId::Viewer,
        title: "Viewer",
        rows: &[
            row(&[&["←"], &["→"]], "Previous or next photo"),
            row(&[&["Home"], &["End"]], "First or last photo"),
            row(&[&["Esc"], &["Backspace"]], "Back to the grid"),
            row(&[&["."]], "Star the photo, or take its star off"),
            row(&[&["+"], &["="]], "Zoom in"),
            row(&[&["-"]], "Zoom out"),
            row(&[&["0"]], "Fit the photo to the window"),
            row(&[&["Mod", "wheel"]], "Zoom in or out where the pointer is"),
            row(&[&["I"]], "Photo information on or off"),
            row(&[&["R"]], "Turn the photo right"),
            row(&[&["Shift", "R"]], "Turn the photo left"),
            row(&[&["C"]], "Crop"),
            row(&[&["H"]], "Hide the photo; again to bring it back"),
            row(&[&["S"]], "Start a slideshow from this photo"),
            row(&[&["Mod", "C"]], "Copy the photo as a picture"),
        ],
    },
    Group {
        id: GroupId::Crop,
        title: "While cropping",
        rows: &[
            row(&[&["Enter"]], "Apply the crop"),
            row(&[&["Esc"]], "Cancel"),
        ],
    },
    Group {
        id: GroupId::Slideshow,
        title: "Slideshow",
        rows: &[
            row(&[&["Space"]], "Pause or carry on"),
            row(&[&["←"], &["→"]], "Step back or forward"),
            row(&[&["Esc"], &["S"]], "End the slideshow"),
        ],
    },
    Group {
        id: GroupId::Video,
        title: "Video",
        rows: &[
            row(&[&["Space"]], "Play or pause"),
            row(
                &[&["Shift", "←"], &["Shift", "→"]],
                "Back or forward five seconds",
            ),
            row(&[&["L"]], "Loop on or off"),
            row(&[&["M"]], "Sound on or off"),
        ],
    },
];

/// A chord as the platform's keyboard spells it: `Mod` is ⌘ on a Mac and Ctrl elsewhere.
pub fn chord_label<'a>(chord: &[&'a str], mac: bool) -> Vec<&'a str> {
    (chord.iter())
        .map(|part| match *part {
            "Mod" if mac => "⌘",
            "Mod" => "Ctrl",
            other => other,
        })
        .collect()
}

pub const SHEET_TITLE: &str = "Keyboard shortcuts";
pub const SECTION_HINT: &str =
    "Press ? anywhere outside a text field to see this list over what you are doing.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The files that answer keys, by which groups their keys are listed under. A key a
    /// file answers must be in one of them, and a key a group lists must be answered by a
    /// file that names the group. A handler in a file that is not here is not checked.
    const ANSWERED_IN: &[(&str, &str, &[GroupId])] = &[
        ("app.rs", include_str!("app.rs"), &[GroupId::Everywhere]),
        (
            "search_bar.rs",
            include_str!("search_bar.rs"),
            &[GroupId::Everywhere],
        ),
        ("shell.rs", include_str!("shell.rs"), &[GroupId::Everywhere]),
        (
            "grid/view.rs",
            include_str!("grid/view.rs"),
            &[GroupId::Grid],
        ),
        (
            "viewer_view.rs",
            include_str!("viewer_view.rs"),
            &[
                GroupId::Viewer,
                GroupId::Crop,
                GroupId::Slideshow,
                GroupId::Video,
            ],
        ),
        (
            "compare_view.rs",
            include_str!("compare_view.rs"),
            &[GroupId::Compare],
        ),
    ];

    /// Parts of a chord that are not a key a handler names.
    const MODIFIERS: [&str; 5] = ["Mod", "Shift", "click", "wheel", "arrow"];

    /// egui's name for a key as the list spells it. A name the list has no spelling for
    /// is the name itself, which no row lists: a handler that answers `Key::Delete` is a
    /// key the list lacks, not a name to pass over.
    fn listed(name: &str) -> String {
        match name {
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "PageUp" => "PgUp",
            "PageDown" => "PgDn",
            "Escape" => "Esc",
            "Period" => ".",
            "Plus" => "+",
            "Equals" => "=",
            "Minus" => "-",
            "Slash" => "/",
            "Questionmark" => "?",
            "Num0" => "0",
            "Num1" | "Num2" | "Num3" | "Num4" => "1–4",
            _ => name,
        }
        .to_owned()
    }

    /// Every key a file's code names, read by line - a Windows checkout is CRLF - from
    /// what is not a comment, and not its tests: the reading stops at the tests' module,
    /// not at the first item only a test build has (`grid/view.rs` has one a third of
    /// the way in). `Key::` is egui's, and not the end of another type's name - the
    /// grid's `GridKey::Home` is a move, not the key.
    fn keys_in(source: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let lines: Vec<&str> = source.lines().map(str::trim).collect();
        for (n, code) in lines.iter().enumerate() {
            let tests = lines[n + 1..].iter().find(|line| !line.is_empty());
            if *code == "#[cfg(test)]" && tests.is_some_and(|next| next.starts_with("mod tests")) {
                break;
            }
            if code.starts_with("//") {
                continue;
            }
            let mut rest: &str = code;
            while let Some(at) = rest.find("Key::") {
                let named = rest[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_ascii_alphanumeric() && before != '_');
                rest = &rest[at + 5..];
                let name: String = rest
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                if named && !name.is_empty() {
                    keys.insert(listed(&name));
                }
            }
        }
        keys
    }

    fn keys_of(id: GroupId) -> BTreeSet<String> {
        (SHORTCUTS.iter())
            .filter(|group| group.id == id)
            .flat_map(|group| group.rows)
            .flat_map(|row| row.keys.iter().copied())
            .flat_map(|chord| chord.iter().copied())
            .filter(|key| !MODIFIERS.contains(key))
            .map(str::to_owned)
            .collect()
    }

    // Each file answers at least one key: an empty set would mean the reading no longer
    // fits the code, and every check below would pass on it.
    #[test]
    fn keys_are_read_out_of_every_file_checked() {
        for (file, source, _) in ANSWERED_IN {
            assert!(!keys_in(source).is_empty(), "{file}");
        }
        let viewer: Vec<String> = keys_in(include_str!("viewer_view.rs"))
            .into_iter()
            .collect();
        for key in [
            "+",
            "-",
            ".",
            "0",
            "=",
            "Backspace",
            "C",
            "Esc",
            "H",
            "I",
            "R",
            "S",
        ] {
            assert!(viewer.contains(&key.to_owned()), "{key} in {viewer:?}");
        }
    }

    #[test]
    fn every_key_a_handler_answers_is_listed() {
        for (file, source, groups) in ANSWERED_IN {
            let known: BTreeSet<String> = groups.iter().flat_map(|id| keys_of(*id)).collect();
            let unlisted: Vec<String> = keys_in(source)
                .into_iter()
                .filter(|key| !known.contains(key))
                .collect();
            assert!(
                unlisted.is_empty(),
                "{file} answers {unlisted:?}, not listed"
            );
        }
    }

    #[test]
    fn no_listed_key_goes_unanswered() {
        for group in SHORTCUTS {
            let answered: BTreeSet<String> = (ANSWERED_IN.iter())
                .filter(|(_, _, groups)| groups.contains(&group.id))
                .flat_map(|(_, source, _)| keys_in(source))
                .collect();
            let unanswered: Vec<String> = keys_of(group.id)
                .into_iter()
                .filter(|key| !answered.contains(key))
                .collect();
            assert!(unanswered.is_empty(), "{:?} lists {unanswered:?}", group.id);
        }
    }

    // The words are `shortcuts.ts`'s: every row's, and the groups' titles.
    #[test]
    fn the_words_are_the_svelte_lists() {
        let svelte = include_str!("../../../ui/src/lib/shortcuts.ts");
        let joined = (svelte.lines())
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");
        for group in SHORTCUTS {
            assert!(
                joined.contains(&format!("title: '{}'", group.title)),
                "{}",
                group.title
            );
            for row in group.rows {
                // Quoted either way: the Svelte list writes a row with an apostrophe in
                // double quotes.
                assert!(
                    joined.contains(&format!("does: '{}'", row.does))
                        || joined.contains(&format!("does: \"{}\"", row.does)),
                    "{}",
                    row.does
                );
            }
        }
        let list = include_str!("../../../ui/src/components/Settings.svelte");
        assert!(list.lines().any(|line| line.contains(SECTION_HINT)));
        let sheet = include_str!("../../../ui/src/components/ShortcutSheet.svelte");
        assert!(sheet.lines().any(|line| line.contains(SHEET_TITLE)));
    }

    #[test]
    fn mod_is_spelt_as_the_platform_spells_it() {
        assert_eq!(chord_label(&["Mod", "A"], false), ["Ctrl", "A"]);
        assert_eq!(chord_label(&["Mod", "A"], true), ["⌘", "A"]);
        assert_eq!(chord_label(&["Esc"], true), ["Esc"]);
    }

    // a test-only item in the middle of a file is not the end of its code:
    // `grid/view.rs` has `#[cfg(test)] fn scroll_to` at its line 332, and every key read
    // in the 950 lines of the grid's code after it went unseen.
    #[test]
    fn the_reading_goes_on_past_a_test_only_item() {
        let source = "fn a() {}\n#[cfg(test)]\nfn helper() {}\nfn b() { let _ = Key::X; }\n";
        assert!(keys_in(source).contains("X"));
    }

    // a key the reading has no name for is not dropped: a handler that answered
    // `Key::Delete`, `Key::Num5` or `Key::F2` passed both checks.
    #[test]
    fn a_key_the_reading_cannot_name_is_not_passed_over() {
        for name in ["Delete", "Num5", "F2", "Comma", "Insert"] {
            assert!(
                !keys_in(&format!("let _ = Key::{name};")).is_empty(),
                "Key::{name} was read as no key"
            );
        }
    }

    #[test]
    fn the_key_that_opens_the_list_is_in_it() {
        assert!(keys_of(GroupId::Everywhere).contains("?"));
    }
}
