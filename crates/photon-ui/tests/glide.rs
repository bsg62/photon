//! The grid gliding after the user's own write, through the whole application and the
//! real engine: a photo hidden fades where it stood and the photos after it glide into its
//! place, while a scan's grid and another view's are on screen at once, and so is a hide's
//! that lands under rows laid out from an older grid, a grid that comes back from the
//! viewer in other columns, and one adopted after a write the library refused. Over real photos in a folder the test made, so that the writes
//! are made.

use crate::common::{Driver, Harness, over_animated, photos_in};
use eframe::egui::{self, Key, Modifiers, PointerButton, Rect};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_ui::{dirs::Dirs, nav::Place};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// A watched folder called Pictures under `dir`, with five photos a minute apart: in the
/// grid `a` to `e`, in one row.
fn pictures(dir: &Path) -> PathBuf {
    photos_in(
        dir,
        "Pictures",
        &["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"],
    )
}

/// The application over `pictures`, with the animations on, once its photos are shown,
/// their scan is over and nothing is on its way.
fn opened<'a>(dir: &Path, pictures: &Path) -> (Harness<'a>, Driver, Dirs) {
    let (mut harness, mut driver, made) = over_animated(dir, Some(pictures));
    driver.until(&mut harness, "the five photos shown", |app| {
        app.photos() == 5
            && app.status().is_empty()
            && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    (harness, driver, made)
}

/// Clicks the photo at grid offset `offset`.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    driver.act(harness);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        driver.act(harness);
    }
}

fn near(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01
}

/// Whether every photo of the grid on screen was drawn where it belongs, none gliding.
fn all_in_place(harness: &Harness<'_>) -> bool {
    let app = harness.state();
    (app.shown().iter().enumerate()).all(|(offset, id)| {
        let (drawn, laid) = (app.drawn_place(*id), app.tile_rect(offset));
        drawn
            .zip(laid)
            .is_some_and(|(drawn, laid)| near(drawn, laid))
    })
}

#[test]
fn a_hidden_photo_fades_and_the_rest_glide_into_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    let (mut harness, mut driver, _) = opened(dir.path(), &pictures);
    let ids = harness.state().shown();
    let (hidden, next) = (ids[1], ids[2]);
    let left = harness.state().tile_rect(1).unwrap();
    let stood = harness.state().tile_rect(2).unwrap();

    pick(&mut driver, &mut harness, 1);
    harness.key_press(Key::H);
    driver.act(&mut harness);
    driver.until(&mut harness, "the grid without the photo shown", |app| {
        !app.shown().contains(&hidden)
    });
    let landed = driver.now;
    // Drawn where it stood in the frame the grid lands in, ...
    let drawn = harness.state().drawn_place(next).unwrap();
    assert!(near(drawn, stood), "{drawn:?} where it stood, {stood:?}");
    // ... on its way at a hundred milliseconds, ...
    driver.frame(&mut harness, landed + 0.1);
    let drawn = harness.state().drawn_place(next).unwrap();
    assert!(
        left.left() < drawn.left() && drawn.left() < stood.left(),
        "{drawn:?} between {stood:?} and {left:?}"
    );
    assert_eq!(drawn.top(), stood.top());
    // ... and in the place the hidden photo left at two hundred.
    driver.frame(&mut harness, landed + 0.2);
    let drawn = harness.state().drawn_place(next).unwrap();
    assert!(near(drawn, left), "{drawn:?} in the place left, {left:?}");
    assert!(near(drawn, harness.state().tile_rect(1).unwrap()));
}

// A photo the scan finds before the ones in view: every photo is in its new place in the
// frame its grid lands in. The rescan asked from the folder's menu is a write too, and
// not one of those that move the grid.
#[test]
fn a_scans_rebuild_does_not_glide() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    let (mut harness, mut driver, _) = opened(dir.path(), &pictures);
    // Older than `a`, so first in the folder; a few bytes longer than any, so no copy of
    // one and no file of theirs moved.
    let earlier = pictures.join("0.jpg");
    let mut bytes = std::fs::read(pictures.join("a.jpg")).unwrap();
    bytes.extend([0; 10]);
    std::fs::write(&earlier, bytes).unwrap();
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    std::fs::File::options()
        .write(true)
        .open(&earlier)
        .unwrap()
        .set_modified(start - Duration::from_secs(60))
        .unwrap();

    let row = harness.get_by_label("Pictures").rect().center();
    harness.event(egui::Event::PointerMoved(row));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: row,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Rescan").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the photo found shown", |app| {
        app.photos() == 6
    });
    assert!(all_in_place(&harness), "a photo glides");
}

