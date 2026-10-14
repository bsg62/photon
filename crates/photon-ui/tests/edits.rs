//! Turns and the crop, through the whole application and the real engine: over a
//! library of real photos made for the test, in a temporary directory.
//!
//! **No photo is written here, by the test or by what it tests.** A turn and a crop are
//! two columns of `library.db`; the picture that follows is the engine's render, and
//! the files stay as the test made them (`the_photos_are_as_they_were`).

mod common;

use common::{Driver, Harness, library, opened, over, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::kittest::Queryable;
use photon_engine::{
    commands::{self, ItemEdit},
    engine::RENDERING,
};
use photon_ui::app::App;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// A folder with three real photos, in the order the grid has them: `a.jpg`, twice as
/// wide as tall - fitted into the window it is 1000 points by 500, from (0, 100);
/// `b.jpg`, upright; `c.jpg`, small.
fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, (name, width, height)) in [("a.jpg", 800, 400), ("b.jpg", 400, 800), ("c.jpg", 64, 48)]
        .into_iter()
        .enumerate()
    {
        let picture = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([40 * n as u8, 120, 200]),
        ));
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        let path = folder.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(start + Duration::from_secs(60 * n as u64))
            .unwrap();
    }
    folder
}

/// The files of a folder, each with its bytes.
type Files = Vec<(PathBuf, Vec<u8>)>;

/// Every file of `folder`, with its bytes.
fn files(folder: &Path) -> Files {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| (path.clone(), std::fs::read(&path).unwrap()))
        .collect();
    files.sort();
    files
}

/// The application over a library that watches `pictures(dir)`, once its three photos
/// are shown; their ids, in the grid's order; and the folder's files as they were made.
fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, Files) {
    let folder = pictures(dir);
    let made = files(&folder);
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    let ids = harness.state().shown();
    (harness, driver, ids, made)
}

