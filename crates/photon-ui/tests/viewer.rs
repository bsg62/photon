//! The viewer, through the whole application and the real engine: over a library of real
//! photos made for the test, in a temporary directory. A star written here goes into
//! that directory's own `.picasa.ini`, and nowhere else.

use crate::common::{
    Driver, Harness, hold_over, library, opened, over, over_animated, photo_ids, said,
};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{
    edit::Edit,
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    grid::GridView,
    library::Library,
    media::ThumbState,
    thumbs::ThumbSize,
};
use photon_engine::{commands, engine::RENDERING};
use photon_ui::{folder_drop::CARD, grid::scroll::Notch};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// A folder with three real photos, in the order the grid has them: `a.jpg`, a strip
/// 4,200 pixels wide, whose full picture has three levels; then `b.jpg` and `c.jpg`,
/// small.
fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, (name, width, height)) in [("a.jpg", 4200, 60), ("b.jpg", 64, 48), ("c.jpg", 48, 64)]
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

/// The application over a library that watches `pictures(dir)`, once its three photos
/// are shown; and their ids, in the grid's order.
fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    three_by(dir, over)
}

/// `three`, with the animations on.
fn three_animated<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    three_by(dir, over_animated)
}

fn three_by<'a>(
    dir: &Path,
    launch: impl FnOnce(&Path, Option<&Path>) -> (Harness<'a>, Driver, photon_ui::dirs::Dirs),
) -> (Harness<'a>, Driver, Vec<i64>, PathBuf) {
    let folder = pictures(dir);
    let (mut harness, mut driver, _) = launch(dir, Some(&folder));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    // A device that holds a texture of any size these tests make: where none says, egui
    // takes it for 2,048 pixels.
    harness.input_mut().max_texture_side = Some(16_384);
    let ids = harness.state().shown();
    (harness, driver, ids, folder)
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The middle of the photo at `offset` of the grid, where it was last drawn.
fn tile(harness: &Harness<'_>, offset: usize) -> Pos2 {
    (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center()
}

/// A click on the photo at `offset` of the grid.
fn click(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = tile(harness, offset);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

/// Opens the viewer on the photo at `offset` by a double click, and waits for the
/// library to have said what it is.
fn open(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    click(driver, harness, offset);
    click(driver, harness, offset);
    assert!(harness.state().viewer().is_some(), "the viewer is open");
    driver.until(harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

/// The file's name, as the library last said it of the photo in the viewer.
fn caption(harness: &Harness<'_>) -> String {
    (harness.state().viewer_item())
        .map(|item| item.file_name.clone())
        .unwrap_or_default()
}

/// The caption in the viewer's bar, as it was last drawn: the one line with a size in
/// pixels in it. (A label's words are its value to AccessKit.)
fn caption_line(harness: &Harness<'_>) -> String {
    let node = harness.get_by_label_contains(" × ").accesskit_node();
    node.value().or_else(|| node.label()).unwrap_or_default()
}

// The whole of it: a double click opens the photo over the whole window, its preview is
// there at once and its full picture a moment later, in the levels photon makes itself;
// the caption says what it is and where it stands.
#[test]
fn a_double_click_opens_the_photo_with_its_preview_and_then_its_full_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 0);
    assert_eq!(viewing(&harness), None, "one click selects");
    assert_eq!(harness.state().selected(), [ids[0]]);
    click(&mut driver, &mut harness, 0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    driver.until(&mut harness, "the preview", |app| {
        app.viewer_item().is_some()
    });
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(
        harness.state().viewer_levels(),
        Some(vec![4200, 2100, 1050, 525, 262])
    );
    // And the copy that is drawn while it is fitted: scaled for this window, a
    // thousand pixels wide.
    assert_eq!(harness.state().viewer_fitted(), Some([1000, 14]));
    assert!(harness.state_mut().viewer_preview());
    driver.act(&mut harness);
    // The shell is not drawn under it.
    assert!(harness.query_by_label("Starred").is_none());
    assert!(harness.query_by_label("Close viewer").is_some());
    let line = caption_line(&harness);
    assert!(line.starts_with("a.jpg · "), "{line}");
    assert!(
        line.contains(" · 4200 × 60 · ") && line.ends_with("(1 / 3)"),
        "{line}"
    );
}

// A texture's side is limited by the device. A level larger than it is not made into a
// texture, and the deepest there is to zoom into is the first half that fits.
#[test]
fn a_level_the_device_cannot_hold_is_not_made_into_a_texture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    harness.input_mut().max_texture_side = Some(2_500);
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(
        harness.state().viewer_levels(),
        Some(vec![2100, 1050, 525, 262])
    );
}

// The copy a fitted photo is drawn from is made for the window's size. In a window made
// another size it is made again - once the size has stood for a moment, since a window
// being dragged is another size in every frame and each copy is a whole decode.
#[test]
fn the_fitted_copy_is_made_again_for_a_window_of_another_size_once_it_has_stood() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_fitted().is_some()
    });
    assert_eq!(harness.state().viewer_fitted(), Some([1000, 14]));
    driver.settle(&mut harness);
    for width in [960.0, 900.0, 840.0] {
        harness.set_size(egui::vec2(width, 700.0));
        driver.act(&mut harness);
    }
    driver.settle(&mut harness);
    assert_eq!(
        harness.state().viewer_fitted(),
        Some([1000, 14]),
        "not for a size that has not stood"
    );
    // The frame the size has stood long enough in is asked for, and asks - once,
    // whatever frames are drawn while the copy is being made: the mouse moves.
    let rendering = RENDERING.lock();
    assert!(driver.frames_in(&mut harness, 0.3) >= 1);
    assert_eq!(harness.state().pictures_asked(), 2);
    for _ in 0..3 {
        driver.act(&mut harness);
    }
    assert_eq!(harness.state().pictures_asked(), 2);
    drop(rendering);
    driver.until(&mut harness, "the copy for the new size", |app| {
        app.viewer_fitted() == Some([840, 12])
    });
    // And then the window is still.
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// Enter opens the photo the keys are on, and each way out hands it back to the grid:
// selected, with the shell there again.
#[test]
fn enter_opens_the_photo_the_keys_are_on_and_every_way_out_hands_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 1);
    driver.frames_in(&mut harness, 1.0);
    for close in 0..4 {
        key(&mut driver, &mut harness, Key::Enter);
        assert_eq!(viewing(&harness), Some(ids[1]), "{close}");
        driver.act(&mut harness);
        match close {
            0 => key(&mut driver, &mut harness, Key::Escape),
            1 => key(&mut driver, &mut harness, Key::Backspace),
            2 => {
                let at = egui::pos2(500.0, 300.0);
                harness.event(button(at, PointerButton::Extra1, true));
                harness.event(button(at, PointerButton::Extra1, false));
                driver.act(&mut harness);
            }
            _ => {
                harness.get_by_label("Close viewer").click();
                driver.act(&mut harness);
            }
        }
        assert_eq!(viewing(&harness), None, "{close}");
        driver.act(&mut harness);
        assert_eq!(harness.state().selected(), [ids[1]]);
        assert!(
            harness.query_by_label("Starred").is_some(),
            "the shell is back"
        );
    }
}

#[test]
fn the_arrows_home_end_and_the_wheel_go_from_photo_to_photo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(harness.state().viewer_holds(), Some(ids[0]));
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    // The picture of the photo left is let go of at the step, not when the next one's
    // is made: a hundred megabytes of a photo nobody is looking at.
    assert_eq!(harness.state().viewer_holds(), None);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[2]));
    // After the last there is none.
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[2]));
    key(&mut driver, &mut harness, Key::Home);
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::End);
    assert_eq!(viewing(&harness), Some(ids[2]));
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[1]));
    // A notch of the wheel is a photo: away from the user, the one before.
    let wheel = |harness: &mut Harness<'_>, driver: &mut Driver, y: f32| {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(harness);
    };
    wheel(&mut harness, &mut driver, 1.0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    wheel(&mut harness, &mut driver, -1.0);
    assert_eq!(viewing(&harness), Some(ids[1]));
    // The buttons at the sides.
    harness.get_by_label("Next photo").click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[2]));
    harness.get_by_label("Previous photo").click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[1]));
    // What the library says of the photo stepped to arrives for that photo.
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    assert_eq!(caption(&harness), "b.jpg");
    // The selection behind the viewer is what it was, until the viewer closes on the
    // photo it came to.
    assert_eq!(harness.state().selected(), [ids[0]]);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(harness.state().selected(), [ids[1]]);
}

// A held arrow key passes photos faster than they can be decoded, and a decode cannot
// be taken back: the full picture is asked for only once the viewer has rested on a
// photo - and at once for the one it opened on.
#[test]
fn the_full_picture_is_asked_for_after_a_rest_and_at_once_at_the_opening() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 1);
    click(&mut driver, &mut harness, 1);
    driver.act(&mut harness);
    assert!(
        harness.state().viewer_asked(),
        "the photo opened on does not wait"
    );
    key(&mut driver, &mut harness, Key::ArrowLeft);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[0]));
    assert!(
        !harness.state().viewer_asked(),
        "a photo just stepped to is not asked for"
    );
    // The frame the rest is over in is asked for, and asks.
    assert!(driver.frames_in(&mut harness, 0.2) >= 1);
    assert!(harness.state().viewer_asked());
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
}

// Closed on a photo far from where the grid stood: it is selected and in sight.
#[test]
fn closing_brings_the_photo_the_viewer_was_on_into_sight() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    let ids = harness.state().shown();
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::End);
    assert_eq!(viewing(&harness), Some(ids[899]));
    key(&mut driver, &mut harness, Key::Escape);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().selected(), [ids[899]]);
    let frame = harness.state().last_frame().unwrap();
    assert!(
        frame.on_screen.contains(&ids[899]) && !frame.on_screen.contains(&ids[0]),
        "the grid is where the photo is"
    );
}

// A photo whose file cannot be read - its drive is not plugged in - is looked at from
// photon's own cache: the preview stays, and nothing is said over it.
#[test]
fn a_photo_whose_file_is_not_there_is_shown_from_its_preview() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_failed().is_some()
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.state_mut().viewer_preview());
    assert_eq!(harness.state().viewer_levels(), None);
    assert!(harness.query_by_label_contains("can't be read").is_none());
    // And it is a still window, not one asking again and again.
    assert_eq!(harness.state().pictures_asked(), 1);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

/// The lines of the folder's `.picasa.ini`.
fn ini(folder: &Path) -> String {
    std::fs::read_to_string(folder.join(".picasa.ini")).unwrap_or_default()
}

// The star is the photo's on screen, written to its folder's INI as the grid's is, and
// the viewer shows it once the library has said so.
#[test]
fn a_star_is_written_from_the_viewer_by_the_key_and_by_the_button() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    // Another photo is selected in the grid behind: the star is not its.
    open(&mut driver, &mut harness, 1);
    assert!(harness.query_by_label("Star").is_some());
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "the star", |app| {
        app.viewer_item().is_some_and(|item| item.starred)
    });
    assert!(
        ini(&folder).contains("[b.jpg]\nstar=yes") || ini(&folder).contains("[b.jpg]\r\nstar=yes")
    );
    assert!(!ini(&folder).contains("[a.jpg]"));
    driver.act(&mut harness);
    harness.get_by_label("Unstar").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the star taken off", |app| {
        app.viewer_item().is_some_and(|item| !item.starred)
    });
    assert!(!ini(&folder).contains("star=yes"));
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert_eq!(said(&harness), Vec::<String>::new());
}

