//! A folder dropped on the window, through the whole application and the real engine:
//! over libraries made for the test, whose folders hold real photos and are scanned.
//! Every folder and file is under a temporary directory the test made.

use crate::common::{Driver, Harness, drop_on, hold_over, over, photos_in, said};
use eframe::egui::{self, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use photon_ui::{
    app::App,
    dialogs::Dialog,
    export_dialog::Dest,
    folder_drop::{ASIDE, CARD},
};
use std::path::Path;

/// The library that watches nothing, once it says so.
fn empty<'a>(dir: &Path) -> (Harness<'a>, Driver) {
    let (mut harness, mut driver, _) = over(dir, None);
    driver.until(&mut harness, "the empty library's panel", |app| {
        app.panel()
            .is_some_and(|panel| panel.text.starts_with("Choose a folder"))
    });
    (harness, driver)
}

fn watched(app: &App) -> Vec<String> {
    (app.watched().iter())
        .map(|folder| folder.path.clone())
        .collect()
}

/// The path photon keeps for `folder`: as the engine canonicalizes it.
fn kept(folder: &Path) -> String {
    photon_core::paths::canonicalize(folder)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

// The whole of it: the empty library offers the drop, the card is up while the folder is
// held over the window, and let go of there the folder is watched, scanned and shown,
// under one message that names it.
#[test]
fn a_folder_dropped_on_the_window_is_watched_scanned_and_shown() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg", "b.jpg", "c.jpg"]);
    let (mut harness, mut driver) = empty(dir.path());
    assert!(harness.query_by_label(ASIDE).is_some());
    assert!(!harness.state().hovering());

    hold_over(&mut driver, &mut harness, &[&holiday]);
    assert!(harness.state().hovering());
    driver.act(&mut harness);
    assert!(harness.query_by_label(CARD).is_some(), "the card says so");
    assert_eq!(watched(harness.state()), Vec::<String>::new());

    drop_on(&mut driver, &mut harness, &[&holiday]);
    assert!(!harness.state().hovering());
    assert!(harness.query_by_label(CARD).is_none());
    driver.until(&mut harness, "the drop answered", |app| {
        !app.toasts().is_empty()
    });
    // In the frame of the answer the folder is in the list on hand - the list read
    // again arrives a moment later - and the empty library no longer asks for one.
    assert_eq!(said(&harness), ["Watching “Holiday”"]);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    assert!(
        (harness.state().panel()).is_none_or(|panel| !panel.text.starts_with("Choose a folder")),
        "{:?}",
        harness.state().panel()
    );
    driver.until(&mut harness, "its photos shown", |app| app.photos() == 3);
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    assert_eq!(said(&harness), ["Watching “Holiday”"]);
    assert!(harness.state().panel().is_none());
}

// The engine is what knows a folder from a file, and one folder inside another: each
// path is offered in its turn, and the drop is reported once - what is watched, and the
// first refusal with a count of the rest.
#[test]
fn what_the_engine_refuses_of_a_drop_is_said_once_and_the_rest_is_watched() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg", "b.jpg"]);
    let inner = photos_in(&holiday, "Day 2", &["c.jpg"]);
    let (photo, other) = (holiday.join("a.jpg"), holiday.join("b.jpg"));
    let (mut harness, mut driver) = empty(dir.path());
    drop_on(
        &mut driver,
        &mut harness,
        &[&photo, &holiday, &inner, &other],
    );
    driver.until(&mut harness, "the drop answered", |app| {
        app.toasts().len() == 2
    });
    let said = said(&harness);
    assert_eq!(said[0], "Watching “Holiday”");
    assert!(
        said[1].contains("a.jpg is a file, not a folder") && said[1].ends_with("(and 2 more)"),
        "{said:?}"
    );
    driver.until(&mut harness, "its photos shown", |app| app.photos() == 3);
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    assert_eq!(crate::common::said(&harness).len(), 2);
}

