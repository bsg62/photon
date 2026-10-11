//! The sidebar's menus and the one field a name is typed in, through the whole
//! application and the real engine: an album made and renamed, a saved search renamed, a
//! folder named in photon, hidden, brought back and scanned again.
//!
//! Every write here goes into a library the test made, in a temporary directory, and
//! none of them outside `library.db`: a folder's name in photon is the library's own.

mod common;

use common::{Driver, Harness, REFRESH, library, opened, photo_ids, sources};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::{grid::GridView, library::Library};
use photon_engine::commands;
use photon_ui::{
    app::App,
    dirs,
    fixture::{Fixture, PER_FOLDER},
    nav::Place,
    rename::Of,
};
use std::{path::Path, time::Duration};

fn other(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The other button on `at`, and the frame its menu is first drawn in.
fn right_click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(other(at, true));
    harness.event(other(at, false));
    driver.act(harness);
    // A new area is measured in one frame and shown in the next.
    driver.act(harness);
}

/// The other button on the sidebar's row called `label`.
fn right_click_row(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    right_click_at(driver, harness, at);
}

/// The other button on the header over the photo at grid offset `offset`.
fn right_click_header(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let tile = (harness.state().tile_rect(offset)).expect("the photo is laid out");
    right_click_at(driver, harness, tile.center_top() - egui::vec2(0.0, 14.0));
}

/// Picks the item of the open menu called `label`.
fn pick_item(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
    assert!(!harness.state().menu_open());
    // The field, where the item opens one, takes the keyboard in the frame it is drawn.
    driver.act(harness);
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key, held: Modifiers) {
    harness.key_press_modifiers(held, key);
    driver.act(harness);
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

/// The words of the messages on screen.
fn said(harness: &Harness<'_>) -> Vec<String> {
    (harness.state().toasts().iter())
        .map(|toast| toast.message.clone())
        .collect()
}

fn album(library: &Fixture, name: &str) -> i64 {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.create_album(name, 1_700_000_000_000).unwrap().id
}

fn album_names(app: &App) -> Vec<String> {
    (app.collections().albums.iter())
        .map(|album| album.name.clone())
        .collect()
}

/// The fixture's folder that is `nth` in the grid: the newest first.
fn folder_at(harness: &Harness<'_>, library: &Fixture, nth: usize) -> i64 {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let id = harness.state().shown()[nth * PER_FOLDER];
    lib.item(id).unwrap().unwrap().folder_id
}

fn alias(app: &App, folder: i64) -> Option<String> {
    app.folders()
        .get(&folder)
        .and_then(|folder| folder.alias.clone())
}

// An album's row has a menu, and "Rename…" opens the field in the row's own place with
// the name in it, all selected: what is typed replaces it, and Enter stores it.
#[test]
fn an_album_is_renamed_in_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let lisbon = album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    assert!(harness.state().menu_open());
    // "Delete…" is in its place and takes no press: it asks first, and what it asks
    // with is the next pull request's.
    harness.get_by_label("Delete…").click();
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(album_names(harness.state()), ["Lisbon"]);
    pick_item(&mut driver, &mut harness, "Rename…");
    assert_eq!(
        harness.state().naming(),
        Some((Of::Album(lisbon), "Lisbon".to_owned(), false))
    );
    typed(&mut driver, &mut harness, "Porto ");
    typed(&mut driver, &mut harness, "2024");
    assert_eq!(
        harness.state().naming(),
        Some((Of::Album(lisbon), "Porto 2024".to_owned(), false))
    );
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the name stored and shown", |app| {
        app.naming().is_none() && album_names(app) == ["Porto 2024"]
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("Porto 2024").is_some());
    assert_eq!(said(&harness), Vec::<String>::new());
    let lib = Library::open(&library.dirs().db_path).unwrap();
    assert_eq!(lib.album(lisbon).unwrap().unwrap().name, "Porto 2024");

    // Escape closes the field and stores nothing.
    right_click_row(&mut driver, &mut harness, "Porto 2024");
    pick_item(&mut driver, &mut harness, "Rename…");
    typed(&mut driver, &mut harness, "Nothing");
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert_eq!(harness.state().naming(), None);
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Porto 2024"]);
}

// "New album…", under the albums, is where a new one's name is typed: with no photos
// in it, and none at all for a blank name.
#[test]
fn a_new_album_is_made_from_the_row_under_the_albums() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    harness.get_by_label("New album…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().naming(),
        Some((Of::NewAlbum, String::new(), false))
    );
    typed(&mut driver, &mut harness, "  Trips ");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the album made and listed", |app| {
        app.naming().is_none() && album_names(app) == ["Trips"]
    });
    assert_eq!(harness.state().collections().albums[0].count, 0);
    // A blank name is a cancel: Escape and an empty Enter feel the same.
    driver.act(&mut harness);
    harness.get_by_label("New album…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "   ");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(harness.state().naming(), None);
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Trips"]);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// A name the library will not take leaves the field open with the name in it, under a
// message, to be put right. (The album was deleted from elsewhere a moment before.)
#[test]
fn a_name_the_library_refuses_leaves_the_field_open() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let lisbon = album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    pick_item(&mut driver, &mut harness, "Rename…");
    typed(&mut driver, &mut harness, "Porto");
    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, lisbon).unwrap();
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert_eq!(
        harness.state().naming(),
        Some((Of::Album(lisbon), "Porto".to_owned(), false))
    );
}

