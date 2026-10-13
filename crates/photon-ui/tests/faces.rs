//! Faces, through the whole application and the real engine: the outlines over the
//! photo in the viewer, the viewer's menu, the dialog that names a face or adds photos
//! to a person.
//!
//! Every write here goes into a library the test made, in a temporary directory, and
//! none of them leaves `library.db`: a name is never written to a photo or an INI.

mod common;

use common::{Driver, Harness, click as row, library, opened, photo_ids, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, accesskit::Role};
use egui_kittest::kittest::Queryable;
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    grid::GridView,
    library::Library,
};
use photon_ui::{app::App, dialogs::Dialog, fixture::Fixture, person_picker};

/// photon has looked at every photo of `library` and found one face on each of
/// `with_face`, and the user has named the faces on `named` after `name`.
fn looked(library: &Fixture, with_face: &[i64], named: &[i64], name: &str) -> Library {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.set_face_detection(true).unwrap();
    let photos = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
    let face = Detection {
        rect: Rect {
            left: 0.3,
            top: 0.2,
            right: 0.6,
            bottom: 0.6,
        },
        landmarks: [
            (0.4, 0.35),
            (0.5, 0.35),
            (0.45, 0.42),
            (0.41, 0.5),
            (0.49, 0.5),
        ],
        score: 0.9,
    };
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let faces = if with_face.contains(&photo.id) {
                vec![face.clone()]
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    if !named.is_empty() {
        let came = lib.name_items(named, name).unwrap();
        assert_eq!(came.named, named.len() as i64);
    }
    lib
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

/// Where the photo `id` stands in the grid on screen.
fn at(harness: &Harness<'_>, id: i64) -> usize {
    (harness.state().shown().iter())
        .position(|shown| *shown == id)
        .expect("the photo is in the grid")
}

/// A click on the photo at `offset` of the grid, with `held` down.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: held,
    });
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: held,
    });
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