/// Nothing in the watched folder has been written, added or taken away.
fn the_photos_are_as_they_were(made: &[(PathBuf, Vec<u8>)]) {
    let folder = made[0].0.parent().unwrap();
    assert!(
        files(folder) == made,
        "a file of the watched folder changed"
    );
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// A click on the photo at `offset` of the grid.
fn click(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn shift(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::SHIFT, key);
    driver.act(harness);
}

/// Several events in one frame: put into the frame's input beside the queue, which is
/// drawn a frame an event.
fn frame_with(driver: &mut Driver, harness: &mut Harness<'_>, keys: &[Key]) {
    let events = keys.iter().map(|key| egui::Event::Key {
        key: *key,
        physical_key: Some(*key),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.input_mut().events.extend(events);
    driver.act(harness);
    let events = keys.iter().map(|key| egui::Event::Key {
        key: *key,
        physical_key: Some(*key),
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.input_mut().events.extend(events);
    driver.act(harness);
}

/// Opens the viewer on the photo at `offset` by a double click, and waits for the
/// library to have said what it is and for its full picture.
fn open(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    click(driver, harness, offset);
    click(driver, harness, offset);
    assert!(harness.state().viewer().is_some(), "the viewer is open");
    driver.until(harness, "the photo's facts and its picture", |app| {
        app.viewer_item().is_some() && app.viewer_levels().is_some()
    });
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

/// The edit the library says the photo in the viewer has.
fn edit(app: &App) -> Option<(u8, Option<[u16; 4]>)> {
    let ItemEdit { turns, crop } = app.viewer_item()?.edit.as_ref()?;
    Some((*turns, *crop))
}

/// The edit the library has for the photo `id`, read from the library itself.
fn stored(harness: &Harness<'_>, id: i64) -> photon_core::edit::Edit {
    (harness.state().engine().lib.item(id).unwrap())
        .expect("the photo")
        .edit
}

/// How wide the full picture on screen is, in pixels.
fn wide(app: &App) -> Option<u32> {
    app.viewer_levels()?.first().copied()
}

/// Lets everything that is due in the next `secs` happen, and the other threads finish.
fn rest(driver: &mut Driver, harness: &mut Harness<'_>, secs: f64) {
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
}

/// The first button down at `from`, taken to `to`, and let go of there.
fn drag(driver: &mut Driver, harness: &mut Harness<'_>, from: Pos2, to: Pos2) {
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, PointerButton::Primary, true));
    harness.event(egui::Event::PointerMoved(to));
    harness.event(button(to, PointerButton::Primary, false));
    driver.act(harness);
}

/// Where the last frame drew the photo: the largest picture painted.
fn picture_at(harness: &Harness<'_>) -> Option<egui::Rect> {
    fn each(shape: &egui::Shape, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each(shape, found)),
            egui::Shape::Rect(rect) if rect.brush.is_some() => found.push(rect.rect),
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                found.push(mesh.calc_bounds());
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &harness.output().shapes {
        each(&clipped.shape, &mut found);
    }
    found
        .into_iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
}

/// The left half of `a.jpg`, as the library stores a crop.
const LEFT_HALF: [u16; 4] = [0, 0, 32768, 65535];

/// Crops the photo `id` from elsewhere, before the viewer is opened on it, and waits
/// for the grid that says so.
fn cropped(driver: &mut Driver, harness: &mut Harness<'_>, id: i64, crop: [u16; 4]) {
    let engine = harness.state().engine().clone();
    let was = harness.state().version();
    commands::set_item_edit(&engine, id, 0, Some(crop)).unwrap();
    driver.until(harness, "the grid with the photo cropped", |app| {
        app.version() != was
    });
    driver.settle(harness);
}

/// The library's write lock, taken from outside: the engine's next write waits for it,
/// and fails when it has waited five seconds.
fn locked(harness: &Harness<'_>) -> rusqlite::Connection {
    let path = harness.state().engine().lib.path().to_owned();
    let other = rusqlite::Connection::open(path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    other
}

// `R` turns the photo to the right and Shift+R to the left: a number in the library,
// and the picture that follows is the engine's - the photo's key is another, and the
// viewer asks for the picture again. Nothing is written to the photo.
#[test]
fn r_and_shift_r_turn_the_photo_and_the_picture_that_follows_is_the_engines() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    assert_eq!(wide(harness.state()), Some(800));
    key(&mut driver, &mut harness, Key::R);
    driver.until(&mut harness, "the photo turned to the right", |app| {
        edit(app) == Some((1, None)) && wide(app) == Some(400)
    });
    assert_eq!(viewing(&harness), Some(ids[0]));
    shift(&mut driver, &mut harness, Key::R);
    driver.until(&mut harness, "turned back", |app| {
        edit(app).is_none() && wide(app) == Some(800)
    });
    shift(&mut driver, &mut harness, Key::R);
    driver.until(&mut harness, "turned to the left", |app| {
        edit(app) == Some((3, None)) && wide(app) == Some(400)
    });
    the_photos_are_as_they_were(&made);
}

// The bar's two buttons turn it too, and "Original" - there only while the photo has
// an edit - takes every turn and the crop off again.
#[test]
fn the_bars_buttons_turn_the_photo_and_original_takes_every_edit_off() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    assert_eq!(edit(harness.state()), Some((0, Some(LEFT_HALF))));
    harness.get_by_label("Rotate right").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "turned to the right", |app| {
        edit(app).is_some_and(|(turns, crop)| turns == 1 && crop.is_some())
    });
    harness.get_by_label("Rotate left").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "and back", |app| {
        edit(app) == Some((0, Some(LEFT_HALF)))
    });
    driver.act(&mut harness);
    harness.get_by_label("Original").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the photo as it was taken", |app| {
        edit(app).is_none() && wide(app) == Some(800)
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("Original").is_none());
    the_photos_are_as_they_were(&made);
}