#[test]
fn a_saved_search_is_renamed_in_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let saved = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Tens", "IMG_00000", 1_700_000_000_000)
            .unwrap()
            .id
    };
    // An album with the same number as the search: one field, in the search's row.
    assert_eq!(album(&library, "Lisbon"), saved);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the lists read", |app| {
        app.collections().searches.len() == 1
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Tens");
    pick_item(&mut driver, &mut harness, "Rename…");
    assert_eq!(
        harness.state().naming(),
        Some((Of::Search(saved), "Tens".to_owned(), false))
    );
    typed(&mut driver, &mut harness, "The first ten");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the name stored and shown", |app| {
        app.naming().is_none() && app.collections().searches[0].name == "The first ten"
    });
    assert_eq!(album_names(harness.state()), ["Lisbon"]);
    // The grid was left alone: a saved search is a name over a query.
    assert_eq!(harness.state().settled(), &Place::of(GridView::All));
}

// A folder's name in photon: the directory keeps its own, and photon shows the folder
// by the name typed - on its row and on its header in the grid. "Use folder name" and a
// blank name both bring the directory's back.
#[test]
fn a_folder_is_named_in_photon_and_given_its_own_name_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    driver.settle(&mut harness);
    let folder = folder_at(&harness, &library, 1);
    let name = harness.state().folders()[&folder].name.clone();
    right_click_row(&mut driver, &mut harness, &name);
    // With no name in photon there is none to take away.
    assert!(harness.query_by_label("Use folder name").is_none());
    pick_item(&mut driver, &mut harness, "Rename in photon…");
    assert_eq!(
        harness.state().naming(),
        Some((Of::Folder(folder), name.clone(), false))
    );
    typed(&mut driver, &mut harness, "Summer");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the name stored and shown", |app| {
        app.naming().is_none() && alias(app, folder).as_deref() == Some("Summer")
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("Summer").is_some());
    assert!(harness.query_by_label(&name).is_none());
    assert_eq!(harness.state().photos(), 900);

    // Its own name back, from the menu.
    right_click_row(&mut driver, &mut harness, "Summer");
    pick_item(&mut driver, &mut harness, "Use folder name");
    driver.until(&mut harness, "its own name shown", |app| {
        alias(app, folder).is_none()
    });
    // And by a blank name: named again, then the field emptied.
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, &name);
    pick_item(&mut driver, &mut harness, "Rename in photon…");
    typed(&mut driver, &mut harness, "Winter");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "named again", |app| {
        app.naming().is_none() && alias(app, folder).as_deref() == Some("Winter")
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Winter");
    pick_item(&mut driver, &mut harness, "Rename in photon…");
    // The name photon shows it by is the one to correct, not the directory's.
    assert_eq!(
        harness.state().naming(),
        Some((Of::Folder(folder), "Winter".to_owned(), false))
    );
    typed(&mut driver, &mut harness, " ");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "its own name shown again", |app| {
        app.naming().is_none() && alias(app, folder).is_none()
    });
    assert_eq!(said(&harness), Vec::<String>::new());
    // Nothing was renamed but in the library: the directory's name is what it was.
    assert_eq!(harness.state().folders()[&folder].name, name);
}

// The folder's menu is the same on its header in the grid. "Rename in photon…" there
// shows a hidden sidebar, with the field in the folder's row; and what is typed in the
// field is typed in the field - an `h` hides nothing.
#[test]
fn a_folder_is_named_from_its_header_in_the_grid_with_the_sidebar_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    driver.settle(&mut harness);
    let folder = folder_at(&harness, &library, 0);
    // A photo selected, as `h` would hide it.
    let at = harness.state().tile_rect(2).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[2]]);
    key(&mut driver, &mut harness, Key::B, Modifiers::COMMAND);
    assert!(harness.state().layout().sidebar_hidden);
    driver.act(&mut harness);

    right_click_header(&mut driver, &mut harness, 0);
    assert!(harness.state().menu_open());
    // The header's press selected nothing and cleared nothing.
    assert_eq!(harness.state().selected(), [ids[2]]);
    pick_item(&mut driver, &mut harness, "Rename in photon…");
    assert!(!harness.state().layout().sidebar_hidden);
    assert!(matches!(
        harness.state().naming(),
        Some((Of::Folder(id), _, false)) if id == folder
    ));
    typed(&mut driver, &mut harness, "h");
    typed(&mut driver, &mut harness, ".");
    harness.key_press_modifiers(Modifiers::NONE, Key::H);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().naming(),
        Some((Of::Folder(folder), "h.".to_owned(), false))
    );
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the name stored", |app| {
        app.naming().is_none() && alias(app, folder).as_deref() == Some("h.")
    });
    driver.settle(&mut harness);
    assert_eq!(harness.state().photos(), 900);
    assert_eq!(harness.state().selected(), [ids[2]]);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// "Hide folder": its photos leave the grid as any hidden photo does, and its row the