// Hidden, the photo leaves the view and stays on screen - the user is looking at it -
// with no place in its caption; `H` again brings it back; and from a hidden photo the
// next step goes on from where it stood.
#[test]
fn a_photo_hidden_in_the_viewer_stays_and_the_next_step_goes_on_from_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 1);
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert_eq!(harness.state().photos(), 2);
    driver.act(&mut harness);
    let line = caption_line(&harness);
    assert!(
        line.starts_with("b.jpg · ") && !line.contains(" / "),
        "{line}"
    );
    // Again: it is a photo of the view again.
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it is back", |app| {
        app.photos() == 3
            && app.viewer().is_some_and(|viewer| !viewer.orphaned())
            // What the library says of it is a round trip behind the grid, and is what
            // the key goes by.
            && app.viewer_item().is_some_and(|item| !item.hidden)
    });
    // Hidden once more, and on: the photo that came after it.
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[2]));
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[0]));
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(harness.state().selected(), [ids[0]]);
}

// A selection of several, the viewer opened on the photo the keys are on and closed on
// it: the selection is still there. Stepped to another photo and closed there, it is that
// photo alone.
#[test]
fn closing_on_the_photo_it_was_opened_on_keeps_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 0);
    driver.frames_in(&mut harness, 1.0);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::End);
    driver.act(&mut harness);
    assert_eq!(harness.state().selected().len(), 3);
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(viewing(&harness), Some(ids[2]));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().selected(), ids);
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::ArrowLeft);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(harness.state().selected(), [ids[1]]);
}

// Closed on a photo the view no longer holds, the viewer hands the grid no photo: the
// selection must not hold one no tile shows, and the photo that stands where it stood
// is one nobody asked about. What was selected before stays.
#[test]
fn closing_on_a_photo_the_view_has_lost_selects_nothing_in_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().selected(), [ids[0]]);
}

// Every photo of the view hidden from the viewer, one after another: the last stays on
// screen over a grid of none, and closing lands nowhere and selects nothing.
#[test]
fn closing_over_a_grid_of_none_selects_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    for left in [2, 1, 0] {
        driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
        key(&mut driver, &mut harness, Key::H);
        driver.until(&mut harness, "it has left the view", |app| {
            app.photos() == left && app.viewer().is_some_and(|viewer| viewer.orphaned())
        });
        key(&mut driver, &mut harness, Key::ArrowRight);
    }
    assert!(
        viewing(&harness).is_some(),
        "the last stays, with nowhere to go"
    );
    key(&mut driver, &mut harness, Key::Escape);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().selected(), Vec::<i64>::new());
}

// A full picture asked for and not yet made when the viewer steps on is another
// photo's by the time it arrives: it is not kept, not for a moment - it is a hundred
// megabytes of a photo nobody is looking at.
#[test]
fn a_picture_made_for_a_photo_the_viewer_has_left_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    // Held at its render, so that the step surely comes first.
    let rendering = photon_engine::engine::RENDERING.lock();
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.act(&mut harness);
    assert!(harness.state().viewer_asked());
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    drop(rendering);
    // The first photo's picture arrives while the viewer rests on the second, whose own
    // is not asked for yet: the clock stands while the other threads are waited for.
    driver.settle(&mut harness);
    assert!(!harness.state().viewer_asked(), "the rest is not over");
    assert_eq!(harness.state().viewer_holds(), None);
    driver.until(&mut harness, "the picture of the photo stepped to", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(harness.state().viewer_holds(), Some(ids[1]));
}

// Turned from elsewhere while it is on screen: the new grid says it is another picture
// before that picture is made, and the one held, of the photo as it was, is not drawn
// under what is now said of it.
#[test]
fn a_picture_of_the_photo_as_it_was_is_not_drawn_once_it_is_another() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    let was = harness.state().viewer().unwrap().key();
    // Held, so that the new picture is not made before anything is looked at.
    let rendering = photon_engine::engine::RENDERING.lock();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the grid with the photo turned", |app| {
        app.viewer().is_some_and(|viewer| viewer.key() != was)
    });
    assert_eq!(harness.state().viewer_levels(), None);
    // And once the library has said so too, it is let go of.
    driver.until(&mut harness, "what the library says of it", |app| {
        let viewer = app.viewer().unwrap();
        viewer
            .shown()
            .is_some_and(|shown| shown.key == viewer.key())
    });
    assert_eq!(harness.state().viewer_holds(), None);
    drop(rendering);
    driver.until(&mut harness, "the picture of the photo turned", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(
        harness.state().viewer_levels(),
        Some(vec![60, 30, 15, 7, 3])
    );
}

// A photo whose file goes while it is on screen: the scan that misses it marks it, the
// engine then refuses it, and the viewer says so in its place.
#[test]
fn a_photo_that_leaves_the_library_under_the_viewer_is_said_to_be_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    open(&mut driver, &mut harness, 1);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    std::fs::remove_file(folder.join("b.jpg")).unwrap();
    let engine = harness.state().engine().clone();
    let watched = harness.state().watched()[0].id;
    commands::rescan_folder(&engine, watched).unwrap();
    driver.until(&mut harness, "it is gone", |app| {
        app.viewer().is_some_and(|viewer| viewer.gone())
    });
    driver.act(&mut harness);
    assert!(
        harness
            .query_by_label("This photo is no longer available.")
            .is_some()
    );
    // It has no menu: there is nothing of it for one to be of, and "Hide photo" on a
    // photo that is gone would be a write to a row that is not there.
    let on_photo = harness.ctx.content_rect().center() - egui::vec2(200.0, 150.0);
    harness.event(egui::Event::PointerMoved(on_photo));
    harness.event(button(on_photo, PointerButton::Secondary, true));
    harness.event(button(on_photo, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    assert!(harness.query_by_label("Hide photo").is_none());
    // Its picture is not held for a photo that is not there, and its star is in its
    // place and takes no press: nothing is known of it.
    assert_eq!(harness.state().viewer_holds(), None);
    assert!(harness.get_by_label("Star").accesskit_node().is_disabled());
    // The star takes no press of a photo that is gone.
    key(&mut driver, &mut harness, Key::Period);
    driver.settle(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
    // On from where it stood, and out.
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[2]));
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(harness.state().selected(), [ids[2]]);
}

// The Enter that opened the viewer is still down when the viewer is closed by a key of
// its own: held on, it opens nothing again.
#[test]
fn the_enter_that_opened_the_viewer_does_not_open_it_again_when_held() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 1);
    driver.frames_in(&mut harness, 1.0);
    harness.key_down(Key::Enter);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[1]));
    harness.key_down(Key::Enter);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
    for _ in 0..3 {
        harness.key_down(Key::Enter);
        driver.act(&mut harness);
        assert_eq!(viewing(&harness), None);
    }
}

// A photo shown is a still window: its preview and its full picture arrive, and then
// nothing asks for a frame.
#[test]
fn a_window_with_a_photo_shown_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

/// Lets everything that is due in the next `secs` happen, and the other threads finish.
fn rest(driver: &mut Driver, harness: &mut Harness<'_>, secs: f64) {
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
}

/// The bytes of `path` and when it was last written.
fn kept(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (
        std::fs::read(path).unwrap(),
        std::fs::metadata(path).unwrap().modified().unwrap(),
    )
}

/// Writes `bytes` to `path`, as last written at `modified`: to a scan it is the file it
/// was, if it is as long.
fn put(path: &Path, bytes: &[u8], modified: std::time::SystemTime) {
    std::fs::write(path, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

// One photo on and straight back - two taps inside the rest, an overshoot put right. The
// picture of the photo come back to was let go of at the step, so it is asked for again:
// decided from what was once asked, it never was, and the photo stood at its preview.
#[test]
fn a_photo_stepped_from_and_straight_back_to_has_its_full_picture_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    driver.settle(&mut harness);
    // Two frames a sixtieth of a second apart: well inside the rest.
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[0]));
    driver.until(&mut harness, "the full picture again", |app| {
        app.viewer_levels().is_some()
    });
    // Once: at the opening, and when it was come back to.
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().pictures_asked(), 2);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A photo whose file went - "no longer available" - and came back as it was: it is in
// the grid again, the library answers for it again, and its picture is made again. Not
// while it is gone: asked for then, the engine's refusal would be what is remembered of
// it when it is back.
#[test]
fn a_photo_that_was_gone_and_is_back_has_its_full_picture_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    let path = folder.join("a.jpg");
    let (bytes, modified) = kept(&path);
    std::fs::remove_file(&path).unwrap();
    let engine = harness.state().engine().clone();
    let watched = harness.state().watched()[0].id;
    commands::rescan_folder(&engine, watched).unwrap();
    driver.until(&mut harness, "it is gone", |app| {
        app.viewer().is_some_and(|viewer| viewer.gone())
    });
    rest(&mut driver, &mut harness, 1.0);
    put(&path, &bytes, modified);
    commands::rescan_folder(&engine, watched).unwrap();
    driver.until(&mut harness, "it is back, with its picture", |app| {
        app.photos() == 3
            && app.viewer().is_some_and(|viewer| !viewer.gone())
            && app.viewer_levels().is_some()
    });
    assert_eq!(viewing(&harness), Some(ids[0]));
}

// The picture worker is held at another photo's render, so the question about this
// photo waits behind it; the photo is turned from elsewhere and turned back with no
// frame drawn between - the window is never told it was another picture - and the
// worker, let go, renders it turned. That picture is not the one asked for and is not
// kept (it would be drawn as the photo upright); and since the photo is under the key
// it had, no grid will say it changed: what the library says of it is read again, and
// the picture asked for after that.
#[test]
fn a_picture_made_from_a_state_the_window_never_saw_is_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    let rendering = RENDERING.lock();
    // Opened on `b.jpg`: its picture is asked for at once, and waits at its render.
    open(&mut driver, &mut harness, 1);
    assert!(harness.state().viewer_asked());
    // On to `a.jpg`, the strip, and rested there: its picture is asked for, behind.
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[0]));
    driver.frames_in(&mut harness, 0.5);
    assert!(harness.state().viewer_asked());
    let was = harness.state().viewer().unwrap().key();
    // Turned from elsewhere. No frame is drawn: the window does not hear of it.
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    drop(rendering);
    // The worker finishes `b.jpg`, which nobody wants, and renders `a.jpg` as it is now.
    driver.quiet();
    // And turned back, to the key it had.
    commands::rotate_item(&engine, ids[0], false).unwrap();
    driver.until(&mut harness, "the picture of the photo as it is", |app| {
        app.viewer_levels().is_some()
    });
    assert_eq!(viewing(&harness), Some(ids[0]));
    assert_eq!(harness.state().viewer().unwrap().key(), was);
    assert_eq!(
        harness.state().viewer_levels(),
        Some(vec![4200, 2100, 1050, 525, 262]),
        "not the photo turned"
    );
}

