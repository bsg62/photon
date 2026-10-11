//! The menus of the sidebar: an album's, a saved search's and a folder's - which items
//! each has, in which words, and which of them work yet. `FolderTree.svelte`'s two and
//! `FolderMenu.svelte`, item for item and string for string.
//!
//! A folder's is one list wherever the folder is drawn - its row in the sidebar, its
//! header in the grid and the copy of that header pinned over the photos - so the three
//! cannot come to offer different things.

use crate::menu::{Item, Look};

/// What an item of one of the sidebar's menus does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideAction {
    RenameAlbum(i64),
    DeleteAlbum(i64),
    RenameSearch(i64),
    DeleteSearch(i64),
    /// Scan the watched folder a folder belongs to again.
    Rescan {
        watched: i64,
    },
    RevealFolder(i64),
    /// "Rename in photon…": the directory keeps its name; only photon's name for it
    /// changes.
    RenameFolder(i64),
    /// Take photon's name for the folder away: it is shown by its directory's again.
    UseFolderName(i64),
    /// Hide the folder - its photos, and any added to it later - or bring it back.
    HideFolder {
        folder: i64,
        hidden: bool,
    },
}

impl SideAction {
    /// Whether the item works yet. "Delete…" asks first, and what it asks with is the
    /// dialog layer's, the next pull request's; the file manager is the platform's, a
    /// later sub-project's. Both are in their place and take no press, so that nothing
    /// moves under a hand that has learnt where it is when they start to work.
    pub fn works(&self) -> bool {
        !matches!(
            self,
            Self::DeleteAlbum(_) | Self::DeleteSearch(_) | Self::RevealFolder(_)
        )
    }
}

fn pick(label: &str, look: Look, works: bool, does: SideAction) -> Item<SideAction> {
    Item::Pick {
        label: label.to_owned(),
        hint: None,
        works: works && does.works(),
        look,
        does,
    }
}

/// The menu of an album of the user's own. An album of Picasa's has none: it is read
/// from Picasa's file and changed in Picasa.
pub fn album_menu(id: i64) -> Vec<Item<SideAction>> {
    vec![
        pick("Rename…", Look::Plain, true, SideAction::RenameAlbum(id)),
        pick("Delete…", Look::Danger, true, SideAction::DeleteAlbum(id)),
    ]
}

pub fn search_menu(id: i64) -> Vec<Item<SideAction>> {
    vec![
        pick("Rename…", Look::Plain, true, SideAction::RenameSearch(id)),
        pick("Delete…", Look::Danger, true, SideAction::DeleteSearch(id)),
    ]
}

/// What a folder's menu is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderFacts {
    pub id: i64,
    /// The watched folder it belongs to, which is what is scanned.
    pub watched: i64,
    /// Whether it has a name in photon.
    pub named: bool,
    pub hidden: bool,
    /// Whether its watched folder is being scanned.
    pub scanning: bool,
    /// Whether the sidebar's list holds a row for it: the name is typed in that row.
    pub listed: bool,
}

