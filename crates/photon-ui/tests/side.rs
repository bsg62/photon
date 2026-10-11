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

// What a review of the menus and the field found, each with the test that showed it.

fn first(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The letter `h` as a keyboard sends it: the key, and what it types.
fn press_h(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.key_press_modifiers(Modifiers::NONE, Key::H);
    harness.event(egui::Event::Text("h".to_owned()));
    driver.act(harness);
    driver.act(harness);
}

/// The middle of the search box's line of text: the text input nearest the window's top
/// (a name's field in the sidebar is one too).
fn search_box(harness: &Harness<'_>) -> Pos2 {
    (harness.query_all_by_role(egui::accesskit::Role::TextInput))
        .map(|node| node.rect())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the search box")
        .center()
}

/// Whether the library says photo `id` is hidden.
fn hidden(library: &Fixture, id: i64) -> bool {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.item(id).unwrap().unwrap().hidden
}

/// Selects the photo at `offset` with a click.
fn select(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = harness.state().tile_rect(offset).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(first(at, true));
    harness.event(first(at, false));
    driver.act(harness);
}

/// The library's write lock, taken from outside: the engine's next write waits for it.
/// It stands in for a write that takes a while - a folder's name waits for a rebuild of
/// the whole grid, and every write waits for the writes queued before it.
fn locked(library: &Fixture) -> rusqlite::Connection {
    let other = rusqlite::Connection::open(&library.dirs().db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    other
}

/// An album's field open with a new name typed in it, and a photo selected for `h` to
/// hide.
fn renaming<'a>(library: &Fixture) -> (Harness<'a>, Driver, Vec<i64>) {
    let ids = photo_ids(library);
    album(library, "Lisbon");
    let (mut harness, mut driver) = opened(library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    select(&mut driver, &mut harness, 2);
    assert_eq!(harness.state().selected(), [ids[2]]);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    pick_item(&mut driver, &mut harness, "Rename…");
    typed(&mut driver, &mut harness, "Porto");
    assert!(matches!(
        harness.state().naming(),
        Some((Of::Album(_), name, false)) if name == "Porto"
    ));
    (harness, driver, ids)
}

// Ctrl+F out of a name being typed stores the name - and the search box, which was
// asked for, has the keyboard: what is typed next is the search. A field that asked for
// the keyboard back while its name was stored took it from the box; when its write
// landed it closed, nothing had the keyboard, and what was typed was the grid's: `h`
// hid the photo. The write is held at the library's lock, so that it is on its way for
// as long as the test needs.
#[test]
fn ctrl_f_out_of_a_name_being_stored_leaves_the_keyboard_in_the_search_box() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, ids) = renaming(&library);
    let other = locked(&library);
    key(&mut driver, &mut harness, Key::F, Modifiers::COMMAND);
    driver.act(&mut harness);
    driver.act(&mut harness);
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the name stored", |app| {
        app.naming().is_none() && album_names(app) == ["Porto"]
    });
    driver.settle(&mut harness);
    press_h(&mut driver, &mut harness);
    driver.settle(&mut harness);
    assert!(!hidden(&library, ids[2]), "`h` hid the selected photo");
    assert_eq!(harness.state().search_text(), "h");
}

// A click in the search box out of a name being typed: the same.
#[test]
fn a_click_in_the_search_box_out_of_a_name_being_stored_leaves_the_keyboard_there() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, ids) = renaming(&library);
    let at = search_box(&harness);
    let other = locked(&library);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(first(at, true));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.event(first(at, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the name stored", |app| {
        app.naming().is_none() && album_names(app) == ["Porto"]
    });
    driver.settle(&mut harness);
    press_h(&mut driver, &mut harness);
    driver.settle(&mut harness);
    assert!(!hidden(&library, ids[2]), "`h` hid the selected photo");
    assert_eq!(harness.state().search_text(), "h");
}

// A press on the sort control out of a name being typed stores the name and opens the
// control's list: a field that took the keyboard back took it from the control, and a
// list whose control has no keyboard closes.
#[test]
fn the_sort_list_opens_at_a_click_out_of_a_name_being_stored() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, _) = renaming(&library);
    let other = locked(&library);
    harness.get_by_label("Sort by: Date taken").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.act(&mut harness);
    let open = harness.query_by_label("Name").is_some();
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the name stored", |app| {
        app.naming().is_none() && album_names(app) == ["Porto"]
    });
    assert!(open, "the list the click opened was closed under it");
}

