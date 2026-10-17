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
            (App::new(cc, dirs, pictures).unwrap().without_animations())
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
fn adding(harness: &Harness<'_>) -> Option<(String, Dest)> {
    match harness.state().dialog() {
        Some(Dialog::Settings(settings)) => (settings.adding.as_ref())
            .map(|adding| (adding.path().to_owned(), adding.dest().clone())),
        _ => None,
    }
}

/// The empty library, its "Add folder…" pressed where no picker can be opened: Settings,
/// at its folders, with the field a path is typed in open - the way left. The picker
/// answers `then` from here on.
fn added<'a>(dir: &Path, picker: Arc<Picker>, then: Picked) -> (Harness<'a>, Driver) {
    *picker.answer.lock().unwrap() = Picked::Unavailable;
    let (mut harness, mut driver) = launch(dir, None, picker.clone());
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    driver.until(
        &mut harness,
        "the field open",
        |app| matches!(app.dialog(), Some(Dialog::Settings(settings)) if settings.adding.is_some()),
    );
    *picker.answer.lock().unwrap() = then;
    (harness, driver)
}

fn passed(folder: &Path) -> Dest {
    Dest::Passed(folder.display().to_string())
}

/// Whether `folder` is among the watched folders the window holds.
fn watching(app: &App, folder: &Path) -> bool {
    (app.watched().iter()).any(|watched| Path::new(&watched.path) == folder)
}

/// A folder of its own under `dir`, by its canonical path.
fn folder(dir: &Path, name: &str) -> PathBuf {
    let made = dir.join(name);
    std::fs::create_dir_all(&made).unwrap();
    photon_core::paths::canonicalize(&made).unwrap()
}

// The empty library's "Add folder…" opens the system's picker at once, and the folder
// chosen is watched - as in the Tauri photon, with no Settings and no field on the way,
// which were steps of their own that the first person to run the window found.
#[test]
fn add_folder_opens_the_picker_and_watches_the_folder_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(chosen.clone());
    let (mut harness, mut driver) = launch(dir.path(), None, picker.clone());
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    driver.until(&mut harness, "the folder watched", |app| {
        watching(app, &chosen)
    });
    assert!(
        harness.state().dialog().is_none(),
        "nothing opened over the window"
    );
    assert_eq!(*picker.asked.lock().unwrap(), ["Add a folder to photon"]);
}

const ADD: &str = photon_ui::watched_list::ADD_FOLDER;

/// Settings opened at its folders by the sidebar's way to it, which asks for no picker.
fn settings_folders(driver: &mut Driver, harness: &mut Harness<'_>) {
    press(driver, harness, "Add a folder in Settings…");
    driver.settle(harness);
    driver.frames_in(harness, 0.5);
    assert!(matches!(
        harness.state().dialog(),
        Some(Dialog::Settings(_))
    ));
}

/// Settings' own "Add folder…", over the empty library's, which is under Settings.
fn press_settings_add(driver: &mut Driver, harness: &mut Harness<'_>, panels: egui::Rect) {
    (harness.query_all_by_label(ADD))
        .find(|node| node.rect() != panels)
        .expect("Settings' Add folder…")
        .click();
    driver.act(harness);
}

// Settings' "Add folder…" is the picker too, and the folder chosen is watched and in its
// list; the field does not open.
#[test]
fn settings_add_folder_opens_the_picker_and_watches_the_folder_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(chosen.clone());
    let (mut harness, mut driver) = launch(dir.path(), None, picker.clone());
    driver.settle(&mut harness);
    let panels = harness.get_by_label(ADD).rect();
    settings_folders(&mut driver, &mut harness);
    press_settings_add(&mut driver, &mut harness, panels);
    driver.until(&mut harness, "the folder watched", |app| {
        watching(app, &chosen)
    });
    assert!(adding(&harness).is_none(), "the field opened");
    assert_eq!(*picker.asked.lock().unwrap(), ["Add a folder to photon"]);
}

// The picker is one at a time: "Add folder…" pressed while one is open - behind the
// window, where nothing ties it to it - opens no second one and takes the open one for
// itself, and the folder chosen there is watched.
#[test]
fn add_folder_while_a_picker_is_open_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) =
        added(dir.path(), picker.clone(), Picked::Folder(chosen.clone()));
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the picker open", |_| {
        picker.asked.lock().unwrap().len() == 2
    });
    let panels = (harness.query_all_by_label(ADD))
        .map(|node| node.rect())
        .next()
        .unwrap();
    // The field closed by its Cancel, and Settings at rest: its "Add folder…" is there
    // to press.
    press(&mut driver, &mut harness, photon_ui::settings::CANCEL);
    assert!(adding(&harness).is_none(), "the field closed");
    driver.frames_in(&mut harness, 0.5);
    press_settings_add(&mut driver, &mut harness, panels);
    assert_eq!(picker.asked.lock().unwrap().len(), 2, "a second picker");
    release.send(()).unwrap();
    driver.until(&mut harness, "the folder watched", |app| {
        watching(app, &chosen)
    });
    assert!(adding(&harness).is_none(), "and not put in a field");
}

// A folder chosen in "Choose…" for a dialog that closed since goes nowhere: not into the
// Settings opened since, and not watched.
#[test]
fn a_folder_chosen_for_a_dialog_that_closed_goes_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) =
        added(dir.path(), picker.clone(), Picked::Folder(chosen.clone()));
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the picker open", |_| {
        picker.asked.lock().unwrap().len() == 2
    });
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    driver.until(&mut harness, "Settings closed", |app| {
        app.dialog().is_none()
    });
    settings_folders(&mut driver, &mut harness);
    release.send(()).unwrap();
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert!(adding(&harness).is_none(), "a field opened for it");
    assert!(!watching(harness.state(), &chosen), "watched");
}

