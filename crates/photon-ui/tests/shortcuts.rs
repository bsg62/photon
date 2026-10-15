//! The list of shortcuts that `?` opens, and Settings' Statistics, through the whole
//! application and the real engine, over libraries the tests made in temporary
//! directories. Nothing here writes.

use crate::common::{Driver, Harness, library, opened, over, photos_in};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_ui::{
    app::App,
    dialogs::Dialog,
    settings::{Counted, Settings},
    stats,
};

fn sheet(app: &App) -> bool {
    matches!(app.dialog(), Some(Dialog::Shortcuts))
}

fn settings(app: &App) -> Option<&Settings> {
    match app.dialog() {
        Some(Dialog::Settings(settings)) => Some(&**settings),
        _ => None,
    }
}

/// `?` as a keyboard types it, with Shift where the layout needs it: the key, and the
/// character it types, in one frame.
fn question_mark(driver: &mut Driver, harness: &mut Harness<'_>) {
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
    driver.act(harness);
    harness.event(egui::Event::Key {
        key: Key::Questionmark,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    });
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn tile(harness: &Harness<'_>, offset: usize) -> Pos2 {
    (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center()
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    }
}

// `?` opens the list over the grid; `?` again closes it, and Escape does.
#[test]
fn a_question_mark_opens_the_list_and_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    question_mark(&mut driver, &mut harness);
    assert!(sheet(harness.state()));
    assert!(harness.query_by_label("Keyboard shortcuts").is_some());
    question_mark(&mut driver, &mut harness);
    assert!(harness.state().dialog().is_none());
    question_mark(&mut driver, &mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
}

// In the search box `?` is a character, and opens nothing.
#[test]
fn a_question_mark_typed_in_the_search_box_is_a_letter() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    question_mark(&mut driver, &mut harness);
    assert!(harness.state().dialog().is_none());
    assert_eq!(harness.state().search_text(), "?");
}

// A held band owns the window: no list opens under it.
#[test]
fn no_list_opens_under_a_held_band() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let (mut harness, mut driver) = opened(&library, 6);
    let from = tile(&harness, 0);
    let to = tile(&harness, 4);
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, true, Modifiers::NONE));
    harness.event(egui::Event::PointerMoved(to));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        harness
            .state()
            .last_frame()
            .is_some_and(|frame| frame.banding)
    );
    question_mark(&mut driver, &mut harness);
    assert!(harness.state().dialog().is_none());
    harness.event(button(to, false, Modifiers::NONE));
    driver.act(&mut harness);
}

/// A library watching a folder of three real photos, shown.
fn three<'a>(dir: &std::path::Path) -> (Harness<'a>, Driver) {
    let pictures = photos_in(dir, "Pictures", &["a.jpg", "b.jpg", "c.jpg"]);
    let (mut harness, mut driver, _) = over(dir, Some(&pictures));
    driver.until(&mut harness, "the photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    (harness, driver)
}

fn wheel(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(harness);
}

// Over the viewer: the list opens, and the viewer under it is deaf - the wheel steps to
// no photo. Escape closes the list, not the viewer.
#[test]
fn the_list_opens_over_the_viewer_and_the_viewer_is_deaf_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = three(dir.path());
    let at = tile(&harness, 0);
    harness.event(egui::Event::PointerMoved(at));
    for _ in 0..2 {
        harness.event(button(at, true, Modifiers::NONE));
        harness.event(button(at, false, Modifiers::NONE));
    }
    driver.act(&mut harness);
    driver.until(&mut harness, "the viewer open", |app| {
        app.viewer().is_some()
    });
    let photo = harness.state().viewer().map(|viewer| viewer.id());
    question_mark(&mut driver, &mut harness);
    assert!(sheet(harness.state()));
    wheel(&mut driver, &mut harness, Pos2::new(500.0, 650.0));
    driver.settle(&mut harness);
    assert_eq!(harness.state().viewer().map(|viewer| viewer.id()), photo);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
    assert!(harness.state().viewer().is_some());
}

/// Two photos selected and compared.
fn comparing(driver: &mut Driver, harness: &mut Harness<'_>) {
    let first = tile(harness, 0);
    harness.event(egui::Event::PointerMoved(first));
    harness.event(button(first, true, Modifiers::NONE));
    harness.event(button(first, false, Modifiers::NONE));
    driver.act(harness);
    let second = tile(harness, 1);
    let adding = if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    };
    harness.event(egui::Event::PointerMoved(second));
    harness.event(egui::Event::ModifiersChanged(adding));
    harness.event(button(second, true, adding));
    harness.event(button(second, false, adding));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
    key(driver, harness, Key::C);
    driver.until(harness, "compare open", |app| app.compare().is_some());
    driver.act(harness);
}