// A turn is of the photo on screen. In a frame with a step before it, it is of the
// photo stepped to, of which nothing has been seen or is known: it is of none. With the
// step after it, it is of the photo that was on screen when it was pressed.
#[test]
fn a_turn_in_the_frame_of_a_step_is_of_no_photo_that_was_not_seen() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    frame_with(&mut driver, &mut harness, &[Key::ArrowRight, Key::R]);
    assert_eq!(viewing(&harness), Some(ids[1]));
    rest(&mut driver, &mut harness, 0.5);
    assert!(stored(&harness, ids[0]).is_identity() && stored(&harness, ids[1]).is_identity());
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    frame_with(&mut driver, &mut harness, &[Key::R, Key::ArrowRight]);
    assert_eq!(viewing(&harness), Some(ids[2]));
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(stored(&harness, ids[1]).turns, 1);
    assert!(stored(&harness, ids[2]).is_identity());
}

// `C` opens the crop tool on the whole photo, fitted to the whole window: the info
// panel shuts and the zoom is put back. Under it nothing leaves the photo - no arrow
// steps - and Escape closes the tool and not the viewer, which the next Escape closes.
#[test]
fn the_crop_tool_opens_on_the_whole_photo_fitted_with_the_panel_shut() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::I);
    key(&mut driver, &mut harness, Key::Plus);
    assert!(harness.state().viewer_info());
    assert!(!harness.state().viewer().unwrap().zoom().is_fit());
    key(&mut driver, &mut harness, Key::C);
    let tool = *harness.state().cropping().expect("the tool");
    assert_eq!(tool.wire(), None, "the whole photo");
    assert!(!harness.state().viewer_info());
    assert!(harness.state().viewer().unwrap().zoom().is_fit());
    driver.act(&mut harness);
    for label in ["Crop ratio: Free", "Whole photo", "Cancel", "Apply"] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    assert!(harness.query_by_label("Next photo").is_none());
    key(&mut driver, &mut harness, Key::ArrowRight);
    key(&mut driver, &mut harness, Key::H);
    assert_eq!(viewing(&harness), Some(ids[0]));
    assert!(harness.state().cropping().is_some());
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().cropping().is_none());
    assert_eq!(viewing(&harness), Some(ids[0]), "the viewer is open still");
    rest(&mut driver, &mut harness, 0.5);
    assert!(!harness.state().viewer_item().unwrap().hidden);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
}

// A rectangle drawn and applied is the photo's crop, with the turns it had: the tool
// closes when the library has stored it, the picture that follows is the engine's, and
// "Original" is offered. No photo is written.
#[test]
fn a_rectangle_drawn_and_applied_is_the_photos_crop_and_the_tool_closes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, made) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    // The handle on the right edge, a quarter of the picture to the left.
    drag(
        &mut driver,
        &mut harness,
        pos2(995.0, 350.0),
        pos2(745.0, 350.0),
    );
    let right = harness.state().cropping().unwrap().rect().right;
    assert!((right - 0.75).abs() < 1e-6, "{right}");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the crop stored and the tool closed", |app| {
        app.cropping().is_none() && edit(app) == Some((0, Some([0, 0, 49151, 65535])))
    });
    driver.until(&mut harness, "the picture cropped", |app| {
        wide(app) == Some(600)
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("Original").is_some());
    // And with a turn it had: the crop is written beside it, not in its place.
    key(&mut driver, &mut harness, Key::R);
    driver.until(&mut harness, "turned", |app| {
        edit(app).is_some_and(|(turns, _)| turns == 1) && wide(app) == Some(400)
    });
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    harness.get_by_label("Whole photo").click();
    driver.act(&mut harness);
    harness.get_by_label("Apply").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the turn alone", |app| {
        app.cropping().is_none() && edit(app) == Some((1, None))
    });
    the_photos_are_as_they_were(&made);
}