// The same, with the photo left as another picture, and no grid to say so: turned in
// the library alone. The stale answer has the library read again, and the picture is
// asked for once more, under the key the library has - not again and again under the
// key the window had, each a whole decode the engine would answer the same way.
#[test]
fn a_stale_picture_is_asked_for_again_under_the_key_the_library_has() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    let rendering = RENDERING.lock();
    open(&mut driver, &mut harness, 1);
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[0]));
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    driver.frames_in(&mut harness, 0.5);
    assert!(harness.state().viewer_asked());
    let was = harness.state().viewer().unwrap().key();
    let engine = harness.state().engine().clone();
    let turned = Edit {
        turns: 1,
        crop: None,
    };
    assert!(engine.lib.set_item_edit(ids[0], turned).unwrap());
    drop(rendering);
    driver.until(&mut harness, "the picture of the photo turned", |app| {
        app.viewer_levels().is_some()
    });
    assert_ne!(harness.state().viewer().unwrap().key(), was);
    assert_eq!(
        harness.state().viewer_levels(),
        Some(vec![60, 30, 15, 7, 3])
    );
    rest(&mut driver, &mut harness, 1.0);
    // `b.jpg` at the opening, `a.jpg` after the rest, and `a.jpg` as the library has it.
    assert_eq!(harness.state().pictures_asked(), 3);
}

// The viewer takes the whole window. Opened by Enter over a press the grid still held,
// the button came up where the grid did not hear it, and with the viewer closed the
// first move of the mouse drew a rubber band with no button down. So Enter opens
// nothing while a button is down on the photos.
#[test]
fn enter_opens_nothing_while_a_button_is_down_on_the_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    click(&mut driver, &mut harness, 0);
    driver.frames_in(&mut harness, 1.0);
    let at = tile(&harness, 0);
    harness.event(button(at, PointerButton::Primary, true));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(viewing(&harness), None, "not over a press that is held");
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().selected(), [ids[0]]);
    // The button up, Enter is the grid's again.
    driver.frames_in(&mut harness, 1.0);
    key(&mut driver, &mut harness, Key::Enter);
    assert_eq!(viewing(&harness), Some(ids[0]));
}

// The other button on a photo and Enter in one frame: the menu is asked for, and the
// viewer does not open under it - where the menu had every key and Escape closed the
// menu, not the viewer.
#[test]
fn enter_opens_nothing_in_the_frame_a_menu_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    click(&mut driver, &mut harness, 0);
    driver.frames_in(&mut harness, 1.0);
    let at = tile(&harness, 0);
    harness.input_mut().events.extend([
        button(at, PointerButton::Secondary, true),
        egui::Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(viewing(&harness), None);
}

// What the wheel has turned towards the next photo is the viewer's it was turned in: a
// trackpad's half notch left over in one does not step at the first touch in the next.
#[test]
fn a_viewer_opened_anew_begins_with_the_wheel_at_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    let wheel = |harness: &mut Harness<'_>, driver: &mut Driver, points: f32| {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, points),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(harness);
    };
    open(&mut driver, &mut harness, 0);
    wheel(&mut harness, &mut driver, -60.0);
    assert_eq!(viewing(&harness), Some(ids[0]), "sixty points are no notch");
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
    driver.frames_in(&mut harness, 5.0);
    open(&mut driver, &mut harness, 0);
    wheel(&mut harness, &mut driver, -60.0);
    assert_eq!(viewing(&harness), Some(ids[0]));
    // And sixty more in this one are a notch.
    wheel(&mut harness, &mut driver, -60.0);
    assert_eq!(viewing(&harness), Some(ids[1]));
}

/// Bytes of texture egui holds.
fn texture_bytes(harness: &Harness<'_>) -> usize {
    let manager = harness.ctx.tex_manager();
    let manager = manager.read();
    manager.allocated().map(|(_, meta)| meta.bytes_used()).sum()
}

// The viewer closed: its full picture goes with it, and so do its previews, which
// nothing else lets go of but their store's own limit.
#[test]
fn a_viewer_closed_holds_no_texture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    // Once before anything is measured: the viewer's icons are textures too, made the
    // first time they are drawn and kept.
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    let before = texture_bytes(&harness);
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 1.0);
    let open_bytes = texture_bytes(&harness);
    assert!(open_bytes > before);
    assert!(harness.state().preview_bytes() > 0);
    key(&mut driver, &mut harness, Key::Escape);
    assert_eq!(viewing(&harness), None);
    // Its previews among it, which the first opening above left none of either.
    assert_eq!(harness.state().preview_bytes(), 0);
    rest(&mut driver, &mut harness, 5.0);
    assert_eq!(
        texture_bytes(&harness),
        before,
        "held after it closed (open: {open_bytes})"
    );
}

// A message lies over the viewer as over the grid, and its button takes the keyboard at
// Tab where no control of the viewer's does: with the keyboard on it the arrows walked
// it on instead of stepping, and the grid's keys were dead once the viewer had closed.
// Tab is nobody's in the viewer.
#[test]
fn a_message_is_drawn_over_the_viewer_and_tab_puts_the_keyboard_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    // A fixture's folders are on a drive that is not there: a star is refused, and said.
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let ids = harness.state().shown();
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "the refusal is said", |app| {
        !app.toasts().is_empty()
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        harness.query_by_label("Dismiss").is_some(),
        "drawn here too"
    );
    key(&mut driver, &mut harness, Key::Tab);
    assert_eq!(harness.ctx.memory(|memory| memory.focused()), None);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    key(&mut driver, &mut harness, Key::Backspace);
    assert_eq!(viewing(&harness), None);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(harness.state().selected(), [ids[2]]);
}

// Closing hands the grid a place at the end of a frame, and the next frame may draw
// another grid: a scan's, with three hundred photos more above. The place is the photo,
// looked up in the grid that is drawn, not the number it had in the one before.
#[test]
fn the_photo_handed_back_is_in_sight_when_a_grid_arrives_as_the_viewer_closes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    // The first three hundred are put away, to come back later.
    commands::set_items_hidden(&engine, &all[..300], true).unwrap();
    driver.until(&mut harness, "six hundred photos", |app| {
        app.photos() == 600 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let last = all[899];
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::End);
    assert_eq!(viewing(&harness), Some(last));
    rest(&mut driver, &mut harness, 1.0);
    // Escape: one event, one frame - the viewer closes in it.
    harness.key_down(Key::Escape);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None);
    // Before the next frame, a grid with three hundred photos more above.
    commands::set_items_hidden(&engine, &all[..300], false).unwrap();
    driver.quiet();
    harness.key_up(Key::Escape);
    driver.act(&mut harness);
    driver.until(&mut harness, "nine hundred photos", |app| {
        app.photos() == 900 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().selected(), [last]);
    let frame = harness.state().last_frame().unwrap();
    assert!(
        frame.on_screen.contains(&last),
        "the photo the viewer was on is selected and not in sight"
    );
}

// Closed on a photo the view has lost, the grid is brought to where it stood - the
// viewer may have come a long way from where the grid was left - and nothing is selected
// there.
#[test]
fn closing_on_a_photo_the_view_has_lost_shows_where_it_stood() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::End);
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "it has left the view", |app| {
        app.viewer().is_some_and(|viewer| viewer.orphaned())
    });
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().selected(), [all[0]]);
    let frame = harness.state().last_frame().unwrap();
    assert!(
        frame.on_screen.contains(&all[898]) && !frame.on_screen.contains(&all[0]),
        "the grid is where the photo stood"
    );
}

/// Types `query` into the search box, and before the typing has stopped for long enough
/// to send it, opens the photo at `offset` of the grid still on screen.
fn search_then_open(driver: &mut Driver, harness: &mut Harness<'_>, query: &str, offset: usize) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(harness);
    harness.event(egui::Event::Text(query.to_owned()));
    driver.act(harness);
    // A click on a photo gives the keys back to the grid, a frame later.
    click(driver, harness, offset);
    driver.act(harness);
    key(driver, harness, Key::Enter);
}

// A search typed and, before it is sent, a photo opened from the grid still on screen:
// other results land under the viewer. Where they do not hold its photo there is no
// list it stands in - "where it stood" is a number of another list, and the step back
// from the ninth of all photos went to the third of three - so the viewer closes onto
// the results that were asked for.
#[test]
fn other_results_that_do_not_hold_the_photo_close_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    search_then_open(&mut driver, &mut harness, "no photo is called this", 8);
    assert_eq!(viewing(&harness), Some(all[8]), "the viewer opened");
    // The search is sent when its time comes, viewer or no viewer: the frame it is due
    // in is asked for, and nothing here waits for another.
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().photos(), 0, "the results of the search");
    assert_eq!(viewing(&harness), None);
    assert_eq!(harness.state().selected(), Vec::<i64>::new());
}

// And where the other results hold the photo too, the viewer stays on it, at the place
// it has in them.
#[test]
fn other_results_that_hold_the_photo_too_leave_the_viewer_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    let name = commands::viewer_item(&engine, all[8]).unwrap().file_name;
    search_then_open(&mut driver, &mut harness, &name, 8);
    assert_eq!(viewing(&harness), Some(all[8]), "the viewer opened");
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().photos(), 1, "the results of the search");
    let viewer = harness.state().viewer().expect("still open").clone();
    assert_eq!((viewer.id(), viewer.offset()), (all[8], 0));
    assert!(!viewer.orphaned() && !viewer.gone());
    driver.act(&mut harness);
    assert!(caption_line(&harness).ends_with("(1 / 1)"));
}

// Why a full picture could not be made is said where there is no picture at all: no
// preview either. Of the photo it is of, and of no other - the next photo, which has no
// preview either and whose picture has not been answered for yet, says nothing.
#[test]
fn why_a_picture_cannot_be_made_is_said_where_there_is_no_preview_either() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let engine = harness.state().engine().clone();
    // photon's own cache, in this test's directory: the previews of the first two.
    let entries = engine.lib.grid_entries().unwrap();
    for entry in &entries[..2] {
        std::fs::remove_file(engine.thumbs.path_for(entry.thumb_key, ThumbSize::Preview)).unwrap();
    }
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "why", |app| app.viewer_failed().is_some());
    driver.act(&mut harness);
    let why = harness.state().viewer_failed().unwrap().to_owned();
    assert!(why.contains("can't be read"), "{why}");
    assert!(!harness.state_mut().viewer_preview());
    assert!(harness.query_by_label(&why).is_some());
    // On, with the next photo's picture held at its render.
    let rendering = RENDERING.lock();
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.frames_in(&mut harness, 0.5);
    assert!(harness.state().viewer_asked());
    driver.act(&mut harness);
    assert!(harness.query_by_label(&why).is_none());
    drop(rendering);
    driver.until(&mut harness, "why, of this one", |app| {
        app.viewer_failed().is_some()
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label(&why).is_some());
    // Nor is it said of this photo once it is another picture - turned from elsewhere -
    // whose own has not been answered for.
    let rendering = RENDERING.lock();
    let was = harness.state().viewer().unwrap().key();
    let id = viewing(&harness).unwrap();
    commands::rotate_item(&engine, id, true).unwrap();
    driver.until(
        &mut harness,
        "the photo turned, and its picture asked for",
        |app| app.viewer().is_some_and(|viewer| viewer.key() != was) && app.viewer_asked(),
    );
    driver.act(&mut harness);
    assert!(harness.query_by_label(&why).is_none());
    drop(rendering);
}

