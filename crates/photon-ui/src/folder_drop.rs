//! Folders dropped on the window become watched folders: what is held over the window,
//! which paths a drop hands on, and what is said of what came of them. `createFolderDrop`
//! in Rust, with the words of `App.svelte` and `Grid.svelte`.
//!
//! Every dropped path is offered to the engine, which is what knows a folder from a
//! file, a folder already watched (answered as itself, not as an error) and one that
//! overlaps another - one at a time, since two folders of one drop can overlap each
//! other and the engine can refuse the second only once it has finished the first. That
//! is the application's doing (`Write::Watch`); what is here is the drop before it and
//! the report after. The drop is reported once: what is now watched, and the first
//! refusal with a count of the rest - a drop of forty photos must not be forty messages.

use raw_window_handle::{HandleError, RawWindowHandle};

/// What the card over the window says while something is held over it.
pub const CARD: &str = "Drop folders to add them to photon";
/// And while the export's dialog is open, where the folder dropped is its destination.
pub const CARD_EXPORT: &str = "Drop a folder to export into it";
/// The line under the empty library's buttons, where a drop can arrive.
pub const ASIDE: &str = "Or drop a folder onto this window.";

/// Whether a folder dropped on a window of this kind can arrive. Not on Wayland: winit
/// 0.30 has dropped files on X11, Windows and macOS and none there, and a window that
/// offered a drop where none arrives would be saying something false. No window at all
/// is a test, which feeds the drop itself.
pub fn can_arrive(window: Result<RawWindowHandle, HandleError>) -> bool {
    !matches!(window, Ok(RawWindowHandle::Wayland(_)))
}

/// The drag over the window, a frame at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderDrop {
    hovering: bool,
}

impl FolderDrop {
    /// Whether something with a path is held over the window, for the card that says
    /// what letting go of it will do.
    pub fn hovering(&self) -> bool {
        self.hovering
    }

    /// A frame's news of the drag: how many paths are held over the window, and those
    /// dropped on it. Answers the paths to act on.
    ///
    /// A drag that carries no path - text dragged out of another program - is not
    /// photon's to announce, and a drop of none is nothing to act on.
    pub fn frame(&mut self, held: usize, dropped: Vec<String>) -> Vec<String> {
        // A drop ends the drag, whatever the system still says is held in its frame.
        self.hovering = held > 0 && dropped.is_empty();
        dropped
    }
}

/// The last component of a path, for a message: "Holiday", not the whole way to it.
/// Whichever way its separators lean: a path is named as the system that dropped it
/// writes it, and a test of a Windows path runs everywhere.
pub fn leaf(path: &str) -> &str {
    path.split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
}

/// What came of offering a drop's paths to the engine, each list in the order offered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watched {
    /// The folders now watched, by the path the engine keeps for each.
    pub added: Vec<String>,
    /// The engine's words for each path it refused.
    pub refused: Vec<String>,
}

impl Watched {
    /// What is now watched: one folder by its name, several by their number. None when
    /// nothing was.
    pub fn said(&self) -> Option<String> {
        match self.added.as_slice() {
            [] => None,
            [one] => Some(format!("Watching “{}”", leaf(one))),
            several => Some(format!("Watching {} folders", several.len())),
        }
    }