// list. In Hidden the same menu brings it back.
#[test]
fn a_folder_is_hidden_from_its_menu_and_brought_back_in_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    driver.settle(&mut harness);
    let folder = folder_at(&harness, &library, 1);
    let name = harness.state().folders()[&folder].name.clone();
    right_click_row(&mut driver, &mut harness, &name);
    pick_item(&mut driver, &mut harness, "Hide folder");
    driver.until(&mut harness, "the folder's photos gone", |app| {
        app.photos() == 900 - PER_FOLDER && app.folders()[&folder].hidden
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label(&name).is_none());
    assert_eq!(said(&harness), Vec::<String>::new());

    harness.get_by_label("Hidden").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the hidden photos shown", |app| {
        app.settled() == &Place::of(GridView::Hidden) && app.photos() == PER_FOLDER
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, &name);
    assert!(harness.query_by_label("Hide folder").is_none());
    pick_item(&mut driver, &mut harness, "Unhide folder");
    driver.until(&mut harness, "nothing hidden", |app| {
        app.photos() == 0 && !app.folders()[&folder].hidden
    });
    harness.get_by_label("All photos").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "all of them back", |app| {
        app.settled() == &Place::of(GridView::All) && app.photos() == 900
    });
}

/// A folder called Pictures under `dir` with four real photos in it.
fn pictures(dir: &Path) -> std::path::PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let picture = std::fs::read(&sources(dir)[0]).unwrap();
    for n in 0..4_usize {
        let mut bytes = picture.clone();
        bytes.extend(std::iter::repeat_n(0, n + 1));
        std::fs::write(folder.join(format!("IMG_{n}.jpg")), bytes).unwrap();
    }
    folder
}

// "Rescan" scans the watched folder a folder belongs to, and is not offered while that
// scan runs: the engine does nothing with a second one, and says nothing back. The scan
// is held at its last write by the library's write lock, taken from outside.
#[test]
fn rescan_starts_a_scan_and_is_not_offered_while_it_runs() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    let made = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    let (dirs, watched) = (made.clone(), folder.clone());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| App::new(cc, dirs, Some(watched)).unwrap());
    let mut driver = Driver::new(&harness);
    driver.until(
        &mut harness,
        "the photos shown and their scan over",
        |app| {
            app.photos() == 4
                && app.status().is_empty()
                && app.last_frame().is_some_and(|frame| frame.settled)
        },
    );
    driver.settle(&mut harness);
    // Nothing in the folder has changed, so nothing but the menu starts a scan; and a
    // scan of a folder that is there always ends by writing that it is.
    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();

    right_click_row(&mut driver, &mut harness, "Pictures");
    pick_item(&mut driver, &mut harness, "Rescan");
    driver.until(&mut harness, "the scan heard from", |app| {
        !app.status().is_empty()
    });
    driver.act(&mut harness);
    // While it runs the item is in its place and takes no press.
    right_click_row(&mut driver, &mut harness, "Pictures");
    harness.get_by_label("Rescan").click();
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the scan over", |app| app.status().is_empty());
    assert_eq!(harness.state().photos(), 4);
}

// A menu is of an album, a search or a folder that is listed: one deleted from
// elsewhere while its menu is open leaves a menu of nothing, which closes.
#[test]
fn a_menu_whose_album_is_gone_closes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let lisbon = album(&library, "Lisbon");
    album(&library, "Porto");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    assert!(harness.state().menu_open());
    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, lisbon).unwrap();
    // The lists are read when the library says its data changed.
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "the album gone from the list", |app| {
        album_names(app) == ["Porto"]
    });
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
}

// The sidebar hidden over a name half typed: the name is stored, as leaving its field
// stores it, and the keys are the grid's.
#[test]
fn hiding_the_sidebar_stores_the_name_being_typed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    pick_item(&mut driver, &mut harness, "Rename…");
    typed(&mut driver, &mut harness, "Porto");
    key(&mut driver, &mut harness, Key::B, Modifiers::COMMAND);
    assert!(harness.state().layout().sidebar_hidden);
    driver.until(&mut harness, "the name stored", |app| {
        app.naming().is_none() && album_names(app) == ["Porto"]
    });
    driver.settle(&mut harness);
    key(&mut driver, &mut harness, Key::Home, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[0]]);
}

// A window with a name's field open and nothing typed draws nothing - but for the
// caret, which blinks as in any application and which this harness switches off.
#[test]
fn a_window_with_a_name_being_typed_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    pick_item(&mut driver, &mut harness, "Rename…");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.state().naming().is_some());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