// A full picture that could not be made is not asked for again and again - and is asked
// for again when the photo is come back to: its file may be readable by then.
#[test]
fn a_picture_that_could_not_be_made_is_asked_for_again_when_the_photo_is_come_back_to() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    // The file is made unreadable as a picture, and to a scan is the file it was: as
    // long, and written when it was.
    let path = folder.join("b.jpg");
    let (bytes, modified) = kept(&path);
    put(&path, &vec![0; bytes.len()], modified);
    open(&mut driver, &mut harness, 1);
    driver.until(&mut harness, "its picture cannot be made", |app| {
        app.viewer_failed().is_some()
    });
    // Looked at from its preview, with nothing said, and the window is still.
    rest(&mut driver, &mut harness, 1.0);
    assert!(harness.state_mut().viewer_preview());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
    put(&path, &bytes, modified);
    key(&mut driver, &mut harness, Key::ArrowRight);
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[1]));
    driver.until(&mut harness, "its picture", |app| {
        app.viewer_levels().is_some()
    });
}

// A photo the library says cannot be shown - its thumbnail could not be made - has that
// said in its place, in the library's words. Its picture is asked for at the opening,
// before the library has said anything, and may well be made: it is not held to be
// drawn, and not asked for again.
//
// "Before the library has said anything" is a race the window does not decide: the picture
// is asked for in the frame after the opening, and the library's answer, read on another
// thread, can be there by then - Windows' CI once had it so, asked for nothing, and the
// rule under test was never reached. There is no seam to hold that read back, so the
// opening is made again until the ask came first: the rule is tested on the attempt that
// reaches it.
#[test]
fn a_photo_that_cannot_be_shown_says_so_and_holds_no_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    let engine = harness.state().engine().clone();
    (engine.lib)
        .set_thumb_state(ids[1], ThumbState::Failed, Some("It is broken."))
        .unwrap();
    let rendering = RENDERING.lock();
    let mut attempts = 0;
    loop {
        attempts += 1;
        open(&mut driver, &mut harness, 1);
        if harness.state().viewer_asked() {
            break;
        }
        assert!(
            attempts < 5,
            "the library answered before the ask, five times"
        );
        key(&mut driver, &mut harness, Key::Escape);
        assert!(harness.state().viewer().is_none());
    }
    driver.act(&mut harness);
    assert!(harness.query_by_label("It is broken.").is_some());
    drop(rendering);
    // Until the picture has been answered, however long that takes: the render waits
    // its turn behind every other test of this file that holds one, and a second of
    // waiting was not always enough on Windows' CI.
    driver.until(&mut harness, "the picture answered", |app| {
        !app.viewer_asked()
    });
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(harness.state().viewer_holds(), None);
    assert!(!harness.state().viewer_asked());
    assert_eq!(harness.state().pictures_asked(), 1);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
    assert!(harness.query_by_label("It is broken.").is_some());
}

// The previews of the two photos on either side are warmed while the viewer is on one,
// so that a step lands on a picture.
#[test]
fn the_neighbours_previews_are_warmed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    assert!(!harness.state().preview_of(ids[1]));
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the next two photos' previews", |app| {
        app.preview_of(ids[1]) && app.preview_of(ids[2])
    });
}

// A folder held over the window is held over the viewer too, and the card says so.
#[test]
fn the_card_of_a_folder_held_over_the_window_lies_over_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    assert!(harness.query_by_label(CARD).is_none());
    hold_over(&mut driver, &mut harness, &[dir.path()]);
    driver.act(&mut harness);
    assert!(harness.query_by_label(CARD).is_some());
}

// The buttons at the sides lead nowhere at the ends, and say so.
#[test]
fn the_buttons_at_the_sides_take_no_press_at_the_ends() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    let disabled = |harness: &Harness<'_>, label: &str| {
        harness.get_by_label(label).accesskit_node().is_disabled()
    };
    open(&mut driver, &mut harness, 0);
    driver.act(&mut harness);
    assert!(disabled(&harness, "Previous photo") && !disabled(&harness, "Next photo"));
    key(&mut driver, &mut harness, Key::End);
    driver.act(&mut harness);
    assert!(!disabled(&harness, "Previous photo") && disabled(&harness, "Next photo"));
}

// A video is shown from its poster, where it has one, under a line that says the system
// plays no video - naming what to install, on Linux. (The test's system plays nothing:
// `common::no_videos`.)
#[test]
fn a_video_on_a_system_that_plays_none_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    // No video at all, but a file called one: which is all the library goes by.
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    put(
        &folder.join("d.mp4"),
        b"not a video",
        start + Duration::from_secs(600),
    );
    let (mut harness, mut driver, _) = over(dir.path(), Some(&folder));
    driver.until(&mut harness, "the four shown", |app| {
        app.photos() == 4 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let ids = harness.state().shown();
    let engine = harness.state().engine().clone();
    let video = (ids.iter().copied())
        .position(|id| commands::viewer_item(&engine, id).unwrap().file_name == "d.mp4")
        .expect("the video is in the grid");
    open(&mut driver, &mut harness, video);
    driver.act(&mut harness);
    let line = if cfg!(target_os = "linux") {
        photon_ui::playback::NO_GSTREAMER
    } else {
        photon_ui::playback::CANNOT_PLAY_HERE
    };
    assert!(
        harness.query_by_label(line).is_some(),
        "{:?}",
        said(&harness)
    );
    // And it is not zoomed, and has no slider.
    key(&mut driver, &mut harness, Key::Plus);
    assert!(harness.state().viewer().unwrap().zoom().is_fit());
    assert!(harness.query_by_label("Zoom").is_none());
}

/// How far the photo in the viewer is zoomed.
fn zoom(harness: &Harness<'_>) -> f32 {
    harness.state().viewer().expect("the viewer").zoom().zoom()
}

/// A click of the first button at `at`.
fn click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

// `+` zooms the photo on screen and `0` fits it again; the next photo is fitted, and so
// is this one when it is come back to: what was looked at closely is one photo's.
#[test]
fn the_keys_zoom_a_photo_and_a_step_puts_the_next_one_back_to_the_fit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    assert_eq!(zoom(&harness), 1.0);
    key(&mut driver, &mut harness, Key::Plus);
    assert_eq!(zoom(&harness), 1.25);
    key(&mut driver, &mut harness, Key::Equals);
    key(&mut driver, &mut harness, Key::Minus);
    assert_eq!(zoom(&harness), 1.25);
    key(&mut driver, &mut harness, Key::Num0);
    assert_eq!(zoom(&harness), 1.0);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert_eq!(zoom(&harness), 1.0);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::ArrowLeft);
    assert_eq!(viewing(&harness), Some(ids[0]));
    assert_eq!(zoom(&harness), 1.0);
    // And the slider says what the keys have done.
    key(&mut driver, &mut harness, Key::Plus);
    driver.act(&mut harness);
    let slider = harness.get_by_label("Zoom").accesskit_node();
    assert_eq!(slider.numeric_value(), Some(1.25));
}

// A star is another fact about the same picture, and leaves the zoom where it is; the
// photo turned from elsewhere is another picture, fitted again - the place that was
// zoomed into is not there any more.
#[test]
fn a_star_keeps_the_zoom_and_another_picture_is_fitted_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 1);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "the star", |app| {
        app.viewer_item().is_some_and(|item| item.starred)
    });
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(zoom(&harness), 1.25);
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[1], true).unwrap();
    // The turned picture and what the library says of the turned photo are two
    // answers, from two tasks, in either order: the zoom is put back by the second,
    // which is nearly always the first to arrive. (Asked the moment the picture had
    // landed, the zoom was still 1.25 in one run in six.)
    driver.until(&mut harness, "the photo turned, and fitted again", |app| {
        app.viewer_levels() == Some(vec![48])
            && app.viewer().is_some_and(|viewer| viewer.zoom().is_fit())
    });
    assert_eq!(zoom(&harness), 1.0);
}

// The double click that opened the viewer is the grid's: a click in the viewer right
// after it is the first of a double click there, not the second. Two in the viewer go to
// the photo's own pixels - as far as the zoom goes, for a strip 4,200 pixels wide in a
// window of a thousand - and two more back to the whole photo.
#[test]
fn a_double_click_zooms_in_the_viewer_and_the_one_that_opened_it_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    let at = tile(&harness, 0);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    assert!(harness.state().viewer().is_some());
    click_at(&mut driver, &mut harness, at);
    assert_eq!(zoom(&harness), 1.0, "the third click of three");
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 1.0);
    let middle = egui::pos2(500.0, 350.0);
    click_at(&mut driver, &mut harness, middle);
    assert_eq!(zoom(&harness), 1.0);
    click_at(&mut driver, &mut harness, middle);
    assert_eq!(zoom(&harness), 4.0);
    driver.frames_in(&mut harness, 1.0);
    click_at(&mut driver, &mut harness, middle);
    click_at(&mut driver, &mut harness, middle);
    assert_eq!(zoom(&harness), 1.0);
}

// The slider is pressed where the zoom is wanted, and a drag on the photo moves it
// then; and a photo zoomed and moved is a still window.
#[test]
fn the_slider_zooms_a_drag_pans_and_the_window_is_still() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    driver.act(&mut harness);
    // The middle of the slider is the middle of its range.
    let slider = harness.get_by_label("Zoom").rect().center();
    harness.event(egui::Event::PointerMoved(slider));
    harness.event(button(slider, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.event(button(slider, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert_eq!(zoom(&harness), 2.5);
    let from = egui::pos2(500.0, 350.0);
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerMoved(from + egui::vec2(-120.0, 0.0)));
    driver.act(&mut harness);
    harness.event(button(from, PointerButton::Primary, false));
    driver.act(&mut harness);
    let pan = harness.state().viewer().unwrap().zoom().pan();
    assert_eq!((pan.x, pan.y), (-120.0, 0.0));
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A message lies over the viewer, and a press on its button is the button's: no pan
// begins under it, where the photo's surface reaches under everything.
#[test]
fn a_press_on_a_message_over_the_viewer_moves_no_photo() {
    let dir = tempfile::tempdir().unwrap();
    // A fixture's folders are on a drive that is not there: a star is refused, and said.
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the preview", |app| app.preview_bytes() > 0);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "the refusal is said", |app| {
        !app.toasts().is_empty()
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    let at = harness.get_by_label("Dismiss").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerMoved(at + egui::vec2(-60.0, -40.0)));
    driver.act(&mut harness);
    let pan = harness.state().viewer().unwrap().zoom().pan();
    assert_eq!((pan.x, pan.y), (0.0, 0.0));
    // The same press beside the message is the photo's.
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(&mut harness);
    let beside = at + egui::vec2(0.0, -200.0);
    harness.event(egui::Event::PointerMoved(beside));
    harness.event(button(beside, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerMoved(beside + egui::vec2(-60.0, -40.0)));
    driver.act(&mut harness);
    let pan = harness.state().viewer().unwrap().zoom().pan();
    assert_eq!((pan.x, pan.y), (-60.0, -40.0));
}

// And the other button on a message asks for no menu of the photo under it; beside the
// message it does.
#[test]
fn the_other_button_on_a_message_over_the_viewer_asks_for_no_menu() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::Period);
    driver.until(&mut harness, "the refusal is said", |app| {
        !app.toasts().is_empty()
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    let at = harness.get_by_label("Dismiss").rect().center();
    let right_click = |driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2| {
        harness.event(egui::Event::PointerMoved(at));
        harness.event(button(at, PointerButton::Secondary, true));
        harness.event(button(at, PointerButton::Secondary, false));
        driver.act(harness);
        driver.act(harness);
    };
    right_click(&mut driver, &mut harness, at);
    assert!(!harness.state().menu_open());
    right_click(&mut driver, &mut harness, at + egui::vec2(0.0, -200.0));
    assert!(harness.state().menu_open());
}

/// One notch of the wheel with Ctrl held: away from the user above nought.
fn zoom_wheel(driver: &mut Driver, harness: &mut Harness<'_>, notches: f32) {
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, notches),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::CTRL,
    });
    driver.act(harness);
}

