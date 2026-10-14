//! Compare at its edges: the review's tests of it (pull request #195) and the fix pass's.
//! Each says what it shows. Over libraries of real photos in temporary
//! directories; a star goes into that directory's own `.picasa.ini`.

mod common;

use common::{Driver, Harness, hold_over, over};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_engine::commands;
use photon_ui::{app::App, compare_view::places};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, (name, width, height)) in [
        ("a.jpg", 1200, 800),
        ("b.jpg", 1200, 800),
        ("c.jpg", 600, 800),
    ]
    .into_iter()
    .enumerate()
    {
        let picture = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([60 * n as u8, 120, 200]),
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

fn launched<'a>(dir: &Path, folder: &Path) -> (Harness<'a>, Driver, Vec<i64>) {
    let (mut harness, mut driver, _) = over(dir, Some(folder));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    let ids = harness.state().shown();
    (harness, driver, ids)
}

fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    let folder = pictures(dir);
    let (harness, driver, ids) = launched(dir, &folder);
    (harness, driver, ids, folder)
}

fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: held,
        });
    }
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

fn adding() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    }
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn select(driver: &mut Driver, harness: &mut Harness<'_>, offsets: &[usize]) {
    pick(driver, harness, offsets[0], Modifiers::NONE);
    for offset in &offsets[1..] {
        pick(driver, harness, *offset, adding());
    }
}

fn compare(driver: &mut Driver, harness: &mut Harness<'_>, offsets: &[usize]) {
    select(driver, harness, offsets);
    key(driver, harness, Key::C);
    driver.until(harness, "compare open", |app| app.compare().is_some());
    driver.act(harness);
}

fn key_event(key: Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn starred_panes(app: &App) -> Vec<bool> {
    (app.compare().unwrap().panes().iter())
        .map(|pane| pane.starred)
        .collect()
}

/// Two of the three photos starred, then the Starred view.
fn in_starred<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    let (mut harness, mut driver, ids, folder) = three(dir);
    select(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "two starred", |app| app.starred().len() == 2);
    driver.settle(&mut harness);
    common::click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "Starred shown", |app| {
        app.photos() == 2 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    (harness, driver, ids, folder)
}

// A pane says what was written of its star, whether or not the grid on screen holds the
// photo: in Starred, one unstarred from compare leaves the grid, and its pane said it was
// starred still.
#[test]
fn a_pane_unstarred_in_starred_says_it_is_no_longer_starred() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = in_starred(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    assert_eq!(starred_panes(harness.state()), [true, true]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "one left in Starred", |app| app.photos() == 1);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        starred_panes(harness.state()),
        [false, true],
        "the library has a.jpg unstarred; its pane still says starred"
    );
}

// And `S` again on that pane stars it back: read from the stale star, it wrote "unstar"
// a second time, and the photo could not be starred again from compare.
#[test]
fn s_twice_on_a_pane_in_starred_stars_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = in_starred(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "one left in Starred", |app| app.photos() == 1);
    driver.settle(&mut harness);
    key(&mut driver, &mut harness, Key::S);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().photos(),
        2,
        "the second S did not star a.jpg back: the grid of Starred holds one photo"
    );
}

// `C` is not answered while a press on the photos is held, as Enter is not: compare
// covers the window, the button came up there where the grid never heard it, and after
// Escape the first move of the mouse drew a band with no button down. Once the button is
// up, `C` compares.
#[test]
fn c_under_a_held_press_compares_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    select(&mut driver, &mut harness, &[0, 1]);
    let at = harness.state().tile_rect(2).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: adding(),
    });
    driver.act(&mut harness);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::C);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().compare().is_none());
    assert_eq!(harness.state().compares_asked(), 0);
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: adding(),
    });
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "compare open", |app| app.compare().is_some());
}

// Compare does not open over whatever has come since it was asked for: `C` and Enter in one
// frame open the viewer, and compare, a round trip later, landed over it - Escape then
// left the user in a viewer nobody had seen open.
#[test]
fn compare_does_not_open_over_a_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    select(&mut driver, &mut harness, &[0, 1]);
    harness.input_mut().events.extend([
        key_event(Key::C, true),
        key_event(Key::C, false),
        key_event(Key::Enter, true),
        key_event(Key::Enter, false),
    ]);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let app = harness.state();
    assert!(
        !(app.compare().is_some() && app.viewer().is_some()),
        "compare stands over an open viewer"
    );
    // Nor does it go up once the viewer has closed: its answer was dropped.
    key(&mut driver, &mut harness, Key::Escape);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none());
    assert!(harness.state().compare().is_none());
}