// The tool on a photo that has a crop opens on that crop, and has the picture without
// it made for itself - the photo's own is of what the crop keeps. Cancelled, the
// photo's own picture is there still, and is not made again.
#[test]
fn the_tool_on_a_cropped_photo_has_the_picture_without_the_crop_made_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    assert_eq!(wide(harness.state()), Some(400));
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    // Until the picture without its crop is made, the photo's own stands in for it, in
    // the left half of the tool's box.
    let rendering = RENDERING.lock();
    key(&mut driver, &mut harness, Key::C);
    assert_eq!(
        harness.state().cropping().unwrap().wire(),
        Some(LEFT_HALF),
        "the crop the photo has"
    );
    driver.act(&mut harness);
    let half = [0.0, 0.0, 32768.0 / 65535.0, 1.0];
    assert_eq!(harness.state().crop_part(), Some(half));
    // Drawn there: the tool's box is 1000 points by 500 from (0, 100).
    let at = picture_at(&harness).expect("the picture");
    let there = egui::Rect::from_min_max(pos2(0.0, 100.0), pos2(500.0, 600.0));
    assert!(
        (at.min - there.min).length() < 1.0 && (at.max - there.max).length() < 1.0,
        "{at:?}"
    );
    drop(rendering);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    assert_eq!(harness.state().crop_picture().unwrap()[0], 800);
    assert_eq!(harness.state().crop_part(), Some([0.0, 0.0, 1.0, 1.0]));
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    harness.get_by_label("Cancel").click();
    driver.act(&mut harness);
    assert!(harness.state().cropping().is_none());
    assert_eq!(wide(harness.state()), Some(400), "the photo's own picture");
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    assert_eq!(edit(harness.state()), Some((0, Some(LEFT_HALF))));
    // A photo with no crop has that picture already: none is made for the tool.
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.until(&mut harness, "the next photo's picture", |app| {
        app.viewer_item().is_some() && app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    key(&mut driver, &mut harness, Key::C);
    rest(&mut driver, &mut harness, 0.5);
    assert!(harness.state().cropping().is_some());
    assert_eq!(harness.state().crop_picture(), None);
    assert_eq!(harness.state().pictures_asked(), asked);
}

// The tool's picture is asked for as the tool opens, whatever is on its way: the
// photo's own, still waiting at its render, gives way - one question at a time. With
// the tool cancelled before either is made, the photo's own is asked for again.
#[test]
fn the_tools_picture_is_asked_for_at_once_whatever_is_on_its_way() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    let rendering = RENDERING.lock();
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(
        &mut harness,
        "its facts, and its picture asked for",
        |app| app.viewer_item().is_some() && app.viewer_asked(),
    );
    let asked = harness.state().pictures_asked();
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    assert_eq!(harness.state().crop_picture(), None);
    assert!(harness.state().cropping().is_some(), "and the tool is open");
    key(&mut driver, &mut harness, Key::Escape);
    driver.act(&mut harness);
    assert_eq!(harness.state().pictures_asked(), asked + 2);
    drop(rendering);
    driver.until(&mut harness, "the photo's own picture", |app| {
        wide(app) == Some(400)
    });
    assert_eq!(harness.state().crop_picture(), None);
}

// The rectangle is drawn on a picture. Turned from elsewhere under the open tool, the
// photo is another picture, and the tool closes: its rectangle was on the one before.
#[test]
fn the_tool_closes_when_its_photo_becomes_another_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_some());
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the photo turned", |app| {
        edit(app) == Some((1, None))
    });
    assert!(harness.state().cropping().is_none());
    assert_eq!(viewing(&harness), Some(ids[0]));
}

// A photo that has left the view is not edited, and the tool is of a photo that can
// be: hidden from elsewhere under the open tool, the photo stays on screen and the
// tool closes. So it does over a photo stepped from in the frame it was opened in.
#[test]
fn the_tool_closes_when_its_photo_can_no_longer_be_edited() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_some());
    let engine = harness.state().engine().clone();
    commands::set_items_hidden(&engine, &[ids[0]], true).unwrap();
    driver.until(&mut harness, "the photo has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    driver.act(&mut harness);
    assert!(harness.state().cropping().is_none());
    assert_eq!(viewing(&harness), Some(ids[0]));
    // And its keys do nothing now.
    key(&mut driver, &mut harness, Key::C);
    key(&mut driver, &mut harness, Key::R);
    assert!(harness.state().cropping().is_none());
    rest(&mut driver, &mut harness, 0.5);
    assert!(stored(&harness, ids[0]).is_identity());
    // Opened and stepped from in one frame.
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.until(&mut harness, "the next photo's facts", |app| {
        app.viewer_item().is_some()
    });
    frame_with(&mut driver, &mut harness, &[Key::C, Key::ArrowRight]);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[2]));
    assert!(harness.state().cropping().is_none());
}