/// Opens the viewer on the photo `id` by a double click, with its info panel, and
/// waits for the library to have said what the photo is and for its picture: the
/// faces are outlined on a picture.
fn informed(driver: &mut Driver, harness: &mut Harness<'_>, id: i64) {
    let offset = at(harness, id);
    pick(driver, harness, offset, Modifiers::NONE);
    pick(driver, harness, offset, Modifiers::NONE);
    assert_eq!(viewing(harness), Some(id), "the viewer is open");
    driver.until(harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    key(driver, harness, Key::I);
    driver.until(harness, "its preview", |app: &App| app.preview_of(id));
    driver.act(harness);
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

/// The other button, down and up at `at`, and the frame its menu is first drawn in.
fn right_click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(harness);
    // A new area is measured in one frame and shown in the next.
    driver.act(harness);
}

/// Picks the item of the open menu called `label`.
fn pick_item(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
    assert!(!harness.state().menu_open());
}

/// Draws a dialog that has just been opened until it stands, and until a press in it
/// is no longer the second of a double click on what opened it.
fn stand(driver: &mut Driver, harness: &mut Harness<'_>) {
    for _ in 0..3 {
        driver.act(harness);
    }
    driver.frames_in(harness, 0.4);
}

/// The person dialog that is open: what it names, and what is typed in it.
fn dialog(harness: &Harness<'_>) -> Option<(person_picker::Target, String)> {
    match harness.state().dialog() {
        Some(Dialog::Person(picker)) => Some((picker.target().clone(), picker.draft().to_owned())),
        _ => None,
    }
}

/// The names on the faces of the photo in the viewer, as the library last said them,
/// and how many faces nobody has named.
fn faces(harness: &Harness<'_>) -> (Vec<String>, usize) {
    (harness.state().viewer_item()).map_or((Vec::new(), 0), |item| {
        (
            item.faces.iter().map(|face| face.name.clone()).collect(),
            item.unnamed_faces.len(),
        )
    })
}

/// The middle of the one face on the photo in the viewer, on screen: by its outline
/// when nobody has named it, and by the plate under its left corner when someone has.
fn face_at(harness: &Harness<'_>, name: Option<&str>) -> Pos2 {
    match name {
        None => harness.get_by_label("Name this face").rect().center(),
        Some(name) => {
            let plate = harness.get_by_role_and_label(Role::Label, name).rect();
            plate.left_top() + egui::vec2(12.0, -12.0)
        }
    }
}

/// Lets everything that is due in the next `secs` happen, and the other threads finish.
fn rest(driver: &mut Driver, harness: &mut Harness<'_>, secs: f64) {
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
}

// The whole of it: a face nobody has named is outlined while the panel is open, a
// press on the outline asks who it is, a name typed is written, the window says so by
// the name, and the face has its plate - on the photo the viewer was on all along.
#[test]
fn a_face_is_named_from_its_outline() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    assert_eq!(faces(&harness), (Vec::new(), 1));
    let face = harness.state().viewer_item().unwrap().unnamed_faces[0].face_id;
    let on_face = face_at(&harness, None);
    harness.event(egui::Event::PointerMoved(on_face));
    harness.event(button(on_face, PointerButton::Primary, true));
    harness.event(button(on_face, PointerButton::Primary, false));
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    assert_eq!(
        dialog(&harness),
        Some((person_picker::Target::Face(face.unwrap()), String::new()))
    );
    assert!(harness.query_by_label("Who is this?").is_some());
    // Nobody is named in this library yet, which the dialog says once it has read so.
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label(person_picker::NO_ONE).is_some());
    typed(&mut driver, &mut harness, "Anna");
    driver.act(&mut harness);
    assert!(harness.query_by_label("New person “Anna”").is_some());
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(dialog(&harness), None, "closed before its write");
    driver.until(&mut harness, "the name written and read", |app| {
        !app.toasts().is_empty() && (app.viewer_item()).is_some_and(|item| item.faces.len() == 1)
    });
    assert_eq!(said(&harness), ["This is Anna."]);
    assert_eq!(faces(&harness), (vec!["Anna".to_owned()], 0));
    assert_eq!(viewing(&harness), Some(ids[1]));
    driver.act(&mut harness);
    // Her plate over the photo, and her name in the panel; no outline left to press.
    assert_eq!(harness.query_all_by_label("Anna").count(), 2);
    assert!(harness.query_by_label("Name this face").is_none());
    // The keys are the viewer's again.
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_ne!(viewing(&harness), Some(ids[1]));
}

// A name someone has, typed in another case, is theirs; and the people are listed, the
// one that matches first, each a row that names at a click.
#[test]
fn a_face_joins_a_person_the_library_has_by_a_typed_name_or_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let lib = looked(&library, &ids[..4], &ids[..1], "Anna");
    assert_eq!(lib.name_items(&ids[1..2], "Hannah").unwrap().named, 1);
    drop(lib);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[2]);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Anna")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Hannah")
            .is_some()
    );
    typed(&mut driver, &mut harness, "ANNA");
    driver.act(&mut harness);
    assert!(harness.query_by_label("Add to Anna").is_some());
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the name written and read", |app| {
        !app.toasts().is_empty() && (app.viewer_item()).is_some_and(|item| item.faces.len() == 1)
    });
    // By the name the library has, not as it was typed.
    assert_eq!(said(&harness), ["This is Anna."]);
    assert_eq!(faces(&harness), (vec!["Anna".to_owned()], 0));

    // The next photo's face, by a click on a row.
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    informed(&mut driver, &mut harness, ids[3]);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "Hannah")
        .click();
    driver.act(&mut harness);
    assert_eq!(dialog(&harness), None);
    driver.until(&mut harness, "the name written and read", |app| {
        app.toasts().len() == 2 || (app.viewer_item()).is_some_and(|item| item.faces.len() == 1)
    });
    driver.until(&mut harness, "it is said", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == "This is Hannah.")
    });
    driver.until(&mut harness, "and read", |app| {
        (app.viewer_item()).is_some_and(|item| item.faces.len() == 1)
    });
    assert_eq!(faces(&harness), (vec!["Hannah".to_owned()], 0));
}