// As many notches of the wheel in as out leave the photo fitted, and a double click
// after them zooms in: a hair over the fit, which is what single precision leaves, was
// "zoomed", and the double click went "back" to where the photo already was.
#[test]
fn the_wheel_turned_in_and_back_out_leaves_the_photo_fitted() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 1.0);
    let middle = egui::pos2(500.0, 350.0);
    harness.event(egui::Event::PointerMoved(middle));
    driver.act(&mut harness);
    for _ in 0..3 {
        zoom_wheel(&mut driver, &mut harness, 1.0);
    }
    assert!(zoom(&harness) > 1.8);
    for _ in 0..3 {
        zoom_wheel(&mut driver, &mut harness, -1.0);
    }
    assert!(harness.state().viewer().unwrap().zoom().is_fit());
    rest(&mut driver, &mut harness, 1.0);
    click_at(&mut driver, &mut harness, middle);
    click_at(&mut driver, &mut harness, middle);
    assert_eq!(zoom(&harness), 4.0);
}

// A double click goes to the photo's own pixels whether or not its full picture has
// landed: the library has said how large the photo is by then, and a moment after a
// step is not another answer than a second later.
#[test]
fn a_double_click_goes_to_the_photos_own_pixels_before_the_full_picture_is_made() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    let rendering = RENDERING.lock();
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the preview", |app| app.preview_bytes() > 0);
    driver.frames_in(&mut harness, 1.0);
    let middle = egui::pos2(500.0, 350.0);
    click_at(&mut driver, &mut harness, middle);
    click_at(&mut driver, &mut harness, middle);
    // 4,200 pixels in a window of a thousand: as far as the zoom goes.
    assert_eq!(zoom(&harness), 4.0);
    drop(rendering);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(zoom(&harness), 4.0);
}

// A photo the camera stored on its side is measured as it is seen: 1,200 by 900 in the
// file, turned by its orientation, is 900 pixels wide, and in a window 700 points high
// it is fitted 525 points wide.
#[test]
fn a_photo_stored_on_its_side_is_zoomed_by_its_size_as_it_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let picture =
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1200, 900, image::Rgb([9; 3])));
    let mut jpeg = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .unwrap();
    // An EXIF block with one entry, straight after the marker that begins the file:
    // orientation 6, "turn a quarter clockwise to see it upright".
    let exif: [u8; 36] = [
        0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2a, 0, 8, 0, 0, 0, 1,
        0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut bytes = jpeg[..2].to_vec();
    bytes.extend_from_slice(&exif);
    bytes.extend_from_slice(&jpeg[2..]);
    std::fs::write(folder.join("side.jpg"), bytes).unwrap();
    let (mut harness, mut driver, _) = over(dir.path(), Some(&folder));
    driver.until(&mut harness, "the photo shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    let rendering = RENDERING.lock();
    open(&mut driver, &mut harness, 0);
    let item = harness.state().viewer_item().unwrap();
    assert_eq!(
        (item.width, item.height, item.orientation),
        (1200, 900, 6),
        "as the file has it"
    );
    driver.until(&mut harness, "the preview", |app| app.preview_bytes() > 0);
    driver.frames_in(&mut harness, 1.0);
    let middle = egui::pos2(500.0, 350.0);
    click_at(&mut driver, &mut harness, middle);
    click_at(&mut driver, &mut harness, middle);
    let zoomed = zoom(&harness);
    assert!((zoomed - 900.0 / 525.0).abs() < 0.001, "{zoomed}");
    drop(rendering);
}

// A viewer closed under a held button: the button comes up over the grid, which draws no
// band for a press it never saw go down, and the viewer opened again follows no pointer.
#[test]
fn a_viewer_closed_under_a_held_button_leaves_no_press_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 1);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::Plus);
    let from = egui::pos2(500.0, 350.0);
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerMoved(from + egui::vec2(-40.0, 10.0)));
    driver.act(&mut harness);
    let pan = harness.state().viewer().unwrap().zoom().pan();
    assert_eq!((pan.x, pan.y), (-40.0, 10.0));
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().viewer().is_none());
    assert_eq!(harness.state().selected(), [ids[1]]);
    for step in 1..=5 {
        let to = from + egui::vec2(-40.0 - 60.0 * step as f32, 10.0 + 30.0 * step as f32);
        harness.event(egui::Event::PointerMoved(to));
        driver.act(&mut harness);
        assert!(
            !(harness.state().last_frame()).is_some_and(|frame| frame.banding),
            "a band for a press the grid never saw go down"
        );
    }
    harness.event(button(from, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[1]]);
    rest(&mut driver, &mut harness, 1.0);
    open(&mut driver, &mut harness, 1);
    key(&mut driver, &mut harness, Key::Plus);
    key(&mut driver, &mut harness, Key::Plus);
    harness.event(egui::Event::PointerMoved(from + egui::vec2(120.0, 80.0)));
    driver.act(&mut harness);
    let pan = harness.state().viewer().unwrap().zoom().pan();
    assert_eq!((pan.x, pan.y), (0.0, 0.0));
}

// Ctrl with `+`, `-` and `0` is the web view's own zoom of the whole page, which the
// Tauri photon has switched off; egui has the same, on unless it is told. Beside the
// viewer's keys it would scale the whole window at a slip of the hand.
#[test]
fn ctrl_with_the_zooms_keys_scales_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    for key in [Key::Plus, Key::Equals, Key::Minus, Key::Num0] {
        harness.key_press_modifiers(Modifiers::COMMAND, key);
        driver.act(&mut harness);
        driver.act(&mut harness);
        assert_eq!(harness.ctx.zoom_factor(), 1.0, "{key:?}");
    }
    open(&mut driver, &mut harness, 0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Plus);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
    assert_eq!(zoom(&harness), 1.0);
}

// `I` opens the info panel and its button in the bar closes it. The photo is fitted
// beside the panel - the copy drawn at fit is made again for the narrower area - and
// the panel stays across a step, and is closed when the viewer is opened anew.
#[test]
fn the_info_panel_opens_by_its_key_and_its_button_and_the_photo_moves_over() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_fitted() == Some([1000, 14])
    });
    assert!(!harness.state().viewer_info());
    // (What is selected is "toggled" to AccessKit.)
    let lit = |harness: &Harness<'_>| {
        let button = harness.get_by_label("Photo information");
        button.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(!lit(&harness));
    key(&mut driver, &mut harness, Key::I);
    assert!(harness.state().viewer_info());
    driver.act(&mut harness);
    assert!(lit(&harness), "its button in the bar is lit");
    // A thousand points less the panel's room of 348.
    driver.until(
        &mut harness,
        "the copy for the area beside the panel",
        |app| app.viewer_fitted() == Some([652, 9]),
    );
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().viewer_info(), "it stays across a step");
    // Once the library has said what the photo is, and the bar has its caption: the
    // button is pressed where it was last drawn.
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    driver.act(&mut harness);
    harness.get_by_label("Photo information").click();
    driver.act(&mut harness);
    assert!(!harness.state().viewer_info());
    key(&mut driver, &mut harness, Key::I);
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    open(&mut driver, &mut harness, 0);
    assert!(!harness.state().viewer_info(), "closed at every opening");
}

/// A key going down alone, in a frame of its own: `key_press` would draw a second frame
/// at the same moment for the key coming up. What it begins is timed from this frame.
fn key_down(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) -> f64 {
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    driver.act(harness);
    driver.now
}

/// The width of the photo's area in the viewer's last frame, in pixels.
fn area_width(harness: &Harness<'_>) -> u32 {
    harness.state().viewer_box().expect("the viewer drawn").0
}

// `I` slides the panel in over its time, from the press, and the photo's area shrinks with
// it in every frame; once it is in, the window is still again. A thousand points wide, the
// area is 652 beside the open panel.
#[test]
fn the_panel_slides_and_then_asks_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three_animated(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    driver.settle(&mut harness);
    assert_eq!(area_width(&harness), 1000);
    let pressed = key_down(&mut driver, &mut harness, Key::I);
    assert!(harness.state().viewer_info(), "open at the press");
    driver.frame(&mut harness, pressed + 0.1);
    let part = area_width(&harness);
    assert!(
        652 < part && part < 1000,
        "{part} at a hundred milliseconds"
    );
    driver.frame(&mut harness, pressed + 0.2);
    assert_eq!(area_width(&harness), 652);
    // The copy for the narrower area is asked for once it has stood, and lands.
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 1.0), 0);
}

// The crop tool shuts the panel at once, not by a slide: in the frame after `C` the photo's
// area is the whole window, and with the tool cancelled straight away the panel is not
// there to be seen sliding out.
#[test]
fn the_crop_tool_shuts_the_panel_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three_animated(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    key(&mut driver, &mut harness, Key::I);
    driver.frames_in(&mut harness, 0.3);
    driver.settle(&mut harness);
    assert_eq!(area_width(&harness), 652);
    let field =
        |harness: &Harness<'_>| (harness.query_by_role(egui::accesskit::Role::TextInput)).is_some();
    assert!(field(&harness), "the panel's keyword field");
    key_down(&mut driver, &mut harness, Key::C);
    assert!(!harness.state().viewer_info());
    driver.act(&mut harness);
    assert_eq!(area_width(&harness), 1000);
    key(&mut driver, &mut harness, Key::Escape);
    driver.act(&mut harness);
    assert!(
        harness.state().viewer().is_some(),
        "the tool cancelled, the viewer open"
    );
    assert_eq!(area_width(&harness), 1000);
    assert!(!field(&harness), "no panel drawn");
}

// A slideshow shuts the panel at once too: in the frame after `S` the photo has the whole
// window.
#[test]
fn a_slideshow_shuts_the_panel_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three_animated(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::I);
    driver.frames_in(&mut harness, 0.3);
    driver.settle(&mut harness);
    assert_eq!(area_width(&harness), 652);
    key_down(&mut driver, &mut harness, Key::S);
    assert!(!harness.state().viewer_info());
    driver.act(&mut harness);
    assert_eq!(area_width(&harness), 1000);
}

/// Opens the viewer on the photo at `offset` with its info panel, drawn with what the
/// library says of the photo.
fn informed(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    open(driver, harness, offset);
    key(driver, harness, Key::I);
    driver.act(harness);
}

