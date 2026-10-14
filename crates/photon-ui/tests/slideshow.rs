//! The slideshow, fullscreen and F11, through the whole application and the real engine:
//! over a library of real photos made for the test, in a temporary directory. The window
//! is the test's: it goes fullscreen when the application asks (`obey`).

mod common;

use common::{Driver, Harness, library, opened, over, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, ViewportId};
use egui_kittest::kittest::Queryable;
use photon_core::library::Library;
use photon_engine::engine::RENDERING;
use photon_ui::app::App;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// A folder of `a.jpg`, `b.jpg`, a file called `v.mp4` and `c.jpg`, in the grid's order:
/// three photos and, third, what the library takes for a video.
fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, name) in ["a.jpg", "b.jpg", "v.mp4", "c.jpg"].into_iter().enumerate() {
        let bytes = if name.ends_with(".mp4") {
            b"not a video".to_vec()
        } else {
            let picture = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                160,
                120,
                image::Rgb([60 * n as u8, 120, 200]),
            ));
            let mut bytes = Vec::new();
            picture
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
                .unwrap();
            bytes
        };
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

/// The application over a library that watches `pictures(dir)`, once its four files are
/// shown, in a window that is not fullscreen; and their ids, in the grid's order.
fn four<'a>(dir: &Path, settings: impl FnOnce(&Library)) -> (Harness<'a>, Driver, Vec<i64>) {
    let folder = pictures(dir);
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    settings(&Library::open(&data.join("library.db")).unwrap());
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
    driver.until(&mut harness, "the four shown", |app| {
        app.photos() == 4 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    window(&mut harness, false);
    let ids = harness.state().shown();
    (harness, driver, ids)
}

/// Says whether the window is fullscreen, as a window does in every frame.
fn window(harness: &mut Harness<'_>, fullscreen: bool) {
    (harness.input_mut().viewports)
        .entry(ViewportId::ROOT)
        .or_default()
        .fullscreen = Some(fullscreen);
}

/// The window does what it was last asked.
fn obey(harness: &mut Harness<'_>) {
    if let Some(on) = harness.state().fullscreen_asked() {
        window(harness, on);
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
    obey(harness);
}

/// Opens the viewer on the photo at `offset` by a double click, and waits for the
/// library to have said what it is and for its full picture.
fn open(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    click(driver, harness, offset);
    click(driver, harness, offset);
    driver.until(harness, "the photo's facts and its picture", |app| {
        app.viewer_item().is_some() && app.viewer_levels().is_some()
    });
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

fn showing(app: &App) -> bool {
    app.slideshow().is_some_and(|show| show.active())
}

/// Draws what is asked for until the viewer is on `id`, the window doing as it is told.
fn until_on(driver: &mut Driver, harness: &mut Harness<'_>, id: i64) {
    driver.until(harness, "the show on the photo", |app| {
        app.viewer().is_some_and(|viewer| viewer.id() == id)
    });
    obey(harness);
}

// `S` starts a show: the window goes fullscreen, and the show goes on by itself from the
// photo on screen, a photo at a time, past the video, round from the last to the first.
// Escape stops it, and the window is given back; the viewer stays, and the next Escape
// closes it.
#[test]
fn a_show_goes_fullscreen_and_on_by_itself_past_the_video_and_round() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::I);
    assert!(harness.state().viewer_info());
    key(&mut driver, &mut harness, Key::S);
    assert!(showing(harness.state()));
    assert!(!harness.state().viewer_info(), "the panel shuts");
    assert_eq!(harness.state().fullscreen_asked(), Some(true));
    // The show's step asks for the picture at once: no rest is waited for.
    let rendering = RENDERING.lock();
    until_on(&mut driver, &mut harness, ids[1]);
    assert!(harness.state().viewer_asked());
    drop(rendering);
    until_on(&mut driver, &mut harness, ids[3]);
    until_on(&mut driver, &mut harness, ids[0]);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(!showing(harness.state()));
    assert_eq!(harness.state().fullscreen_asked(), Some(false));
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
}

// A window the user had made fullscreen is not taken out of it by the show's end.
#[test]
fn a_show_leaves_alone_a_fullscreen_the_user_was_in() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path(), |_| {});
    window(&mut harness, true);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    assert!(showing(harness.state()));
    key(&mut driver, &mut harness, Key::S);
    assert!(!showing(harness.state()));
    assert_eq!(harness.state().fullscreen_asked(), None);
}

// Closed under a show, by Backspace or anything else, the viewer gives the window back.
#[test]
fn closing_the_viewer_stops_the_show_and_gives_the_window_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::Backspace);
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().fullscreen_asked(), Some(false));
}

// The countdown is the photo's: one whose picture is still being made is not counted,
// however long that takes; counted from when it is shown, it goes on.
#[test]
fn the_countdown_waits_for_the_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    let rendering = RENDERING.lock();
    until_on(&mut driver, &mut harness, ids[1]);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 20.0);
    assert_eq!(viewing(&harness), Some(ids[1]));
    drop(rendering);
    until_on(&mut driver, &mut harness, ids[3]);
}