// A picker cancelled leaves what was typed in the field as it was, and says nothing.
#[test]
fn a_cancelled_picker_leaves_the_field_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = added(dir.path(), picker.clone(), Picked::Cancelled);
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, photon_ui::watched_list::FIELD);
    harness.event(egui::Event::Text("/photos/new".to_owned()));
    driver.act(&mut harness);
    assert_eq!(
        adding(&harness).unwrap().0,
        "/photos/new",
        "typed into the field"
    );
    let said = harness.state().toasts().len();
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the picker asked again", |_| {
        picker.asked.lock().unwrap().len() == 2
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let (path, _) = adding(&harness).unwrap();
    assert_eq!(path, "/photos/new");
    assert_eq!(
        harness.state().toasts().len(),
        said,
        "a cancel says nothing"
    );
}

// "Choose…" beside the field puts the folder chosen in it, asked about as a typed one is,
// and watches nothing until "Add".
#[test]
fn choose_beside_the_field_puts_the_folder_chosen_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) =
        added(dir.path(), picker.clone(), Picked::Folder(chosen.clone()));
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the folder asked about", |app| {
        matches!(app.dialog(), Some(Dialog::Settings(settings))
            if settings.adding.as_ref().is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_))))
    });
    let (path, dest) = adding(&harness).unwrap();
    assert_eq!(path, chosen.display().to_string());
    assert_eq!(dest, passed(&chosen));
    assert!(!watching(harness.state(), &chosen));
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

// Where no picker could be opened - no portal and no `zenity` - photon says so, and
// Settings' field a path is typed in is the way left: told apart from a cancel, which says
// nothing and opens nothing.
#[test]
fn where_no_picker_can_be_opened_it_is_said_and_the_field_opens() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (harness, _driver) = added(dir.path(), picker.clone(), Picked::Cancelled);
    assert!(
        (harness.state().toasts().iter())
            .any(|toast| toast.message == photon_ui::platform::NO_PICKER)
    );
    assert!(adding(&harness).is_some_and(|(path, _)| path.is_empty()));
}

// A cancelled "Add folder…" opens nothing and says nothing.
#[test]
fn a_cancelled_add_folder_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = launch(dir.path(), None, picker.clone());
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    driver.until(&mut harness, "the picker asked", |_| {
        picker.asked.lock().unwrap().len() == 1
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert!(harness.state().dialog().is_none());
    assert!(harness.state().toasts().is_empty());
}

// A folder chosen for "Add folder…" is watched whatever has opened over the window
// meanwhile: it was asked for, for nothing but itself.
#[test]
fn a_folder_chosen_to_add_is_watched_when_settings_opened_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = folder(dir.path(), "Holidays");
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(chosen.clone());
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    let (mut harness, mut driver) = launch(dir.path(), None, picker.clone());
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    driver.until(&mut harness, "the picker open", |_| {
        picker.asked.lock().unwrap().len() == 1
    });
    settings_folders(&mut driver, &mut harness);
    release.send(()).unwrap();
    driver.until(&mut harness, "the folder watched", |app| {
        watching(app, &chosen)
    });
}

// A picker open for the export, under its title "Export copies to…", is not taken for
// "Add folder…" pressed while it is open: the folder chosen there would have been watched,
// at once, as the destination of an export (a review found it).
#[test]
fn a_picker_open_for_the_export_is_not_taken_for_add_folder() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = photos_in(dir.path(), "Pictures", &["a.jpg"]);
    let out = folder(dir.path(), "out");
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Folder(out.clone());
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    let (mut harness, mut driver) = launch(dir.path(), Some(pictures), picker.clone());
    driver.until(&mut harness, "the photo shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    open_export(&mut driver, &mut harness);
    press(&mut driver, &mut harness, CHOOSE);
    driver.until(&mut harness, "the picker open", |_| {
        picker.asked.lock().unwrap().len() == 1
    });
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    driver.until(&mut harness, "the export's dialog closed", |app| {
        app.dialog().is_none()
    });
    press(&mut driver, &mut harness, "Settings");
    press(&mut driver, &mut harness, ADD);
    release.send(()).unwrap();
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert!(
        !watching(harness.state(), &out),
        "the export's destination was watched"
    );
    assert_eq!(*picker.asked.lock().unwrap(), ["Export copies to…"]);
}

// Where no picker could be opened and a dialog has opened over the window meanwhile, the
// dialog stays: only Settings is the field's place, and nothing else is put out for it.
#[test]
fn a_picker_that_could_not_open_leaves_a_dialog_opened_since() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = Picked::Unavailable;
    let (release, held) = mpsc::channel();
    *picker.held.lock().unwrap() = Some(held);
    let (mut harness, mut driver) = launch(dir.path(), None, picker.clone());
    driver.settle(&mut harness);
    press(&mut driver, &mut harness, "Add folder…");
    driver.until(&mut harness, "the picker asked", |_| {
        picker.asked.lock().unwrap().len() == 1
    });
    harness.input_mut().events.extend([
        egui::Event::Key {
            key: Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        egui::Event::Text("?".to_owned()),
    ]);
    driver.act(&mut harness);
    driver.until(&mut harness, "the list of keys open", |app| {
        matches!(app.dialog(), Some(Dialog::Shortcuts))
    });
    release.send(()).unwrap();
    driver.until(&mut harness, "the picker's answer said", |app| {
        (app.toasts().iter()).any(|toast| toast.message == photon_ui::platform::NO_PICKER)
    });
    assert!(matches!(harness.state().dialog(), Some(Dialog::Shortcuts)));
}