// Nothing watched, and nothing claimed to be.
#[test]
fn a_file_dropped_alone_is_refused_in_the_engines_words() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg"]);
    let photo = holiday.join("a.jpg");
    let (mut harness, mut driver) = empty(dir.path());
    drop_on(&mut driver, &mut harness, &[&photo]);
    driver.until(&mut harness, "the refusal", |app| !app.toasts().is_empty());
    let said = said(&harness);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("is a file, not a folder"), "{said:?}");
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), Vec::<String>::new());
    assert_eq!(harness.state().photos(), 0);
}

// A folder already watched is answered as itself, not as an error: said again, and
// there is still one of it.
#[test]
fn a_folder_already_watched_is_watched_once() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg", "b.jpg"]);
    let (mut harness, mut driver, _) = over(dir.path(), Some(&holiday));
    driver.until(&mut harness, "its photos shown", |app| app.photos() == 2);
    driver.settle(&mut harness);
    drop_on(&mut driver, &mut harness, &[&holiday]);
    driver.until(&mut harness, "the drop answered", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Watching “Holiday”"]);
    // In the frame of the answer already, and not only once the list is read again.
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    assert_eq!(harness.state().photos(), 2);
}

// A drag that carries no path - text out of another program - is not photon's to
// announce, and a drop of nothing is nothing.
#[test]
fn a_drag_that_carries_no_path_is_not_announced() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = empty(dir.path());
    driver.held.push(egui::HoveredFile::default());
    driver.act(&mut harness);
    assert!(!harness.state().hovering());
    drop_on(&mut driver, &mut harness, &[]);
    driver.settle(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// On a Wayland window no drop arrives, and none is offered: not in the empty library's
// panel, and nothing is made of what the system never sends.
#[test]
fn where_no_drop_can_arrive_none_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg"]);
    let made = photon_ui::dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(crate::common::REFRESH as f32)
        .build_eframe(move |cc| App::new(cc, made, None).unwrap().without_drops());
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the empty library's panel", |app| {
        app.panel().is_some()
    });
    assert!(harness.query_by_label("Add folder…").is_some());
    assert!(harness.query_by_label(ASIDE).is_none());
    hold_over(&mut driver, &mut harness, &[&holiday]);
    assert!(!harness.state().hovering());
    drop_on(&mut driver, &mut harness, &[&holiday]);
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), Vec::<String>::new());
    assert_eq!(said(&harness), Vec::<String>::new());
}