/// A press on the panel's link, tick or cross called `label`, where it was last drawn.
fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// Puts the keyboard in the field a keyword is typed into, by a click.
fn into_field(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .click();
    driver.act(harness);
    assert!(harness.ctx.memory(|memory| memory.focused().is_some()));
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

fn tags(harness: &Harness<'_>) -> Vec<String> {
    (harness.state().viewer_item())
        .map(|item| item.tags.clone())
        .unwrap_or_default()
}

/// Runs `sql` over the library before it is launched: for what a fixture's photos do
/// not have and no call of the library's sets - a camera, a copy.
fn doctored(library: &photon_ui::fixture::Fixture, sql: &str, ids: &[i64]) {
    let conn = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    for id in ids {
        assert_eq!(conn.execute(sql, [id]).unwrap(), 1);
    }
}

/// Makes the photos `ids` byte for byte copies of one another, as the engine's pass
/// would have found them: a fixture's files are on a drive that is not there, which
/// that pass does not read.
fn copies(library: &photon_ui::fixture::Fixture, ids: &[i64]) {
    doctored(
        library,
        "UPDATE items SET content_hash = X'00112233445566778899aabbccddeeff' WHERE id = ?1",
        ids,
    );
}

fn path_of(harness: &Harness<'_>, id: i64) -> String {
    let engine = harness.state().engine().clone();
    commands::viewer_item(&engine, id).unwrap().path
}

// A keyword typed into the panel's field is the photo's in the library at Enter, and
// the cross beside one takes it off. The panel says what the library says once it has
// been written, and `h` typed into the field is a letter: it hides no photo.
#[test]
fn a_keyword_is_added_and_taken_off_in_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 3);
    assert!(harness.query_by_label("No keywords yet.").is_some());
    // photon does not look for faces in this library: the only ones there could be are
    // Picasa's.
    assert!(
        harness
            .query_by_label("No faces named in Picasa.")
            .is_some()
    );
    into_field(&mut driver, &mut harness);
    // The key comes with the letter it types, as a keyboard sends them.
    harness.key_press_modifiers(Modifiers::NONE, Key::H);
    typed(&mut driver, &mut harness, "hills");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the keyword said", |app| {
        app.viewer_item().is_some_and(|item| item.tags == ["hills"])
    });
    let stored = commands::viewer_item(&engine, all[3]).unwrap();
    assert_eq!(stored.tags, ["hills"]);
    assert!(!stored.hidden, "the letter hid nothing");
    assert_eq!(viewing(&harness), Some(all[3]));
    // The field still has the keyboard: the next keyword is typed straight on.
    typed(&mut driver, &mut harness, "alps");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "both said", |app| {
        app.viewer_item().is_some_and(|item| item.tags.len() == 2)
    });
    driver.act(&mut harness);
    press(&mut driver, &mut harness, "Remove hills");
    driver.until(&mut harness, "one taken off", |app| {
        app.viewer_item().is_some_and(|item| item.tags == ["alps"])
    });
    assert_eq!(
        commands::viewer_item(&engine, all[3]).unwrap().tags,
        ["alps"]
    );
    // And no other photo has it.
    assert!(
        commands::viewer_item(&engine, all[4])
            .unwrap()
            .tags
            .is_empty()
    );
    assert_eq!(tags(&harness), ["alps"]);
}

// An album of photon's own is ticked in the panel and the photo is in it; ticked again,
// it is out. The engine announces nothing for an album that is not on screen: the
// write's own answer has the photo and the lists read again.
#[test]
fn an_album_is_ticked_and_unticked_in_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let album = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_album("Porto", 1_700_000_000_000).unwrap().id
    };
    let (mut harness, mut driver) = opened(&library, 12);
    driver.until(&mut harness, "the albums read", |app| {
        !app.collections().albums.is_empty()
    });
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    let count = |app: &photon_ui::app::App| {
        (app.collections().albums.iter())
            .find(|listed| listed.id == album)
            .map(|listed| listed.count)
    };
    informed(&mut driver, &mut harness, 2);
    press(&mut driver, &mut harness, "Porto");
    driver.until(&mut harness, "the photo said to be in it", |app| {
        app.viewer_item().is_some_and(|item| item.albums == [album]) && count(app) == Some(1)
    });
    assert_eq!(
        commands::viewer_item(&engine, all[2]).unwrap().albums,
        [album]
    );
    driver.act(&mut harness);
    press(&mut driver, &mut harness, "Porto");
    driver.until(&mut harness, "the photo said to be out of it", |app| {
        app.viewer_item().is_some_and(|item| item.albums.is_empty()) && count(app) == Some(0)
    });
    assert!(
        commands::viewer_item(&engine, all[2])
            .unwrap()
            .albums
            .is_empty()
    );
    assert_eq!(viewing(&harness), Some(all[2]), "and the viewer stayed");
}

// A camera in the panel is a link: it closes the viewer and searches for the camera,
// with the search in the box as if it had been typed.
#[test]
fn a_cameras_link_closes_the_viewer_and_searches_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    doctored(
        &library,
        "UPDATE items SET make = 'Canon', model = 'Canon EOS 5D' WHERE id = ?1",
        &ids[4..5],
    );
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let at = all.iter().position(|id| *id == ids[4]).unwrap();
    informed(&mut driver, &mut harness, at);
    press(&mut driver, &mut harness, "Canon EOS 5D");
    assert_eq!(viewing(&harness), None, "the viewer closed at the press");
    driver.until(&mut harness, "the camera's photos", |app| {
        app.settled().view == GridView::Search && app.photos() == 1
    });
    assert_eq!(harness.state().shown(), [ids[4]]);
    assert_eq!(harness.state().search_text(), "camera:\"Canon EOS 5D\"");
    // The photo is not handed back to a grid that is being left.
    assert_eq!(harness.state().selected(), Vec::<i64>::new());
}

/// photon has looked at every photo of `library`, found a face on `ids`, and the user
/// has named them all `name`.
fn named(library: &photon_ui::fixture::Fixture, ids: &[i64], name: &str) {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.set_face_detection(true).unwrap();
    let looked = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
    let face = Detection {
        rect: Rect {
            left: 0.3,
            top: 0.2,
            right: 0.6,
            bottom: 0.6,
        },
        landmarks: [
            (0.4, 0.35),
            (0.5, 0.35),
            (0.45, 0.42),
            (0.41, 0.5),
            (0.49, 0.5),
        ],
        score: 0.9,
    };
    let batch: Vec<_> = (looked.into_iter())
        .map(|photo| {
            let faces = if ids.contains(&photo.id) {
                vec![face.clone()]
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    assert_eq!(lib.name_items(ids, name).unwrap().named, ids.len() as i64);
}

// A person named on the photo is a link to their photos.
#[test]
fn a_persons_link_closes_the_viewer_and_shows_their_photos() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    named(&library, &ids[..2], "Anna");
    let (mut harness, mut driver) = opened(&library, 6);
    driver.settle(&mut harness);
    let shown = harness.state().shown();
    let at = |id: i64| shown.iter().position(|shown| *shown == id).unwrap();
    // photon looks for faces here, has looked at every photo, and is not looking now:
    // a photo with none is said to have none.
    informed(&mut driver, &mut harness, at(ids[4]));
    // (While the launch's pass is still going through the two faces it says "yet".)
    driver.until(&mut harness, "the pass over", |app| {
        (app.status().iter()).all(|line| !line.label.starts_with("Recognising"))
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("No faces found.").is_some());
    key(&mut driver, &mut harness, Key::Escape);
    rest(&mut driver, &mut harness, 1.0);
    informed(&mut driver, &mut harness, at(ids[1]));
    // (Her plate over the photo says "Anna" too: the link is the one of the two that
    // is one.)
    harness
        .get_by_role_and_label(egui::accesskit::Role::Link, "Anna")
        .click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None);
    driver.until(&mut harness, "Anna's photos", |app| {
        app.settled().view == GridView::Person && app.photos() == 2
    });
    let mut shown = harness.state().shown();
    shown.sort_unstable();
    let mut hers = ids[..2].to_vec();
    hers.sort_unstable();
    assert_eq!(shown, hers);
}

// A copy in the panel is a link that finds it in the grid: the viewer closes, and the
// copy is selected, alone, and brought into sight - here two hundred photos on, in the
// grid that was under the viewer all along.
#[test]
fn a_copy_is_found_in_the_grid_under_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 200, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[0], ids[190]]);
    let (mut harness, mut driver) = opened(&library, 200);
    driver.settle(&mut harness);
    let window = harness.ctx.content_rect();
    let far = harness.state().tile_rect(190).expect("laid out");
    assert!(!window.intersects(far), "out of sight: {far:?}");
    let version = harness.state().version();
    informed(&mut driver, &mut harness, 0);
    let copy = path_of(&harness, ids[190]);
    press(&mut driver, &mut harness, &copy);
    assert_eq!(viewing(&harness), None);
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[190]]);
    assert_eq!(
        harness.state().lead(),
        Some(ids[190]),
        "the keys go on from it"
    );
    let tile = harness.state().tile_rect(190).expect("in sight");
    assert!(window.contains_rect(tile), "{tile:?}");
    // No view was asked for: the grid is the one it was.
    assert_eq!(harness.state().version(), version);
    assert_eq!(harness.state().settled().view, GridView::All);
}

// From an excursion - a search - a copy is found in All photos: that view is asked for
// first, the search box is told, and the copy is looked up in the grid that arrives.
#[test]
fn a_copy_is_found_in_all_photos_from_a_search() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 200, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[0], ids[190]]);
    let (mut harness, mut driver) = opened(&library, 200);
    driver.settle(&mut harness);
    let engine = harness.state().engine().clone();
    let name = commands::viewer_item(&engine, ids[0]).unwrap().file_name;
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, &name);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the search's one photo", |app| {
        app.settled().view == GridView::Search && app.photos() == 1
    });
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, 0);
    let copy = path_of(&harness, ids[190]);
    press(&mut driver, &mut harness, &copy);
    assert_eq!(viewing(&harness), None);
    driver.until(&mut harness, "all photos, and the copy", |app| {
        app.settled().view == GridView::All && app.selected() == [ids[190]]
    });
    driver.act(&mut harness);
    assert_eq!(harness.state().search_text(), "");
    let window = harness.ctx.content_rect();
    let tile = harness.state().tile_rect(190).expect("in sight");
    assert!(window.contains_rect(tile), "{tile:?}");
}

// A copy the panel lists is never a hidden photo, so All photos is where it is found,
// from Hidden too: a photo unhidden in the viewer stays on screen there, with its
// copies listed as of any photo in sight. Looked up in Hidden, where a folder asked for
// is looked up, the press closed the viewer and found nothing.
#[test]
fn a_copy_is_found_in_all_photos_from_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 3);
    let ids = photo_ids(&library);
    let hidden: Vec<i64> = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        let visible: Vec<i64> = (lib.grid_entries().unwrap().iter())
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids, visible);
        let conn = rusqlite::Connection::open(library.dirs().db_path).unwrap();
        let mut rows = conn
            .prepare("SELECT id FROM items WHERE hidden = 1 ORDER BY id")
            .unwrap();
        rows.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(hidden.len(), 3);
    copies(&library, &[hidden[0], ids[5]]);
    let (mut harness, mut driver) = opened(&library, 9);
    driver.settle(&mut harness);
    crate::common::click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photos", |app| {
        app.settled().view == GridView::Hidden && app.photos() == 3
    });
    driver.settle(&mut harness);
    let at = (harness.state().shown().iter())
        .position(|id| *id == hidden[0])
        .unwrap();
    informed(&mut driver, &mut harness, at);
    // Hidden, it has no copies to list; brought back, it has.
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "its copy listed", |app| {
        (app.viewer_item()).is_some_and(|item| !item.hidden && !item.copies.is_empty())
    });
    driver.act(&mut harness);
    let copy = path_of(&harness, ids[5]);
    press(&mut driver, &mut harness, &copy);
    assert_eq!(viewing(&harness), None);
    driver.until(&mut harness, "all photos, and the copy", |app| {
        app.settled().view == GridView::All && app.selected() == [ids[5]]
    });
}