// The other button on the photo: on a face nobody has named, "Name this face…" over
// the photo's own items; on a person's face, "Not {them}"; beside the faces, "Not
// {them}" for each person on the photo. Escape closes the menu and not the viewer.
#[test]
fn the_viewers_menu_is_of_what_the_button_went_down_on() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &ids[..1], "Anna");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);

    informed(&mut driver, &mut harness, ids[1]);
    let on_face = face_at(&harness, None);
    right_click_at(&mut driver, &mut harness, on_face);
    assert!(harness.state().menu_open());
    for item in [
        "Name this face…",
        "Locate in photon",
        "Reveal in file manager",
        "Open in default app",
        "Hide photo",
    ] {
        assert!(harness.query_by_label(item).is_some(), "{item}");
    }
    key(&mut driver, &mut harness, Key::Escape);
    assert!(!harness.state().menu_open());
    assert_eq!(
        viewing(&harness),
        Some(ids[1]),
        "Escape closed the menu alone"
    );
    // Beside the face nothing is offered for it: nobody on this photo is named.
    right_click_at(&mut driver, &mut harness, on_face + egui::vec2(0.0, 200.0));
    assert!(harness.state().menu_open());
    assert!(harness.query_by_label("Name this face…").is_none());
    assert!(harness.query_by_label("Locate in photon").is_some());
    key(&mut driver, &mut harness, Key::Escape);
    // The item opens the dialog, for that face.
    right_click_at(&mut driver, &mut harness, on_face);
    pick_item(&mut driver, &mut harness, "Name this face…");
    stand(&mut driver, &mut harness);
    let face = harness.state().viewer_item().unwrap().unnamed_faces[0].face_id;
    assert_eq!(
        dialog(&harness).map(|(target, _)| target),
        Some(person_picker::Target::Face(face.unwrap()))
    );
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(dialog(&harness), None);
    assert_eq!(
        viewing(&harness),
        Some(ids[1]),
        "Escape closed the dialog alone"
    );

    // Anna's photo: on her face and beside it, "Not Anna".
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    informed(&mut driver, &mut harness, ids[0]);
    let hers = face_at(&harness, Some("Anna"));
    right_click_at(&mut driver, &mut harness, hers + egui::vec2(0.0, 200.0));
    assert!(harness.query_by_label("Not Anna").is_some());
    key(&mut driver, &mut harness, Key::Escape);
    right_click_at(&mut driver, &mut harness, hers);
    assert!(harness.query_by_label("Name this face…").is_none());
    pick_item(&mut driver, &mut harness, "Not Anna");
    driver.until(&mut harness, "her face taken off and read", |app| {
        !app.toasts().is_empty() && (app.viewer_item()).is_some_and(|item| item.faces.is_empty())
    });
    assert_eq!(said(&harness), ["Removed 1 photo from Anna."]);
    assert_eq!(viewing(&harness), Some(ids[0]), "the photo stays on screen");
    // Nobody is on it now: the menu has no "Not" for anyone.
    driver.act(&mut harness);
    right_click_at(&mut driver, &mut harness, hers + egui::vec2(0.0, 200.0));
    assert!(harness.state().menu_open());
    assert!(harness.query_by_label("Not Anna").is_none());
}

