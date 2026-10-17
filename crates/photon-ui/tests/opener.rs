//! Reveal and Open, through the whole application: every place that shows a photo, a
//! folder or the library in the system's file manager, opens a photo in the system's app
//! for it, or the place it was taken in the browser. Over real photos the test made - a
//! path is asked about before the system is - and with the system a stand-in
//! (`common::Picker`, whose `opened` says what it was asked): a file manager is something
//! a CI runner has none of, and a test must not open windows on the machine it runs on.

use crate::common::{Driver, Harness, Picker, REFRESH, library, opened_with, photos_in, said};
use eframe::egui::{self, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use photon_engine::commands;
use photon_ui::{app::App, dirs};
use std::{path::Path, sync::Arc};

/// The application watching three real photos - `a`, `b`, `c` - with `picker` for the
/// system, once they are shown.
fn three<'a>(dir: &Path, picker: Arc<Picker>) -> (Harness<'a>, Driver) {
    let pictures = photos_in(dir, "Pictures", &["a.jpg", "b.jpg", "c.jpg"]);
    let dirs = dirs::within(&dir.join("data"), &dir.join("cache"));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, Some(pictures))
                .unwrap()
                .without_animations())
            .with_videos(crate::common::no_videos())
            .with_platform(picker)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    (harness, driver)
}

fn button(
    at: egui::Pos2,
    button: PointerButton,
    pressed: bool,
    modifiers: Modifiers,
) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers,
    }
}

/// A click on the photo at `offset`, with `held`.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(button(at, PointerButton::Primary, true, held));
    harness.event(button(at, PointerButton::Primary, false, held));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

/// The other button at `at`, and the menu's item `label`.
fn menu_item_at(driver: &mut Driver, harness: &mut Harness<'_>, at: egui::Pos2, label: &str) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(harness);
    driver.act(harness);
    harness.get_by_label_contains(label).click();
    driver.act(harness);
}

/// The other button on the photo at `offset`, and the menu's item `label`.
fn menu_item(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, label: &str) {
    let at = (harness.state().tile_rect(offset)).unwrap().center();
    menu_item_at(driver, harness, at, label);
}

/// What the stand-in was asked, once it has been asked `n` things.
fn asked(driver: &mut Driver, harness: &mut Harness<'_>, picker: &Picker, n: usize) -> Vec<String> {
    driver.until(harness, "the system asked", |_| {
        picker.opened.lock().unwrap().len() >= n
    });
    picker.opened.lock().unwrap().clone()
}

fn path_of(harness: &Harness<'_>, id: i64) -> String {
    let engine = harness.state().engine().clone();
    commands::item_path(&engine, id)
        .unwrap()
        .display()
        .to_string()
}

/// The one photo selected.
fn selected(harness: &Harness<'_>) -> i64 {
    let selected = harness.state().selected();
    assert_eq!(selected.len(), 1);
    selected[0]
}

// The tile menu's "Reveal in file manager" and "Open in default app" ask the system
// about the photo's own file.
#[test]
fn the_tile_menu_reveals_and_opens_the_photo() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    menu_item(&mut driver, &mut harness, 2, "Reveal in file manager");
    let path = path_of(&harness, selected(&harness));
    driver.frames_in(&mut harness, 0.5);
    menu_item(&mut driver, &mut harness, 2, "Open in default app");
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 2),
        [format!("reveal {path}"), format!("open {path}")]
    );
}

// Ctrl/Cmd+Shift+R reveals the one photo selected, and nothing with two.
#[test]
fn ctrl_shift_r_reveals_the_one_photo_selected() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 1, Modifiers::SHIFT);
    let chord = Modifiers::COMMAND | Modifiers::SHIFT;
    harness.key_press_modifiers(chord, Key::R);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(
        picker.opened.lock().unwrap().len(),
        0,
        "two photos selected"
    );
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    harness.key_press_modifiers(chord, Key::R);
    driver.act(&mut harness);
    let path = path_of(&harness, selected(&harness));
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 1),
        [format!("reveal {path}")]
    );
}