/// The right arrow pressed and let go of within one frame, and that frame.
fn arrow_in_a_frame(driver: &mut Driver, harness: &mut Harness<'_>) {
    for pressed in [false, true, false] {
        harness.input_mut().events.push(egui::Event::Key {
            key: Key::ArrowRight,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(harness);
}

// The keyword's field goes with the viewer. Closed by the mouse's back button while a
// keyword is typed, the very next key is the grid's: left with the keyboard, a field
// that is no longer drawn had the grid deaf to it.
#[test]
fn the_keyboard_is_the_grids_again_once_the_viewer_has_closed_over_a_keyword() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    informed(&mut driver, &mut harness, 3);
    into_field(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "al");
    // The button goes down, which closes the viewer, and the arrow is in the next frame,
    // the first the grid is drawn in, with the button still held. (Let go of first, the
    // click elsewhere would have had egui take the keyboard off the field by itself; and
    // a field that is not drawn it lets go of at the end of the frame that did not draw
    // it - a frame too late for this key.)
    let at = harness.ctx.content_rect().center();
    harness
        .input_mut()
        .events
        .push(button(at, PointerButton::Extra1, true));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None);
    arrow_in_a_frame(&mut driver, &mut harness);
    assert_eq!(harness.state().selected(), [all[4]], "the arrow moved on");
    harness
        .input_mut()
        .events
        .push(button(at, PointerButton::Extra1, false));
    driver.act(&mut harness);
    // Nothing was written for a keyword half typed.
    let engine = harness.state().engine().clone();
    assert!(
        commands::viewer_item(&engine, all[3])
            .unwrap()
            .tags
            .is_empty()
    );
}

// And where it is a grid of other results that shuts the viewer - a search sent under
// it, which does not hold its photo - the field's keyboard goes in that frame too: an
// arrow in the frame the results are drawn in is the grid's, and selects their first.
#[test]
fn the_keyboard_is_the_grids_in_the_frame_other_results_shut_the_viewer_over_a_keyword() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    let name = commands::viewer_item(&engine, all[2]).unwrap().file_name;
    search_then_open(&mut driver, &mut harness, &name, 8);
    assert_eq!(viewing(&harness), Some(all[8]), "the viewer opened");
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    key(&mut driver, &mut harness, Key::I);
    driver.act(&mut harness);
    into_field(&mut driver, &mut harness);
    // An arrow pressed and let go of in every frame, until the search has been sent
    // and answered: in the field it moves a caret.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while viewing(&harness).is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "the search never landed"
        );
        arrow_in_a_frame(&mut driver, &mut harness);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        harness.state().shown(),
        [all[2]],
        "the results of the search"
    );
    assert_eq!(harness.state().selected(), [all[2]]);
}

// The panel open is a still window as the photo alone is: once what it says is drawn,
// nothing asks for a frame.
#[test]
fn a_window_with_the_info_panel_open_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    informed(&mut driver, &mut harness, 0);
    driver.until(
        &mut harness,
        "the copy for the area beside the panel",
        |app| app.viewer_fitted() == Some([652, 9]),
    );
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// In a low window the panel is taller than its place: the wheel over it scrolls it and
// steps to no photo, and what is scrolled out of it is not there to be pressed or read -
// by a test, or by a screen reader - until it is scrolled back.
#[test]
fn the_wheel_scrolls_a_panel_taller_than_the_window_and_what_is_out_of_it_is_not_there() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    doctored(
        &library,
        "UPDATE items SET make = 'Canon', model = 'Canon EOS 5D' WHERE id = ?1",
        &ids[4..5],
    );
    copies(&library, &[ids[4], ids[7]]);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_album("Porto", 1_700_000_000_000).unwrap();
        lib.add_items_tag(&ids[4..5], "alps").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 12);
    driver.until(&mut harness, "the albums read", |app| {
        !app.collections().albums.is_empty()
    });
    driver.settle(&mut harness);
    let at = (harness.state().shown().iter())
        .position(|id| *id == ids[4])
        .unwrap();
    informed(&mut driver, &mut harness, at);
    harness.set_size(egui::vec2(1000.0, 300.0));
    driver.act(&mut harness);
    driver.act(&mut harness);
    // The camera's link is at the panel's top and the copy's at its foot.
    let foot = path_of(&harness, ids[7]);
    let foot = foot.as_str();
    let engine = harness.state().engine().clone();
    let name = commands::viewer_item(&engine, ids[4]).unwrap().file_name;
    let name = name.as_str();
    assert!(harness.query_by_label(name).is_some());
    assert!(harness.query_by_label("Camera").is_some());
    assert!(harness.query_by_label("Canon EOS 5D").is_some());
    assert!(harness.query_by_label(foot).is_none(), "below the panel");
    // And of every other kind of thing in the panel: a heading, a keyword and the
    // cross on it, an album's tick.
    let below = ["Keywords", "alps", "Remove alps", "Porto"];
    for label in below {
        assert!(harness.query_by_label(label).is_none(), "{label}");
    }
    let over = harness.get_by_label("Canon EOS 5D").rect().center();
    let turn = |driver: &mut Driver, harness: &mut Harness<'_>, lines: f32| {
        harness.event(egui::Event::PointerMoved(over));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, lines),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(harness);
        driver.act(harness);
    };
    // A notch at a time, down to the panel's foot: each of them comes into sight on
    // the way. (Not all at once at the end: how many lines the copy's path takes is
    // the temporary folder's business, and on a Mac it is long.)
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 {
        turn(&mut driver, &mut harness, -1.0);
        seen.extend(
            below
                .iter()
                .filter(|label| harness.query_by_label(label).is_some()),
        );
        if harness.query_by_label(foot).is_some() {
            break;
        }
    }
    assert_eq!(seen, below.iter().collect());
    turn(&mut driver, &mut harness, -100.0);
    assert_eq!(viewing(&harness), Some(ids[4]), "no step to another photo");
    assert!(
        harness.query_by_label(foot).is_some(),
        "scrolled to its end"
    );
    assert!(harness.query_by_label("Dates").is_none(), "a heading above");
    assert!(
        harness.query_by_label("Canon EOS 5D").is_none(),
        "above the panel"
    );
    assert!(harness.query_by_label(name).is_none(), "its name with it");
    assert!(harness.query_by_label("Camera").is_none(), "and its row");
    turn(&mut driver, &mut harness, 100.0);
    assert!(harness.query_by_label("Canon EOS 5D").is_some());
    assert_eq!(viewing(&harness), Some(ids[4]));
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

/// Whether the keyboard is the keyword's field's.
fn typing(harness: &Harness<'_>) -> bool {
    harness.state().viewer_typing()
}

// An album is ticked for the photo whose panel was on screen. With an arrow key in the
// frame the button comes up in - a key that is held repeats thirty times a second - the
// step is acted on first, and the tick is written by `open.viewer.id()`, which is by
// then the next photo: one whose panel was never drawn.
#[test]
fn a_tick_in_the_frame_of_a_step_is_written_to_the_photo_it_was_shown_for() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let album = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_album("Porto", 1_700_000_000_000).unwrap().id
    };
    let (mut harness, mut driver) = opened(&library, 12);
    driver.until(&mut harness, "the albums read", |app| {
        !app.collections().albums.is_empty()
    });
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 2);
    let at = harness.get_by_label("Porto").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    driver.act(&mut harness);
    // The button comes up, and the arrow key goes down, in one frame.
    harness.input_mut().events.extend([
        button(at, PointerButton::Primary, false),
        key_event(Key::ArrowRight, true, false),
    ]);
    driver.act(&mut harness);
    harness
        .input_mut()
        .events
        .push(key_event(Key::ArrowRight, false, false));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(all[3]), "the arrow stepped");
    rest(&mut driver, &mut harness, 1.0);
    let albums = |id: i64| commands::viewer_item(&engine, id).unwrap().albums;
    assert_eq!(
        (albums(all[2]), albums(all[3])),
        (vec![album], Vec::new()),
        "the tick was pressed in the panel of the photo {} and written to {}",
        all[2],
        all[3],
    );
}

// The same for a keyword's cross: taken off the photo stepped to, and left on the one
// whose chip was pressed.
#[test]
fn a_keyword_taken_off_in_the_frame_of_a_step_is_taken_off_the_photo_it_was_shown_on() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.add_items_tag(&ids[2..4], "hills").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    assert_eq!(all, ids);
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 2);
    let at = harness.get_by_label("Remove hills").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.input_mut().events.extend([
        button(at, PointerButton::Primary, false),
        key_event(Key::ArrowRight, true, false),
    ]);
    driver.act(&mut harness);
    harness
        .input_mut()
        .events
        .push(key_event(Key::ArrowRight, false, false));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(all[3]), "the arrow stepped");
    rest(&mut driver, &mut harness, 1.0);
    let tags = |id: i64| commands::viewer_item(&engine, id).unwrap().tags;
    assert_eq!(
        (tags(all[2]), tags(all[3])),
        (Vec::<String>::new(), vec!["hills".to_owned()]),
        "the cross was pressed on the photo {} and the keyword taken off {}",
        all[2],
        all[3],
    );
}

// And for a keyword added: typed, Enter, and the wheel turned over the photo in the
// frame of that Enter (the pointer rests on the photo: the field was clicked and the
// mouse moved away, which leaves the keyboard in the field). The wheel's step is acted
// on first, and the keyword is the next photo's.
#[test]
fn a_keyword_added_in_the_frame_of_a_wheels_step_is_added_to_the_photo_it_was_typed_for() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 2);
    into_field(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "hills");
    // The pointer is moved onto the photo, with no button: the field keeps the keyboard.
    let over_photo = egui::pos2(300.0, 200.0);
    harness.event(egui::Event::PointerMoved(over_photo));
    driver.act(&mut harness);
    assert!(typing(&harness));
    harness.input_mut().events.extend([
        key_event(Key::Enter, true, false),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    driver.act(&mut harness);
    harness
        .input_mut()
        .events
        .push(key_event(Key::Enter, false, false));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(all[3]), "the wheel stepped");
    rest(&mut driver, &mut harness, 1.0);
    let tags = |id: i64| commands::viewer_item(&engine, id).unwrap().tags;
    assert_eq!(
        (tags(all[2]), tags(all[3])),
        (vec!["hills".to_owned()], Vec::<String>::new()),
        "the keyword was typed for the photo {} and written to {}",
        all[2],
        all[3],
    );
}

/// Whether the keyword's field still has the keyboard once the viewer has stepped by
/// the wheel with the caret in it and the next photo's panel is drawn - the frame after
/// the step being drawn `after` the step, in real time.
fn typing_after_a_wheels_step(after: Duration) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    informed(&mut driver, &mut harness, 2);
    into_field(&mut driver, &mut harness);
    let over_photo = egui::pos2(300.0, 200.0);
    harness.event(egui::Event::PointerMoved(over_photo));
    driver.act(&mut harness);
    assert!(typing(&harness));
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(all[3]), "the wheel stepped");
    // The next frame: a refresh later on a fast machine, later on a slow one.
    std::thread::sleep(after);
    driver.act(&mut harness);
    driver.until(&mut harness, "the next photo's facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == all[3])
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    typing(&harness)
}

