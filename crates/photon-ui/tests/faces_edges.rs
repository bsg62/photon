//! Faces, where a review of the pull request that brought them found the edges: two
//! things in one frame, a dialog or a menu that outlives what it is of, a read that
//! fails, the second click of a double click. Five of these failed on the branch as it
//! was first written; the other four hold rules no test held until then.
//!
//! Every write goes into a library the test made, in a temporary directory.

use crate::common::{Driver, Harness, library, opened, photo_ids, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, accesskit::Role};
use egui_kittest::kittest::Queryable;
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    grid::GridView,
    library::Library,
};
use photon_ui::{app::App, dialogs::Dialog, fixture::Fixture, person_picker};

fn detection(rect: [f64; 4]) -> Detection {
    let [left, top, right, bottom] = rect;
    let (w, h) = (right - left, bottom - top);
    Detection {
        rect: Rect {
            left,
            top,
            right,
            bottom,
        },
        landmarks: [
            ((left + 0.3 * w) as f32, (top + 0.4 * h) as f32),
            ((left + 0.7 * w) as f32, (top + 0.4 * h) as f32),
            ((left + 0.5 * w) as f32, (top + 0.55 * h) as f32),
            ((left + 0.35 * w) as f32, (top + 0.75 * h) as f32),
            ((left + 0.65 * w) as f32, (top + 0.75 * h) as f32),
        ],
        score: 0.9,
    }
}

/// photon has looked at every photo of `library` and found on each photo what `faces`
/// says of it (by id), and the user has named the one face on each of `named`.
fn looked(
    library: &Fixture,
    faces: impl Fn(i64) -> Vec<[f64; 4]>,
    named: &[i64],
    name: &str,
) -> Library {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.set_face_detection(true).unwrap();
    let photos = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let found = faces(photo.id).into_iter().map(detection).collect();
            (photo, found)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    if !named.is_empty() {
        let came = lib.name_items(named, name).unwrap();
        assert_eq!(came.named, named.len() as i64);
    }
    lib
}

const ONE: [f64; 4] = [0.3, 0.2, 0.6, 0.6];

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key_down(key: Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
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

fn at(harness: &Harness<'_>, id: i64) -> usize {
    (harness.state().shown().iter())
        .position(|shown| *shown == id)
        .expect("the photo is in the grid")
}

fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

/// The viewer on `id`, its panel open, the library's word and the preview there.
fn informed(driver: &mut Driver, harness: &mut Harness<'_>, id: i64) {
    let offset = at(harness, id);
    pick(driver, harness, offset);
    pick(driver, harness, offset);
    assert_eq!(viewing(harness), Some(id), "the viewer is open");
    driver.until(harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    key(driver, harness, Key::I);
    driver.until(harness, "its preview", |app: &App| app.preview_of(id));
    driver.act(harness);
}

fn stand(driver: &mut Driver, harness: &mut Harness<'_>) {
    for _ in 0..3 {
        driver.act(harness);
    }
    driver.frames_in(harness, 0.4);
}

fn dialog(harness: &Harness<'_>) -> Option<(person_picker::Target, String)> {
    match harness.state().dialog() {
        Some(Dialog::Person(picker)) => Some((picker.target().clone(), picker.draft().to_owned())),
        _ => None,
    }
}

fn names_on(engine: &photon_engine::engine::Engine, id: i64) -> Vec<String> {
    let item = photon_engine::commands::viewer_item(engine, id).unwrap();
    item.faces.into_iter().map(|face| face.name).collect()
}

// A press on an outline and a step in one frame - an arrow key held to go through the
// photos repeats thirty times a second, a wheel is turned - open no dialog: it would
// stand over the *next* photo, asking "Who is this?" of a face on the one that was
// left, over a face of the next one's own in the same place. A name typed then is
// written to no face that is not on screen.
#[test]
fn a_name_typed_into_the_dialog_is_written_to_a_face_of_the_photo_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let with_face = ids[..3].to_vec();
    looked(
        &library,
        |id| {
            if with_face.contains(&id) {
                vec![ONE]
            } else {
                Vec::new()
            }
        },
        &[],
        "",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let on_face = harness.get_by_label("Name this face").rect().center();
    harness.event(egui::Event::PointerMoved(on_face));
    harness.event(button(on_face, PointerButton::Primary, true));
    driver.act(&mut harness);
    // The button up, and the arrow key, in one frame.
    (harness.input_mut().events).extend([
        button(on_face, PointerButton::Primary, false),
        key_down(Key::ArrowRight),
    ]);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[2]), "the arrow stepped");
    stand(&mut driver, &mut harness);
    driver.until(&mut harness, "the next photo's facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[2])
    });
    driver.act(&mut harness);
    if dialog(&harness).is_none() {
        // No dialog over a photo the face is not on: nothing more to hold.
        return;
    }
    // The dialog stands over the next photo, which has an unnamed face where the one
    // pressed was. Whoever reads "Who is this?" there names the face they see.
    assert!(harness.query_by_label("Who is this?").is_some());
    typed(&mut driver, &mut harness, "Anna");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "it is said", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["This is Anna."]);
    let engine = harness.state().engine().clone();
    assert_eq!(
        (names_on(&engine, ids[2]), names_on(&engine, ids[1])),
        (vec!["Anna".to_owned()], Vec::new()),
        "the photo on screen under the dialog, and the photo that was left"
    );
}