// The viewer's menu reveals and opens the photo on screen, and the info panel's "Open in
// OpenStreetMap" opens the place it was taken, the viewer staying open.
#[test]
fn the_viewer_reveals_opens_and_shows_the_map() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    let id = selected(&harness);
    {
        let db = dir.path().join("data").join("library.db");
        let conn = rusqlite::Connection::open(db).unwrap();
        let sql = "UPDATE items SET gps_lat = 48.1374, gps_lon = 11.5755 WHERE id = ?1";
        assert_eq!(conn.execute(sql, [id]).unwrap(), 1);
    }
    driver.frames_in(&mut harness, 0.5);
    harness.key_press_modifiers(Modifiers::NONE, Key::Enter);
    driver.act(&mut harness);
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    let at = egui::pos2(500.0, 300.0);
    menu_item_at(&mut driver, &mut harness, at, "Reveal in file manager");
    let path = path_of(&harness, id);
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 1),
        [format!("reveal {path}")]
    );
    driver.frames_in(&mut harness, 0.5);
    menu_item_at(&mut driver, &mut harness, at, "Open in default app");
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 2)[1],
        format!("open {path}")
    );
    harness.key_press_modifiers(Modifiers::NONE, Key::I);
    driver.act(&mut harness);
    driver.seen(&mut harness, "Open in OpenStreetMap");
    driver.frames_in(&mut harness, 0.5);
    harness.get_by_label("Open in OpenStreetMap").click();
    driver.act(&mut harness);
    let opened = asked(&mut driver, &mut harness, &picker, 3);
    assert!(
        opened[2].starts_with("open https://www.openstreetmap.org/?mlat=48.137400"),
        "{opened:?}"
    );
    assert!(harness.state().viewer().is_some(), "the viewer stays");
}

// The folder's menu - here on its header over the photos - reveals the folder itself.
#[test]
fn the_folder_menu_reveals_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    let photo = path_of(&harness, selected(&harness));
    let tile = (harness.state().tile_rect(0)).expect("the photo is laid out");
    let at = tile.center_top() - egui::vec2(0.0, 14.0);
    menu_item_at(&mut driver, &mut harness, at, "Reveal in file manager");
    let folder = Path::new(&photo).parent().unwrap().display().to_string();
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 1),
        [format!("reveal {folder}")]
    );
}

// Settings' folders reveal each watched folder from its row - opened at its folders by
// the gear - and About the library.
#[test]
fn settings_reveals_a_watched_folder_and_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    harness.get_by_label("Settings").click();
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    driver.until(&mut harness, "the folders listed", |app| {
        !app.watched().is_empty()
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Reveal").click();
    driver.act(&mut harness);
    let watched = format!("reveal {}", harness.state().watched()[0].path);
    assert_eq!(asked(&mut driver, &mut harness, &picker, 1), [watched]);
    harness.get_by_label("About").click();
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    harness.get_by_label("Reveal").click();
    driver.act(&mut harness);
    let engine = harness.state().engine().clone();
    let library_path = commands::app_info(&engine)
        .library_path
        .display()
        .to_string();
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 2)[1],
        format!("reveal {library_path}")
    );
}

// A photo that is not there - its drive unplugged, as a fixture's is - is said to be
// gone, and the system is not asked: on Linux the file manager answers for any path, and
// the reveal went to nobody with nothing said.
#[test]
fn a_photo_that_is_not_there_is_said_so_and_the_system_not_asked() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
    menu_item(&mut driver, &mut harness, 0, "Reveal in file manager");
    driver.until(&mut harness, "it said", |app| !app.toasts().is_empty());
    let words = said(&harness);
    assert!(words[0].starts_with("It isn't there any more"), "{words:?}");
    driver.frames_in(&mut harness, 0.5);
    menu_item(&mut driver, &mut harness, 1, "Open in default app");
    driver.until(&mut harness, "it said again", |app| app.toasts().len() == 2);
    assert_eq!(picker.opened.lock().unwrap().len(), 0);
}

// What the system could not do is said, in its own words.
#[test]
fn a_reveal_the_system_refuses_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    *picker.open_refused.lock().unwrap() = Some("no file manager".to_owned());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    menu_item(&mut driver, &mut harness, 0, "Reveal in file manager");
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["no file manager"]);
}
