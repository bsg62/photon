//! What a review of Settings found (PR #205), each a case `tests/settings.rs` does not
//! reach, through the whole application and the real engine, in a library the test made.

use crate::common::{Driver, Harness, library, opened};
use eframe::egui::{self, Key, Modifiers, PointerButton, accesskit::Role, pos2};
use egui_kittest::kittest::Queryable;
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    library::Library,
};
use photon_ui::{
    app::App,
    dialogs::Dialog,
    fixture::Fixture,
    settings::{FIND_FACES_BOX, INTERVAL_BEFORE, Question, Settings},
};

fn settings(app: &App) -> Option<&Settings> {
    match app.dialog() {
        Some(Dialog::Settings(settings)) => Some(&**settings),
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

fn frame_with(driver: &mut Driver, harness: &mut Harness<'_>, events: Vec<egui::Event>) {
    harness.input_mut().events.extend(events);
    driver.act(harness);
}

fn gear(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.get_by_label("Settings").click();
    driver.act(harness);
    driver.until(harness, "Settings read", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some())
    });
    driver.settle(harness);
    driver.frames_in(harness, 0.5);
}

fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    let nodes = harness.query_all_by_role_and_label(Role::Button, label);
    let node = (nodes.into_iter())
        .find(|node| node.rect().left() > 140.0 && node.rect().top() > 80.0)
        .unwrap_or_else(|| panic!("no button {label} in the dialog"));
    node.click();
    driver.act(harness);
    driver.settle(harness);
}

fn stored(library: &Fixture) -> Library {
    Library::open(&library.dirs().db_path).unwrap()
}

/// The interval's field with the keyboard, holding `typed`.
fn type_interval(driver: &mut Driver, harness: &mut Harness<'_>, typed: &str) {
    harness.get_by_label(INTERVAL_BEFORE).click();
    driver.act(harness);
    key_with(driver, harness, Key::A, Modifiers::COMMAND);
    harness.event(egui::Event::Text(typed.to_owned()));
    driver.act(harness);
    assert_eq!(
        settings(harness.state()).map(Settings::interval),
        Some(typed)
    );
}

/// A click whose press and release arrive in one frame - a touchpad's tap, which libinput
/// sends as the two together - at `at`.
fn tap(driver: &mut Driver, harness: &mut Harness<'_>, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame_with(
        driver,
        harness,
        vec![egui::Event::PointerMoved(at), button(true), button(false)],
    );
}

// A press on the scrim closes Settings whatever else the frame asked of it. With the
// interval's field holding the keyboard, a click whose press and release land in one
// frame - a touchpad's tap - has the field give up the keyboard in that very frame (egui
// surrenders the focus at a click elsewhere) and ask for its interval to be stored; the
// close was dropped for that ask, and the dialog stayed.
#[test]
fn a_tap_on_the_scrim_closes_settings_while_the_interval_has_the_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    type_interval(&mut driver, &mut harness, "15");
    // Outside the dialog, which is 720 by 520 in the middle of a window 1000 by 700.
    tap(&mut driver, &mut harness, pos2(60.0, 650.0));
    driver.settle(&mut harness);
    assert!(
        harness.state().dialog().is_none(),
        "a tap on the scrim left Settings open"
    );
    assert_eq!(stored(&library).slideshow_interval_s().unwrap(), 15);
}

// The same tap with nothing holding the keyboard closes it: the difference above is the
// field's ask in the same frame.
#[test]
fn a_tap_on_the_scrim_closes_settings() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    tap(&mut driver, &mut harness, pos2(60.0, 650.0));
    assert!(harness.state().dialog().is_none());
}

/// photon has looked at every photo, found a face on the first two, and the user named
/// them Anna.
fn named_anna(library: &Fixture) {
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
    lib.name_items(&ids, "Anna").unwrap();
}