pub fn folder_menu(facts: &FolderFacts) -> Vec<Item<SideAction>> {
    let FolderFacts { id, watched, .. } = *facts;
    let mut items = vec![
        // The engine does nothing with a rescan asked for while that folder is being
        // scanned, and says nothing back: it is not offered then.
        pick(
            "Rescan",
            Look::Plain,
            !facts.scanning,
            SideAction::Rescan { watched },
        ),
        pick(
            "Reveal in file manager",
            Look::Plain,
            true,
            SideAction::RevealFolder(id),
        ),
        // A folder with no row has no place to type its name in: in an order that
        // lists the folders it has one, and a hidden sidebar is shown for it.
        pick(
            "Rename in photon…",
            Look::Plain,
            facts.listed,
            SideAction::RenameFolder(id),
        ),
    ];
    if facts.named {
        items.push(pick(
            "Use folder name",
            Look::Plain,
            true,
            SideAction::UseFolderName(id),
        ));
    }
    // Hidden, its row leaves the sidebar by itself: the rows are the view's folders, and
    // a hidden folder's photos are in no view but Hidden - where this offers to bring
    // it back.
    let hide = if facts.hidden {
        "Unhide folder"
    } else {
        "Hide folder"
    };
    items.push(pick(
        hide,
        Look::Plain,
        true,
        SideAction::HideFolder {
            folder: id,
            hidden: !facts.hidden,
        },
    ));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu as lines: a label, `!` before a dangerous one, `(off)` after one that
    /// takes no press.
    fn lines(items: &[Item<SideAction>]) -> Vec<String> {
        (items.iter())
            .map(|item| match item {
                Item::Pick {
                    label, works, look, ..
                } => {
                    let danger = if *look == Look::Danger { "!" } else { "" };
                    let off = if *works { "" } else { " (off)" };
                    format!("{danger}{label}{off}")
                }
                other => panic!("the sidebar's menus hold items only: {other:?}"),
            })
            .collect()
    }

    fn does(items: &[Item<SideAction>], label: &str) -> SideAction {
        (items.iter())
            .find_map(|item| match item {
                Item::Pick {
                    label: at, does, ..
                } if at == label => Some(does.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no item {label:?}"))
    }

    fn folder() -> FolderFacts {
        FolderFacts {
            id: 7,
            watched: 2,
            named: false,
            hidden: false,
            scanning: false,
            listed: true,
        }
    }

    // "Delete…" is in its place, dangerous and not yet working: it asks first, and the
    // dialog it asks with is the next pull request's.
    #[test]
    fn an_album_and_a_saved_search_are_renamed_and_not_yet_deleted() {
        let album = album_menu(4);
        assert_eq!(lines(&album), ["Rename…", "!Delete… (off)"]);
        assert_eq!(does(&album, "Rename…"), SideAction::RenameAlbum(4));
        assert_eq!(does(&album, "Delete…"), SideAction::DeleteAlbum(4));
        let search = search_menu(4);
        assert_eq!(lines(&search), ["Rename…", "!Delete… (off)"]);
        assert_eq!(does(&search, "Rename…"), SideAction::RenameSearch(4));
        assert_eq!(does(&search, "Delete…"), SideAction::DeleteSearch(4));
    }

    #[test]
    fn the_menu_of_a_folder() {
        let menu = folder_menu(&folder());
        assert_eq!(
            lines(&menu),
            [
                "Rescan",
                "Reveal in file manager (off)",
                "Rename in photon…",
                "Hide folder",
            ]
        );
        // The scan is of the watched folder it belongs to; the rest is the folder's own.
        assert_eq!(does(&menu, "Rescan"), SideAction::Rescan { watched: 2 });
        assert_eq!(
            does(&menu, "Reveal in file manager"),
            SideAction::RevealFolder(7)
        );
        assert_eq!(
            does(&menu, "Rename in photon…"),
            SideAction::RenameFolder(7)
        );
        assert_eq!(
            does(&menu, "Hide folder"),
            SideAction::HideFolder {
                folder: 7,
                hidden: true
            }
        );
    }

    #[test]
    fn a_folder_with_a_name_in_photon_can_have_its_own_back() {
        let menu = folder_menu(&FolderFacts {
            named: true,
            ..folder()
        });
        assert_eq!(
            lines(&menu),
            [
                "Rescan",
                "Reveal in file manager (off)",
                "Rename in photon…",
                "Use folder name",
                "Hide folder",
            ]
        );
        assert_eq!(does(&menu, "Use folder name"), SideAction::UseFolderName(7));
    }

    #[test]
    fn a_hidden_folder_is_offered_back() {
        let menu = folder_menu(&FolderFacts {
            hidden: true,
            ..folder()
        });
        assert_eq!(lines(&menu)[3], "Unhide folder");
        assert_eq!(
            does(&menu, "Unhide folder"),
            SideAction::HideFolder {
                folder: 7,
                hidden: false
            }
        );
    }

    // The engine does nothing with a rescan of a folder that is being scanned.
    #[test]
    fn a_folder_being_scanned_is_not_offered_another_scan() {
        let menu = folder_menu(&FolderFacts {
            scanning: true,
            ..folder()
        });
        assert_eq!(lines(&menu)[0], "Rescan (off)");
        assert_eq!(lines(&menu)[2], "Rename in photon…");
    }

    // Its name is typed in its row, and it has none.
    #[test]
    fn a_folder_the_list_does_not_hold_cannot_be_named() {
        let menu = folder_menu(&FolderFacts {
            listed: false,
            ..folder()
        });
        assert_eq!(lines(&menu)[2], "Rename in photon… (off)");
        assert_eq!(lines(&menu)[0], "Rescan");
    }

    #[test]
    fn what_asks_first_and_what_is_the_platforms_does_not_work_yet() {
        for action in [
            SideAction::DeleteAlbum(1),
            SideAction::DeleteSearch(1),
            SideAction::RevealFolder(1),
        ] {
            assert!(!action.works(), "{action:?}");
        }
        for action in [
            SideAction::RenameAlbum(1),
            SideAction::RenameSearch(1),
            SideAction::Rescan { watched: 1 },
            SideAction::RenameFolder(1),
            SideAction::UseFolderName(1),
            SideAction::HideFolder {
                folder: 1,
                hidden: true,
            },
        ] {
            assert!(action.works(), "{action:?}");
        }
    }

    // The words are the Svelte menus': read out of the components that draw them.
    #[test]
    fn the_words_are_the_svelte_menus() {
        let folder = include_str!("../../../ui/src/components/FolderMenu.svelte");
        for words in [
            ">Rescan</button",
            ">Reveal in file manager</button>",
            ">Rename in photon…</button>",
            ">Use folder name</button",
            "'Unhide folder' : 'Hide folder'",
        ] {
            assert!(
                folder.lines().any(|line| line.contains(words)),
                "FolderMenu.svelte no longer says {words:?}"
            );
        }
        let tree = include_str!("../../../ui/src/components/FolderTree.svelte");
        for words in [">Rename…</button>", ">Delete…</button>"] {
            let said = tree.lines().filter(|line| line.contains(words)).count();
            assert_eq!(said, 2, "FolderTree.svelte's two menus say {words:?}");
        }
    }
}