// A pan held in compare when the list opens over it is let go of: the pointer moving on
// under the list pans nothing.
#[test]
fn a_pan_held_in_compare_is_let_go_of_when_the_list_opens() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = three(dir.path());
    comparing(&mut driver, &mut harness);
    // Zoomed in over the first pane, which can then be panned.
    let pane = Pos2::new(250.0, 175.0);
    harness.event(egui::Event::PointerMoved(pane));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    assert!(!harness.state().compare().unwrap().zoom().is_fit());
    harness.event(button(pane, true, Modifiers::NONE));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerMoved(pane + egui::vec2(20.0, 10.0)));
    driver.act(&mut harness);
    question_mark(&mut driver, &mut harness);
    assert!(sheet(harness.state()));
    let held = harness.state().compare().unwrap().zoom().pan();
    harness.event(egui::Event::PointerMoved(pane + egui::vec2(60.0, 40.0)));
    driver.act(&mut harness);
    harness.event(button(
        pane + egui::vec2(60.0, 40.0),
        false,
        Modifiers::NONE,
    ));
    driver.act(&mut harness);
    assert_eq!(harness.state().compare().unwrap().zoom().pan(), held);
}

// Over compare: the list opens, and the wheel under it zooms no pane. Escape closes the
// list, and compare stays.
#[test]
fn the_list_opens_over_compare_and_compare_is_deaf_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = three(dir.path());
    let first = tile(&harness, 0);
    harness.event(egui::Event::PointerMoved(first));
    harness.event(button(first, true, Modifiers::NONE));
    harness.event(button(first, false, Modifiers::NONE));
    driver.act(&mut harness);
    let second = tile(&harness, 1);
    let adding = if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    };
    harness.event(egui::Event::PointerMoved(second));
    harness.event(egui::Event::ModifiersChanged(adding));
    harness.event(button(second, true, adding));
    harness.event(button(second, false, adding));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "compare open", |app| app.compare().is_some());
    driver.act(&mut harness);
    question_mark(&mut driver, &mut harness);
    assert!(sheet(harness.state()));
    wheel(&mut driver, &mut harness, Pos2::new(500.0, 650.0));
    let compare = harness.state().compare().expect("compare stays");
    assert!(
        compare.zoom().is_fit(),
        "the wheel zoomed a pane under the list"
    );
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().dialog().is_none());
    assert!(harness.state().compare().is_some());
}

// A row of Statistics closes Settings and searches for its photos, the search box told.
#[test]
fn a_row_of_statistics_searches_for_its_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = three(dir.path());
    harness.get_by_label("Settings").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "Settings read", |app| {
        settings(app).is_some_and(|settings| settings.stored().is_some())
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    let nodes = harness.query_all_by_label("Statistics");
    let row = (nodes.into_iter())
        .find(|node| node.rect().left() > 140.0 && node.rect().top() > 80.0)
        .expect("the section's row");
    row.click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the library counted", |app| {
        settings(app).is_some_and(|settings| matches!(settings.stats, Counted::Counted(_)))
    });
    driver.act(&mut harness);
    let Some(Counted::Counted(counted)) = settings(harness.state()).map(|s| s.stats.clone()) else {
        panic!("counted");
    };
    let year = stats::year_rows(&counted).remove(0);
    let label = format!("{}, {}", year.label, year.count);
    harness.get_by_label(&label).click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the year searched for", |app| {
        let place = app.settled();
        place.view == GridView::Search && place.arg == year.search
    });
    assert!(harness.state().dialog().is_none());
    assert_eq!(harness.state().search_text(), year.search);
}

// The frame the list opens in is the list's: a click on compare's cross in that very
// frame - egui hands it to the cross whatever compare reads - closes nothing.
#[test]
fn a_click_in_the_frame_the_list_opens_closes_no_compare() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = three(dir.path());
    comparing(&mut driver, &mut harness);
    let cross = harness
        .get_by_label(photon_ui::compare_view::CLOSE_COMPARE)
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(cross));
    driver.act(&mut harness);
    harness.input_mut().events.extend([
        egui::Event::Key {
            key: Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        egui::Event::Text("?".to_owned()),
        button(cross, true, Modifiers::NONE),
        button(cross, false, Modifiers::NONE),
    ]);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(sheet(harness.state()));
    assert!(
        harness.state().compare().is_some(),
        "compare closed under the list"
    );
}