// A photo whose picture cannot be made - its drive is not there - is looked at from
// its preview, and is not edited: there is nothing to turn that could be seen turned.
#[test]
fn a_photo_whose_picture_cannot_be_made_is_not_edited() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_item().is_some() && app.viewer_failed().is_some()
    });
    let id = viewing(&harness).unwrap();
    key(&mut driver, &mut harness, Key::R);
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_none());
    rest(&mut driver, &mut harness, 0.5);
    assert!(stored(&harness, id).is_identity());
}

// "Apply" is a write, answered frames later: until then the tool stands, and a second
// Enter sends nothing more. And the answer closes the tool that sent it, not one
// opened since - cancelled and opened again while the write was on its way.
#[test]
fn an_answer_closes_only_the_tool_that_sent_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    // The crop it has, written again: the same picture, so that nothing but the answer
    // closes the tool - which it does.
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the tool closed by its answer", |app| {
        app.cropping().is_none()
    });
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    let lock = locked(&harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    assert!(
        harness.state().cropping().is_some(),
        "it waits for its answer"
    );
    // And meanwhile it takes no drag: the rectangle on its way is the one on screen.
    let before = harness.state().cropping().unwrap().rect();
    drag(
        &mut driver,
        &mut harness,
        pos2(495.0, 350.0),
        pos2(395.0, 350.0),
    );
    assert_eq!(harness.state().cropping().unwrap().rect(), before);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().cropping().is_none());
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_some());
    lock.execute_batch("COMMIT").unwrap();
    drop(lock);
    rest(&mut driver, &mut harness, 0.5);
    assert!(harness.state().cropping().is_some(), "another tool");
    assert_eq!(edit(harness.state()), Some((0, Some(LEFT_HALF))));
    assert_eq!(said(&harness), Vec::<String>::new());
}

// A rectangle the library will not store - here it cannot be written to at all -
// leaves the tool open, with the library's words for why, and "Apply" works again.
#[test]
fn a_rectangle_the_library_will_not_store_leaves_the_tool_open_to_apply_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    drag(
        &mut driver,
        &mut harness,
        pos2(995.0, 350.0),
        pos2(745.0, 350.0),
    );
    // Held past the five seconds a write waits.
    let lock = locked(&harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the library's refusal", |app| {
        !app.toasts().is_empty()
    });
    assert!(harness.state().cropping().is_some());
    assert_eq!(edit(harness.state()), None);
    lock.execute_batch("COMMIT").unwrap();
    drop(lock);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the crop stored", |app| {
        app.cropping().is_none() && edit(app) == Some((0, Some([0, 0, 49151, 65535])))
    });
}

// The picture without the crop cannot be made - the file has gone since the photo's
// own was: why is said, once, the tool stands on the picture it has, and the picture
// is not asked for again and again.
#[test]
fn the_tools_picture_that_cannot_be_made_is_said_and_not_asked_for_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    // (The test's own file, in the test's own directory. The folder is watched: what
    // follows is looked at before the scan that will miss the file.)
    std::fs::remove_file(&made[0].0).unwrap();
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "why", |app| !app.toasts().is_empty());
    assert!(harness.state().cropping().is_some());
    assert_eq!(harness.state().crop_picture(), None);
    for _ in 0..5 {
        driver.act(&mut harness);
    }
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    assert_eq!(harness.state().toasts().len(), 1);
}