// A step by the wheel with the caret in the keyword's field. If the library has
// answered for the next photo by the time the next frame is drawn, the field is drawn
// in that frame and keeps the keyboard; if it has not, the field is not drawn for a
// frame, egui lets go of it, and the next letter typed is the viewer's key - `h` hides
// the photo stepped to. Which of the two happens is a matter of milliseconds.
#[test]
fn who_has_the_keyboard_after_a_wheels_step_does_not_depend_on_how_fast_the_library_answers() {
    let at_once = typing_after_a_wheels_step(Duration::ZERO);
    let a_moment_later = typing_after_a_wheels_step(Duration::from_millis(300));
    assert!(
        at_once && a_moment_later,
        "the field kept the keyboard: with the next frame at once {at_once}, with it a \
         moment later {a_moment_later}"
    );
}

// What that costs when the frame comes at once: `h` typed after the step hides the
// photo stepped to. (With the caret in the field when the wheel was turned, and nothing
// pressed since.)
#[test]
fn a_letter_typed_after_a_wheels_step_from_the_field_hides_no_photo() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 2);
    into_field(&mut driver, &mut harness);
    let over_photo = egui::pos2(300.0, 200.0);
    harness.event(egui::Event::PointerMoved(over_photo));
    driver.act(&mut harness);
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.until(&mut harness, "the next photo's facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == all[3])
    });
    driver.act(&mut harness);
    // "hills", as a keyboard sends it: each key with the letter it types.
    for letter in ["h", "i", "l", "l", "s"] {
        let key = Key::from_name(&letter.to_uppercase()).unwrap();
        harness.input_mut().events.extend([
            key_event(key, true, false),
            egui::Event::Text(letter.to_owned()),
        ]);
        driver.act(&mut harness);
        harness
            .input_mut()
            .events
            .push(key_event(key, false, false));
        driver.act(&mut harness);
    }
    rest(&mut driver, &mut harness, 1.0);
    assert!(
        !commands::viewer_item(&engine, all[3]).unwrap().hidden,
        "the `h` of a keyword hid the photo stepped to"
    );
    assert!(
        harness.state().viewer_info(),
        "and its `i` closed the panel"
    );
    // And the letters are in the field of the photo stepped to, for its keyword.
    let field = harness.get_by_role(egui::accesskit::Role::TextInput);
    assert_eq!(field.accesskit_node().value().as_deref(), Some("hills"));
}

/// The viewer with its panel open, in a window low enough for the panel to scroll, where
/// the system's notch is `notch`: and where "Dates" stood, beside the library's folder.
fn a_low_panel<'a>(notch: fn() -> Notch) -> (tempfile::TempDir, Harness<'a>, Driver, f32) {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    copies(&library, &[ids[4], ids[7]]);
    let dirs = library.dirs();
    let mut harness: Harness<'_> = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(crate::common::REFRESH as f32)
        .build_eframe(move |cc| {
            photon_ui::app::App::new(cc, dirs, None)
                .unwrap()
                .without_animations()
                .with_wheel(notch)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == 12 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let at = (harness.state().shown().iter())
        .position(|id| *id == ids[4])
        .unwrap();
    informed(&mut driver, &mut harness, at);
    harness.set_size(egui::vec2(1000.0, 260.0));
    driver.act(&mut harness);
    driver.act(&mut harness);
    let before = harness.get_by_label("Dates").rect().top();
    (dir, harness, driver, before)
}

/// How far "Dates" has gone up since it stood at `before`: everything, when it is out
/// of sight.
fn gone_up(harness: &Harness<'_>, before: f32) -> f32 {
    match harness.query_by_label("Dates") {
        Some(dates) => before - dates.rect().top(),
        None => f32::INFINITY,
    }
}

/// How far one notch of the wheel over the panel moves it, where the system's notch is
/// `notch`.
fn a_notch_over_the_panel(notch: fn() -> Notch) -> f32 {
    let (_dir, mut harness, mut driver, before) = a_low_panel(notch);
    let over = harness.get_by_label("Dates").rect().center();
    harness.event(egui::Event::PointerMoved(over));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    gone_up(&harness, before)
}

// A notch moves the panel as far as it moves the grid: the system's own, a hundred points
// at Windows' three lines. It moved forty whatever the system said, and the People page,
// which went by the same rule, was "slow" beside the grid to the first person to run it on
// Windows.
#[test]
fn the_panels_notch_is_the_grids() {
    let moved = a_notch_over_the_panel(|| Notch::Points(100.0));
    assert!(
        (moved - 100.0).abs() < 0.5,
        "a notch moved the panel {moved} points where the system's notch is a hundred"
    );
}

// Where Windows is set to scroll no lines a notch, the wheel moves the panel no more than
// it moves the grid, and the panel's scrollbar - the grid's own - takes it down: a press
// at the foot of its track brings its end into sight.
#[test]
fn a_panel_the_wheel_does_not_move_is_taken_down_by_its_scrollbar() {
    assert_eq!(a_notch_over_the_panel(|| Notch::Points(0.0)), 0.0);
    let (_dir, mut harness, mut driver, before) = a_low_panel(|| Notch::Points(0.0));
    let size = harness.ctx.content_rect();
    let panel = photon_ui::viewer_layout::info_place(
        photon_ui::viewer_layout::Frame {
            left: size.left(),
            top: size.top(),
            width: size.width(),
            height: size.height(),
        },
        1.0,
    );
    let foot = Pos2::new(
        panel.left + panel.width - 4.0,
        panel.top + panel.height - 2.0,
    );
    harness.event(egui::Event::PointerMoved(foot));
    driver.act(&mut harness);
    harness.event(egui::Event::PointerButton {
        pos: foot,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        gone_up(&harness, before) > 0.0,
        "a press on the panel's scrollbar moved nothing"
    );
    // And the photo under the panel is not what was pressed: the viewer is still open.
    assert!(harness.state().viewer_info());
}

// "The photo is not handed back to the grid first: the grid that was under the viewer
// is being left too." Not when the link leads to where the user is already: in a
// camera's search, the camera's link asks for no step (`Nav::wants`), the grid under
// the viewer stays - and the photo the viewer was on is not handed back to it. The
// selection is still the photo the viewer was opened on, however far it was stepped.
#[test]
fn a_link_to_where_the_user_already_is_hands_the_photo_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    doctored(
        &library,
        "UPDATE items SET make = 'Canon', model = 'Canon EOS 5D' WHERE id = ?1",
        &ids[2..6],
    );
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "camera:\"Canon EOS 5D\"");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the camera's four photos", |app| {
        app.settled().view == GridView::Search && app.photos() == 4
    });
    driver.settle(&mut harness);
    let shown = harness.state().shown();
    informed(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::ArrowRight);
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(shown[2]));
    driver.until(&mut harness, "its facts", |app| {
        app.viewer_item().is_some_and(|item| item.id == shown[2])
    });
    driver.act(&mut harness);
    harness.get_by_label("Canon EOS 5D").click();
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None, "the viewer closed");
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(
        harness.state().shown(),
        shown,
        "the same grid: nothing was left"
    );
    assert_eq!(
        harness.state().selected(),
        [shown[2]],
        "the photo that was on screen is the one handed back"
    );
}

// An album is ticked and Escape pressed straight after, in one frame: the frame's keys
// are acted on before what was pressed in the panel, whichever came first, and `Close`
// ends the frame's list. The tick is never written, and nothing says so.
#[test]
fn a_tick_in_the_frame_of_the_escape_that_closes_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let album = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_album("Porto", 1_700_000_000_000).unwrap().id
    };
    let (mut harness, mut driver) = opened(&library, 12);
    driver.until(&mut harness, "the albums read", |app| {
        !app.collections().albums.is_empty()
    });
    driver.settle(&mut harness);
    let all = harness.state().shown();
    let engine = harness.state().engine().clone();
    informed(&mut driver, &mut harness, 2);
    let at = harness.get_by_label("Porto").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.input_mut().events.extend([
        button(at, PointerButton::Primary, false),
        key_event(Key::Escape, true, false),
    ]);
    driver.act(&mut harness);
    harness
        .input_mut()
        .events
        .push(key_event(Key::Escape, false, false));
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), None, "Escape closed the viewer");
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(
        commands::viewer_item(&engine, all[2]).unwrap().albums,
        [album],
        "the tick that was pressed before the Escape"
    );
}

// The rows take it as nothing; the engine does not. `add_item_tag` and
// `remove_item_tag` rebuild the whole grid whether or not anything changed, so a
// keyword the photo already has, typed again - which the Svelte panel drops before it
// asks (`createTagEditor::add`: "a name already shown") - is a query over the whole
// library, a new grid version, and the viewer, the counts and the lists read again.
#[test]
fn a_keyword_the_photo_already_has_rebuilds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.add_items_tag(&ids[2..3], "hills").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    informed(&mut driver, &mut harness, 2);
    assert!(harness.query_by_label("Remove hills").is_some());
    rest(&mut driver, &mut harness, 1.0);
    let version = harness.state().version();
    into_field(&mut driver, &mut harness);
    typed(&mut driver, &mut harness, "hills");
    key(&mut driver, &mut harness, Key::Enter);
    rest(&mut driver, &mut harness, 1.0);
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(
        harness.state().version(),
        version,
        "the grid was built again for a keyword the photo had"
    );
}

// the zoom's slider held when `?` opens the list over the viewer is let go of, as
// a pan held in compare is: the pointer moving on under the list zooms nothing.
#[test]
fn the_slider_held_when_the_list_opens_zooms_nothing_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    driver.act(&mut harness);
    let slider = harness.get_by_label("Zoom").rect();
    let middle = slider.center();
    harness.event(egui::Event::PointerMoved(middle));
    harness.event(button(middle, PointerButton::Primary, true));
    driver.act(&mut harness);
    harness.input_mut().events.extend([
        egui::Event::Key {
            key: Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        egui::Event::Text("?".to_owned()),
    ]);
    driver.act(&mut harness);
    assert!(matches!(
        harness.state().dialog(),
        Some(photon_ui::dialogs::Dialog::Shortcuts)
    ));
    let held = zoom(&harness);
    let further = egui::pos2(slider.right() - 2.0, middle.y);
    harness.event(egui::Event::PointerMoved(further));
    driver.act(&mut harness);
    driver.act(&mut harness);
    let under = zoom(&harness);
    assert_eq!(under, held, "the slider zoomed the photo under the list");
    // Nor once the list is shut with the button still down - by `?`: egui itself ends a
    // drag at Escape - the drag being over when the list opened, and the button maybe
    // come up under it unheard.
    for pressed in [false, true, false] {
        harness.event(egui::Event::Key {
            key: Key::Questionmark,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        });
        driver.act(&mut harness);
    }
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    harness.event(egui::Event::PointerMoved(egui::pos2(
        slider.left() + 2.0,
        middle.y,
    )));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.event(button(further, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert_eq!(
        zoom(&harness),
        held,
        "the slider followed again after the list"
    );
}
