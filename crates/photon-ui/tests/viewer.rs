//! The viewer, through the whole application and the real engine: over a library of real
//! photos made for the test, in a temporary directory. A star written here goes into
//! that directory's own `.picasa.ini`, and nowhere else.

mod common;

use common::{Driver, Harness, hold_over, library, opened, over, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{edit::Edit, media::ThumbState, thumbs::ThumbSize};
use photon_engine::{commands, engine::RENDERING};
use photon_ui::folder_drop::CARD;
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
    let folder = pictures(dir);
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
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
#[test]
fn a_photo_that_cannot_be_shown_says_so_and_holds_no_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    let engine = harness.state().engine().clone();
    (engine.lib)
        .set_thumb_state(ids[1], ThumbState::Failed, Some("It is broken."))
        .unwrap();
    let rendering = RENDERING.lock();
    open(&mut driver, &mut harness, 1);
    assert!(harness.state().viewer_asked(), "asked for at the opening");
    driver.act(&mut harness);
    assert!(harness.query_by_label("It is broken.").is_some());
    drop(rendering);
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

// A video is shown from its poster, where it has one, under a line that says the native
// window does not play it yet.
#[test]
fn a_video_says_that_it_is_not_played_yet() {
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
    assert!(
        harness
            .query_by_label("The native window does not play videos yet.")
            .is_some()
    );
}