// The menu's own items: "Hide photo" hides the photo on screen, which stays there, and
// "Locate in photon" finds it in Hidden - the one view that holds it by then.
#[test]
fn the_viewers_menu_hides_the_photo_and_finds_it_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[2]);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    right_click_at(&mut driver, &mut harness, on_photo);
    pick_item(&mut driver, &mut harness, "Hide photo");
    driver.until(&mut harness, "hidden, as the library says", |app| {
        app.viewer_item().is_some_and(|item| item.hidden) && app.photos() == 5
    });
    assert_eq!(viewing(&harness), Some(ids[2]));
    driver.act(&mut harness);
    right_click_at(&mut driver, &mut harness, on_photo);
    assert!(harness.query_by_label("Unhide photo").is_some());
    pick_item(&mut driver, &mut harness, "Locate in photon");
    assert_eq!(viewing(&harness), None, "the viewer is shut");
    driver.until(&mut harness, "found in Hidden", |app| {
        app.settled().view == GridView::Hidden && app.selected() == [ids[2]]
    });
}

// From a view that is a part of the library, a photo that is not hidden is found in
// All photos: selected, alone.
#[test]
fn locate_in_photon_finds_a_photo_in_all_photos() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 2, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    row(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos", |app| {
        app.settled().view == GridView::Starred && app.photos() == 2
    });
    let id = harness.state().shown()[1];
    informed(&mut driver, &mut harness, id);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    right_click_at(&mut driver, &mut harness, on_photo);
    pick_item(&mut driver, &mut harness, "Locate in photon");
    assert_eq!(viewing(&harness), None);
    driver.until(&mut harness, "found in All photos", |app| {
        app.settled().view == GridView::All && app.photos() == 6 && app.selected() == [id]
    });
}

fn wheel(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(harness);
}

// Under the dialog nothing is the viewer's: a letter is typed into the field and hides
// no photo, an arrow moves the caret and steps to none, and the wheel - which a dialog
// does not take out of the frame as it does the keys - steps to none either. Under the
// viewer's own menu the wheel steps nothing, wherever the pointer is.
#[test]
fn nothing_reaches_the_viewer_under_its_dialog_or_its_menu() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    // The wheel is the viewer's here: a notch is a photo, and one back.
    wheel(&mut driver, &mut harness, on_photo);
    assert_eq!(viewing(&harness), Some(ids[2]));
    key(&mut driver, &mut harness, Key::ArrowLeft);
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[1])
    });
    driver.act(&mut harness);

    // Under the menu, beside it.
    right_click_at(&mut driver, &mut harness, on_photo);
    assert!(harness.state().menu_open());
    wheel(
        &mut driver,
        &mut harness,
        on_photo - egui::vec2(150.0, 150.0),
    );
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().menu_open());
    key(&mut driver, &mut harness, Key::Escape);

    // Under the dialog.
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "h");
    key(&mut driver, &mut harness, Key::ArrowLeft);
    key(&mut driver, &mut harness, Key::I);
    wheel(&mut driver, &mut harness, on_photo);
    // Nor does the wheel with Ctrl zoom the photo behind it, which is no step and no
    // write - nothing the application would have to drop - but the viewer's own state.
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::CTRL,
    });
    driver.act(&mut harness);
    assert!(harness.state().viewer().unwrap().zoom().is_fit());
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(
        dialog(&harness).map(|(_, draft)| draft),
        Some("h".to_owned())
    );
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().viewer_info(), "the panel is as it was");
    assert_eq!(harness.state().photos(), 6, "no photo was hidden");
    // A press on the scrim closes the dialog, and is no press on the photo's menu:
    // beside the dialog, on the viewer's ground.
    let beside = egui::pos2(60.0, 60.0);
    harness.event(egui::Event::PointerMoved(beside));
    harness.event(button(beside, PointerButton::Secondary, true));
    harness.event(button(beside, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(dialog(&harness), None);
    assert!(!harness.state().menu_open());
    assert_eq!(viewing(&harness), Some(ids[1]));
}

// The menu is of the photo it was opened on: when the viewer is shut under it - here
// by a grid of other results that does not hold the photo - the menu goes with it, and
// does not stand over the grid offering to hide a photo nobody sees.
#[test]
fn the_viewers_menu_does_not_outlive_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 2, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    // A photo that is not starred, and the menu on it.
    let starred = harness.state().starred();
    let id = (harness.state().shown().into_iter())
        .find(|id| !starred.contains(id))
        .unwrap();
    informed(&mut driver, &mut harness, id);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    right_click_at(&mut driver, &mut harness, on_photo);
    assert!(harness.state().menu_open());
    // The view is switched from elsewhere, as a step asked before the viewer opened
    // would land: Starred does not hold the photo.
    harness
        .state_mut()
        .go(photon_ui::nav::Step::View(GridView::Starred));
    driver.until(&mut harness, "the starred photos", |app| {
        app.settled().view == GridView::Starred && app.photos() == 2
    });
    assert_eq!(viewing(&harness), None, "other results shut the viewer");
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    assert!(harness.query_by_label("Hide photo").is_none());
}

