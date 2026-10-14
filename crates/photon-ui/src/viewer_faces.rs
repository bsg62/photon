//! The faces on the photo in the viewer: where each is drawn, which one a point is on,
//! and what the viewer's menu offers for it. `faces.ts`, with its tests' cases.
//!
//! A face's rectangle is in fractions of the picture as shown - `ViewerItem` has mapped
//! Picasa's through the photo's edit - so it is placed in the photo as it is drawn, zoom
//! and pan and all, by one multiplication.

use crate::viewer_layout::Frame;
use photon_engine::commands::ViewerItem;

/// A face as the viewer draws it. `key` is a plate's person (`p:<id>`) or Picasa contact
/// (`c:<hash>`), none for an unnamed outline; `face_id` is the detection of photon's that
/// the face is, none for a face that is Picasa's alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn<'a> {
    pub key: Option<&'a str>,
    pub name: Option<&'a str>,
    pub face_id: Option<i64>,
    /// Left, top, right and bottom, each a fraction of the picture as shown.
    pub within: [f64; 4],
}

/// Every face of the photo, the named ones first, in the order the library gives them:
/// what the outlines show while the info panel is open, and what the menu asks about
/// whether it is or not.
pub fn drawn(item: &ViewerItem) -> Vec<Drawn<'_>> {
    let named = item.faces.iter().map(|face| Drawn {
        key: Some(face.key.as_str()),
        name: Some(face.name.as_str()),
        face_id: face.face_id,
        within: [face.left, face.top, face.right, face.bottom],
    });
    let unnamed = item.unnamed_faces.iter().map(|face| Drawn {
        key: None,
        name: None,
        face_id: face.face_id,
        within: [face.left, face.top, face.right, face.bottom],
    });
    named.chain(unnamed).collect()
}

/// A face's rectangle on screen, in the photo as it is drawn at `image`.
pub fn face_box([left, top, right, bottom]: [f64; 4], image: Frame) -> Frame {
    Frame {
        left: image.left + left as f32 * image.width,
        top: image.top + top as f32 * image.height,
        width: (right - left) as f32 * image.width,
        height: (bottom - top) as f32 * image.height,
    }
}

/// The face under a point: of the boxes that hold it (their edges included), the
/// smallest, so that a face inside another's rectangle can be reached; of two as small,
/// the first.
pub fn face_at(x: f32, y: f32, boxes: &[Frame]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, face) in boxes.iter().enumerate() {
        let inside = x >= face.left
            && x <= face.left + face.width
            && y >= face.top
            && y <= face.top + face.height;
        let area = face.width * face.height;
        if inside && best.is_none_or(|(_, smallest)| area < smallest) {
            best = Some((index, area));
        }
    }
    best.map(|(index, _)| index)
}

/// What the viewer's menu offers for faces: to name an unnamed face, or to say the photo
/// is not a person's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaceAction {
    Name(i64),
    Not { person: i64, name: String },
}

/// The person a `p:` key is of; none for a contact's `c:` key, which photon never changes.
pub fn person_of(key: &str) -> Option<i64> {
    key.strip_prefix("p:")?.parse().ok()
}

/// What the viewer's menu was opened on, by what it is and not by its number among the
/// faces: a menu's items are built again in every frame, and the faces can be another
/// list by then - the photo read again after a write, a detection landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
    /// The photo, away from any face.
    Nowhere,
    /// An unnamed face photon found, by its id.
    Unnamed(i64),
    /// The plate of a person photon has a face under, by the person's id.
    Person(i64),
    /// A face nothing is offered on: Picasa's alone, or a contact's no person is linked to.
    Other,
}

/// "Not {them}" for a face, where photon holds one of a person's under it.
fn not(face: &Drawn<'_>) -> Option<(i64, FaceAction)> {
    let person = person_of(face.key?)?;
    face.face_id?;
    let name = face.name.unwrap_or_default().to_owned();
    Some((person, FaceAction::Not { person, name }))
}

