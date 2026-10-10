//! What the status bar says while photon works and what an empty library says in place
//! of photos, through the whole application and a real engine: a folder of files scanned
//! for the first time, a library whose photos are all hidden, one that watches nothing,
//! one whose drive is away.

mod common;

use common::{Driver, Harness, REFRESH, click, launch, library, sources};
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{grid::GridView, library::Library};
use photon_ui::{app::App, dirs};
use std::path::{Path, PathBuf};

/// What the application says of its library, and of what it is doing.
#[derive(Clone, Debug, PartialEq)]
struct Said {
    photos: usize,
    /// The sentence of the empty library's panel.
    panel: Option<String>,
    /// The status bar's lines.
    lines: Vec<String>,
}

/// Draws frames until `done`, and answers every different thing the application said on
/// the way.
fn watched(
    driver: &mut Driver,
    harness: &mut Harness<'_>,
    what: &str,
    done: impl Fn(&App) -> bool,
) -> Vec<Said> {
    let seen = std::cell::RefCell::new(Vec::<Said>::new());
    driver.until(harness, what, |app| {
        let now = Said {
            photos: app.photos(),
            panel: app.panel().map(|panel| panel.text),
            lines: (app.status().into_iter().map(|line| line.label)).collect(),
        };
        let mut seen = seen.borrow_mut();
        if seen.last() != Some(&now) {
            seen.push(now);
        }
        done(app)
    });
    seen.into_inner()
}

/// A folder called Pictures under `dir`, holding `photos` small JPEGs.
fn pictures(dir: &Path, photos: usize) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let picture = std::fs::read(&sources(dir)[0]).unwrap();
    for n in 0..photos {
        std::fs::write(folder.join(format!("IMG_{n:05}.jpg")), &picture).unwrap();
    }
    folder
}

/// The application over a library of its own making under `dir`, as a first run has it:
/// `pictures` is the folder it starts by watching, when there is one.
fn first_run<'a>(dir: &Path, pictures: Option<PathBuf>) -> (Harness<'a>, Driver) {
    let dirs = dirs::within(&dir.join("data"), &dir.join("cache"));
    let harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| App::new(cc, dirs, pictures).unwrap());
    let driver = Driver::new(&harness);
    (harness, driver)
}

const LOOKING: &str = "Looking for photos…";

/// A library that watches `folder` and has not scanned it, with its write lock held from
/// outside: the scan that the launch starts finds its root, says so, reads its files and
/// then waits at its first write, for as long as the answer is kept. How fast a machine
/// reads eight hundred files then decides nothing - on one of CI's the whole scan was
/// over before the window had drawn its second frame.
fn held_at_its_first_write(dir: &Path, folder: &Path) -> rusqlite::Connection {
    let made = dirs::within(&dir.join("data"), &dir.join("cache"));
    std::fs::create_dir_all(made.db_path.parent().unwrap()).unwrap();
    let library = Library::open(&made.db_path).unwrap();
    library.add_watched_folder(folder, &[]).unwrap();
    drop(library);
    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    other
}

// A folder being scanned for the first time. From the first thing the window says to
// the photos being there, it is looking - never offering a folder to a library that has
// one, never saying it has found nothing in a folder it is a moment into reading. And
// the scan has its line in the status bar meanwhile.
#[test]
fn a_first_scan_is_looked_through_and_said_so_until_its_photos_are_there() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path(), 200);
    let other = held_at_its_first_write(dir.path(), &folder);
    let (mut harness, mut driver) = first_run(dir.path(), None);
    let mut seen = watched(&mut driver, &mut harness, "the scan heard from", |app| {
        !app.status().is_empty() && app.panel().is_some()
    });
    other.execute_batch("ROLLBACK").unwrap();
    seen.extend(watched(
        &mut driver,
        &mut harness,
        "the photos shown",
        |app| app.photos() == 200 && app.status().is_empty(),
    ));

    let panels: Vec<&str> = (seen.iter())
        .filter_map(|said| said.panel.as_deref())
        .collect();
    assert!(panels.contains(&LOOKING), "{seen:#?}");
    assert!(
        panels.iter().all(|text| *text == LOOKING),
        "something else was said on the way: {panels:#?}"
    );
    let lines: Vec<&String> = seen.iter().flat_map(|said| &said.lines).collect();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Scanning Pictures… ")),
        "{seen:#?}"
    );
    // A first scan has nothing to be measured against, and says so by saying no more.
    assert!(
        lines.iter().all(|line| !line.contains(" of ~")),
        "{lines:#?}"
    );
    // And at the end nothing is said: the photos are.
    let last = seen.last().unwrap();
    assert_eq!(
        (last.photos, &last.panel, last.lines.len()),
        (200, &None, 0)
    );
    assert_eq!(harness.state().watched().len(), 1);
}