// The dialog that asks who a face is does not outlive the viewer: other results that
// do not hold the photo shut the viewer behind it (`adopt`), and the dialog goes with
// it, as the viewer's menu does.
#[test]
fn the_dialog_that_names_a_face_does_not_outlive_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 2, 0);
    let ids = photo_ids(&library);
    looked(&library, |_| vec![ONE], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    let starred = harness.state().starred();
    let id = *ids.iter().find(|id| !starred.contains(id)).unwrap();
    informed(&mut driver, &mut harness, id);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    assert!(dialog(&harness).is_some());
    // As a step asked before the viewer opened would land.
    (harness.state_mut()).go(photon_ui::nav::Step::View(GridView::Starred));
    driver.until(&mut harness, "the starred photos", |app| {
        app.settled().view == GridView::Starred && app.photos() == 2
    });
    assert_eq!(viewing(&harness), None, "other results shut the viewer");
    driver.act(&mut harness);
    assert_eq!(
        dialog(&harness),
        None,
        "the dialog asks who a face is on a photo nobody sees"
    );
}

// A menu asked for in the frame of a step is not opened. Opened, it stood over the next
// photo for as long as the library took to say what that photo is and then - a race
// between that answer and the next frame, which the answer wins here, nothing being
// drawn until the other threads rest - either closed or became a menu of the next
// photo: asked for on Anna's face of one photo, a menu of the next one's Anna.
#[test]
fn a_menu_asked_for_in_the_frame_of_a_step_closes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let with_face = ids[..3].to_vec();
    looked(
        &library,
        |id| {
            if with_face.contains(&id) {
                vec![ONE]
            } else {
                Vec::new()
            }
        },
        &ids[..2],
        "Anna",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[0]);
    let plate = harness.get_by_role_and_label(Role::Label, "Anna").rect();
    let hers = plate.left_top() + egui::vec2(12.0, -12.0);
    harness.event(egui::Event::PointerMoved(hers));
    driver.act(&mut harness);
    (harness.input_mut().events).extend([
        button(hers, PointerButton::Secondary, true),
        key_down(Key::ArrowRight),
    ]);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[1]));
    // The library answers before the next frame is drawn.
    driver.quiet();
    harness.event(button(hers, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        !harness.state().menu_open(),
        "the menu asked for on the photo that was left stands over the next: {:?}",
        harness.query_by_label("Not Anna").is_some()
    );
}

