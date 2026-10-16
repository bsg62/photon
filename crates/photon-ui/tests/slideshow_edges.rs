//! The slideshow at its edges: the review's tests of it (pull request #194) and the fix
//! pass's. Each says what it shows.

use crate::common::{Driver, Harness, over};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, ViewportId};
use egui_kittest::kittest::Queryable;
use photon_engine::engine::RENDERING;
use photon_ui::{
    app::App,
    viewer_layout::{CLOSE, EDGE},
};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// A folder of real JPEGs (and a file the library takes for a video, for a name ending
/// in `.mp4`), in the grid's order.
fn pictures(dir: &Path, names: &[&str]) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, name) in names.iter().enumerate() {
        let bytes = if name.ends_with(".mp4") {
            b"not a video".to_vec()
        } else if name.starts_with("broken") {
            b"not a picture".to_vec()
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

fn launched<'a>(dir: &Path, names: &[&str]) -> (Harness<'a>, Driver, Vec<i64>) {
    let folder = pictures(dir, names);
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
    driver.until(&mut harness, "the photos shown", |app| {
        app.photos() == names.len() && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    window(&mut harness, false);
    let ids = harness.state().shown();
    (harness, driver, ids)
}

/// `a.jpg`, `b.jpg`, `v.mp4`, `c.jpg`: `tests/slideshow.rs`'s `four`.
fn four<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>) {
    launched(dir, &["a.jpg", "b.jpg", "v.mp4", "c.jpg"])
}

fn window(harness: &mut Harness<'_>, fullscreen: bool) {
    (harness.input_mut().viewports)
        .entry(ViewportId::ROOT)
        .or_default()
        .fullscreen = Some(fullscreen);
}

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

/// A key pressed, with the window not yet having done what it was asked.
fn key_unobeyed(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn key_event(key: Key, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers: Modifiers::NONE,
    }
}

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

// Whatever stands at a lost photo's offset is the photo that came after it (`Viewer::step`,
// and the Svelte `goto`'s `orphaned && next === current + 1`): `H` on `a.jpg` during a
// show, in All photos, and the show goes on to `b.jpg` - it went on to `c.jpg`.
#[test]
fn a_show_goes_on_to_the_photo_after_one_hidden_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    assert!(showing(harness.state()));
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    driver.until(&mut harness, "the show going on", |app| {
        app.viewer().is_some_and(|viewer| viewer.id() != ids[0])
    });
    assert_eq!(
        viewing(&harness),
        Some(ids[1]),
        "the photo after the hidden one is b.jpg"
    );
}

// In a view of two photos, the one on screen hidden under the show: the other now stands at
// its offset, and the show goes on to it - it stood on the hidden photo for good.
#[test]
fn a_show_of_two_goes_on_to_the_other_once_the_first_is_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = launched(dir.path(), &["a.jpg", "b.jpg"]);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 20.0);
    assert!(showing(harness.state()));
    assert_eq!(
        viewing(&harness),
        Some(ids[1]),
        "twenty seconds on b.jpg's way"
    );
}

// The show is held while a keyword is typed into the info panel's field (opened during a
// show with `I`): the field keeps the keyboard across a step, and the rest of a half-typed
// keyword - "on" of "Lisbon" - went to a photo nobody was typing for.
#[test]
fn a_show_holds_while_a_keyword_is_typed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::I);
    assert!(harness.state().viewer_info());
    driver.act(&mut harness);
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .click();
    driver.act(&mut harness);
    assert!(
        harness.ctx.memory(|memory| memory.focused().is_some()),
        "the field has it"
    );
    harness.event(egui::Event::Text("Lisb".to_owned()));
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 10.0);
    assert_eq!(
        viewing(&harness),
        Some(ids[0]),
        "the show went on under the keyword being typed"
    );
}

// A press of a pointer resting in a show is the photo's: a double click where the cross
// was zooms. The controls are woken by the pointer moving and nothing else, as in the
// Svelte viewer - woken by the press, the second click pressed the cross they woke.
#[test]
fn a_double_click_where_the_cross_was_is_the_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::Space);
    let cross = egui::pos2(1000.0 - EDGE - CLOSE / 2.0, EDGE + CLOSE / 2.0);
    harness.event(egui::Event::PointerMoved(cross));
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 3.0);
    assert!(harness.query_by_label("Close viewer").is_none(), "at rest");
    for _ in 0..2 {
        harness.event(button(cross, PointerButton::Primary, true));
        driver.act(&mut harness);
        harness.event(button(cross, PointerButton::Primary, false));
        driver.act(&mut harness);
    }
    assert_eq!(viewing(&harness), Some(ids[0]), "the viewer closed");
    assert!(
        !harness.state().viewer().unwrap().zoom().is_fit(),
        "a double click on the photo zooms"
    );
}