// "Hide folder" on a folder whose photos are all hidden already - one by one, which is
// how a folder comes to be listed in Hidden without being a hidden folder - changes the
// folder and no photo. The engine rebuilds nothing for it, so the folder list is read
// again by the write's own answer: left to a rebuild, the menu went on offering "Hide
// folder" for a folder that was hidden, with no way to bring it back.
#[test]
fn a_folder_hidden_with_no_photo_left_to_hide_offers_to_bring_it_back() {
    let dir = tempfile::tempdir().unwrap();
    // The last folder's photos, hidden one by one.
    let library = library(dir.path(), 900, 0, PER_FOLDER);
    let (mut harness, mut driver) = opened(&library, 900 - PER_FOLDER);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    harness.get_by_label("Hidden").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the hidden photos shown", |app| {
        app.settled() == &Place::of(GridView::Hidden) && app.photos() == PER_FOLDER
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let folder = lib
        .item(harness.state().shown()[0])
        .unwrap()
        .unwrap()
        .folder_id;
    let name = harness.state().folders()[&folder].name.clone();
    assert!(!harness.state().folders()[&folder].hidden);
    right_click_row(&mut driver, &mut harness, &name);
    pick_item(&mut driver, &mut harness, "Hide folder");
    // The write is made: the library says the folder is hidden.
    let stored = |lib: &Library| {
        (lib.folders().unwrap().iter())
            .find(|listed| listed.id == folder)
            .is_some_and(|listed| listed.hidden)
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !stored(&lib) {
        assert!(
            std::time::Instant::now() < deadline,
            "the hide was never stored"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
    right_click_row(&mut driver, &mut harness, &name);
    assert!(harness.state().menu_open());
    assert!(
        harness.query_by_label("Unhide folder").is_some(),
        "a hidden folder's menu still offers to hide it"
    );
}

// A name the library refused, with the sidebar hidden over it: the field is left open
// (`Rename::refused`) where nobody sees it. Shown again it is drawn with the name in
// it, and has the keyboard - without it, what was typed "into" it was the grid's. And
// showing the sidebar sends no name: only hiding it stores one.
#[test]
fn a_refused_field_shown_again_with_the_sidebar_has_the_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, ids) = renaming(&library);
    let lisbon = harness.state().collections().albums[0].id;
    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, lisbon).unwrap();
    key(&mut driver, &mut harness, Key::B, Modifiers::COMMAND);
    assert!(harness.state().layout().sidebar_hidden);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert!(
        harness.state().naming().is_some(),
        "the field is open, unseen"
    );
    key(&mut driver, &mut harness, Key::B, Modifiers::COMMAND);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().naming().is_some());
    press_h(&mut driver, &mut harness);
    assert!(
        harness.state().naming().is_some(),
        "the field is still open"
    );
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(said(&harness).len(), 1, "{:?}", said(&harness));
    assert!(
        !hidden(&library, ids[2]),
        "`h` typed at an open field hid the selected photo"
    );
}

/// Turns the wheel over the sidebar's list and draws the frames egui spreads it over.
fn wheel(driver: &mut Driver, harness: &mut Harness<'_>, delta: f32) {
    let at = egui::pos2(100.0, 400.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, delta),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..40 {
        driver.act(harness);
    }
}

// A name the library refuses, in a field whose row has been scrolled out of sight, is
// sent once: sent in every frame it was neither seen nor being stored, each was a
// write, and each refusal a message and a frame - for as long as the row stayed out of
// sight.
#[test]
fn a_refused_name_scrolled_out_of_sight_is_refused_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    for n in 0..60 {
        album(&library, &format!("Album {n:02}"));
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 60
    });
    driver.act(&mut harness);
    // The list has followed the grid to its folder, under sixty albums: back to its top,
    // with the pointer left resting on it.
    wheel(&mut driver, &mut harness, 5000.0);
    // The album listed first, whichever that is.
    let (first, name) = {
        let album = &harness.state().collections().albums[0];
        (album.id, album.name.clone())
    };
    right_click_row(&mut driver, &mut harness, &name);
    pick_item(&mut driver, &mut harness, "Rename…");
    typed(&mut driver, &mut harness, "Porto");
    // Deleted from elsewhere: the name will be refused.
    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, first).unwrap();
    // The wheel over the list, far enough for the field's row to leave.
    wheel(&mut driver, &mut harness, -1500.0);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert!(harness.state().naming().is_some());
    // The refusal has been answered, and the frames after it send nothing more: the
    // queue of writes is waited for after each.
    for _ in 0..5 {
        driver.settle(&mut harness);
        driver.act(&mut harness);
    }
    assert_eq!(said(&harness).len(), 1, "{:?}", said(&harness));
}

