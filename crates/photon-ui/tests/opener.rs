//! Reveal and Open, through the whole application: every place that shows a photo, a
//! folder or the library in the system's file manager, opens a photo in the system's app
//! for it, or the place it was taken in the browser. The system is a stand-in
//! (`common::Picker`, whose `opened` says what it was asked): a file manager is something
//! a CI runner has none of, and a test must not open windows on the machine it runs on.

use crate::common::{Driver, Harness, Picker, library, opened_with, photo_ids};
use eframe::egui::{self, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use photon_engine::commands;
use std::sync::Arc;

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

/// The other button on the photo at `offset`, and the menu's item `label`.
fn menu_item(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, label: &str) {
    let at = (harness.state().tile_rect(offset)).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(harness);
    driver.act(harness);
    harness.get_by_label_contains(label).click();
    driver.act(harness);
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

// The tile menu's "Reveal in file manager" and "Open in default app" ask the system
// about the photo's own file.
#[test]
fn the_tile_menu_reveals_and_opens_the_photo() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
    let index = 2;
    menu_item(&mut driver, &mut harness, index, "Reveal in file manager");
    driver.frames_in(&mut harness, 0.5);
    menu_item(&mut driver, &mut harness, index, "Open in default app");
    let path = path_of(&harness, ids[2]);
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 2),
        [format!("reveal {path}"), format!("open {path}")]
    );
}

// Ctrl/Cmd+Shift+R reveals the one photo selected, and nothing with two.
#[test]
fn ctrl_shift_r_reveals_the_one_photo_selected() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
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
    let id = harness.state().selected()[0];
    let path = path_of(&harness, id);
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
    let library = library(dir.path(), 3, 0, 0);
    let ids = photo_ids(&library);
    {
        let conn = rusqlite::Connection::open(library.dirs().db_path).unwrap();
        let sql = "UPDATE items SET gps_lat = 48.1374, gps_lon = 11.5755 WHERE id = ?1";
        assert_eq!(conn.execute(sql, [ids[0]]).unwrap(), 1);
    }
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 3, picker.clone());
    let index = 0;
    pick(&mut driver, &mut harness, index, Modifiers::NONE);
    driver.frames_in(&mut harness, 0.5);
    harness.key_press_modifiers(Modifiers::NONE, Key::Enter);
    driver.act(&mut harness);
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    let at = egui::pos2(500.0, 300.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Reveal in file manager").click();
    driver.act(&mut harness);
    let path = path_of(&harness, ids[0]);
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 1),
        [format!("reveal {path}")]
    );
    driver.frames_in(&mut harness, 0.5);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Open in default app").click();
    driver.act(&mut harness);
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

// What the system could not do is said, in its own words.
#[test]
fn a_reveal_that_fails_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let picker = Arc::new(Picker::default());
    *picker.open_refused.lock().unwrap() = Some("no file manager".to_owned());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
    menu_item(&mut driver, &mut harness, 0, "Reveal in file manager");
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(crate::common::said(&harness), ["no file manager"]);
}

// The folder's menu - here on its header over the photos - reveals the folder itself.
#[test]
fn the_folder_menu_reveals_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
    let tile = (harness.state().tile_rect(0)).expect("the photo is laid out");
    let at = tile.center_top() - egui::vec2(0.0, 14.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Reveal in file manager").click();
    driver.act(&mut harness);
    let photo = path_of(&harness, ids[0]);
    let folder = std::path::Path::new(&photo)
        .parent()
        .unwrap()
        .display()
        .to_string();
    assert_eq!(
        asked(&mut driver, &mut harness, &picker, 1),
        [format!("reveal {folder}")]
    );
}

/// Settings open - at its folders, where the gear opens it - past a double click's time
/// of its first frame.
fn settings(driver: &mut Driver, harness: &mut Harness<'_>) {
    driver.settle(harness);
    harness.get_by_label("Settings").click();
    driver.act(harness);
    driver.settle(harness);
    driver.frames_in(harness, 0.5);
}

// Settings' folders reveal each watched folder from its row, and About the library.
#[test]
fn settings_reveals_a_watched_folder_and_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = opened_with(&library, 12, picker.clone());
    settings(&mut driver, &mut harness);
    driver.until(&mut harness, "the folders listed", |app| {
        !app.watched().is_empty()
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let rows: Vec<_> = harness.query_all_by_label("Reveal").collect();
    let nearest = rows
        .iter()
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("a row's Reveal");
    nearest.click();
    driver.act(&mut harness);
    let opened = asked(&mut driver, &mut harness, &picker, 1);
    let watched: Vec<String> = (harness.state().watched().iter())
        .map(|folder| format!("reveal {}", folder.path))
        .collect();
    assert!(watched.contains(&opened[0]), "{opened:?} of {watched:?}");
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