// A menu asked for in the frame before the countdown's end holds the show: the hold is
// asked before the countdown is. (Asked after, the show went on under the menu, which then
// closed: it was of a photo nothing was known of any more.)
#[test]
fn a_menu_asked_for_just_before_the_countdown_ends_holds_the_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    driver.settle(&mut harness);
    // Past the controls' rest, the next frame asked for is the countdown's end.
    driver.frames_in(&mut harness, 3.0);
    let now = driver.now * 1000.0;
    let due = (harness.state().slideshow().unwrap().next(now)).unwrap() / 1000.0;
    assert!(due > driver.now + 0.05, "the countdown is still running");
    let at = egui::pos2(500.0, 300.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    driver.frame(&mut harness, due - 0.010);
    assert!(harness.state().menu_open());
    harness.event(button(at, PointerButton::Secondary, false));
    driver.frame(&mut harness, due + 0.001);
    assert_eq!(
        viewing(&harness),
        Some(ids[0]),
        "the show stepped under the menu"
    );
    assert!(harness.state().menu_open());
}

// An arrow and `S` in one frame: the step is acted on first, and the show counts the photo
// stepped to only from when its picture is shown - not from the frame's word for the photo
// stepped from.
#[test]
fn an_arrow_and_s_in_one_frame_do_not_count_a_picture_still_being_made() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    let rendering = RENDERING.lock();
    (harness.input_mut().events).extend([
        key_event(Key::ArrowRight, true, false),
        key_event(Key::S, true, false),
    ]);
    driver.act(&mut harness);
    obey(&mut harness);
    assert!(showing(harness.state()));
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(
        harness.state().viewer_levels().is_none(),
        "still being made"
    );
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 6.0);
    assert_eq!(
        viewing(&harness),
        Some(ids[1]),
        "the show went on from a photo whose picture was never shown"
    );
    drop(rendering);
}

// A held F11 toggles fullscreen once, at its press: answered at its repeats too, it
// flickered the window for as long as it was held.
#[test]
fn a_held_f11_does_not_flicker() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path());
    harness.event(key_event(Key::F11, true, false));
    driver.act(&mut harness);
    obey(&mut harness);
    assert_eq!(harness.state().fullscreen_asked(), Some(true));
    for _ in 0..3 {
        harness.event(key_event(Key::F11, true, true));
        driver.act(&mut harness);
        obey(&mut harness);
    }
    assert_eq!(
        harness.state().fullscreen_asked(),
        Some(true),
        "the key's repeats toggled it"
    );
}

// A show stopped and started again before the window has left fullscreen goes fullscreen:
// egui reports what the window last said, a compositor says it a configure later, and the
// second start took the window for one the user had made fullscreen.
#[test]
fn a_show_started_again_at_once_is_fullscreen() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    assert_eq!(harness.state().fullscreen_asked(), Some(true));
    // Stopped, and started again before the window has said it has left fullscreen.
    key_unobeyed(&mut driver, &mut harness, Key::S);
    key_unobeyed(&mut driver, &mut harness, Key::S);
    obey(&mut harness);
    driver.act(&mut harness);
    assert!(showing(harness.state()));
    assert_eq!(
        harness.state().fullscreen_asked(),
        Some(true),
        "the second show plays in a window"
    );
}

// A playing show between two photos, at rest, after the fade, draws nothing until its
// countdown's end. Over a library on a drive that is not there, as the paused test is.
#[test]
fn a_playing_show_between_two_photos_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = crate::common::library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = crate::common::opened(&library, 12);
    driver.settle(&mut harness);
    window(&mut harness, false);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_item().is_some() && app.viewer_failed().is_some()
    });
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "the first step", |app| {
        app.viewer().is_some_and(|viewer| viewer.offset() != 0)
    });
    driver.until(&mut harness, "the fade over", |app| !app.fading());
    driver.settle(&mut harness);
    let stepped = driver.frames;
    let before = harness.state().viewer().unwrap().offset();
    let drawn = driver.frames_in(&mut harness, 2.5);
    assert_eq!(harness.state().viewer().unwrap().offset(), before);
    assert_eq!(drawn, 0, "frames between two photos ({stepped} before)");
}

/// Draws what is asked for until the viewer is on `id`.
fn until_on(driver: &mut Driver, harness: &mut Harness<'_>, id: i64) {
    driver.until(harness, "the show on the photo", |app| {
        app.viewer().is_some_and(|viewer| viewer.id() == id)
    });
}

