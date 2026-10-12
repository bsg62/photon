//! The export, through the whole application and the real engine: over a library of real
//! photos made for the test, into a destination the test made. Every file written lands
//! under the test's own temporary directory.

mod common;

use common::{Driver, Harness, over, photos_in, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::library::Library;
use photon_engine::commands;
use photon_ui::{
    dialogs::Dialog,
    dirs::Dirs,
    export_dialog::{Dest, ExportDialog, NO_FOLDER, NOT_WHOLE},
    toasts::Kind,
};
use std::path::{Path, PathBuf};

fn button(pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers,
    }
}

fn middle(harness: &Harness<'_>, offset: usize) -> Pos2 {
    (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center()
}

/// A click on the photo at `offset`, with `held`.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = middle(harness, offset);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(button(at, PointerButton::Primary, true, held));
    harness.event(button(at, PointerButton::Primary, false, held));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

/// The other button on the photo at `offset`, and the frame its menu is first drawn in.
fn right_click(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = middle(harness, offset);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

/// A library that watches a folder of three real photos - `a`, `b`, `c`, in that order
/// in the grid - once they are shown; and an empty folder beside it to export into.
fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Dirs, PathBuf, PathBuf) {
    let pictures = photos_in(dir, "Pictures", &["a.jpg", "b.jpg", "c.jpg"]);
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let (mut harness, mut driver, dirs) = over(dir, Some(&pictures));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    (harness, driver, dirs, pictures, out)
}

/// Selects the photos from `from` to `to` and opens the export's dialog for them.
fn open_export(driver: &mut Driver, harness: &mut Harness<'_>, (from, to): (usize, usize)) {
    pick(driver, harness, from, Modifiers::NONE);
    if to != from {
        pick(driver, harness, to, Modifiers::SHIFT);
    }
    right_click(driver, harness, from);
    let count = to - from + 1;
    let item = if count == 1 {
        "Export photo…".to_owned()
    } else {
        format!("Export {count} photos…")
    };
    harness.get_by_label(&item).click();
    driver.act(harness);
    // Until the dialog stands: an area is measured in one frame and shown in the next,
    // and what lies under it is kept from the pointer from the one after.
    for _ in 0..3 {
        driver.act(harness);
    }
    // And until a press in it is no longer the second of a double click on the item.
    driver.frames_in(harness, 0.4);
    assert!(exporting(harness).is_some(), "the export's dialog is open");
}

/// The export's dialog, when it is the one open.
fn exporting<'a>(harness: &'a Harness<'_>) -> Option<&'a ExportDialog> {
    match harness.state().dialog() {
        Some(Dialog::Export(export)) => Some(export),
        _ => None,
    }
}

/// Types `path` into the dialog's field and waits for what the engine says of it.
fn type_path(driver: &mut Driver, harness: &mut Harness<'_>, path: &str) -> Dest {
    typed(driver, harness, path);
    driver.until(harness, "the path asked about", |app| {
        matches!(app.dialog(), Some(Dialog::Export(export))
            if !matches!(export.dest(), Dest::Checking | Dest::Empty))
    });
    exporting(harness).unwrap().dest().clone()
}

/// The names of the files in `folder`, sorted.
fn files(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = (std::fs::read_dir(folder).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// The whole of it: three photos selected, the dialog says so, a folder's path typed and
// passed, Enter - and the copies are in the folder, each the photo it is a copy of,
// under one message. The dialog closes before the export runs.
#[test]
fn the_photos_selected_are_copied_into_the_folder_typed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, pictures, out) = three(dir.path());
    open_export(&mut driver, &mut harness, (0, 2));
    assert_eq!(exporting(&harness).unwrap().title(), "Export 3 photos");
    assert!(harness.query_by_label("Folder to export to").is_some());
    // Nothing typed: Enter exports nothing, and the dialog stays.
    key(&mut driver, &mut harness, Key::Enter);
    assert!(exporting(&harness).is_some());

    let dest = out.to_string_lossy().into_owned();
    assert_eq!(
        type_path(&mut driver, &mut harness, &dest),
        Dest::Passed(dest.clone())
    );
    key(&mut driver, &mut harness, Key::Enter);
    assert!(harness.state().dialog().is_none(), "closed before it runs");
    driver.until(&mut harness, "the export over", |app| {
        !app.exporting() && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), [format!("Exported 3 photos to {dest}")]);
    assert_eq!(files(&out), ["a.jpg", "b.jpg", "c.jpg"]);
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        assert_eq!(
            std::fs::read(out.join(name)).unwrap(),
            std::fs::read(pictures.join(name)).unwrap(),
            "{name} is a copy, byte for byte"
        );
    }
    // The library is as it was: nothing was written where photon watches.
    assert_eq!(files(&pictures), ["a.jpg", "b.jpg", "c.jpg"]);
    driver.settle(&mut harness);
    assert_eq!(harness.state().photos(), 3);
}

