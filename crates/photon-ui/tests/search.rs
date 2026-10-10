//! The search box, through the whole application: typed into, with the engine searching
//! as it is typed, on the clock a window would keep.

mod common;

use common::{Driver, Harness, click, library, opened};
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use photon_core::{grid::GridView, library::Library, now_ms};
use photon_ui::nav::Place;

/// Ctrl+F, and the frame it is in.
fn find(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(harness);
}

/// Types `text` into whatever has the keys, and draws the frame it is in.
fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

fn escape(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(harness);
}

/// Draws frames until the grid shows `place`.
fn shown(driver: &mut Driver, harness: &mut Harness<'_>, place: &Place) {
    driver.until(harness, "the place shown", |app| app.settled() == place);
}

// Ten file names begin so: IMG_000000 to IMG_000009.
const TEN: &str = "IMG_00000";

#[test]
fn a_search_is_made_as_it_is_typed_once_the_typing_stops() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "IMG_0");
    typed(&mut driver, &mut harness, "0000");
    assert_eq!(harness.state().search_text(), TEN);
    // Not yet: the typing may go on.
    driver.frames_in(&mut harness, 0.1);
    assert_eq!(harness.state().place(), Place::of(GridView::All));

    shown(&mut driver, &mut harness, &Place::search(TEN));
    assert_eq!(harness.state().photos(), 10);
    assert_eq!(harness.state().photo_count().as_deref(), Some("10 photos"));
    // Refined, it is another search, from its top.
    typed(&mut driver, &mut harness, "5");
    shown(&mut driver, &mut harness, &Place::search("IMG_000005"));
    assert_eq!(harness.state().photos(), 1);
    // And one that finds nothing says so, with the words that were typed.
    typed(&mut driver, &mut harness, " nothing");
    driver.until(&mut harness, "the empty search shown", |app| {
        app.photos() == 0
    });
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("No photos match “IMG_000005 nothing”")
    );
}

// A search typed and not yet sent, and a click on Starred: sent after the click it would
// re-enter Search and replace the grid the user had just asked for.
#[test]
fn a_click_on_a_view_is_not_followed_by_the_search_that_was_half_typed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, TEN);
    click(&mut driver, &mut harness, "Starred");
    shown(&mut driver, &mut harness, &Place::of(GridView::Starred));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(app.settled(), &Place::of(GridView::Starred));
    assert_eq!(app.place(), Place::of(GridView::Starred));
    // And the box is empty, as the engine's query is.
    assert_eq!(app.search_text(), "");
}

// The same of a folder: the grid goes to it in All photos, and stays there.
#[test]
fn a_click_on_a_folder_is_not_followed_by_the_search_that_was_half_typed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        !app.folders().is_empty()
    });
    driver.settle(&mut harness);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, TEN);
    click(&mut driver, &mut harness, "folder-0000");
    driver.settle(&mut harness);
    // Past the moment the search was due, and then for as long as a search would take.
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().place(), Place::of(GridView::All));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(app.settled(), &Place::of(GridView::All));
    assert!(app.last_frame().unwrap().position > 1000.0);
}

#[test]
fn the_box_holds_the_search_that_is_shown_and_is_empty_outside_one() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    Library::open(&library.dirs().db_path)
        .unwrap()
        .create_saved_search("Lakes", TEN, now_ms())
        .unwrap();
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the saved search read", |app| {
        app.collections().searches.len() == 1
    });
    driver.settle(&mut harness);

    // A saved search's row runs it as though it had been typed.
    click(&mut driver, &mut harness, "Lakes");
    assert_eq!(harness.state().search_text(), TEN);
    shown(&mut driver, &mut harness, &Place::search(TEN));
    // A switch away clears the engine's query, and the box with it.
    click(&mut driver, &mut harness, "Starred");
    assert_eq!(harness.state().search_text(), "");
    shown(&mut driver, &mut harness, &Place::of(GridView::Starred));
    assert_eq!(harness.state().search_text(), "");
}

#[test]
fn escape_leaves_a_search_and_leaves_any_other_view_alone() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, TEN);
    shown(&mut driver, &mut harness, &Place::search(TEN));

    escape(&mut driver, &mut harness);
    assert_eq!(harness.state().search_text(), "");
    shown(&mut driver, &mut harness, &Place::of(GridView::All));
    assert_eq!(harness.state().photos(), 900);

    // In Starred with an empty box there is nothing to clear: clearing is the empty
    // search, which is All photos, and the key would throw the user out of the view.
    click(&mut driver, &mut harness, "Starred");
    shown(&mut driver, &mut harness, &Place::of(GridView::Starred));
    driver.settle(&mut harness);
    let version = harness.state().version();
    find(&mut driver, &mut harness);
    driver.act(&mut harness);
    escape(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().place(), Place::of(GridView::Starred));
    assert_eq!(harness.state().version(), version);
}

#[test]
fn the_bookmark_saves_the_search_and_the_sidebar_lists_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, TEN);
    shown(&mut driver, &mut harness, &Place::search(TEN));
    driver.settle(&mut harness);
    assert!(harness.state().collections().searches.is_empty());

    click(&mut driver, &mut harness, "Save this search");
    driver.until(&mut harness, "the search saved and read back", |app| {
        app.collections().searches.len() == 1
    });
    driver.settle(&mut harness);
    let saved = &harness.state().collections().searches[0];
    assert_eq!((saved.name.as_str(), saved.query.as_str()), (TEN, TEN));
    // Its row is in the sidebar, and the bookmark says it is saved.
    assert!(harness.query_by_label("Searches").is_some());
    assert!(harness.query_by_label("Save this search").is_none());
    assert!(harness.query_by_label("Saved as “IMG_00000”").is_some());
}

#[test]
fn a_term_picked_from_the_help_is_searched_for() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    click(&mut driver, &mut harness, "What you can search for");
    driver.settle(&mut harness);
    assert!(harness.state().search_help_open());
    click(&mut driver, &mut harness, "is:starred");
    assert_eq!(harness.state().search_text(), "is:starred");
    shown(&mut driver, &mut harness, &Place::search("is:starred"));
    assert_eq!(harness.state().photos(), 5);
    assert!(!harness.state().search_help_open());
}

// A search shown is as still as any other view, and the frame its send was due in is the
// last one asked for.
#[test]
fn a_window_with_a_search_shown_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    find(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, TEN);
    shown(&mut driver, &mut harness, &Place::search(TEN));
    harness.key_press_modifiers(Modifiers::NONE, Key::Enter);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