// A double click on a person's row names one face and asks about no other: the first
// click names and closes the dialog, the second lands on whatever the dialog had
// covered - here the outline of another face nobody has named. (`DialogView::deaf` is
// the same double click, the other way round.)
#[test]
fn a_double_click_on_a_persons_row_names_one_face_and_asks_about_no_other() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let (anna, two) = (ids[0], ids[1]);
    const SMALL: [f64; 4] = [0.02, 0.03, 0.10, 0.15];
    const BIG: [f64; 4] = [0.15, 0.05, 0.98, 0.98];
    looked(
        &library,
        |id| {
            if id == anna {
                vec![ONE]
            } else if id == two {
                vec![SMALL, BIG]
            } else {
                Vec::new()
            }
        },
        &[anna],
        "Anna",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, two);
    let mut outlines: Vec<egui::Rect> = (harness.query_all_by_label("Name this face"))
        .map(|node| node.rect())
        .collect();
    assert_eq!(outlines.len(), 2);
    outlines.sort_by(|a, b| a.area().total_cmp(&b.area()));
    let (small, big) = (outlines[0], outlines[1]);
    let unnamed = &harness.state().viewer_item().unwrap().unnamed_faces;
    let small_id = (unnamed.iter())
        .find(|face| face.left < 0.1)
        .and_then(|face| face.face_id)
        .unwrap();
    harness.event(egui::Event::PointerMoved(small.center()));
    harness.event(button(small.center(), PointerButton::Primary, true));
    harness.event(button(small.center(), PointerButton::Primary, false));
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        dialog(&harness).map(|(target, _)| target),
        Some(person_picker::Target::Face(small_id))
    );
    let row = harness.get_by_role_and_label(Role::Button, "Anna").rect();
    let on_row = egui::pos2(row.left() + 20.0, row.center().y);
    assert!(
        big.contains(on_row),
        "the test's row {row:?} lies over its other outline {big:?}"
    );
    // The first click of the two.
    harness.event(egui::Event::PointerMoved(on_row));
    harness.event(button(on_row, PointerButton::Primary, true));
    harness.event(button(on_row, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert_eq!(dialog(&harness), None, "the first click named the face");
    // The second, a tenth of a second on.
    driver.frames_in(&mut harness, 0.1);
    driver.act(&mut harness);
    harness.event(button(on_row, PointerButton::Primary, true));
    harness.event(button(on_row, PointerButton::Primary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        dialog(&harness).map(|(target, _)| target),
        None,
        "the second click of the double click asked about another face"
    );
}

// A still window draws nothing: with the outlines shown, with the pointer resting on
// one, with the viewer's menu open, with the dialog open (the caret's blink is off under
// `egui_kittest`).
#[test]
fn a_still_viewer_with_faces_a_menu_or_the_dialog_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    looked(&library, |_| vec![ONE], &[], "");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let still = |driver: &mut Driver, harness: &mut Harness<'_>, what: &str| {
        driver.settle(harness);
        driver.frames_in(harness, 1.0);
        driver.settle(harness);
        assert_eq!(driver.frames_in(harness, 10.0), 0, "{what}");
    };
    still(&mut driver, &mut harness, "the outlines shown");
    let on_face = harness.get_by_label("Name this face").rect().center();
    harness.event(egui::Event::PointerMoved(on_face));
    driver.act(&mut harness);
    still(
        &mut driver,
        &mut harness,
        "the pointer resting on an outline",
    );
    harness.event(button(on_face, PointerButton::Secondary, true));
    harness.event(button(on_face, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    still(&mut driver, &mut harness, "the menu open");
    harness.get_by_label("Name this face…").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    assert!(dialog(&harness).is_some());
    still(&mut driver, &mut harness, "the dialog open");
}

// A read of the people that fails says nothing of them: not "No one is named yet.", of
// a library with a person in it, and not that her name would be a new person's. The
// read is made to fail by the seam the launch's tests use, a second connection - here
// one that renames the table out from under it.
#[test]
fn a_read_of_the_people_that_fails_says_nothing_of_them() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let with_face = ids[..3].to_vec();
    looked(
        &library,
        |id| {
            if with_face.contains(&id) {
                vec![ONE]
            } else {
                Vec::new()
            }
        },
        &ids[..1],
        "Anna",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    driver.settle(&mut harness);
    // The table is out of the way for as long as the dialog reads.
    let conn = rusqlite::Connection::open(&library.dirs().db_path).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    conn.execute_batch("ALTER TABLE people RENAME TO people_away")
        .unwrap();
    let engine = harness.state().engine().clone();
    assert!(
        photon_engine::commands::named_people(&engine).is_err(),
        "the read is refused"
    );
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "anna");
    driver.act(&mut harness);
    let no_one = harness.query_by_label(person_picker::NO_ONE).is_some();
    let no_match = harness.query_by_label(person_picker::NO_MATCH).is_some();
    let new_person = harness.query_by_label("New person “anna”").is_some();
    conn.execute_batch("ALTER TABLE people_away RENAME TO people")
        .unwrap();
    assert_eq!(
        (no_one, no_match, new_person),
        (false, false, false),
        "said of people who were never read: no one named, no one matching, a new person"
    );
}

/// The other button, down and up at `at`, and the frame its menu is first drawn in.
fn right_click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(harness);
    driver.act(harness);
}