// The question is answered from the keyboard - it opens with the keyboard on Cancel - and
// Cancel then goes, the box coming back in its place: the keyboard goes back to the box,
// as the system's `ask` the Svelte dialog uses gives it back. On nothing, Space would
// tick nothing and Tab start again from the top of the dialog.
#[test]
fn the_keyboard_comes_back_to_the_box_when_the_question_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 4, 0, 0);
    named_anna(&library);
    let (mut harness, mut driver) = opened(&library, 4);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "People");
    harness
        .get_by_role_and_label(Role::CheckBox, FIND_FACES_BOX)
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the question asked", |app| {
        settings(app).is_some_and(|settings| settings.question() == Some(&Question::SwitchOff(1)))
    });
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(settings(harness.state()).and_then(Settings::question), None);
    let boxed = egui::Id::new(("settings-box", FIND_FACES_BOX));
    let focused = harness.ctx.memory(|memory| memory.focused());
    assert_eq!(
        focused,
        Some(boxed),
        "the keyboard is on {focused:?}, not on the box"
    );
}

// The interval's field opens with a history of its own: it is one field under one id for
// every Settings opened, egui keeps a field's undo by id, and Ctrl+Z in a Settings opened
// again brought back what was typed in the last one ("9").
#[test]
fn the_interval_opens_with_a_history_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    type_interval(&mut driver, &mut harness, "9");
    driver.frames_in(&mut harness, 2.0);
    key_with(&mut driver, &mut harness, Key::A, Modifiers::COMMAND);
    harness.event(egui::Event::Text("12".to_owned()));
    driver.act(&mut harness);
    driver.frames_in(&mut harness, 2.0);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "12 stored", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some_and(|s| s.interval == 12))
    });
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
    driver.settle(&mut harness);

    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    harness.get_by_label(INTERVAL_BEFORE).click();
    driver.act(&mut harness);
    key_with(&mut driver, &mut harness, Key::Z, Modifiers::COMMAND);
    driver.act(&mut harness);
    assert_eq!(
        settings(harness.state()).map(Settings::interval),
        Some("12"),
        "Ctrl+Z in a new Settings undid an edit made in the last one"
    );
}

// An interval typed and left by a click on another section is
// stored then (the field gives up the keyboard in the frame of the click, and asks
// before the section changes), not only when the dialog closes.
#[test]
fn an_interval_left_for_another_section_is_stored_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    type_interval(&mut driver, &mut harness, "15");
    press(&mut driver, &mut harness, "About");
    driver.until(&mut harness, "15 stored", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some_and(|s| s.interval == 15))
    });
    assert_eq!(stored(&library).slideshow_interval_s().unwrap(), 15);
}

// Tab out of the field stores it.
#[test]
fn an_interval_left_by_tab_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    gear(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Slideshow");
    type_interval(&mut driver, &mut harness, "15");
    key(&mut driver, &mut harness, Key::Tab);
    driver.act(&mut harness);
    driver.until(&mut harness, "15 stored", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some_and(|s| s.interval == 15))
    });
    assert!(harness.state().dialog().is_some());
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

// A theme pinned against the desktop's is put on the title bar again when the system says
// the window's theme has changed: winit on Windows puts the system's back at every
// settings broadcast and says so. With `System` there is nothing to keep, and nothing is
// sent.
#[test]
fn a_pinned_theme_is_put_back_on_the_title_bar_when_the_system_changes_it() {
    for (pinned, told_again) in [
        (
            photon_core::library::ThemeChoice::Dark,
            Some(egui::SystemTheme::Dark),
        ),
        (photon_core::library::ThemeChoice::System, None),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let library = library(dir.path(), 6, 0, 0);
        stored(&library).set_theme(pinned).unwrap();
        let (mut harness, mut driver) = opened(&library, 6);
        harness.input_mut().system_theme = Some(egui::Theme::Dark);
        driver.act(&mut harness);
        driver.settle(&mut harness);
        harness.input_mut().system_theme = Some(egui::Theme::Light);
        driver.act(&mut harness);
        assert_eq!(told(&harness), told_again, "{pinned:?}");
    }
}
