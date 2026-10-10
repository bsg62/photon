//! The shell and the views, through the whole application: a library on disk, the engine,
//! and the frames a window would draw (`common::Driver`).

mod common;

use common::{Driver, Harness, launch, library};
use eframe::egui::{self, Key, Modifiers, pos2, vec2};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_ui::{
    nav::Place,
    sidebar::rows::Counts,
    window_layout::{Layout, SIDEBAR_DEFAULT},
};
use std::time::{Duration, Instant};

/// Clicks the sidebar row called `label`, and draws the frames the click is in.
fn click(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// Launches over `library` and waits until its grid and its counts are there.
fn opened<'a>(library: &photon_ui::fixture::Fixture, photos: usize) -> (Harness<'a>, Driver) {
    let mut harness = launch(library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == photos && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver)
}

#[test]
fn a_click_on_a_view_shows_it_and_the_row_follows_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 3);
    let (mut harness, mut driver) = opened(&library, 897);
    driver.until(&mut harness, "the counts read", |app| {
        *app.counts()
            == Counts {
                starred: 5,
                hidden: 3,
                ..Counts::default()
            }
    });
    assert_eq!(harness.state().photo_count().as_deref(), Some("897 photos"));
    // The rows a library has only while it holds something of the kind.
    assert!(harness.query_by_label("Hidden").is_some());
    assert!(harness.query_by_label("Videos").is_none());
    assert!(harness.query_by_label("Duplicates").is_none());

    click(&mut driver, &mut harness, "Starred");
    // Where the user is going, before the rebuild has landed.
    assert_eq!(harness.state().place(), Place::of(GridView::Starred));
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    assert_eq!(harness.state().photos(), 5);
    assert_eq!(harness.state().photo_count().as_deref(), Some("5 photos"));

    click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photos shown", |app| {
        *app.settled() == Place::of(GridView::Hidden)
    });
    assert_eq!(harness.state().photos(), 3);

    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "every photo shown again", |app| {
        *app.settled() == Place::of(GridView::All)
    });
    assert_eq!(harness.state().photos(), 897);
    assert_eq!(harness.state().notice(), None);
}

// Two clicks before the first has landed. Each is a rebuild on the engine's side that
// cannot be taken back once begun, so they are made in the order asked: answered by the
// latest alone, the first could land last, under a row marking the second.
#[test]
fn clicks_land_in_the_order_they_were_made() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    for _ in 0..4 {
        click(&mut driver, &mut harness, "Starred");
        click(&mut driver, &mut harness, "Recent");
        click(&mut driver, &mut harness, "All photos");
        click(&mut driver, &mut harness, "Starred");
        assert_eq!(harness.state().place(), Place::of(GridView::Starred));
        driver.until(&mut harness, "the last click shown", |app| {
            *app.settled() == app.place()
        });
        driver.settle(&mut harness);
        assert_eq!(harness.state().settled(), &Place::of(GridView::Starred));
        assert_eq!(harness.state().photos(), 5);
        click(&mut driver, &mut harness, "All photos");
        driver.until(&mut harness, "back in all photos", |app| {
            *app.settled() == Place::of(GridView::All) && app.photos() == 900
        });
    }
}

// All photos most of all: the click means "back to where I was", and the user is there.
// Asked of the engine anyway it is a rebuild of the whole library for nothing.
#[test]
fn a_click_on_the_row_that_is_shown_asks_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 2, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    driver.settle(&mut harness);
    let version = harness.state().version();
    click(&mut driver, &mut harness, "All photos");
    click(&mut driver, &mut harness, "All photos");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().version(), version, "no grid was built");

    // The same of a row clicked twice before its view has landed: one rebuild, not two.
    click(&mut driver, &mut harness, "Starred");
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().version(), version + 1);
}

#[test]
fn an_empty_view_says_what_it_is_empty_of() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    assert_eq!(harness.state().notice(), None);
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred view shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    assert_eq!(harness.state().photos(), 0);
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("No starred photos. Star one in the viewer, or in Picasa.")
    );
    assert_eq!(harness.state().photo_count().as_deref(), Some("0 photos"));
}

