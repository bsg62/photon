//! Settings, through the whole application and the real engine: the ways it opens, and
//! each of its sections that stores something - into a library the test made, in a
//! temporary directory. Nothing here writes outside `library.db`.

use crate::common::{Driver, Harness, launch, library, opened};
use eframe::egui::{self, Key, Modifiers, ThemePreference, accesskit::Role};
use egui_kittest::kittest::Queryable;
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    library::{Library, ThemeChoice},
};
use photon_ui::{
    app::App,
    dialogs::Dialog,
    fixture::Fixture,
    settings::{
        DISTANCES, FIND_FACES_BOX, INTERVAL_BEFORE, STOP_FINDING, Section, Settings,
        switch_off_warning,
    },
};

fn settings(app: &App) -> Option<&Settings> {
    match app.dialog() {
        Some(Dialog::Settings(settings)) => Some(settings),
        _ => None,
    }
}

fn key_with(driver: &mut Driver, harness: &mut Harness<'_>, key: Key, held: Modifiers) {
    harness.key_press_modifiers(held, key);
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    key_with(driver, harness, key, Modifiers::NONE);
}

/// The gear, and Settings drawn and read.
fn gear(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.get_by_label("Settings").click();
    driver.act(harness);
    driver.until(harness, "Settings read", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some())
    });
    // A press within a double click's time of the dialog's first frame is the second
    // press of whatever opened it, and does nothing (`DialogView::deaf`).
    driver.settle(harness);
    driver.frames_in(harness, 0.5);
}

/// A button of the dialog called `label`: the sidebar and the top bar under it may have
/// one of that name - a row, the size control - and the dialog stands in the middle of
/// the window, 720 by 520 in a window 1000 by 700.
fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    let nodes = harness.query_all_by_role_and_label(Role::Button, label);
    let node = (nodes.into_iter())
        .find(|node| node.rect().left() > 140.0 && node.rect().top() > 80.0)
        .unwrap_or_else(|| panic!("no button {label} in the dialog"));
    node.click();
    driver.act(harness);
    // What it asked for is done after the frame that drew it, and drawn in the next.
    driver.settle(harness);
}

/// The theme the window's title bar was told in the last frame drawn.
fn told(harness: &Harness<'_>) -> Option<egui::SystemTheme> {
    let output = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)?;
    output.commands.iter().find_map(|command| match command {
        egui::ViewportCommand::SetTheme(theme) => Some(*theme),
        _ => None,
    })
}

fn stored(library: &Fixture) -> Library {
    Library::open(&library.dirs().db_path).unwrap()
}

// The gear opens Settings at its folders; Escape closes it.
#[test]
fn the_gear_opens_settings_and_escape_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    assert_eq!(
        settings(harness.state()).map(|settings| settings.section),
        Some(Section::Folders)
    );
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
}

// A theme chosen is put on the window at once and stored; the photo size is the top
// bar's own control again.
#[test]
fn a_theme_and_a_size_chosen_are_put_on_the_window_and_stored() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Appearance");
    let nodes = harness.query_all_by_role_and_label(Role::Button, "Dark");
    let dark = (nodes.into_iter()).next().expect("the theme's control");
    dark.click();
    driver.act(&mut harness);
    // The title bar the system draws is told, in the frame of the press.
    assert_eq!(told(&harness), Some(egui::SystemTheme::Dark));
    driver.settle(&mut harness);
    assert_eq!(
        harness.ctx.options(|options| options.theme_preference),
        ThemePreference::Dark
    );
    press(&mut driver, &mut harness, "Large");
    assert_eq!(
        harness.state().tile_size(),
        photon_core::library::GridTile::Large
    );
    driver.settle(&mut harness);
    let lib = stored(&library);
    assert_eq!(lib.theme().unwrap(), ThemeChoice::Dark);
    assert_eq!(
        lib.grid_tile().unwrap(),
        photon_core::library::GridTile::Large
    );
}

// The interval is stored as typed at Enter, clamped by the library, and the field shows
// what was stored; one typed and left by Escape, which closes Settings, is stored too.
#[test]
fn the_interval_is_stored_as_the_library_clamps_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    harness.get_by_label(INTERVAL_BEFORE).click();
    driver.act(&mut harness);
    key_with(&mut driver, &mut harness, Key::A, Modifiers::COMMAND);
    harness.event(egui::Event::Text("90".to_owned()));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the interval stored", |app| {
        settings(app).is_some_and(|settings| settings.interval() == "60")
    });
    assert_eq!(stored(&library).slideshow_interval_s().unwrap(), 60);

    key_with(&mut driver, &mut harness, Key::A, Modifiers::COMMAND);
    harness.event(egui::Event::Text("12".to_owned()));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
    driver.settle(&mut harness);
    assert_eq!(stored(&library).slideshow_interval_s().unwrap(), 12);
}

// Shuffle and the look-alikes are stored as they are pressed, and say what is stored.
#[test]
fn shuffle_and_the_look_alikes_are_stored() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    harness
        .get_by_role_and_label(Role::CheckBox, photon_ui::settings::SHUFFLE)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "shuffle stored", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some_and(|s| s.shuffle))
    });
    assert!(stored(&library).slideshow_shuffle().unwrap());

    press(&mut driver, &mut harness, "Duplicates");
    press(&mut driver, &mut harness, "Loose");
    driver.until(&mut harness, "the look-alikes stored", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some_and(|s| s.distance == 10))
    });
    assert_eq!(stored(&library).similar_distance().unwrap(), 10);
    assert!(harness.query_by_label(DISTANCES[2].hint).is_some());
}