// Copies written inside a watched folder would be scanned back in as new photos: the
// engine refuses the folder, and it is said under the field while the dialog is open.
// "Export" takes no press, and nothing is written.
#[test]
fn a_folder_inside_the_library_is_refused_under_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, pictures, _) = three(dir.path());
    let inside = pictures.join("copies");
    std::fs::create_dir_all(&inside).unwrap();
    open_export(&mut driver, &mut harness, (1, 1));
    let dest = type_path(&mut driver, &mut harness, &inside.to_string_lossy());
    let Dest::Refused(why) = dest else {
        panic!("{dest:?}");
    };
    assert!(why.contains("is inside the watched folder"), "{why}");
    driver.act(&mut harness);
    assert!(harness.query_by_label(&why).is_some(), "said in the dialog");
    harness.get_by_label("Export").click();
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Tab);
    driver.settle(&mut harness);
    assert!(exporting(&harness).is_some());
    assert!(!harness.state().exporting());
    assert_eq!(files(&inside), Vec::<String>::new());
    // The watched folder itself, too.
    key(&mut driver, &mut harness, Key::Escape);
    open_export(&mut driver, &mut harness, (1, 1));
    let dest = type_path(&mut driver, &mut harness, &pictures.to_string_lossy());
    assert!(matches!(dest, Dest::Refused(_)), "{dest:?}");
}

// What is typed has to be the whole path of a folder that is there.
#[test]
fn a_path_that_is_no_folder_is_said_to_be_none() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, pictures, out) = three(dir.path());
    for (path, why) in [
        (
            out.join("nowhere").to_string_lossy().into_owned(),
            NO_FOLDER,
        ),
        // A file passes the engine's own check, which asks only where the path lies.
        (
            dir.path()
                .join("source-0.jpg")
                .to_string_lossy()
                .into_owned(),
            NO_FOLDER,
        ),
        // A folder there is, read from where the test runs: the crate's own `src`.
        // No whole path all the same - it would be read from wherever photon was
        // started.
        ("src".to_owned(), NOT_WHOLE),
    ] {
        open_export(&mut driver, &mut harness, (0, 0));
        // A file that is no photo of the library's, beside it.
        std::fs::write(dir.path().join("source-0.jpg"), b"not a folder").unwrap();
        assert_eq!(
            type_path(&mut driver, &mut harness, &path),
            Dest::Refused(why.to_owned()),
            "{path}"
        );
        driver.act(&mut harness);
        assert!(harness.query_by_label(why).is_some());
        key(&mut driver, &mut harness, Key::Escape);
        assert!(harness.state().dialog().is_none());
    }
    assert_eq!(files(&out), Vec::<String>::new());
    assert_eq!(files(&pictures).len(), 3);
}

// The dialog said "Export 2 photos" when it opened, and two are what it exports -
// whatever has left the selection behind it since.
#[test]
fn the_photos_exported_are_those_selected_when_the_dialog_opened() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    let shown = harness.state().shown().to_vec();
    open_export(&mut driver, &mut harness, (0, 1));
    assert_eq!(harness.state().selected(), shown[..2]);
    // One of the two is hidden from elsewhere, and leaves the grid and the selection.
    let engine = harness.state().engine().clone();
    commands::set_items_hidden(&engine, &shown[..1], true).unwrap();
    driver.until(&mut harness, "the photo gone from the grid", |app| {
        app.photos() == 2 && app.selected() == shown[1..2]
    });
    let dest = out.to_string_lossy().into_owned();
    type_path(&mut driver, &mut harness, &dest);
    harness.get_by_label("Export").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the export over", |app| {
        !app.exporting() && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), [format!("Exported 2 photos to {dest}")]);
    assert_eq!(files(&out), ["a.jpg", "b.jpg"]);
}