fn button(pos: egui::Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

// The export's dialog is asking for a folder, and a folder dropped then is its answer:
// it is the destination, asked about as a typed one is, and it is not watched.
#[test]
fn under_the_exports_dialog_a_dropped_folder_is_its_destination_and_is_not_watched() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg", "b.jpg"]);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let (mut harness, mut driver, _) = over(dir.path(), Some(&holiday));
    driver.until(&mut harness, "its photos shown", |app| {
        app.photos() == 2 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let at = harness.state().tile_rect(0).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(&mut harness);
    harness.get_by_label("Export photo…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(matches!(harness.state().dialog(), Some(Dialog::Export(_))));

    // Held over the window, the card says where it would go.
    hold_over(&mut driver, &mut harness, &[&out]);
    assert!(harness.state().hovering());
    driver.act(&mut harness);
    assert!(
        harness
            .query_by_label(photon_ui::folder_drop::CARD_EXPORT)
            .is_some()
    );
    assert!(harness.query_by_label(CARD).is_none());
    drop_on(&mut driver, &mut harness, &[&out, &holiday]);
    let dropped = out.to_string_lossy().into_owned();
    driver.until(&mut harness, "the folder passed", |app| {
        matches!(app.dialog(), Some(Dialog::Export(export))
            if export.path() == dropped && matches!(export.dest(), Dest::Passed(_)))
    });
    driver.settle(&mut harness);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// egui asks for the next frame at once for as long as it is told that something is held
// over the window, so it is not told (`App::raw_input_hook`): a folder held still over
// the window is a still window, with the card up.
#[test]
fn a_window_with_a_folder_held_over_it_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg"]);
    let (mut harness, mut driver) = empty(dir.path());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0, "still before");
    hold_over(&mut driver, &mut harness, &[&holiday]);
    assert!(harness.state().hovering());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.query_by_label(CARD).is_some());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// The system does not always say that a drag has left: a folder carried quickly across
// the window can be reported held after the report that it is gone, and is then held
// for good as far as the window is told. A pointer that moves in the window is proof
// that no drag is over it - a drag's source has the pointer for as long as it lasts -
// and takes the card down; the next drag puts it up again.
#[test]
fn a_pointer_in_the_window_takes_down_a_card_the_system_left_up() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg"]);
    let other = photos_in(dir.path(), "Other", &["b.jpg"]);
    let (mut harness, mut driver) = empty(dir.path());
    hold_over(&mut driver, &mut harness, &[&holiday]);
    assert!(harness.state().hovering());
    // The system goes on saying it is held.
    harness.event(egui::Event::PointerMoved(egui::pos2(500.0, 400.0)));
    driver.act(&mut harness);
    assert!(!harness.state().hovering());
    driver.act(&mut harness);
    assert!(harness.query_by_label(CARD).is_none());
    // And stays down, frame after frame, with the pointer at rest.
    driver.act(&mut harness);
    assert!(!harness.state().hovering());
    // Another drag is one more path held than was left over, and a button pressed in
    // the window ends it as a move does.
    hold_over(&mut driver, &mut harness, &[&holiday, &other]);
    assert!(harness.state().hovering());
    harness.event(button(
        egui::pos2(500.0, 400.0),
        PointerButton::Primary,
        true,
    ));
    driver.act(&mut harness);
    assert!(!harness.state().hovering());
    harness.event(button(
        egui::pos2(500.0, 400.0),
        PointerButton::Primary,
        false,
    ));
    driver.act(&mut harness);
    // And what is dropped is acted on, whatever was left over before it.
    hold_over(&mut driver, &mut harness, &[&holiday, &other, &other]);
    assert!(harness.state().hovering());
    drop_on(&mut driver, &mut harness, &[&other]);
    assert!(!harness.state().hovering());
    driver.until(&mut harness, "the drop answered", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Watching “Other”"]);
}

// One folder twice in a drop - by its path, and by a link to it - is one folder
// watched, and is said as one.
#[test]
fn a_folder_twice_in_one_drop_is_one_folder() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg"]);
    let (mut harness, mut driver) = empty(dir.path());
    #[cfg(unix)]
    let again = {
        let link = dir.path().join("Link");
        std::os::unix::fs::symlink(&holiday, &link).unwrap();
        link
    };
    #[cfg(not(unix))]
    let again = holiday.clone();
    drop_on(&mut driver, &mut harness, &[&holiday, &holiday, &again]);
    driver.until(&mut harness, "the drop answered", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Watching “Holiday”"]);
    assert_eq!(watched(harness.state()), [kept(&holiday)]);
}

// A drop is the window's whatever is open: under a photo's menu a folder dropped is
// watched as under nothing, and the menu is left as it was.
#[test]
fn a_folder_dropped_under_an_open_menu_is_watched() {
    let dir = tempfile::tempdir().unwrap();
    let holiday = photos_in(dir.path(), "Holiday", &["a.jpg", "b.jpg"]);
    let other = photos_in(dir.path(), "Other", &["c.jpg"]);
    let (mut harness, mut driver, _) = over(dir.path(), Some(&holiday));
    driver.until(&mut harness, "its photos shown", |app| {
        app.photos() == 2 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let at = harness.state().tile_rect(0).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    hold_over(&mut driver, &mut harness, &[&other]);
    assert!(harness.state().hovering());
    driver.act(&mut harness);
    assert!(harness.query_by_label(CARD).is_some());
    drop_on(&mut driver, &mut harness, &[&other]);
    driver.until(&mut harness, "the drop answered", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Watching “Other”"]);
    assert_eq!(watched(harness.state()).len(), 2);
    assert!(harness.state().menu_open(), "the menu is where it was");
}
