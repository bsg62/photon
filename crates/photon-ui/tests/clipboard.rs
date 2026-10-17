//! Copy photo, through the whole application and the real engine: the grid's Ctrl+C, the
//! tile menu's "Copy photo" and the viewer's Ctrl+C, over real photos the test made, and
//! the viewer's caption copying the file name. The clipboard is a stand-in
//! (`common::Picker`): a CI runner has none, and a test must not write to the clipboard
//! of whoever runs it.

use crate::common::{Driver, Harness, Picker, REFRESH, photos_in, said};
use eframe::egui::{self, Key, Modifiers, PointerButton};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_ui::{app::App, dirs};
use std::{path::Path, sync::Arc};

/// The application watching three real photos - `a`, `b`, `c` - with `picker` for the
/// system, once they are shown.
fn three<'a>(dir: &Path, picker: Arc<Picker>) -> (Harness<'a>, Driver) {
    let pictures = photos_in(dir, "Pictures", &["a.jpg", "b.jpg", "c.jpg"]);
    let dirs = dirs::within(&dir.join("data"), &dir.join("cache"));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, Some(pictures))
                .unwrap()
                .without_animations())
            .with_videos(crate::common::no_videos())
            .with_platform(picker)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    (harness, driver)
}

fn button(
    at: egui::Pos2,
    button: PointerButton,
    pressed: bool,
    modifiers: Modifiers,
) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers,
    }
}

/// A click on the photo at `offset`, with `held`.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(button(at, PointerButton::Primary, true, held));
    harness.event(button(at, PointerButton::Primary, false, held));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

/// Ctrl/Cmd+C, as egui-winit hands it over: a copy, and no key.
fn copy(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.event(egui::Event::Copy);
    driver.act(harness);
}

/// The photos the stand-in's clipboard was given, by their size.
fn copied(picker: &Picker) -> Vec<(usize, usize)> {
    picker.copied.lock().unwrap().clone()
}

// Ctrl+C with one photo selected puts it on the clipboard, and says so: a copy changes
// nothing on screen.
#[test]
fn ctrl_c_copies_the_photo_selected() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    copy(&mut driver, &mut harness);
    driver.until(&mut harness, "Photo copied", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == "Photo copied")
    });
    let pictures = copied(&picker);
    assert_eq!(pictures.len(), 1);
    assert!(pictures[0].0 > 0 && pictures[0].1 > 0);
}

// The clipboard holds one picture: with two photos selected, Ctrl+C copies nothing - not
// the lead of a wider selection, the Svelte grid's rule.
#[test]
fn ctrl_c_with_two_photos_selected_copies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 1, Modifiers::SHIFT);
    assert_eq!(harness.state().selected().len(), 2);
    copy(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(copied(&picker), []);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// The tile menu's "Copy photo" copies the photo it was opened on.
#[test]
fn the_menus_copy_photo_copies_it() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    let at = (harness.state().tile_rect(2)).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Copy photo").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "Photo copied", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == "Photo copied")
    });
    assert_eq!(copied(&picker).len(), 1);
}

// One copy at a time: a second press while the first is on its way - its full-size
// decode waiting behind another render - is dropped, not a second decode of the same
// photo and a second "Photo copied".
#[test]
fn a_second_copy_while_one_is_on_its_way_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    let rendering = photon_engine::engine::RENDERING.lock();
    copy(&mut driver, &mut harness);
    copy(&mut driver, &mut harness);
    drop(rendering);
    driver.until(&mut harness, "Photo copied", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == "Photo copied")
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(copied(&picker).len(), 1);
    assert_eq!(said(&harness), ["Photo copied"]);
}

// A clipboard that refuses the picture is said, in its own words.
#[test]
fn a_clipboard_that_refuses_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    *picker.refused.lock().unwrap() = Some("the clipboard is busy".to_owned());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    copy(&mut driver, &mut harness);
    driver.until(&mut harness, "the refusal", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["the clipboard is busy"]);
}

// In the viewer Ctrl+C copies the photo on screen, and a click on the caption its file
// name, which the caption says it did for a moment.
#[test]
fn the_viewer_copies_its_photo_and_its_caption_its_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    driver.frames_in(&mut harness, 0.5);
    harness.key_press_modifiers(Modifiers::NONE, Key::Enter);
    driver.act(&mut harness);
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    driver.act(&mut harness);
    copy(&mut driver, &mut harness);
    driver.until(&mut harness, "Photo copied", |app| {
        app.toasts()
            .iter()
            .any(|toast| toast.message == "Photo copied")
    });
    assert_eq!(copied(&picker).len(), 1);
    let name = harness.state().viewer_item().unwrap().file_name.clone();
    let caption = (harness.query_all_by_label_contains(&name))
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("the bar's caption")
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(caption));
    harness.event(button(
        caption,
        PointerButton::Primary,
        true,
        Modifiers::NONE,
    ));
    harness.event(button(
        caption,
        PointerButton::Primary,
        false,
        Modifiers::NONE,
    ));
    harness.step();
    let commands = harness.output().platform_output.commands.clone();
    assert!(
        commands.contains(&egui::OutputCommand::CopyText(name.clone())),
        "{commands:?}"
    );
    driver.act(&mut harness);
    driver.seen(&mut harness, "Copied");
    driver.frames_in(&mut harness, 1.5);
    assert!(
        harness.query_by_label("Copied").is_none(),
        "the file name back"
    );
}

// A press for another photo while one is being copied is not dropped: it goes after it,
// so that what is on the clipboard at the end is the last photo asked for. Dropped, the
// "Photo copied" that came was the first photo's, and that is what was pasted.
#[test]
fn a_copy_of_another_photo_goes_after_the_one_on_its_way() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    let rendering = photon_engine::engine::RENDERING.lock();
    copy(&mut driver, &mut harness);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    copy(&mut driver, &mut harness);
    drop(rendering);
    driver.until(&mut harness, "both copied", |_| copied(&picker).len() == 2);
}

// Ctrl+Shift+C is no copy of a photo, though egui makes a copy of it too.
#[test]
fn ctrl_shift_c_copies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let picker = Arc::new(Picker::default());
    let (mut harness, mut driver) = three(dir.path(), picker.clone());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    harness.event(egui::Event::ModifiersChanged(
        Modifiers::COMMAND | Modifiers::SHIFT,
    ));
    harness.event(egui::Event::Copy);
    driver.act(&mut harness);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(copied(&picker), []);
}