/// photon has looked at every photo of `library`, found a face on each of the first
/// two, and the user has named them `name` - unless `name` is empty.
fn looked(library: &Fixture, name: &str) {
    let lib = stored(library);
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
    let ids: Vec<i64> = photos.iter().take(2).map(|photo| photo.id).collect();
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let faces = if ids.contains(&photo.id) {
                vec![face.clone()]
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    if !name.is_empty() {
        lib.name_items(&ids, name).unwrap();
    }
}

// Switching Find faces off deletes the people named: with any, the row asks first, and
// Cancel keeps them. "Stop finding faces" switches it off.
#[test]
fn find_faces_off_with_people_named_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 4, 0, 0);
    looked(&library, "Anna");
    let (mut harness, mut driver) = opened(&library, 4);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "People");
    harness
        .get_by_role_and_label(Role::CheckBox, FIND_FACES_BOX)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the question asked", |app| {
        settings(app).is_some_and(|settings| settings.question() == Some(1))
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label(&switch_off_warning(1)).is_some());
    assert!(
        harness.state().engine().face_detection(),
        "not switched off yet"
    );
    // The keyboard is on Cancel: Enter keeps them.
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    assert_eq!(settings(harness.state()).and_then(Settings::question), None);
    driver.settle(&mut harness);
    assert!(harness.state().engine().face_detection());
    assert_eq!(stored(&library).named_people_count().unwrap(), 1);

    harness
        .get_by_role_and_label(Role::CheckBox, FIND_FACES_BOX)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "asked again", |app| {
        settings(app).is_some_and(|settings| settings.question().is_some())
    });
    driver.act(&mut harness);
    press(&mut driver, &mut harness, STOP_FINDING);
    driver.until(&mut harness, "switched off", |app| {
        !app.engine().face_detection()
    });
    driver.settle(&mut harness);
    assert_eq!(stored(&library).named_people_count().unwrap(), 0);
    assert!(!stored(&library).face_detection().unwrap());
}

// With nobody named there is nothing to lose, and nothing is asked.
#[test]
fn find_faces_off_with_nobody_named_switches_off_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 4, 0, 0);
    looked(&library, "");
    let (mut harness, mut driver) = opened(&library, 4);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "People");
    harness
        .get_by_role_and_label(Role::CheckBox, FIND_FACES_BOX)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "switched off", |app| {
        !app.engine().face_detection()
    });
    assert_eq!(settings(harness.state()).and_then(Settings::question), None);
    // And on again.
    driver.act(&mut harness);
    harness
        .get_by_role_and_label(Role::CheckBox, FIND_FACES_BOX)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "switched on", |app| {
        app.engine().face_detection()
    });
}

// About says the version and photon's memory, read as it is shown.
#[test]
fn about_says_the_version_and_the_memory() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "About");
    let version = format!("photon {}", env!("CARGO_PKG_VERSION"));
    assert!(harness.query_by_label(&version).is_some());
    driver.until(&mut harness, "the memory read", |app| {
        settings(app).is_some_and(|settings| settings.memory.is_some())
    });
}

// A library that watches a drive that is not there offers its list of watched folders,
// which is Settings at Folders; one that watches nothing offers it from the sidebar.
#[test]
fn the_empty_library_and_the_sidebar_open_settings_at_folders() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 0, 0, 0);
    let mut harness = launch(&library);
    let mut driver = Driver::new(&harness);
    driver.seen(&mut harness, "Watched folders…");
    driver.settle(&mut harness);
    harness.get_by_label("Watched folders…").click();
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(
        settings(harness.state()).map(|settings| settings.section),
        Some(Section::Folders)
    );
    key(&mut driver, &mut harness, Key::Escape);

    let other = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = crate::common::over(other.path(), None);
    driver.seen(&mut harness, "Add a folder in Settings…");
    driver.settle(&mut harness);
    harness.get_by_label("Add a folder in Settings…").click();
    driver.act(&mut harness);
    assert_eq!(
        settings(harness.state()).map(|settings| settings.section),
        Some(Section::Folders)
    );
}

// About reads photon's memory every two seconds, and only while it is shown: Settings at
// any other section is a still window.
#[test]
fn the_memory_is_read_again_only_while_about_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0, "Folders is still");
    press(&mut driver, &mut harness, "About");
    driver.until(&mut harness, "the memory read", |app| {
        settings(app).is_some_and(|settings| settings.memory.is_some())
    });
    driver.settle(&mut harness);
    // Each read is a frame that asks and the frame its answer is drawn in, egui draws a
    // frame asked for after a delay a frame early, and a slow runner's other threads
    // answer into the stretch too (ten frames on macOS' CI, where eight was the bound):
    // a few for each two seconds, and nowhere near one at every refresh, which is three
    // hundred.
    let frames = driver.frames_in(&mut harness, 5.0);
    assert!((2..=30).contains(&frames), "{frames} frames in five seconds");
}