// The engine publishes its first grid, and only then watches the Pictures folder and
// starts its scan. Held there - the library's write lock taken from outside, so that the
// write which watches the folder waits - the window has an empty grid, a folder list
// with nothing in it and a count of hidden photos: everything it would need to say "add
// a folder", and to say "No folders yet." in the sidebar. It says neither, because it
// does not know yet; a webview would not have loaded by then, and this window is drawn.
#[test]
fn nothing_is_said_of_an_empty_library_before_the_engine_has_started_its_scans() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path(), 40);
    let made = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    std::fs::create_dir_all(made.db_path.parent().unwrap()).unwrap();
    drop(Library::open(&made.db_path).unwrap());
    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();

    let (mut harness, mut driver) = first_run(dir.path(), Some(folder));
    driver.until(&mut harness, "the empty grid counted", |app| {
        app.photo_count().is_some()
    });
    // And for a good while after it, with every read the window makes long answered.
    let held = std::time::Instant::now();
    let seen = watched(&mut driver, &mut harness, "the engine held", |_| {
        held.elapsed() > std::time::Duration::from_millis(700)
    });
    assert!(seen.iter().all(|said| said.panel.is_none()), "{seen:#?}");
    assert!(harness.query_by_label("No folders yet.").is_none());
    assert!(harness.state().watched().is_empty(), "the engine is held");

    other.execute_batch("ROLLBACK").unwrap();
    let seen = watched(&mut driver, &mut harness, "the photos shown", |app| {
        app.photos() == 40 && app.status().is_empty()
    });
    let panels: Vec<&str> = (seen.iter())
        .filter_map(|said| said.panel.as_deref())
        .collect();
    assert!(panels.iter().all(|text| *text == LOOKING), "{panels:#?}");
}

// The line is drawn, and the sidebar lists no want of folders beside it.
#[test]
fn a_first_scan_has_its_line_drawn_in_the_status_bar() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path(), 200);
    let other = held_at_its_first_write(dir.path(), &folder);
    let (mut harness, mut driver) = first_run(dir.path(), None);
    driver.until(&mut harness, "the scan's line", |app| {
        !app.status().is_empty()
    });
    let line = harness.state().status()[0].label.clone();
    assert!(line.starts_with("Scanning Pictures… "), "{line}");
    assert!(
        harness.query_by_label(&line).is_some(),
        "{line} is not drawn"
    );
    assert!(harness.query_by_label("No folders yet.").is_none());
    other.execute_batch("ROLLBACK").unwrap();
}

// Hide folder on the only folder: the library is not empty, and says where its photos
// are. The button shows them.
#[test]
fn a_library_whose_photos_are_all_hidden_says_so_and_shows_them() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 300);
    let mut harness = launch(&library);
    let mut driver = Driver::new(&harness);
    let seen = watched(&mut driver, &mut harness, "the panel", |app| {
        app.panel().is_some()
    });
    let panels: Vec<&str> = (seen.iter())
        .filter_map(|said| said.panel.as_deref())
        .collect();
    assert_eq!(
        panels,
        ["The library has no photo to show here: all of them are in Hidden, in the sidebar."],
        "nothing else is said first"
    );
    assert_eq!(
        harness.state().panel().unwrap().title,
        "Every photo is hidden"
    );
    driver.settle(&mut harness);

    click(&mut driver, &mut harness, "Show hidden photos");
    driver.until(&mut harness, "the hidden photos shown", |app| {
        app.settled().view == GridView::Hidden && app.photos() == 300
    });
    assert_eq!(harness.state().panel(), None);
}

// Nothing watched and no Pictures folder to start with: the first folder is offered, by
// a button that is drawn where it will be and takes no press yet.
#[test]
fn a_library_that_watches_nothing_offers_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver) = first_run(dir.path(), None);
    let seen = watched(&mut driver, &mut harness, "the panel", |app| {
        app.panel().is_some()
    });
    let panels: Vec<&str> = (seen.iter())
        .filter_map(|said| said.panel.as_deref())
        .collect();
    assert_eq!(panels.len(), 1, "{panels:#?}");
    assert!(panels[0].starts_with("Choose a folder and photon shows the photos in it."));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);

    let add = harness.get_by_label("Add folder…");
    assert!(add.accesskit_node().is_disabled());
    assert!(harness.query_by_label("Watched folders…").is_none());
    // And the sidebar says there is no folder, where its folders would be listed.
    assert!(harness.query_by_label("No folders yet.").is_some());
    assert_eq!(harness.state().photo_count().as_deref(), Some("0 photos"));
    // With nothing reporting, the window is still.
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A watched folder on a drive that is not there: out of reach, not empty - it may hold
// every photo the user has.
#[test]
fn a_library_whose_drive_is_away_says_it_cannot_reach_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 0, 0, 0);
    let mut harness = launch(&library);
    let mut driver = Driver::new(&harness);
    let seen = watched(
        &mut driver,
        &mut harness,
        "the drive known to be away",
        |app| app.panel().is_some_and(|panel| panel.text != LOOKING),
    );
    let panels: Vec<&str> = (seen.iter())
        .filter_map(|said| said.panel.as_deref())
        .collect();
    let last = panels.last().unwrap();
    assert!(last.starts_with("photon cannot reach "), "{last}");
    assert!(last.ends_with("photon never moves, changes or deletes them."));
    // Until its scan had said so, photon was looking - and said nothing else.
    assert!(
        panels[..panels.len() - 1]
            .iter()
            .all(|text| *text == LOOKING),
        "{panels:#?}"
    );
    // Both buttons are there, and neither does anything yet.
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    for label in ["Add folder…", "Watched folders…"] {
        assert!(harness.get_by_label(label).accesskit_node().is_disabled());
    }
}
