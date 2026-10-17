//! Compare, through the whole application and the real engine: over a library of real
//! photos made for the test, in a temporary directory. A star written here goes into
//! that directory's own `.picasa.ini`, and nowhere else.

use crate::common::{Driver, Harness, over};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_ui::{app::App, compare_view::places};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// A folder of three real photos, in the grid's order: `a.jpg` and `b.jpg` of one size,
/// `c.jpg` of another.
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

fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    let folder = pictures(dir);
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    let ids = harness.state().shown();
    (harness, driver, ids, folder)
}

/// A click on the photo at `offset` of the grid, with `held` down.
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

/// What a click is held with to add a photo to the selection: Ctrl, or Cmd on a Mac,
/// which egui reports as its `command` either way.
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

/// Selects the photos at `offsets` and opens compare on them with `C`.
fn compare(driver: &mut Driver, harness: &mut Harness<'_>, offsets: &[usize]) {
    pick(driver, harness, offsets[0], Modifiers::NONE);
    for offset in &offsets[1..] {
        pick(driver, harness, *offset, adding());
    }
    key(driver, harness, Key::C);
    driver.until(harness, "compare open", |app| app.compare().is_some());
    driver.act(harness);
}

/// Whether the last frame painted `words`, whole.
fn drew(harness: &Harness<'_>, words: &str) -> bool {
    fn each(shape: &egui::Shape, words: &str) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| each(shape, words)),
            egui::Shape::Text(text) => text.galley.text() == words,
            _ => false,
        }
    }
    (harness.output().shapes.iter()).any(|clipped| each(&clipped.shape, words))
}

fn panes(app: &App) -> Vec<i64> {
    (app.compare().unwrap().panes().iter())
        .map(|pane| pane.id)
        .collect()
}

// `C` with two to four photos selected compares them, in the grid's order; Escape goes
// back to the grid, the selection as it was.
#[test]
fn c_compares_the_photos_selected_and_escape_goes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 2]);
    assert_eq!(panes(harness.state()), [ids[0], ids[2]]);
    assert!(
        harness
            .query_by_label_contains("Pane 1: a.jpg, 1200 × 800")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Pane 2: c.jpg, 600 × 800")
            .is_some()
    );
    // They differ in size, and each says its own.
    assert!(drew(&harness, "1200 × 800") && drew(&harness, "600 × 800"));
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().compare().is_none());
    assert_eq!(harness.state().selected().len(), 2);
}

// One photo is no comparison: `C` does nothing.
#[test]
fn one_photo_is_no_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::C);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().compare().is_none());
    // Nor is the library asked anything of it.
    assert_eq!(harness.state().compares_asked(), 0);
}

// The tile's menu offers it too, for the photos selected.
#[test]
fn the_tiles_menu_compares_the_photos_selected() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 1, adding());
    pick(&mut driver, &mut harness, 2, adding());
    let at: Pos2 = harness.state().tile_rect(1).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Compare 3 photos").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "compare open", |app| app.compare().is_some());
    assert_eq!(panes(harness.state()), ids);
}

// Enter opens the pane with the keys in the viewer; compare is gone.
#[test]
fn enter_opens_the_pane_with_the_keys_in_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1, 2]);
    key(&mut driver, &mut harness, Key::Num2);
    assert_eq!(harness.state().compare().unwrap().focus(), 1);
    key(&mut driver, &mut harness, Key::Enter);
    assert!(harness.state().compare().is_none());
    assert_eq!(
        harness.state().viewer().map(|viewer| viewer.id()),
        Some(ids[1])
    );
}

// `S` stars the pane with the keys, in the folder's `.picasa.ini`; the pane says so once
// the grid does.
#[test]
fn s_stars_the_pane_with_the_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    key(&mut driver, &mut harness, Key::Tab);
    key(&mut driver, &mut harness, Key::S);
    driver.until(&mut harness, "the pane starred", |app| {
        app.compare().unwrap().panes()[1].starred
    });
    assert!(!harness.state().compare().unwrap().panes()[0].starred);
    let ini = std::fs::read_to_string(folder.join(".picasa.ini")).unwrap();
    assert!(ini.contains("[b.jpg]") && ini.contains("star=yes"), "{ini}");
    assert_eq!(panes(harness.state()), [ids[0], ids[1]]);
}

// At fit no full picture is made - a pane's preview is about as large as the pane; once
// zoomed, every pane's is, one after another.
#[test]
fn every_panes_full_picture_is_made_once_zoomed_and_none_at_fit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1, 2]);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().compare_pictures(),
        Some(vec![None, None, None])
    );
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        3,
    )[0];
    harness.event(egui::Event::PointerMoved(place.center()));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    assert!(!harness.state().compare().unwrap().zoom().is_fit());
    driver.until(&mut harness, "every pane's full picture", |app| {
        app.compare_pictures()
            .is_some_and(|pictures| pictures.iter().all(Option::is_some))
    });
    assert_eq!(
        harness.state().compare_pictures(),
        Some(vec![Some(1200), Some(1200), Some(600)])
    );
}

// A pane's full picture has no level past twice the pane until a zoom outgrows the
// largest it holds, and then the whole picture is made again: a notch is 588 pixels drawn
// of a level 600 wide, three are 848.
#[test]
fn a_panes_whole_picture_is_made_only_once_a_zoom_outgrows_its_levels() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1, 2]);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let place = places(
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0)),
        3,
    )[0];
    let notch = |harness: &mut Harness<'_>, lines: f32| {
        harness.event(egui::Event::PointerMoved(place.center()));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, lines),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
    };
    notch(&mut harness, 1.0);
    driver.act(&mut harness);
    driver.until(&mut harness, "every pane's full picture", |app| {
        app.compare_pictures()
            .is_some_and(|pictures| pictures.iter().all(Option::is_some))
    });
    driver.frames_in(&mut harness, 0.5);
    driver.settle(&mut harness);
    assert_eq!(
        harness.state().compare_pictures(),
        Some(vec![Some(600), Some(600), Some(600)])
    );
    notch(&mut harness, 2.0);
    driver.act(&mut harness);
    driver.until(&mut harness, "the whole pictures", |app| {
        app.compare_pictures() == Some(vec![Some(1200), Some(1200), Some(600)])
    });
}

// A compare left alone is a still window.
#[test]
fn compare_left_alone_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    compare(&mut driver, &mut harness, &[0, 1]);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