// The menu is of the photo on screen, and so is what its items do. The other button
// and an arrow key in one frame: the menu was asked for over one photo and the viewer
// is on the next. Whether the menu then stands - the library has said what the next
// photo is - or has closed for want of anything to be of, "Hide photo" hides the photo
// that is on screen, and never the one that was left.
#[test]
fn the_viewers_menu_acts_on_the_photo_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    harness.event(egui::Event::PointerMoved(on_photo));
    driver.act(&mut harness);
    harness.input_mut().events.extend([
        button(on_photo, PointerButton::Secondary, true),
        egui::Event::Key {
            key: Key::ArrowRight,
            physical_key: Some(Key::ArrowRight),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[2]));
    harness.event(button(on_photo, PointerButton::Secondary, false));
    driver.until(&mut harness, "the next photo's facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[2])
    });
    driver.act(&mut harness);
    if !harness.state().menu_open() {
        right_click_at(&mut driver, &mut harness, on_photo);
    }
    pick_item(&mut driver, &mut harness, "Hide photo");
    driver.until(&mut harness, "a photo hidden", |app| app.photos() == 5);
    let shown = harness.state().shown();
    assert!(
        shown.contains(&ids[1]),
        "the photo that was left is as it was"
    );
    assert!(!shown.contains(&ids[2]), "the one on screen is hidden");
}