// The photo the show left fades from when the photo arriving is shown, a frame at a time
// for as long as it fades; and a show stopped lets go of it at once.
#[test]
fn the_fade_draws_its_frames_and_a_stopped_show_lets_go_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().fading());
    // Counted from the step to the fade's end, and not over 600 ms from the arriving
    // photo's full picture: on a slow runner (macOS' CI) the fade, begun at its preview,
    // was over on the test's clock before that picture had been made, and none of its
    // frames was counted. A fade that asked for no frame of its own lets the clock jump
    // to its end in one step, which counts a frame or two.
    let from = driver.frames;
    driver.until(&mut harness, "the fade over", |app| !app.fading());
    let frames = driver.frames - from;
    assert!(frames > 10, "a frame at a time: {frames}");
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert!(harness.state().fading());
    key(&mut driver, &mut harness, Key::Escape);
    assert!(!showing(harness.state()));
    assert!(!harness.state().fading());
}

// A show started on the video goes to the next photo, whose picture is still being made:
// it is counted from when that picture is shown, not from the video's refusal.
#[test]
fn a_show_started_on_a_video_counts_the_next_photo_from_its_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    click(&mut driver, &mut harness, 2);
    click(&mut driver, &mut harness, 2);
    driver.until(&mut harness, "the video's facts and its refusal", |app| {
        app.viewer_item().is_some() && app.viewer_failed().is_some()
    });
    let rendering = RENDERING.lock();
    key(&mut driver, &mut harness, Key::S);
    assert_eq!(viewing(&harness), Some(ids[3]));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 20.0);
    assert_eq!(viewing(&harness), Some(ids[3]));
    drop(rendering);
    until_on(&mut driver, &mut harness, ids[0]);
}

// A photo whose picture cannot be shown counts as shown: the show goes on past it.
#[test]
fn a_photo_that_cannot_be_shown_does_not_stop_the_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = launched(dir.path(), &["a.jpg", "broken.jpg", "c.jpg"]);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    until_on(&mut driver, &mut harness, ids[1]);
    until_on(&mut driver, &mut harness, ids[2]);
}

// A photo that leaves the library under the show, before its picture was shown, counts as
// shown - there will be none - and the show goes on to the photo after it: the photo now at
// its offset, not the one after that.
#[test]
fn a_photo_gone_under_the_show_does_not_stop_it() {
    let dir = tempfile::tempdir().unwrap();
    let names = ["a.jpg", "b.jpg", "c.jpg", "d.jpg"];
    let (mut harness, mut driver, ids) = launched(dir.path(), &names);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    let rendering = RENDERING.lock();
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    std::fs::remove_file(dir.path().join("Pictures").join("b.jpg")).unwrap();
    driver.until(&mut harness, "the photo gone", |app| {
        app.viewer().is_some_and(|viewer| viewer.gone())
    });
    until_on(&mut driver, &mut harness, ids[2]);
    drop(rendering);
}

// A photo the library says cannot be shown counts as shown though its picture, made all
// the same, is not held: the show goes on past it.
#[test]
fn a_photo_the_library_cannot_show_does_not_stop_the_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = launched(dir.path(), &["a.jpg", "b.jpg", "c.jpg"]);
    let engine = harness.state().engine().clone();
    (engine.lib)
        .set_thumb_state(
            ids[1],
            photon_core::media::ThumbState::Failed,
            Some("It is broken."),
        )
        .unwrap();
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    let rendering = RENDERING.lock();
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.until(&mut harness, "what the library says of it", |app| {
        app.viewer_item().is_some_and(|item| item.id == ids[1])
    });
    drop(rendering);
    until_on(&mut driver, &mut harness, ids[2]);
}

// A window the user has made fullscreen themselves, after photon had last asked it to leave
// fullscreen, is the user's: a show leaves it alone.
#[test]
fn a_fullscreen_the_user_made_after_photon_asked_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = four(dir.path());
    key(&mut driver, &mut harness, Key::F11);
    key(&mut driver, &mut harness, Key::F11);
    assert_eq!(harness.state().fullscreen_asked(), Some(false));
    driver.act(&mut harness);
    window(&mut harness, true);
    driver.act(&mut harness);
    open(&mut driver, &mut harness, 0);
    key_unobeyed(&mut driver, &mut harness, Key::S);
    assert!(showing(harness.state()));
    assert_eq!(
        harness.state().fullscreen_asked(),
        Some(false),
        "nothing asked"
    );
}

// The crop tool stops a show: the countdown would step from under the tool.
#[test]
fn the_crop_tool_stops_a_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = four(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_some());
    assert!(!showing(harness.state()));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 10.0);
    assert!(harness.state().cropping().is_some());
    assert_eq!(viewing(&harness), Some(ids[0]));
}

// A view of one photo has nowhere to go: the show stands, and makes nothing again at
// each interval.
#[test]
fn a_show_of_one_photo_stands_and_makes_nothing_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = launched(dir.path(), &["a.jpg"]);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::S);
    driver.settle(&mut harness);
    let asked = harness.state().pictures_asked();
    driver.frames_in(&mut harness, 20.0);
    driver.settle(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[0]));
    assert_eq!(harness.state().pictures_asked(), asked);
    assert!(!harness.state().fading());
}