// A pane's photo turned from elsewhere has its full picture made under its new key: under
// the key compare opened with, the engine answered "stale" and the pane stayed at its old
// preview.
#[test]
fn a_pane_turned_from_elsewhere_has_its_full_picture_made() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[1], true).unwrap();
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        2,
    )[0];
    harness.event(egui::Event::PointerMoved(place.center()));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the first pane's full picture", |app| {
        app.compare_pictures().is_some_and(|p| p[0].is_some())
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().compare_pictures(),
        Some(vec![Some(1200), Some(800)]),
        "the turned pane's picture is never made"
    );
}

// Enter on a pane the grid on screen no longer holds - unstarred in Starred - opens
// nothing, the viewer going through the grid's photos, and leaves compare as it was: it
// closed compare and opened nothing.
#[test]
fn enter_on_a_pane_the_grid_no_longer_holds_leaves_compare_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = in_starred(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "one left in Starred", |app| app.photos() == 1);
    driver.settle(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none());
    assert!(harness.state().compare().is_some(), "compare stands");
}

// A pinch zooms the panes: egui-winit reports a trackpad's pinch as `Event::Zoom`, which
// the viewer answers, and the Svelte compare got as a Ctrl+wheel.
#[test]
fn a_pinch_zooms_the_panes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        2,
    )[0];
    harness.event(egui::Event::PointerMoved(place.center()));
    harness.event(egui::Event::Zoom(1.5));
    driver.act(&mut harness);
    assert!(!harness.state().compare().unwrap().zoom().is_fit());
}

// A press on a message that lies over a pane is the message's alone - the one judge,
// `layer_id_at`: it dismissed the message and gave the pane under it the keys.
#[test]
fn a_press_on_a_message_over_a_pane_is_not_the_panes() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    // An INI that is no regular file: the star is refused, and said.
    std::fs::create_dir(folder.join(".picasa.ini")).unwrap();
    let (mut harness, mut driver, _) = launched(dir.path(), &folder);
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(harness.state().compare().unwrap().focus(), 0);
    let dismiss = harness.get_by_label("Dismiss").rect().center();
    let panes = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        2,
    );
    assert!(panes[1].contains(dismiss), "the message lies over pane 2");
    harness.event(egui::Event::PointerMoved(dismiss));
    driver.act(&mut harness);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: dismiss,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    assert!(harness.state().toasts().is_empty(), "dismissed");
    assert_eq!(
        harness.state().compare().unwrap().focus(),
        0,
        "the press on the message gave the pane under it the keys"
    );
}

// AccessKit hears whether a pane is starred, as the Svelte pane labels its star.
#[test]
fn a_starred_pane_says_so_to_accesskit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "starred", |app| {
        app.compare().unwrap().panes()[0].starred
    });
    driver.act(&mut harness);
    assert!(
        harness.query_by_label_contains("Starred").is_some()
            || harness.query_by_label_contains("starred").is_some(),
        "nothing tells AccessKit the pane is starred"
    );
}

// A key pressed before compare was ever drawn is the grid's: compare goes up at the end of
// the frame its answer arrives in. Read by compare in that frame, the `.` meant for the
// grid starred pane 1 alone.
#[test]
fn a_key_pressed_before_compare_was_drawn_writes_nothing_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    select(&mut driver, &mut harness, &[0, 1]);
    // `C`'s press and release in one frame: the frame reads the library's answers before it
    // asks, so none can be read in it. With a frame for each, as `key` draws them, a quick
    // answer was read in the second (macOS' and Windows' CI, the same run).
    harness
        .input_mut()
        .events
        .extend([key_event(Key::C, true), key_event(Key::C, false)]);
    driver.act(&mut harness);
    assert!(harness.state().compare().is_none(), "not answered yet");
    // The library answers; no frame is drawn meanwhile.
    driver.quiet();
    harness
        .input_mut()
        .events
        .extend([key_event(Key::Period, true), key_event(Key::Period, false)]);
    driver.act(&mut harness);
    assert!(harness.state().compare().is_some(), "opened in this frame");
    driver.settle(&mut harness);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_ne!(
        harness.state().starred().len(),
        1,
        "the first pane was starred by a key pressed before compare was drawn"
    );
}

// Of the full pictures, the pane with the keys has its own first.
#[test]
fn the_pane_with_the_keys_has_its_full_picture_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1, 2]);
    key(&mut driver, &mut harness, Key::Num3);
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        3,
    )[2];
    harness.event(egui::Event::PointerMoved(place.center()));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "a full picture", |app| {
        app.compare_pictures()
            .is_some_and(|p| p.iter().any(Option::is_some))
    });
    assert!(
        harness.state().compare_pictures().unwrap()[2].is_some(),
        "{:?}",
        harness.state().compare_pictures()
    );
}