// A face pressed in the very frame that closes the viewer is asked about nowhere: the
// dialog would stand over the grid, asking who a face is on a photo nobody sees.
#[test]
fn a_face_pressed_in_the_frame_the_viewer_closes_in_is_not_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let on_face = face_at(&harness, None);
    harness.event(egui::Event::PointerMoved(on_face));
    harness.event(button(on_face, PointerButton::Primary, true));
    driver.act(&mut harness);
    // The button up, and Escape, in one frame.
    harness.input_mut().events.extend([
        button(on_face, PointerButton::Primary, false),
        egui::Event::Key {
            key: Key::Escape,
            physical_key: Some(Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None, "Escape closed the viewer");
    driver.act(&mut harness);
    assert_eq!(dialog(&harness), None);
    assert!(harness.query_by_label("Who is this?").is_none());
}

// From the grid: the photos selected are added to a person, each by its one unnamed
// face, and the window says what came of it - the photo with no face among them.
#[test]
fn photos_are_added_to_a_person_from_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let lib = looked(&library, &ids[..4], &ids[..1], "Anna");
    drop(lib);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.until(&mut harness, "the people read", |app| {
        !app.collections().people.is_empty()
    });
    driver.settle(&mut harness);
    // Three photos with a face nobody has named, and one photon found no face on.
    let picked = [ids[1], ids[2], ids[3], ids[4]];
    let offsets: Vec<usize> = picked.iter().map(|id| at(&harness, *id)).collect();
    pick(&mut driver, &mut harness, offsets[0], Modifiers::NONE);
    for offset in &offsets[1..] {
        pick(&mut driver, &mut harness, *offset, Modifiers::COMMAND);
    }
    assert_eq!(harness.state().selected().len(), 4);
    let on_tile = harness.state().tile_rect(offsets[0]).unwrap().center();
    right_click_at(&mut driver, &mut harness, on_tile);
    pick_item(&mut driver, &mut harness, "Add 4 photos to a person…");
    stand(&mut driver, &mut harness);
    let mut asked = picked.to_vec();
    asked.sort_unstable();
    let target = dialog(&harness).map(|(target, _)| match target {
        person_picker::Target::Items(mut ids) => {
            ids.sort_unstable();
            ids
        }
        person_picker::Target::Face(_) => Vec::new(),
    });
    assert_eq!(target, Some(asked));
    assert!(harness.query_by_label("Add 4 photos to…").is_some());
    typed(&mut driver, &mut harness, "anna");
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(dialog(&harness), None);
    driver.until(&mut harness, "it is said", |app| !app.toasts().is_empty());
    assert_eq!(
        said(&harness),
        ["Added 3 photos to Anna. 1 has no unnamed face."]
    );
    // And she has four photos now, as the sidebar counts them.
    driver.until(&mut harness, "her count", |app| {
        (app.collections().people.iter()).any(|person| person.name == "Anna" && person.count == 4)
    });
}

// The keys walk the dialog over the viewer as they do over the grid: Tab from the field
// is the first person's row, an arrow from there the next one's, and Enter names the
// face theirs. The viewer calls off egui's walk of the keyboard for itself, and must
// not call the dialog's off with it.
#[test]
fn the_keys_walk_the_dialog_over_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let lib = looked(&library, &ids[..3], &ids[..1], "Anna");
    assert_eq!(lib.name_items(&ids[1..2], "Hannah").unwrap().named, 1);
    drop(lib);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[2]);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    // The two people, in the order the dialog lists them.
    let top = |name: &str| {
        (harness.get_by_role_and_label(Role::Button, name))
            .rect()
            .top()
    };
    let second = if top("Anna") < top("Hannah") {
        "Hannah"
    } else {
        "Anna"
    };
    key(&mut driver, &mut harness, Key::Tab);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::ArrowDown);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(dialog(&harness), None);
    driver.until(&mut harness, "the name written and read", |app| {
        !app.toasts().is_empty() && (app.viewer_item()).is_some_and(|item| item.faces.len() == 1)
    });
    assert_eq!(said(&harness), [format!("This is {second}.")]);
}

// A face that is gone by the time its name is typed - here with every face photon had
// found, the switch turned off behind the dialog - is said to be gone: nothing is
// written, and no person is made of the name.
#[test]
fn a_name_for_a_face_that_is_gone_is_said_to_have_gone_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    let engine = harness.state().engine().clone();
    photon_engine::commands::set_face_detection(&engine, false).unwrap();
    typed(&mut driver, &mut harness, "Anna");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "it is said", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), [person_picker::FACE_GONE]);
    assert_eq!(
        photon_engine::commands::named_people(&engine).unwrap(),
        Vec::new()
    );
}

// In the frame a dialog is first drawn in, egui does not yet keep the pointer from what
// lies under it: a click that is over within that frame - down and up together, as a
// long frame hands them over - on the star in the viewer's bar is the star's to egui.
// What the viewer answers under a dialog is dropped, as the shell's is. (A fixture's
// folders are on a drive that is not there: a star written is a star refused, and
// said.)
#[test]
fn a_click_in_the_frame_the_dialog_is_first_drawn_in_presses_nothing_in_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, &ids[..3], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let star = harness.get_by_label("Star").rect().center();
    let on_face = face_at(&harness, None);
    harness.event(egui::Event::PointerMoved(on_face));
    harness.event(button(on_face, PointerButton::Primary, true));
    harness.event(button(on_face, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(dialog(&harness).is_some());
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(star),
        button(star, PointerButton::Primary, true),
        button(star, PointerButton::Primary, false),
    ]);
    driver.act(&mut harness);
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(
        said(&harness),
        Vec::<String>::new(),
        "no star was asked for"
    );
}
