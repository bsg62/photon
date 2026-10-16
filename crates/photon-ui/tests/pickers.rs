//! The system's folder picker, through the whole application: "Choose…" beside the
//! export's destination and beside Settings' folder field, and the empty library's "Add
//! folder…". The picker is a stand-in (`common::Picker`) - a portal or a picker is
//! something a CI runner has none of - and every folder it chooses is one the test made.

use crate::common::{Driver, Harness, Picker, REFRESH, photos_in};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_ui::{
    app::App,
    dialogs::Dialog,
    dirs,
    export_dialog::{CHOOSE, Dest},
    platform::Picked,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// The application over a library of its own under `dir`, watching `pictures` when given,
/// with `picker` for the system's folder picker.
fn launch<'a>(dir: &Path, pictures: Option<PathBuf>, picker: Arc<Picker>) -> (Harness<'a>, Driver) {
    let dirs = dirs::within(&dir.join("data"), &dir.join("cache"));
    let harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, pictures).unwrap())
                .with_videos(crate::common::no_videos())
                .with_platform(picker)
        });
    let driver = Driver::new(&harness);
    (harness, driver)
}

/// A press on what AccessKit calls `label`, a double click's time after the last.
fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    driver.frames_in(harness, 0.4);
    driver.seen(harness, label);
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// What Settings' folder field holds, while it is open.
fn adding(harness: &Harness<'_>) -> Option<(String, Dest, bool)> {
    match harness.state().dialog() {
        Some(Dialog::Settings(settings)) => (settings.adding.as_ref()).map(|adding| {
            (
                adding.path().to_owned(),
                adding.dest().clone(),
                adding.choosing,
            )
        }),
        _ => None,
    }
}

/// The empty library, the "Add folder…" of its panel pressed: the picker and Settings.
fn added<'a>(dir: &Path, picker: Arc<Picker>) -> (Harness<'a>, Driver) {
    let (mut harness, mut driver) = launch(dir, None, picker);
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    (harness, driver)
}

fn passed(folder: &Path) -> Dest {
    Dest::Passed(folder.display().to_string())
}

// The empty library's "Add folder…" opens the system's picker at once, over Settings'
// folders with the field open; the folder chosen is put in the field and asked about as a
// typed one is.
#[test]
fn add_folder_opens_the_picker_and_the_folder_chosen_is_in_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = dir.path().join("Holidays");
    std::fs::create_dir_all(&chosen).unwrap();
    let chosen = photon_core::paths::canonicalize(&chosen).unwrap();
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(chosen.clone());
    let (mut harness, mut driver) = added(dir.path(), picker.clone());
    driver.until(&mut harness, "the folder asked about", |app| {
        matches!(app.dialog(), Some(Dialog::Settings(settings))
            if settings.adding.as_ref().is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_))))
    });
    let (path, dest, choosing) = adding(&harness).unwrap();
    assert_eq!(path, chosen.display().to_string());
    assert_eq!(dest, passed(&chosen));
    assert!(!choosing);
    assert_eq!(*picker.asked.lock().unwrap(), ["Add a folder to photon"]);
}

// The picker is one at a time: while it is open, "Choose…" takes no press and asks
// nothing; and a folder chosen for a dialog that closed meanwhile is put into no other.
#[test]
fn a_folder_chosen_for_a_dialog_that_closed_goes_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = dir.path().join("Holidays");
    std::fs::create_dir_all(&chosen).unwrap();
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(chosen.clone());
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    let (mut harness, mut driver) = added(dir.path(), picker.clone());
    driver.until(&mut harness, "the picker open", |_| {
        picker.asked.lock().unwrap().len() == 1
    });
    assert!(adding(&harness).is_some_and(|(_, _, choosing)| choosing));
    press(&mut driver, &mut harness, CHOOSE);
    assert_eq!(picker.asked.lock().unwrap().len(), 1, "a second picker");
    // Settings closed, and opened again with its field, before the picker answers.
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    driver.until(&mut harness, "Settings closed", |app| {
        app.dialog().is_none()
    });
    press(&mut driver, &mut harness, "Add folder…");
    release.send(()).unwrap();
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    let (path, _, choosing) = adding(&harness).expect("the field open");
    assert_eq!(path, "", "the folder chosen for the Settings that closed");
    assert!(!choosing);
}

// A picker cancelled - or one that could not be opened, which rfd answers the same way -
// leaves what was typed in the field as it was.
#[test]
fn a_cancelled_picker_leaves_the_field_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = added(dir.path(), picker.clone());
    driver.until(&mut harness, "the picker answered", |app| {
        matches!(app.dialog(), Some(Dialog::Settings(settings))
            if settings.adding.as_ref().is_some_and(|adding| !adding.choosing))
    });
    press(&mut driver, &mut harness, photon_ui::watched_list::FIELD);
    harness.event(egui::Event::Text("/photos/new".to_owned()));
    driver.act(&mut harness);
    assert_eq!(
        adding(&harness).unwrap().0,
        "/photos/new",
        "typed into the field"
    );
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the picker asked again", |_| {
        picker.asked.lock().unwrap().len() == 2
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let (path, _, choosing) = adding(&harness).unwrap();
    assert_eq!(path, "/photos/new");
    assert!(!choosing);
    assert!(harness.state().toasts().is_empty(), "a cancel says nothing");
}

/// The photo at `offset`, the other button on it, and the menu's "Export photo…".
fn open_export(driver: &mut Driver, harness: &mut Harness<'_>) {
    let at: Pos2 = (harness.state().tile_rect(0))
        .expect("the photo is on screen")
        .center();
    let button = |pressed, button| egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(true, PointerButton::Primary));
    harness.event(button(false, PointerButton::Primary));
    driver.act(harness);
    harness.event(button(true, PointerButton::Secondary));
    harness.event(button(false, PointerButton::Secondary));
    driver.act(harness);
    harness.get_by_label("Export photo…").click();
    driver.act(harness);
    for _ in 0..3 {
        driver.act(harness);
    }
    assert!(matches!(harness.state().dialog(), Some(Dialog::Export(_))));
}

// The export's "Choose…" opens the picker, and the folder chosen is the destination, asked
// about as a typed one is.
#[test]
fn the_exports_choose_puts_the_folder_chosen_in_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = photos_in(dir.path(), "Pictures", &["a.jpg"]);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let out = photon_core::paths::canonicalize(&out).unwrap();
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(out.clone());
    let (mut harness, mut driver) = launch(dir.path(), Some(pictures), picker.clone());
    driver.until(&mut harness, "the photo shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    open_export(&mut driver, &mut harness);
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the destination asked about", |app| {
        matches!(app.dialog(), Some(Dialog::Export(export)) if matches!(export.dest(), Dest::Passed(_)))
    });
    let Some(Dialog::Export(export)) = harness.state().dialog() else {
        panic!("the export's dialog closed");
    };
    assert_eq!(export.path(), out.display().to_string());
    assert_eq!(*export.dest(), passed(&out));
    assert_eq!(*picker.asked.lock().unwrap(), ["Export copies to…"]);
}

// Where no picker could be opened - no portal and no `zenity` - photon says so, and the
// field is the way left: told apart from a cancel, which says nothing.
#[test]
fn where_no_picker_can_be_opened_it_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Unavailable;
    let (mut harness, mut driver) = added(dir.path(), picker.clone());
    driver.until(&mut harness, "the picker answered", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == photon_ui::platform::NO_PICKER)
    });
    assert!(adding(&harness).is_some_and(|(path, _, choosing)| path.is_empty() && !choosing));
}