// An outline is where the library says the face is. The view's tests hand it rectangles
// made by hand, and the application's other tests press an outline wherever it is
// drawn: this one holds `viewer_faces::drawn`, which takes the library's four numbers
// in their order.
#[test]
fn an_outline_is_where_the_library_says_the_face_is() {
    use photon_ui::viewer_layout::{Frame, contained, photo_area};
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let with_face = ids[..3].to_vec();
    looked(
        &library,
        |id| {
            if with_face.contains(&id) {
                vec![ONE]
            } else {
                Vec::new()
            }
        },
        &ids[..1],
        "Anna",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    // Where the face is, by the library's word and the window's size alone.
    let expected = |harness: &Harness<'_>| {
        let window = harness.ctx.content_rect();
        let item = harness.state().viewer_item().unwrap();
        let area = photo_area(
            Frame {
                left: window.left(),
                top: window.top(),
                width: window.width(),
                height: window.height(),
            },
            true,
        );
        let picture = contained(item.width as f32, item.height as f32, area);
        egui::Rect::from_min_max(
            egui::pos2(
                picture.left + ONE[0] as f32 * picture.width,
                picture.top + ONE[1] as f32 * picture.height,
            ),
            egui::pos2(
                picture.left + ONE[2] as f32 * picture.width,
                picture.top + ONE[3] as f32 * picture.height,
            ),
        )
    };
    let near = |a: Pos2, b: Pos2| (a - b).length() < 3.0;

    // A face nobody has named: its outline.
    informed(&mut driver, &mut harness, ids[1]);
    let outline = harness.get_by_label("Name this face").rect();
    let face = expected(&harness);
    assert!(
        near(outline.min, face.min) && near(outline.max, face.max),
        "the outline {outline:?} is not on the face {face:?}"
    );
    // A named one: its plate begins at the outline's left bottom corner.
    key(&mut driver, &mut harness, Key::Escape);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[0]);
    let plate = harness.get_by_role_and_label(Role::Label, "Anna").rect();
    let face = expected(&harness);
    assert!(
        near(plate.left_top(), face.left_bottom()),
        "the plate {plate:?} is not under the face {face:?}"
    );
}

// "Unhide photo" brings the photo back: the item is picked, where the menu's other test
// only reads it.
#[test]
fn unhide_photo_in_the_viewers_menu_brings_the_photo_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[2]);
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 0.0);
    right_click_at(&mut driver, &mut harness, on_photo);
    harness.get_by_label("Hide photo").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "hidden, as the library says", |app| {
        app.viewer_item().is_some_and(|item| item.hidden) && app.photos() == 5
    });
    driver.act(&mut harness);
    right_click_at(&mut driver, &mut harness, on_photo);
    harness.get_by_label("Unhide photo").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "back, as the library says", |app| {
        app.viewer_item().is_some_and(|item| !item.hidden) && app.photos() == 6
    });
    assert_eq!(viewing(&harness), Some(ids[2]));
}

// A letter and the Enter after it in one frame, through the application: the view's
// test holds that the dialog answers with the letter, this one that the name written
// has it.
#[test]
fn a_letter_and_enter_in_one_frame_name_with_the_letter_through_the_application() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    let with_face = ids[..3].to_vec();
    looked(
        &library,
        |id| {
            if with_face.contains(&id) {
                vec![ONE]
            } else {
                Vec::new()
            }
        },
        &[],
        "",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    harness.get_by_label("Name this face").click();
    driver.act(&mut harness);
    stand(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "Ann");
    (harness.input_mut().events).extend([egui::Event::Text("a".to_owned()), key_down(Key::Enter)]);
    driver.act(&mut harness);
    assert_eq!(dialog(&harness), None);
    driver.until(&mut harness, "it is said", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["This is Anna."]);
}

// And a double click on an item of the viewer's menu that lies over an outline: the
// first click picks the item and closes the menu, the second is on the outline the item
// had covered. It asks about no face; a click there a moment later does.
#[test]
fn a_double_click_on_a_menus_item_over_an_outline_asks_about_no_face() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    const BIG: [f64; 4] = [0.05, 0.05, 0.95, 0.95];
    looked(
        &library,
        |id| if id == ids[1] { vec![BIG] } else { Vec::new() },
        &[],
        "",
    );
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, ids[1]);
    let outline = harness.get_by_label("Name this face").rect();
    right_click_at(
        &mut driver,
        &mut harness,
        outline.left_top() + egui::vec2(12.0, 12.0),
    );
    let item = harness.get_by_label("Hide photo").rect().center();
    assert!(
        outline.contains(item),
        "the test's item {item:?} lies over its outline {outline:?}"
    );
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true));
    harness.event(button(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(
        !harness.state().menu_open(),
        "the first click picked the item"
    );
    // The second, a tenth of a second on.
    driver.frames_in(&mut harness, 0.1);
    driver.act(&mut harness);
    harness.event(button(item, PointerButton::Primary, true));
    harness.event(button(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        dialog(&harness),
        None,
        "the second click asked about the face"
    );
    // Once a double click's time is over, a click on the outline is a click on it.
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    harness.event(button(item, PointerButton::Primary, true));
    harness.event(button(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(dialog(&harness).is_some());
}
