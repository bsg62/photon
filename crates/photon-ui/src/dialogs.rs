//! The dialogs there are: the question asked before an album, a saved search or a person is
//! deleted or a person merged into another, the keyword picker, the export's, the one that names a face or adds photos to
//! a person, and Settings. One at a time, over the whole window, held by the application and
//! drawn by `dialog_view.rs`.
//!
//! **What a dialog acts on is taken when it opens** - the album or the search of the
//! question, by its id and under the name it had; the photos of the picker and of the
//! export; the face or the photos of the person dialog - and not read when it answers. This is the opposite of a menu's rule, on purpose. A menu's
//! items are built again in every frame and say what they would act on now; a dialog's
//! words were read once, and what the user was told it would act on is what it acts on.

use crate::{
    export_dialog::ExportDialog, person_picker::PersonPicker, settings::Settings,
    tag_picker::TagPicker,
};

/// A question asked before something is done that cannot be taken back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    Album {
        id: i64,
        name: String,
    },
    Search {
        id: i64,
        name: String,
    },
    /// A named person deleted, from the People page: their faces stay, a group with no
    /// name.
    Person {
        id: i64,
        name: String,
    },
    /// One person merged into another, from the People page's "Merge into…".
    Merge {
        from: i64,
        from_name: String,
        into: i64,
        into_name: String,
    },
}

/// An album of the user's own or a saved search, as what a question is about and what
/// a delete deletes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Album(i64),
    Search(i64),
}

impl Confirm {
    /// The album or search a delete deletes; none for a question about people.
    pub fn subject(&self) -> Option<Subject> {
        match self {
            Confirm::Album { id, .. } => Some(Subject::Album(*id)),
            Confirm::Search { id, .. } => Some(Subject::Search(*id)),
            Confirm::Person { .. } | Confirm::Merge { .. } => None,
        }
    }

    /// The name the question was asked under.
    pub fn name(&self) -> &str {
        match self {
            Confirm::Album { name, .. }
            | Confirm::Search { name, .. }
            | Confirm::Person { name, .. } => name,
            Confirm::Merge { from_name, .. } => from_name,
        }
    }

    /// The button that does what was asked.
    pub fn yes(&self) -> &'static str {
        match self {
            Confirm::Merge { .. } => MERGE,
            _ => DELETE,
        }
    }
}

/// The button that does what was asked about, and the one that does not.
pub const DELETE: &str = "Delete";
pub const MERGE: &str = "Merge";
pub const CANCEL: &str = "Cancel";

impl Confirm {
    /// The Svelte UI's `ask` has the system draw these; photon's own dialog stands in
    /// for it, under the same title and with the same question.
    pub fn title(&self) -> &'static str {
        match self {
            Confirm::Album { .. } => "Delete album",
            Confirm::Search { .. } => "Delete saved search",
            Confirm::Person { .. } => "Delete person",
            Confirm::Merge { .. } => "Merge people",
        }
    }

    /// The question, which says what is *not* lost: photon never deletes a photo.
    pub fn question(&self) -> String {
        match self {
            Confirm::Album { name, .. } => {
                format!("Delete the album “{name}”? The photos stay in your library.")
            }
            Confirm::Search { name, .. } => {
                format!("Delete the saved search “{name}”? Your photos are not affected.")
            }
            Confirm::Person { name, .. } => crate::people_page::delete_question(name),
            Confirm::Merge {
                from_name,
                into_name,
                ..
            } => crate::people_page::merge_question(from_name, into_name),
        }
    }
}

/// The dialog that is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    Confirm(Confirm),
    Tags(TagPicker),
    Export(ExportDialog),
    Person(PersonPicker),
    /// Boxed: it holds the lists of its sections, and every other dialog would be as
    /// large.
    Settings(Box<Settings>),
    /// The list of shortcuts, over whatever the `?` was pressed in.
    Shortcuts,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_what_is_deleted_and_says_the_photos_stay() {
        let album = Confirm::Album {
            id: 4,
            name: "Lisbon".to_owned(),
        };
        assert_eq!(album.title(), "Delete album");
        assert_eq!(
            album.question(),
            "Delete the album “Lisbon”? The photos stay in your library."
        );
        let search = Confirm::Search {
            id: 4,
            name: "Lakes".to_owned(),
        };
        assert_eq!(search.title(), "Delete saved search");
        assert_eq!(
            search.question(),
            "Delete the saved search “Lakes”? Your photos are not affected."
        );
    }

    // The words are the Svelte sidebar's: read out of the component that asks them.
    #[test]
    fn the_words_are_the_svelte_sidebars() {
        let tree = include_str!("../../../ui/src/components/FolderTree.svelte");
        for words in [
            "`Delete the album “${album.name}”? The photos stay in your library.`",
            "`Delete the saved search “${search.name}”? Your photos are not affected.`",
            "title: 'Delete album',",
            "title: 'Delete saved search',",
        ] {
            assert!(
                tree.lines().any(|line| line.contains(words)),
                "FolderTree.svelte no longer says {words:?}"
            );
        }
    }
}