#[test]
fn on_this_day_is_a_search_for_todays_date() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    click(&mut driver, &mut harness, "On this day");
    driver.until(&mut harness, "the search shown", |app| {
        app.settled().view == GridView::Search
    });
    let query = harness.state().settled().arg.clone();
    assert!(
        query.starts_with("on:") && query.len() == "on:MM-DD".len(),
        "{query}"
    );
}

// The photos are another list: a place in the old one names nothing in this one.
#[test]
fn another_view_is_shown_from_its_top() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    // The wheel, over the grid.
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the grid scrolled", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.position > 1000.0)
    });

    click(&mut driver, &mut harness, "Recent");
    driver.until(&mut harness, "the recent photos shown", |app| {
        *app.settled() == Place::of(GridView::Recent)
    });
    driver.until(&mut harness, "the grid drawn again", |app| {
        app.last_frame().is_some_and(|frame| frame.position == 0.0)
    });
    assert_eq!(harness.state().photos(), 500);
}

/// Waits, in real time, until the layout's file holds what the application does: it is
/// written off the UI's thread.
fn stored(harness: &Harness<'_>, path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let wanted = harness.state().layout().written();
    while std::fs::read_to_string(path).ok().as_deref() != Some(wanted.as_str()) {
        assert!(Instant::now() < deadline, "the layout was never stored");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_sidebar_is_as_wide_at_the_next_launch_as_it_was_dragged() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let path = library.dirs().layout_path;
    {
        let (mut harness, mut driver) = opened(&library, 300);
        assert_eq!(*harness.state().layout(), Layout::default());
        // The splitter, dragged eighty points to the right.
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 300.0);
        harness.event(egui::Event::PointerMoved(hold));
        harness.event(egui::Event::PointerButton {
            pos: hold,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        let to = pos2(hold.x + 80.0, 300.0);
        harness.event(egui::Event::PointerMoved(to));
        driver.act(&mut harness);
        // Followed while it is held.
        assert_eq!(
            harness.state().layout().sidebar_width,
            SIDEBAR_DEFAULT + 80.0
        );
        harness.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        driver.act(&mut harness);
        driver.act(&mut harness);
        stored(&harness, &path);
    }
    let (harness, _) = opened(&library, 300);
    assert_eq!(
        harness.state().layout().sidebar_width,
        SIDEBAR_DEFAULT + 80.0
    );
    assert!(!harness.state().layout().sidebar_hidden);
}

#[test]
fn a_sidebar_hidden_is_hidden_at_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let path = library.dirs().layout_path;
    {
        let (mut harness, mut driver) = opened(&library, 300);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
        driver.act(&mut harness);
        assert!(harness.state().layout().sidebar_hidden);
        stored(&harness, &path);
    }
    let (harness, _) = opened(&library, 300);
    assert!(harness.state().layout().sidebar_hidden);
    // Hidden, its rows are not there to be clicked.
    assert!(harness.query_by_label("Starred").is_none());
    assert!(harness.query_by_label("Show sidebar").is_some());
}

// The counts are read once at launch and again whenever the library says it has changed:
// a photo hidden is one more in Hidden and one fewer in the grid.
#[test]
fn the_counts_follow_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 2);
    let (mut harness, mut driver) = opened(&library, 298);
    driver.until(&mut harness, "the counts read", |app| {
        app.counts().hidden == 2
    });
    let first = harness.state().last_frame().unwrap().on_screen[0];
    let engine = harness.state().engine().clone();
    photon_engine::commands::set_items_hidden(&engine, &[first], true).unwrap();
    driver.until(&mut harness, "the count moved", |app| {
        app.counts().hidden == 3 && app.photos() == 297
    });
}

// The shell around a still grid is as still as the grid: no frame in five seconds that
// nothing asked for. A bar that repainted by itself would be drawn sixty times a second
// for as long as the window is open.
#[test]
fn a_still_window_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 3);
    let (mut harness, mut driver) = opened(&library, 897);
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred) && app.counts().starred == 5
    });
    driver.settle(&mut harness);
    // What the click and the scroll back to the top left to be drawn, drawn.
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