// All photos to Starred while a hide is on its way - held at the library's write lock -
// so that the grid that lands is adopted while a write that moves the grid is waiting:
// the starred photos are where they belong in Starred at once, not gliding from their
// places in All photos.
#[test]
fn another_view_does_not_glide() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    std::fs::write(
        pictures.join(".picasa.ini"),
        "[b.jpg]\nstar=yes\n[d.jpg]\nstar=yes\n",
    )
    .unwrap();
    let (mut harness, mut driver, made) = over_animated(dir.path(), Some(&pictures));
    driver.until(&mut harness, "the five photos shown", |app| {
        app.photos() == 5
            && app.starred().len() == 2
            && app.status().is_empty()
            && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.seen(&mut harness, "Starred");
    driver.settle(&mut harness);

    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    // `e`, which is not starred: the hide changes nothing in Starred.
    pick(&mut driver, &mut harness, 4);
    harness.key_press(Key::H);
    driver.act(&mut harness);
    crate::common::click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred) && app.photos() == 2
    });
    assert!(all_in_place(&harness), "a photo glides");
    other.execute_batch("ROLLBACK").unwrap();
    driver.settle(&mut harness);
}

/// A key going down alone, in a frame of its own: `key_press` draws a second frame for
/// the key coming up.
fn key_down(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    driver.act(harness);
}

// The rows on screen are of the grid they were laid out from. One adopted under the
// viewer - a photo hidden from elsewhere - leaves them a grid behind, and the user's own
// hide that lands in the first frame the grid is drawn again, as the viewer closes,
// cannot glide from the grid it replaced: that grid does not say which photo stood where
// in those rows. Its photos are where they belong at once. The hide is held at the
// library's write lock until the viewer has closed.
#[test]
fn a_write_that_lands_as_the_viewer_closes_does_not_glide_from_rows_it_did_not_lay_out() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    let (mut harness, mut driver, made) = opened(dir.path(), &pictures);
    let ids = harness.state().shown();

    pick(&mut driver, &mut harness, 0);
    key_down(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the photo's facts read", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[0])
    });
    let engine = harness.state().engine().clone();
    photon_engine::commands::set_items_hidden(&engine, &[ids[3]], true).unwrap();
    driver.until(
        &mut harness,
        "the grid without it adopted under the viewer",
        |app| !app.shown().contains(&ids[3]),
    );
    driver.settle(&mut harness);

    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    key_down(&mut driver, &mut harness, Key::H);
    key_down(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().viewer().is_none());
    other.execute_batch("ROLLBACK").unwrap();
    // The hide made, its grid published and its answer sent, with no frame drawn.
    driver.quiet();
    driver.act(&mut harness);
    assert!(
        !harness.state().shown().contains(&ids[0]),
        "the hide landed"
    );
    assert!(all_in_place(&harness), "a photo glides");
}

// The window narrowed while the viewer stands in the grid's place - a maximized window
// given back, a tiling window manager - has the grid come back in other columns. The rows
// it was last drawn in are a window size ago and nobody has seen them since: its photos
// are where they belong in the first frame it is drawn again, not gliding from there.
#[test]
fn the_grid_does_not_glide_as_it_comes_back_from_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    let (mut harness, mut driver, _) = opened(dir.path(), &pictures);
    let ids = harness.state().shown();
    let stood = harness.state().tile_rect(4).unwrap();

    pick(&mut driver, &mut harness, 1);
    key_down(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the photo's facts read", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[1])
    });
    harness.set_size(egui::vec2(700.0, 900.0));
    driver.act(&mut harness);
    driver.act(&mut harness);
    key_down(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().viewer().is_none());
    assert!(all_in_place(&harness), "a photo glides");
    driver.act(&mut harness);
    assert!(
        !near(harness.state().tile_rect(4).unwrap(), stood),
        "the grid in other columns"
    );
    assert!(all_in_place(&harness), "a photo glides");
}

// A star the library refuses - the folder's INI is a directory, which the writer will
// not write over - publishes no grid, and its answer, read with no grid of its own to
// come, leaves nothing behind for the next: a photo hidden from elsewhere a moment later
// is not the user's write landing, and its photos are where they belong at once.
#[test]
fn a_refused_write_leaves_nothing_to_glide_for_the_next_grid() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = pictures(dir.path());
    let (mut harness, mut driver, _) = opened(dir.path(), &pictures);
    let ids = harness.state().shown();
    std::fs::create_dir(pictures.join(".picasa.ini")).unwrap();

    pick(&mut driver, &mut harness, 0);
    harness.key_press(Key::Period);
    driver.act(&mut harness);
    driver.until(&mut harness, "the refusal", |app| !app.toasts().is_empty());
    driver.settle(&mut harness);

    let engine = harness.state().engine().clone();
    photon_engine::commands::set_items_hidden(&engine, &[ids[0]], true).unwrap();
    driver.until(&mut harness, "the grid without it", |app| {
        !app.shown().contains(&ids[0])
    });
    assert!(all_in_place(&harness), "a photo glides");
}