    /// The first refusal, with a count of the rest. None when nothing was refused.
    pub fn refusal(&self) -> Option<String> {
        let (first, rest) = self.refused.split_first()?;
        Some(if rest.is_empty() {
            first.clone()
        } else {
            format!("{first} (and {} more)", rest.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    // folder-drop.svelte.test.ts, as far as its cases are this module's: the adding one
    // at a time, and the list read once, are the application's.

    #[test]
    fn the_card_is_up_while_paths_are_held_and_not_for_a_drag_that_carries_none() {
        let mut drop = FolderDrop::default();
        assert!(!drop.hovering());
        assert_eq!(drop.frame(1, Vec::new()), Vec::<String>::new());
        assert!(drop.hovering());
        // Held still, frame after frame.
        drop.frame(1, Vec::new());
        assert!(drop.hovering());
        // The drag left the window.
        drop.frame(0, Vec::new());
        assert!(!drop.hovering());
    }

    #[test]
    fn a_drop_hands_its_paths_on_and_takes_the_card_down() {
        let mut drop = FolderDrop::default();
        drop.frame(1, Vec::new());
        let dropped = paths(&["/home/ada/Holiday"]);
        assert_eq!(drop.frame(0, dropped.clone()), dropped);
        assert!(!drop.hovering());
        // The system may still call the file held in the frame it is dropped in.
        drop.frame(2, Vec::new());
        assert_eq!(drop.frame(2, dropped.clone()), dropped);
        assert!(!drop.hovering());
    }

    #[test]
    fn a_drop_that_carries_no_paths_is_nothing_to_act_on() {
        let mut drop = FolderDrop::default();
        assert_eq!(drop.frame(0, Vec::new()), Vec::<String>::new());
        assert!(!drop.hovering());
    }

    #[test]
    fn one_folder_watched_is_named_by_its_last_component() {
        let watched = Watched {
            added: paths(&["/home/ada/Holiday"]),
            refused: Vec::new(),
        };
        assert_eq!(watched.said().as_deref(), Some("Watching “Holiday”"));
        assert_eq!(watched.refusal(), None);
    }

    #[test]
    fn a_windows_folder_is_named_by_its_last_component_too() {
        assert_eq!(leaf("C:\\Users\\ada\\Pictures\\"), "Pictures");
        assert_eq!(leaf("\\\\nas\\photos\\2024"), "2024");
        assert_eq!(leaf("/home/ada/Holiday/"), "Holiday");
        // Nothing to name it by but itself.
        assert_eq!(leaf("/"), "/");
        assert_eq!(leaf(""), "");
        let watched = Watched {
            added: paths(&["C:\\Users\\ada\\Pictures\\"]),
            refused: Vec::new(),
        };
        assert_eq!(watched.said().as_deref(), Some("Watching “Pictures”"));
    }

    #[test]
    fn several_folders_are_counted() {
        let watched = Watched {
            added: paths(&["/a", "/b", "/c"]),
            refused: Vec::new(),
        };
        assert_eq!(watched.said().as_deref(), Some("Watching 3 folders"));
    }

    // A drop of forty photos must not be forty messages.
    #[test]
    fn what_was_refused_is_said_once_and_the_rest_is_still_watched() {
        let watched = Watched {
            added: paths(&["/x/Holiday"]),
            refused: paths(&[
                "a.jpg is a file, not a folder",
                "b.jpg is a file, not a folder",
                "c.jpg is a file, not a folder",
            ]),
        };
        assert_eq!(watched.said().as_deref(), Some("Watching “Holiday”"));
        assert_eq!(
            watched.refusal().as_deref(),
            Some("a.jpg is a file, not a folder (and 2 more)")
        );
    }

    #[test]
    fn nothing_is_said_to_be_watched_when_nothing_was() {
        let watched = Watched {
            added: Vec::new(),
            refused: paths(&["a.jpg is a file, not a folder"]),
        };
        assert_eq!(watched.said(), None);
        assert_eq!(
            watched.refusal().as_deref(),
            Some("a.jpg is a file, not a folder")
        );
        assert_eq!(Watched::default().said(), None);
        assert_eq!(Watched::default().refusal(), None);
    }

    #[test]
    fn a_drop_arrives_in_every_kind_of_window_but_waylands() {
        use raw_window_handle::{WaylandWindowHandle, XlibWindowHandle};
        let surface = std::ptr::NonNull::<std::ffi::c_void>::dangling();
        let wayland = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface));
        assert!(!can_arrive(Ok(wayland)));
        assert!(can_arrive(Ok(RawWindowHandle::Xlib(
            XlibWindowHandle::new(7)
        ))));
        // A test has no window, and drops what it likes.
        assert!(can_arrive(Err(HandleError::NotSupported)));
        assert!(can_arrive(Err(HandleError::Unavailable)));
    }

    // The words are the Svelte window's: read out of the files that hold them.
    #[test]
    fn the_words_are_the_svelte_windows() {
        // Each with what stands on either side of it there: a sentence cut short is
        // still found inside the whole one.
        let app = include_str!("../../../ui/src/App.svelte");
        let card = format!("/> {CARD}</div>");
        assert!(app.lines().any(|line| line.contains(&card)), "{CARD:?}");
        let grid = include_str!("../../../ui/src/components/Grid.svelte");
        let aside = format!(">{ASIDE}</p>");
        assert!(grid.lines().any(|line| line.contains(&aside)), "{ASIDE:?}");
        let state = include_str!("../../../ui/src/lib/folder-drop.svelte.ts");
        for words in [
            "`Watching “${leaf(added[0].path)}”`",
            "`Watching ${added.length} folders`",
            "(and ${refused.length - 1} more)",
        ] {
            assert!(
                state.lines().any(|line| line.contains(words)),
                "folder-drop.svelte.ts no longer says {words:?}"
            );
        }
    }
}