/// What the face under the pointer is (`face_at`'s answer), for a menu to be of.
pub fn on(faces: &[Drawn<'_>], hit: Option<usize>) -> On {
    let Some(hit) = hit else {
        return On::Nowhere;
    };
    match faces.get(hit) {
        Some(Drawn {
            key: None,
            face_id: Some(id),
            ..
        }) => On::Unnamed(*id),
        Some(face) => not(face).map_or(On::Other, |(person, _)| On::Person(person)),
        None => On::Other,
    }
}

/// The menu's face actions, as the faces are now.
///
/// On an unnamed face photon found: "Name this face…". On a person's plate photon has a
/// face under: "Not {them}". Anywhere else on the photo: "Not …" once for each person
/// photon has a face of, in the order they are drawn. "Not" is of the photo
/// (`remove_from_person`, the grid's Remove for one photo), so it is offered only where
/// photon holds a face of that person to take off: a person only Picasa names here stays,
/// since photon never changes Picasa's faces, and offering them would promise what the
/// write cannot do.
///
/// A face that is no longer what the menu was opened on - named since, or gone - offers
/// nothing: not the photo's own actions in its place, which were not what was asked for.
pub fn actions(faces: &[Drawn<'_>], on: On) -> Vec<FaceAction> {
    match on {
        On::Other => Vec::new(),
        On::Unnamed(id) => (faces.iter())
            .any(|face| face.key.is_none() && face.face_id == Some(id))
            .then_some(FaceAction::Name(id))
            .into_iter()
            .collect(),
        On::Person(person) => (faces.iter().filter_map(not))
            .find(|(of, _)| *of == person)
            .map(|(_, action)| action)
            .into_iter()
            .collect(),
        On::Nowhere => {
            let mut people = Vec::new();
            let mut actions = Vec::new();
            for (person, action) in faces.iter().filter_map(not) {
                if !people.contains(&person) {
                    people.push(person);
                    actions.push(action);
                }
            }
            actions
        }
    }
}

/// The menu's face actions, `hit` being the face under the pointer (`face_at`).
pub fn face_actions_at(faces: &[Drawn<'_>], hit: Option<usize>) -> Vec<FaceAction> {
    actions(faces, on(faces, hit))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The faces of a photo as the library gives them: the named ones first, then those
    // nobody has named, each with its four numbers in the order left, top, right,
    // bottom - which is the order a menu's "Not …" items stand in, and where an
    // outline is.
    #[test]
    fn the_faces_are_the_librarys_named_first_each_with_its_own_rectangle() {
        use crate::viewer_info_view::testing::{bare, face};
        use photon_engine::commands::UnnamedFace;
        let mut item = bare();
        item.faces = vec![
            photon_core::library::ItemFace {
                left: 0.1,
                top: 0.2,
                right: 0.3,
                bottom: 0.5,
                face_id: Some(4),
                ..face("p:1", "Anna")
            },
            face("c:9", "Ben"),
        ];
        item.unnamed_faces = vec![UnnamedFace {
            left: 0.6,
            top: 0.7,
            right: 0.8,
            bottom: 0.9,
            face_id: Some(12),
        }];
        let drawn = drawn(&item);
        assert_eq!(
            drawn[0],
            Drawn {
                key: Some("p:1"),
                name: Some("Anna"),
                face_id: Some(4),
                within: [0.1, 0.2, 0.3, 0.5],
            }
        );
        assert_eq!((drawn[1].key, drawn[1].face_id), (Some("c:9"), None));
        assert_eq!(
            drawn[2],
            Drawn {
                key: None,
                name: None,
                face_id: Some(12),
                within: [0.6, 0.7, 0.8, 0.9],
            }
        );
        assert_eq!(drawn.len(), 3);
    }

    #[test]
    fn a_face_is_placed_in_the_photo_as_drawn_and_not_in_the_window() {
        let image = Frame {
            left: 0.0,
            top: 100.0,
            width: 800.0,
            height: 600.0,
        };
        assert_eq!(
            face_box([0.25, 0.5, 0.5, 1.0], image),
            Frame {
                left: 200.0,
                top: 400.0,
                width: 200.0,
                height: 300.0,
            }
        );
        // A photo zoomed to twice and moved: the face with it.
        let zoomed = Frame {
            left: -100.0,
            top: -50.0,
            width: 1600.0,
            height: 1200.0,
        };
        assert_eq!(
            face_box([0.25, 0.5, 0.5, 1.0], zoomed),
            Frame {
                left: 300.0,
                top: 550.0,
                width: 400.0,
                height: 600.0,
            }
        );
    }

    fn square(left: f32, top: f32, side: f32) -> Frame {
        Frame {
            left,
            top,
            width: side,
            height: side,
        }
    }

    #[test]
    fn the_face_under_a_point_is_the_smallest_that_holds_it() {
        let (big, small) = (square(0.0, 0.0, 100.0), square(40.0, 40.0, 20.0));
        assert_eq!(face_at(50.0, 50.0, &[big, small]), Some(1));
        assert_eq!(face_at(50.0, 50.0, &[small, big]), Some(0));
        assert_eq!(face_at(10.0, 10.0, &[big, small]), Some(0));
    }

    #[test]
    fn edges_count_ties_go_to_the_first_and_a_point_on_no_face_is_on_none() {
        let (big, small) = (square(0.0, 0.0, 100.0), square(40.0, 40.0, 20.0));
        assert_eq!(face_at(100.0, 100.0, &[big]), Some(0));
        assert_eq!(face_at(50.0, 50.0, &[big, big]), Some(0));
        assert_eq!(face_at(150.0, 50.0, &[big, small]), None);
        assert_eq!(face_at(40.0, 50.0, &[small]), Some(0));
        assert_eq!(face_at(50.0, 40.0, &[small]), Some(0));
        assert_eq!(face_at(60.0, 60.0, &[small]), Some(0));
        assert_eq!(face_at(39.0, 50.0, &[small]), None);
        assert_eq!(face_at(50.0, 39.0, &[small]), None);
        assert_eq!(face_at(61.0, 50.0, &[small]), None);
        assert_eq!(face_at(50.0, 61.0, &[small]), None);
        assert_eq!(face_at(50.0, 50.0, &[]), None);
    }

    fn unnamed(face_id: Option<i64>) -> Drawn<'static> {
        Drawn {
            key: None,
            name: None,
            face_id,
            within: [0.0; 4],
        }
    }

    fn plate(key: &'static str, name: &'static str, face_id: Option<i64>) -> Drawn<'static> {
        Drawn {
            key: Some(key),
            name: Some(name),
            face_id,
            within: [0.0; 4],
        }
    }

    fn not(person: i64, name: &str) -> FaceAction {
        FaceAction::Not {
            person,
            name: name.to_owned(),
        }
    }

    #[test]
    fn an_unnamed_face_photon_found_is_named() {
        let faces = [plate("p:1", "Anna", Some(5)), unnamed(Some(9))];
        assert_eq!(face_actions_at(&faces, Some(1)), [FaceAction::Name(9)]);
    }

    #[test]
    fn nothing_is_offered_on_a_face_that_is_picasas_alone() {
        assert_eq!(
            face_actions_at(&[unnamed(None)], Some(0)),
            Vec::<FaceAction>::new()
        );
        assert_eq!(
            face_actions_at(&[plate("p:1", "Anna", None)], Some(0)),
            Vec::<FaceAction>::new()
        );
        // Nor what the photo itself would offer, where someone else on it is named: the
        // button went down on this face, and what is offered is this face's.
        let faces = [unnamed(None), plate("p:2", "Ben", Some(6))];
        assert_eq!(face_actions_at(&faces, Some(0)), Vec::<FaceAction>::new());
    }

    #[test]
    fn on_a_persons_plate_the_photo_is_said_not_to_be_theirs() {
        let faces = [unnamed(Some(9)), plate("p:1", "Anna", Some(5))];
        assert_eq!(face_actions_at(&faces, Some(1)), [not(1, "Anna")]);
    }

    // A `c:` plate never carries an id today; the key alone must still keep it out.
    #[test]
    fn nothing_is_offered_on_the_plate_of_a_contact_no_person_is_linked_to() {
        assert_eq!(
            face_actions_at(&[plate("c:abc", "Ben", Some(7))], Some(0)),
            Vec::<FaceAction>::new()
        );
        assert_eq!(
            face_actions_at(&[plate("c:abc", "Ben", Some(7))], None),
            Vec::<FaceAction>::new()
        );
    }

    #[test]
    fn away_from_any_face_each_person_photon_has_a_face_of_is_offered_once_as_drawn() {
        let faces = [
            plate("p:2", "Ben", Some(6)),
            unnamed(Some(9)),
            plate("p:1", "Anna", Some(5)),
            plate("p:2", "Ben", Some(8)),
            plate("c:abc", "Cy", Some(10)),
        ];
        assert_eq!(
            face_actions_at(&faces, None),
            [not(2, "Ben"), not(1, "Anna")]
        );
    }

    #[test]
    fn a_person_only_picasa_names_here_is_not_offered() {
        let faces = [plate("p:1", "Anna", None), plate("p:2", "Ben", Some(6))];
        assert_eq!(face_actions_at(&faces, None), [not(2, "Ben")]);
    }

    #[test]
    fn a_menu_is_of_a_face_by_what_the_face_is() {
        let faces = [
            plate("p:2", "Ben", Some(6)),
            unnamed(Some(9)),
            unnamed(None),
            plate("c:abc", "Cy", Some(10)),
            plate("p:1", "Anna", None),
        ];
        assert_eq!(on(&faces, None), On::Nowhere);
        assert_eq!(on(&faces, Some(0)), On::Person(2));
        assert_eq!(on(&faces, Some(1)), On::Unnamed(9));
        assert_eq!(on(&faces, Some(2)), On::Other);
        assert_eq!(on(&faces, Some(3)), On::Other);
        assert_eq!(on(&faces, Some(4)), On::Other);
        assert_eq!(on(&faces, Some(9)), On::Other, "a number out of the list");
    }

    // The items are built again in every frame, from the faces as they are then: the
    // face is found by what it is, wherever it now stands in the list.
    #[test]
    fn a_menus_face_is_found_again_in_another_list() {
        let before = [unnamed(Some(9)), plate("p:1", "Anna", Some(5))];
        let (face, anna) = (on(&before, Some(0)), on(&before, Some(1)));
        // A detection has landed before both, and Anna has been renamed.
        let after = [
            unnamed(Some(12)),
            plate("p:1", "Anna B", Some(5)),
            unnamed(Some(9)),
        ];
        assert_eq!(actions(&after, face), [FaceAction::Name(9)]);
        assert_eq!(actions(&after, anna), [not(1, "Anna B")]);
    }

    // Named since, or gone: nothing is offered in its place - not the photo's own "Not
    // …" items, which are not what the menu was opened for.
    #[test]
    fn a_face_that_is_no_longer_what_the_menu_was_opened_on_offers_nothing() {
        let before = [unnamed(Some(9)), plate("p:1", "Anna", Some(5))];
        let (face, anna) = (on(&before, Some(0)), on(&before, Some(1)));
        // The unnamed face is Ben's now, and Anna's face has been taken off.
        let after = [plate("p:2", "Ben", Some(9))];
        assert_eq!(actions(&after, face), Vec::<FaceAction>::new());
        assert_eq!(actions(&after, anna), Vec::<FaceAction>::new());
        // Picasa still names Anna here: that is not a face of photon's to take off.
        let picasa = [plate("p:1", "Anna", None)];
        assert_eq!(actions(&picasa, anna), Vec::<FaceAction>::new());
        assert_eq!(actions(&[], On::Other), Vec::<FaceAction>::new());
    }

    #[test]
    fn a_key_is_a_persons_only_with_the_prefix_and_a_number() {
        assert_eq!(person_of("p:12"), Some(12));
        assert_eq!(person_of("c:12"), None);
        assert_eq!(person_of("p:"), None);
        assert_eq!(person_of("p:1x"), None);
        assert_eq!(person_of("12"), None);
    }
}
