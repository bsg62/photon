//! The viewer, through the whole application and the real engine: over a library of real
//! photos made for the test, in a temporary directory. A star written here goes into
//! that directory's own `.picasa.ini`, and nowhere else.

mod common;

use common::{Driver, Harness, library, opened, over, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_engine::commands;
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
        Some(vec![4200, 2100, 1050])
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
    assert_eq!(harness.state().viewer_levels(), Some(vec![2100, 1050]));
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
    // The frame the size has stood long enough in is asked for.
    assert!(driver.frames_in(&mut harness, 0.3) >= 1);
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
    driver.until(&mut harness, "the preview", |app| app.viewer_asked());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.state_mut().viewer_preview());
    assert_eq!(harness.state().viewer_levels(), None);
    assert!(harness.query_by_label_contains("can't be read").is_none());
    // And it is a still window, not one asking again and again.
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
    assert_eq!(harness.state().viewer_levels(), Some(vec![60, 30, 15]));
}

// A photo whose file goes while it is on screen: the scan that misses it marks it, the
// engine then refuses it, and the viewer says so in its place.
#[test]
fn a_photo_that_leaves_the_library_under_the_viewer_is_said_to_be_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, folder) = three(dir.path());
    open(&mut driver, &mut harness, 1);
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
