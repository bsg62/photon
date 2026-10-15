//! The view of a photo's copies, through the whole application and the real engine: opened
//! from the tile's menu and from the info panel, and what it says of itself as its photos
//! leave. A fixture's files are on a drive that is not there, so the copies are made by
//! writing the content hash the engine's pass would have found; a hide is `library.db`'s.

use crate::common::{Driver, Harness, click, library, opened, photo_ids};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_engine::commands;

/// Makes the photos `ids` byte for byte copies of one another, as the engine's pass would
/// have found them.
fn copies(library: &photon_ui::fixture::Fixture, ids: &[i64]) {
    let conn = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    for id in ids {
        let sql =
            "UPDATE items SET content_hash = X'00112233445566778899aabbccddeeff' WHERE id = ?1";
        assert_eq!(conn.execute(sql, [id]).unwrap(), 1);
    }
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// A press and release of `which` on the photo at `offset`, where it was last drawn.
fn press_on(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, which: PointerButton) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, which, true));
    harness.event(button(at, which, false));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn name_of(harness: &Harness<'_>, id: i64) -> String {
    let engine = harness.state().engine().clone();
    commands::viewer_item(&engine, id).unwrap().file_name
}

/// The menu of the photo at `offset` and its item for the photo's copies, which is there
/// once the library has counted them; the view of them once that is on screen.
fn shown_from_the_menu(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, of: usize) {
    press_on(driver, harness, offset, PointerButton::Secondary);
    let item = if of == 1 {
        "Show 1 duplicate".to_owned()
    } else {
        format!("Show {of} duplicates")
    };
    driver.seen(harness, &item);
    harness.get_by_label(&item).click();
    driver.act(harness);
    driver.until(harness, "the photo and its copies", |app| {
        app.settled().view == GridView::Copies && app.photos() == of + 1
    });
    driver.act(harness);
}

// The tile's menu offers a photo's copies once they are counted, and its item shows them:
// the photo and its copies in a view of their own, named in the sidebar by the photo, with
// the photo selected in it. A photo with no copy has no such item.
#[test]
fn the_menu_shows_a_photo_s_copies_with_the_photo_selected() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 200, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[3], ids[150]]);
    let (mut harness, mut driver) = opened(&library, 200);
    driver.settle(&mut harness);
    // Not for a photo with none: the count is asked and answered, and there is no item.
    press_on(&mut driver, &mut harness, 4, PointerButton::Primary);
    press_on(&mut driver, &mut harness, 4, PointerButton::Secondary);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label_contains("duplicate").is_none());
    key(&mut driver, &mut harness, Key::Escape);
    let name = name_of(&harness, ids[3]);
    shown_from_the_menu(&mut driver, &mut harness, 3, 1);
    let mut shown = harness.state().shown();
    shown.sort_unstable();
    assert_eq!(shown, [ids[3], ids[150]]);
    assert_eq!(harness.state().selected(), [ids[3]]);
    assert_eq!(harness.state().lead(), Some(ids[3]));
    driver.seen(&mut harness, &format!("Copies of {name}"));
    // While the view holds more than the photo there is nothing to say of it.
    assert_eq!(harness.state().foot(), None);
    // Any row of the sidebar leaves it.
    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "all photos again", |app| {
        app.settled().view == GridView::All && app.photos() == 200
    });
}

// The info panel's "Show N duplicates in the grid" is a link now: the viewer shuts, and
// the grid is the view of the photo's copies, the photo selected in it.
#[test]
fn the_panel_s_link_shows_the_photo_s_copies() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 200, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[3], ids[150], ids[160]]);
    let (mut harness, mut driver) = opened(&library, 200);
    driver.settle(&mut harness);
    press_on(&mut driver, &mut harness, 3, PointerButton::Primary);
    press_on(&mut driver, &mut harness, 3, PointerButton::Primary);
    assert!(harness.state().viewer().is_some(), "the viewer is open");
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    key(&mut driver, &mut harness, Key::I);
    let link = "Show 2 duplicates in the grid";
    driver.seen(&mut harness, link);
    harness.get_by_label(link).click();
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none(), "the viewer shut");
    driver.until(&mut harness, "the photo and its copies", |app| {
        app.settled().view == GridView::Copies && app.photos() == 3
    });
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[3]]);
}

// What the view says of itself as its photos leave, under the photos that are left: the
// photo hidden, its copies still there; the photo gone from the library, by the name it
// had; and over the empty view once its last copy has gone too. Nothing while the view
// holds more than the photo.
#[test]
fn the_view_says_what_became_of_its_photo() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 60, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[3], ids[50]]);
    let (mut harness, mut driver) = opened(&library, 60);
    driver.settle(&mut harness);
    let name = name_of(&harness, ids[3]);
    shown_from_the_menu(&mut driver, &mut harness, 3, 1);
    let at = harness.state().shown().iter().position(|id| *id == ids[3]);
    // The photo hidden: it leaves the view, and its copy stays.
    press_on(
        &mut driver,
        &mut harness,
        at.unwrap(),
        PointerButton::Primary,
    );
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "the photo hidden", |app| {
        app.shown() == [ids[50]]
    });
    let hidden = format!("{name} is hidden; these are its copies. Unhide it from Hidden.");
    driver.seen(&mut harness, &hidden);
    assert_eq!(harness.state().foot().as_deref(), Some(hidden.as_str()));
    // The photo gone from the library: the library has no name for it any more, and the
    // view still says it by the one it had - in its line and in the sidebar.
    let engine = harness.state().engine().clone();
    let before = harness.state().version();
    engine.lib.purge_items(&[ids[3]]).unwrap();
    engine.refresh_grid().unwrap();
    // In the frame its grid lands in, what was said of the photo with the grid before -
    // that it is hidden - is not said of it: the library is asked again with this grid.
    driver.until(&mut harness, "the grid without it", |app| {
        app.version() != before
    });
    assert_ne!(harness.state().foot().as_deref(), Some(hidden.as_str()));
    let gone = format!("{name} is no longer in the library; its copies are under Duplicates.");
    driver.seen(&mut harness, &gone);
    driver.seen(&mut harness, &format!("Copies of {name}"));
    // Its copy hidden as well: the view is empty, and says the same over it.
    press_on(&mut driver, &mut harness, 0, PointerButton::Primary);
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "the view emptied", |app| app.photos() == 0);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(harness.state().notice().as_deref(), Some(gone.as_str()));
    assert_eq!(harness.state().foot(), None);
}

// The copy hidden and not the photo: the view has shrunk to the photo alone, and says so
// under it.
#[test]
fn a_view_shrunk_to_its_photo_says_it_has_no_other_copies() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 60, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[3], ids[50]]);
    let (mut harness, mut driver) = opened(&library, 60);
    driver.settle(&mut harness);
    let name = name_of(&harness, ids[3]);
    shown_from_the_menu(&mut driver, &mut harness, 3, 1);
    let at = harness.state().shown().iter().position(|id| *id == ids[50]);
    press_on(
        &mut driver,
        &mut harness,
        at.unwrap(),
        PointerButton::Primary,
    );
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "the copy hidden", |app| {
        app.shown() == [ids[3]]
    });
    let alone = format!("No other copies of {name} any more.");
    driver.seen(&mut harness, &alone);
    assert_eq!(harness.state().foot().as_deref(), Some(alone.as_str()));
    assert_eq!(harness.state().notice(), None);
}