// The checkbox is stored as it is ticked, not when an export runs - an export the user
// cancels still said how they want exports to work - and read when the dialog next
// opens.
#[test]
fn the_checkbox_is_stored_when_it_is_ticked_and_read_when_the_dialog_next_opens() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, dirs, _, _) = three(dir.path());
    let stored = || {
        Library::open(&dirs.db_path)
            .unwrap()
            .export_apply_edits()
            .unwrap()
    };
    assert!(stored());
    open_export(&mut driver, &mut harness, (0, 0));
    assert!(exporting(&harness).unwrap().apply_edits());
    harness.get_by_label("Apply edits to the copies").click();
    driver.act(&mut harness);
    assert!(!exporting(&harness).unwrap().apply_edits());
    // A size is the dialog's own, and stored nowhere.
    harness.get_by_label("1920 px").click();
    driver.act(&mut harness);
    assert_eq!(exporting(&harness).unwrap().max_edge(), Some(1920));
    driver.settle(&mut harness);
    assert!(!stored(), "stored at the tick");
    key(&mut driver, &mut harness, Key::Escape);

    // The next dialog opens ticked, as any does, until the stored choice is read.
    open_export(&mut driver, &mut harness, (0, 0));
    driver.until(
        &mut harness,
        "the stored choice read",
        |app| matches!(app.dialog(), Some(Dialog::Export(export)) if !export.apply_edits()),
    );
    driver.settle(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
}

// One export at a time. A photo that has been turned is rendered for its copy, one
// render at a time behind the engine's own lock: held here, the export stands at its
// first photo for as long as the test needs.
#[test]
fn export_takes_no_press_while_an_export_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    let shown = harness.state().shown().to_vec();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, shown[0], true).unwrap();
    driver.settle(&mut harness);
    let rendering = photon_engine::engine::RENDERING.lock();

    open_export(&mut driver, &mut harness, (0, 0));
    let dest = out.to_string_lossy().into_owned();
    type_path(&mut driver, &mut harness, &dest);
    key(&mut driver, &mut harness, Key::Enter);
    assert!(harness.state().dialog().is_none());
    assert!(harness.state().exporting());
    // The menu has the item in its place, and it takes no press.
    right_click(&mut driver, &mut harness, 1);
    assert!(harness.state().menu_open());
    harness.get_by_label("Export photo…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    assert!(harness.state().menu_open(), "nothing was picked");
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().exporting());

    drop(rendering);
    driver.until(&mut harness, "the export over", |app| {
        !app.exporting() && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), [format!("Exported 1 photo to {dest}")]);
    assert_eq!(files(&out).len(), 1);
    // And now it can be asked for again.
    open_export(&mut driver, &mut harness, (1, 1));
}

// The library will not store the tick - it is held by another writer for longer than a
// write waits - and the box goes back to what is stored, under the engine's words: the
// box and the setting cannot disagree.
#[test]
fn a_tick_the_library_will_not_store_is_put_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, dirs, _, _) = three(dir.path());
    open_export(&mut driver, &mut harness, (0, 0));
    driver.until(&mut harness, "the stored choice read", |app| !app.busy());
    driver.settle(&mut harness);
    let other = rusqlite::Connection::open(&dirs.db_path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    harness.get_by_label("Apply edits to the copies").click();
    driver.act(&mut harness);
    assert!(!exporting(&harness).unwrap().apply_edits());
    driver.until(&mut harness, "the refusal", |app| !app.toasts().is_empty());
    other.execute_batch("ROLLBACK").unwrap();
    assert!(exporting(&harness).unwrap().apply_edits(), "put back");
    assert_eq!(said(&harness).len(), 1);
    assert!(
        Library::open(&dirs.db_path)
            .unwrap()
            .export_apply_edits()
            .unwrap()
    );
}

