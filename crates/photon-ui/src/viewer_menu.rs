//! What the viewer's menu holds: what its faces offer, over what it offers for the photo.
//! The items of `Viewer.svelte`'s menu, in its order and with its words.

use crate::{
    menu::{Item, Look},
    viewer_faces::FaceAction,
};

/// What an item of the viewer's menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewerPick {
    /// "Name this face…": the person dialog, for this `detected_faces.id`.
    Name(i64),
    /// "Not {name}": the photo is taken off the person.
    Not {
        person: i64,
        name: String,
    },
    Locate,
    Reveal,
    OpenInApp,
    /// Hide the photo, or bring it back.
    Hide(bool),
}

fn plain(label: String, hint: Option<&'static str>, does: ViewerPick) -> Item<ViewerPick> {
    Item::Pick {
        label,
        hint,
        works: true,
        look: Look::Plain,
        does,
    }
}

/// The menu: `faces`, what the face under the pointer or the photo's people offer
/// (`viewer_faces::actions`), then what there is for the photo itself. `hidden` is
/// whether the library says the photo is hidden.
pub fn viewer_menu(faces: &[FaceAction], hidden: bool) -> Vec<Item<ViewerPick>> {
    let mut items: Vec<_> = (faces.iter())
        .map(|action| match action {
            FaceAction::Name(face) => {
                plain("Name this face…".to_owned(), None, ViewerPick::Name(*face))
            }
            FaceAction::Not { person, name } => plain(
                format!("Not {name}"),
                None,
                ViewerPick::Not {
                    person: *person,
                    name: name.clone(),
                },
            ),
        })
        .collect();
    if !items.is_empty() {
        items.push(Item::Separator);
    }
    items.push(plain(
        "Locate in photon".to_owned(),
        None,
        ViewerPick::Locate,
    ));
    items.push(plain(
        "Reveal in file manager".to_owned(),
        None,
        ViewerPick::Reveal,
    ));
    items.push(plain(
        "Open in default app".to_owned(),
        None,
        ViewerPick::OpenInApp,
    ));
    // Not "Delete": photon never deletes a photo.
    let hide = if hidden { "Unhide photo" } else { "Hide photo" };
    items.push(plain(hide.to_owned(), Some("H"), ViewerPick::Hide(!hidden)));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line as a test reads it: its words, and whether it takes a press.
    fn lines(items: &[Item<ViewerPick>]) -> Vec<(String, bool)> {
        (items.iter())
            .map(|item| match item {
                Item::Pick { label, works, .. } => (label.clone(), *works),
                Item::Separator => ("-".to_owned(), false),
                Item::Heading(words) | Item::Note(words) => ((*words).to_owned(), false),
            })
            .collect()
    }

    fn line(words: &str, works: bool) -> (String, bool) {
        (words.to_owned(), works)
    }

    #[test]
    fn with_no_face_action_the_menu_is_the_photos_four_items() {
        assert_eq!(
            lines(&viewer_menu(&[], false)),
            [
                line("Locate in photon", true),
                line("Reveal in file manager", true),
                line("Open in default app", true),
                line("Hide photo", true),
            ]
        );
    }

    #[test]
    fn what_the_faces_offer_stands_over_a_separator() {
        let faces = [
            FaceAction::Name(9),
            FaceAction::Not {
                person: 1,
                name: "Anna".to_owned(),
            },
        ];
        let items = viewer_menu(&faces, false);
        assert_eq!(
            lines(&items)[..4],
            [
                line("Name this face…", true),
                line("Not Anna", true),
                line("-", false),
                line("Locate in photon", true),
            ]
        );
        let does: Vec<&ViewerPick> = (items.iter())
            .filter_map(|item| match item {
                Item::Pick { does, .. } => Some(does),
                _ => None,
            })
            .collect();
        assert_eq!(does[0], &ViewerPick::Name(9));
        assert_eq!(
            does[1],
            &ViewerPick::Not {
                person: 1,
                name: "Anna".to_owned()
            }
        );
    }

    #[test]
    fn a_hidden_photo_is_brought_back_and_the_key_is_said_beside_it() {
        let last = |hidden| viewer_menu(&[], hidden).pop().expect("an item");
        assert_eq!(
            last(false),
            Item::Pick {
                label: "Hide photo".to_owned(),
                hint: Some("H"),
                works: true,
                look: Look::Plain,
                does: ViewerPick::Hide(true),
            }
        );
        assert_eq!(
            last(true),
            Item::Pick {
                label: "Unhide photo".to_owned(),
                hint: Some("H"),
                works: true,
                look: Look::Plain,
                does: ViewerPick::Hide(false),
            }
        );
    }

    // The words are the Svelte viewer's: read out of the component that has them.
    #[test]
    fn the_words_are_the_svelte_viewers() {
        let viewer = include_str!("../../../ui/src/components/Viewer.svelte");
        for words in [
            ">Name this face…</button>",
            ">Not {name}</button>",
            ">Locate in photon</button>",
            ">Reveal in file manager</button>",
            ">Open in default app</button>",
            "{item.hidden ? 'Unhide photo' : 'Hide photo'} <span class=\"hint\">H</span>",
        ] {
            assert!(
                viewer.lines().any(|line| line.contains(words)),
                "Viewer.svelte no longer says {words:?}"
            );
        }
    }
}