// The interval the library has stored is the show's: two seconds, not four.
#[test]
fn the_stored_interval_is_the_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |lib| {
        lib.set_slideshow_interval_s(2).unwrap();
    });
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 2.5);
    assert_eq!(viewing(&harness), Some(ids[1]));
}

// Space pauses the show and plays it again; paused, it stays where it is.
#[test]
fn space_pauses_the_show_and_plays_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::Space);
    assert!(!harness.state().slideshow().unwrap().playing());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 20.0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::Space);
    until_on(&mut driver, &mut harness, ids[1]);
}

// In a show the buttons at the sides go round the ends too.
#[test]
fn in_a_show_the_buttons_at_the_sides_go_round() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 3);
    key(&mut driver, &mut harness, Key::S);
    driver.act(&mut harness);
    harness.get_by_label("Next photo").click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[0]));
    driver.act(&mut harness);
    harness.get_by_label("Previous photo").click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[3]));
}

// In a show the arrows follow its order: past the video, and round the ends.
#[test]
fn in_a_show_the_arrows_skip_the_video_and_go_round() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[3]));
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[1]));
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[3]));
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[0]));
}

// Started on the video, the show begins at the next photo.
#[test]
fn a_show_started_on_a_video_begins_at_the_next_photo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    click(&mut driver, &mut harness, 2);
    click(&mut driver, &mut harness, 2);
    driver.until(&mut harness, "the video's facts", |app| {
        app.viewer_item().is_some()
    });
    key(&mut driver, &mut harness, Key::S);
    assert_eq!(viewing(&harness), Some(ids[3]));
    assert!(showing(harness.state()));
}

// The photo a show leaves fades over the one arriving, and is let go of once it has.
#[test]
fn the_photo_a_show_leaves_fades_and_is_let_go_of() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().fading());
    driver.until(&mut harness, "the fade over", |app| !app.fading());
}

// At rest the controls go, the pointer with them; a move brings them back.
#[test]
fn the_controls_go_when_the_pointer_rests() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::Space);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Photo information").is_some());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 3.0);
    assert!(harness.query_by_label("Photo information").is_none());
    harness.event(egui::Event::PointerMoved(egui::pos2(500.0, 300.0)));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Photo information").is_some());
}

// A paused show whose controls have gone is a still window. (Over a library on a drive
// that is not there, as every still window's test is: a watched folder's own work asks
// for frames of its own.)
#[test]
fn a_paused_show_at_rest_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    window(&mut harness, false);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_item().is_some() && app.viewer_failed().is_some()
    });
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::Space);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 3.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 10.0), 0);
}

// Under the viewer's menu the show holds; once it is gone the photo has a whole interval.
#[test]
fn the_show_holds_under_the_viewers_menu() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path(), |_| {});
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    let at = egui::pos2(500.0, 300.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 20.0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::Escape);
    assert!(!harness.state().menu_open());
    assert!(showing(harness.state()), "the menu's Escape is the menu's");
    until_on(&mut driver, &mut harness, ids[1]);
}

// F11 puts the window in and out of fullscreen, in the grid and in the viewer.
#[test]
fn f11_puts_the_window_in_and_out_of_fullscreen_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path(), |_| {});
    key(&mut driver, &mut harness, Key::F11);
    assert_eq!(harness.state().fullscreen_asked(), Some(true));
    key(&mut driver, &mut harness, Key::F11);
    assert_eq!(harness.state().fullscreen_asked(), Some(false));
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::F11);
    assert_eq!(harness.state().fullscreen_asked(), Some(true));
    assert!(!showing(harness.state()));
}

// A shuffled show walks the view by a stride that shares no factor with its length:
// in a view of twelve, never to the photo beside it.
#[test]
fn a_shuffled_show_does_not_go_to_the_photo_beside() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    Library::open(&library.dirs().db_path)
        .unwrap()
        .set_slideshow_shuffle(true)
        .unwrap();
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_item().is_some() && app.viewer_failed().is_some()
    });
    key(&mut driver, &mut harness, Key::S);
    driver.settle(&mut harness);
    let first = harness.state().viewer().unwrap().offset();
    driver.until(&mut harness, "the next photo", |app| {
        app.viewer().is_some_and(|viewer| viewer.offset() != first)
    });
    let step = harness.state().viewer().unwrap().offset().abs_diff(first);
    assert!(step > 1 && step < 11, "{step}");
}

// A view with no photo to show says so, and starts nothing.
#[test]
fn a_view_of_videos_alone_has_no_show() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("v.mp4"), b"not a video").unwrap();
    let (mut harness, mut driver, _) = over(dir.path(), Some(&folder));
    driver.until(&mut harness, "the video shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    key(&mut driver, &mut harness, Key::S);
    driver.act(&mut harness);
    assert!(!showing(harness.state()));
    assert_eq!(said(&harness), ["There are no photos here to show."]);
}