// "Rename in photon…" on a header, with the sidebar hidden and the folder's row far
// down a long list: the row is brought into sight (`Shell::reveal`). Without that it is
// not drawn, and a field that is not drawn is stored and closed in the frame it opens -
// which a library of three folders, whose rows are all in sight, cannot show.
#[test]
fn a_folder_far_down_the_list_is_named_from_its_header_with_the_sidebar_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12_000, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 12_000);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 40
    });
    driver.settle(&mut harness);
    // Where the grid's top edge is: the first header's own row is over the first photo.
    let first = harness.state().tile_rect(0).unwrap();
    let header = egui::pos2(first.center().x, first.top() - 14.0);
    key(&mut driver, &mut harness, Key::B, Modifiers::COMMAND);
    assert!(harness.state().layout().sidebar_hidden);
    driver.act(&mut harness);
    // To the end of the library: the last folder's header is pinned over its photos.
    key(&mut driver, &mut harness, Key::End, Modifiers::NONE);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let folder = lib.item(ids[ids.len() - 1]).unwrap().unwrap().folder_id;
    right_click_at(&mut driver, &mut harness, header);
    assert!(harness.state().menu_open());
    pick_item(&mut driver, &mut harness, "Rename in photon…");
    driver.act(&mut harness);
    assert!(!harness.state().layout().sidebar_hidden);
    assert!(
        matches!(harness.state().naming(), Some((Of::Folder(id), _, false)) if id == folder),
        "{:?} for folder {folder}",
        harness.state().naming()
    );
    typed(&mut driver, &mut harness, "Last");
    assert_eq!(
        harness.state().naming(),
        Some((Of::Folder(folder), "Last".to_owned(), false))
    );
}

fn enter_down() -> egui::Event {
    egui::Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

// "Rename…" picked with Enter, and the key held a moment longer: its repeat is not the
// field's Enter, or the field was stored and closed before anything was typed in it.
#[test]
fn the_enter_that_picks_rename_does_not_store_the_field_at_its_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Lisbon");
    key(&mut driver, &mut harness, Key::ArrowDown, Modifiers::NONE);
    // Enter goes down, and stays down.
    harness.event(enter_down());
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(matches!(
        harness.state().naming(),
        Some((Of::Album(_), _, false))
    ));
    // The keyboard's repeat of the key that is still down.
    harness.event(enter_down());
    driver.act(&mut harness);
    assert!(
        matches!(harness.state().naming(), Some((Of::Album(_), _, false))),
        "the repeat stored the field: {:?}",
        harness.state().naming()
    );
}

// The other button inside the field itself - where a text field's own menu would be -
// opens no menu of the row: it took the keyboard from the field, which stored the name
// half typed.
#[test]
fn the_other_button_in_the_field_does_not_store_the_name_half_typed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, _) = renaming(&library);
    let field = (harness.query_all_by_role(egui::accesskit::Role::TextInput))
        .map(|node| node.rect())
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the name's field")
        .center();
    right_click_at(&mut driver, &mut harness, field);
    driver.settle(&mut harness);
    assert_eq!(
        album_names(harness.state()),
        ["Lisbon"],
        "the half-typed name was stored"
    );
}

// A saved search deleted from elsewhere while its menu is open: as an album's.
#[test]
fn a_menu_whose_search_is_gone_closes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let saved = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Tens", "IMG_00000", 1_700_000_000_000)
            .unwrap()
            .id
    };
    // A second one, which stays: the menu is not closed for want of any search at all.
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Ones", "IMG_00001", 1_700_000_000_001)
            .unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the lists read", |app| {
        app.collections().searches.len() == 2
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Tens");
    assert!(harness.state().menu_open());
    let engine = harness.state().engine().clone();
    commands::delete_saved_search(&engine, saved).unwrap();
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "the search gone from the list", |app| {
        app.collections().searches.len() == 1
    });
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
}

// A folder's menu is of a folder the view lists: its name is typed in its row, and the
// menu was opened on that row or on its header. Hidden from elsewhere under its open
// menu, the folder has neither any more, and the menu closes - open still, "Rename in
// photon…" opened a field with no row to stand in, which closed in the frame it opened.
#[test]
fn a_menu_whose_folder_is_no_longer_listed_closes() {
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
    assert!(harness.state().menu_open());
    let engine = harness.state().engine().clone();
    commands::set_folder_hidden(&engine, folder, true).unwrap();
    driver.until(&mut harness, "the folder's photos gone", |app| {
        app.photos() == 900 - PER_FOLDER
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label(&name).is_none());
    assert!(!harness.state().menu_open());
}

// A menu of the selection closes when the photo the keys were on leaves the grid, since
// its items would act on the photo landed on. A folder's menu is of the folder, which
// is still there: it stays.
#[test]
fn a_folders_menu_stays_open_when_the_selected_photo_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    driver.settle(&mut harness);
    select(&mut driver, &mut harness, 2);
    assert_eq!(harness.state().selected(), [ids[2]]);
    let folder = folder_at(&harness, &library, 1);
    let name = harness.state().folders()[&folder].name.clone();
    right_click_row(&mut driver, &mut harness, &name);
    assert!(harness.state().menu_open());
    let engine = harness.state().engine().clone();
    commands::set_items_hidden(&engine, &[ids[2]], true).unwrap();
    driver.until(&mut harness, "the photo gone, and the lead landed", |app| {
        app.photos() == 899 && app.selected() == [ids[3]]
    });
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert!(harness.query_by_label("Rename in photon…").is_some());
}