// A pane whose picture cannot be made leaves a still window: it is not asked again and
// again.
#[test]
fn a_pane_whose_picture_cannot_be_made_leaves_a_still_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, folder) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    driver.settle(&mut harness);
    std::fs::remove_file(folder.join("b.jpg")).unwrap();
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        2,
    )[0];
    harness.event(egui::Event::PointerMoved(place.center()));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the first pane's picture", |app| {
        app.compare_pictures().is_some_and(|p| p[0].is_some())
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A window resized under a pan holds the pan to the new panes.
#[test]
fn a_window_resized_under_a_pan_holds_the_pan_to_the_new_panes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    let window = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let place = places(window, 2)[0];
    // Zoomed about the pane's corner: the pan at its far bound.
    let corner = place.left_top() + egui::vec2(5.0, 5.0);
    harness.event(egui::Event::PointerMoved(corner));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 2.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    let zoom = *harness.state().compare().unwrap().zoom();
    let pan = zoom.pan();
    assert!(pan.x > 0.0 && pan.y > 0.0, "{pan:?}");
    // Narrower: the bounds shrink with the pane.
    harness.set_size(egui::vec2(600.0, 400.0));
    driver.act(&mut harness);
    let small = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0)),
        2,
    )[0];
    let after = harness.state().compare().unwrap().zoom().pan();
    let bound_x = small.width() * (zoom.zoom() - 1.0) / 2.0;
    assert!(after.x <= bound_x + 0.01, "{after:?} beyond {bound_x}");
}

// Under a message over the panes, Tab still moves between the panes and gives the
// message's button no keyboard: egui's own walk is called off.
#[test]
fn tab_gives_a_messages_button_no_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    std::fs::create_dir(folder.join(".picasa.ini")).unwrap();
    let (mut harness, mut driver, _) = launched(dir.path(), &folder);
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Tab);
    driver.act(&mut harness);
    assert!(harness.ctx.memory(|memory| memory.focused()).is_none());
    assert_eq!(harness.state().compare().unwrap().focus(), 1);
}

// Compare closed holds no preview as a texture.
#[test]
fn compare_closed_holds_no_preview() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    driver.until(&mut harness, "the previews", |app| app.preview_bytes() > 0);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(harness.state().preview_bytes(), 0);
}

// Escape and `S` in one frame: compare closes, and the star after the close is no one's.
#[test]
fn a_key_after_the_escape_that_closes_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, folder) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    harness
        .input_mut()
        .events
        .extend([key_event(Key::Escape, true), key_event(Key::S, true)]);
    driver.act(&mut harness);
    assert!(harness.state().compare().is_none());
    driver.settle(&mut harness);
    assert!(!folder.join(".picasa.ini").exists(), "nothing starred");
}

// A folder held over the window shows its card over compare, as over the viewer.
#[test]
fn a_folder_held_over_compare_shows_its_card() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, folder) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    hold_over(&mut driver, &mut harness, &[&folder]);
    driver.act(&mut harness);
    assert!(
        harness
            .query_by_label(photon_ui::folder_drop::CARD)
            .is_some()
    );
}

// AccessKit hears which pane has the keys.
#[test]
fn accesskit_hears_which_pane_has_the_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::Num2);
    driver.act(&mut harness);
    let first = harness.get_by_label_contains("Pane 1: ");
    let second = harness.get_by_label_contains("Pane 2: ");
    // (egui says "selected" through AccessKit's toggled state.)
    let toggled = egui::accesskit::Toggled::True;
    assert_ne!(first.accesskit_node().toggled(), Some(toggled));
    assert_eq!(second.accesskit_node().toggled(), Some(toggled));
}

// A star refused - the folder cannot be written - leaves the pane as it was, the grid on
// screen holding the photo or not: in Starred, one unstarred from compare has left the
// grid, and a refusal to star it back must not have its pane say "starred".
#[cfg(unix)]
#[test]
fn a_star_refused_for_a_pane_the_grid_does_not_hold_leaves_it_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, folder) = in_starred(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "one left in Starred", |app| app.photos() == 1);
    driver.settle(&mut harness);
    assert!(!starred_panes(harness.state())[0]);
    // The folder made read-only: the INI's write is refused.
    let mut readonly = std::fs::metadata(&folder).unwrap().permissions();
    readonly.set_mode(0o555);
    std::fs::set_permissions(&folder, readonly).unwrap();
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    let mut writable = std::fs::metadata(&folder).unwrap().permissions();
    writable.set_mode(0o755);
    std::fs::set_permissions(&folder, writable).unwrap();
    assert!(!starred_panes(harness.state())[0], "as it was");
}