#[test]
fn a_window_with_the_exports_dialog_open_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    open_export(&mut driver, &mut harness, (0, 1));
    type_path(&mut driver, &mut harness, &out.to_string_lossy());
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(exporting(&harness).is_some());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A folder's name can end in a space, and a dropped folder's path is put in the field as
// the system gave it. Asked about without the spaces at its ends, as a typed path is,
// "out " dropped was the folder "out" beside it: the dialog passed that one, and the
// copies were written into a folder nobody had named.
#[cfg(unix)]
#[test]
fn a_dropped_folder_whose_name_ends_in_a_space_is_the_folder_exported_to() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    // "out" is there, made by `three`; "out " is the one dropped.
    let spaced = dir.path().join("out ");
    std::fs::create_dir_all(&spaced).unwrap();
    open_export(&mut driver, &mut harness, (0, 0));
    common::drop_on(&mut driver, &mut harness, &[&spaced]);
    driver.until(&mut harness, "the path asked about", |app| {
        matches!(app.dialog(), Some(Dialog::Export(export))
            if !matches!(export.dest(), Dest::Checking | Dest::Empty))
    });
    assert_eq!(
        exporting(&harness).unwrap().dest(),
        &Dest::Passed(spaced.to_string_lossy().into_owned())
    );
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the export over", |app| {
        !app.exporting() && !app.toasts().is_empty()
    });
    assert_eq!(files(&spaced), ["a.jpg"]);
    assert_eq!(
        files(&out),
        Vec::<String>::new(),
        "nothing was dropped there"
    );
    // Typed with a space after it, a path that has no folder of that very name is the
    // folder without the space: a path pasted from a terminal ends in one.
    std::fs::remove_file(spaced.join("a.jpg")).unwrap();
    std::fs::remove_dir(&spaced).unwrap();
    open_export(&mut driver, &mut harness, (0, 0));
    let dest = out.to_string_lossy().into_owned();
    assert_eq!(
        type_path(&mut driver, &mut harness, &format!("{dest} ")),
        Dest::Passed(dest)
    );
}

// A `~` typed is the home folder of whoever runs photon. Read and never written to: the
// path is asked about, and nothing is exported.
#[test]
fn a_tilde_typed_is_the_home_folder() {
    let Some(home) = ::dirs::home_dir().filter(|home| home.is_dir()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, _) = three(dir.path());
    open_export(&mut driver, &mut harness, (0, 0));
    assert_eq!(
        type_path(&mut driver, &mut harness, "~"),
        Dest::Passed(home.to_string_lossy().into_owned())
    );
}

// The destination passed while the dialog was open, and by the time the export begins
// it is no longer one to write into: gone, or watched since. The engine asks again
// before it writes anything, and what it says is the message - an error, with nothing
// written and the window ready for the next export.
#[test]
fn a_destination_gone_or_watched_since_it_passed_is_said_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    open_export(&mut driver, &mut harness, (0, 0));
    let dest = out.to_string_lossy().into_owned();
    assert!(matches!(
        type_path(&mut driver, &mut harness, &dest),
        Dest::Passed(_)
    ));
    std::fs::remove_dir(&out).unwrap();
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the export over", |app| {
        !app.exporting() && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness).len(), 1);
    assert!(
        !said(&harness)[0].starts_with("Exported"),
        "{:?}",
        said(&harness)
    );
    assert_eq!(harness.state().toasts()[0].kind, Kind::Error);
    assert!(!out.exists(), "and the folder was not made again");

    // Watched since.
    std::fs::create_dir_all(&out).unwrap();
    open_export(&mut driver, &mut harness, (0, 0));
    assert!(matches!(
        type_path(&mut driver, &mut harness, &dest),
        Dest::Passed(_)
    ));
    let engine = harness.state().engine().clone();
    commands::add_folder(&engine, &dest).unwrap();
    key(&mut driver, &mut harness, Key::Enter);
    // Waited for by what it says, not by how many messages stand: the first one's time
    // can be over by then - a wait longer than a moment lets the clock go on to it.
    let refused = |app: &photon_ui::app::App| {
        (app.toasts().iter())
            .find(|toast| toast.message.contains("is inside the watched folder"))
            .map(|toast| toast.kind)
    };
    driver.until(&mut harness, "the export refused", |app| {
        !app.exporting() && refused(app).is_some()
    });
    assert_eq!(refused(harness.state()), Some(Kind::Error));
    assert_eq!(files(&out), Vec::<String>::new());
}

// The export has a thread of its own, and is not on the queue of writes: a photo hidden
// while an export stands at its first photo is hidden at once.
#[test]
fn a_write_made_while_an_export_runs_does_not_wait_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _, out) = three(dir.path());
    let shown = harness.state().shown().to_vec();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, shown[0], true).unwrap();
    driver.settle(&mut harness);
    let rendering = photon_engine::engine::RENDERING.lock();
    open_export(&mut driver, &mut harness, (0, 0));
    type_path(&mut driver, &mut harness, &out.to_string_lossy());
    key(&mut driver, &mut harness, Key::Enter);
    assert!(harness.state().exporting());
    pick(&mut driver, &mut harness, 2, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::H);
    driver.until(&mut harness, "the photo hidden", |app| app.photos() == 2);
    assert!(harness.state().exporting(), "and the export still stands");
    drop(rendering);
    driver.until(&mut harness, "the export over", |app| !app.exporting());
}
