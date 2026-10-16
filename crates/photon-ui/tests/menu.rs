//! The tile's menu and the keys that write, through the whole application and the real
//! engine: a star is read back from the folder's own `.picasa.ini`, a hide and an album
//! from the library.
//!
//! Every write here goes into a library the test made, in a temporary directory.

use crate::common::{Driver, Harness, REFRESH, click, library, opened, photo_ids, sources};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    grid::GridView,
    library::Library,
};
use photon_engine::commands;
use photon_ui::{
    app::App,
    dirs::{self, Dirs},
    nav::{Place, Step},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn button(pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers,
    }
}

/// The middle of the photo at grid offset `offset`.
fn middle(harness: &Harness<'_>, offset: usize) -> Pos2 {
    (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center()
}

/// A click on the photo at `offset`, with `held` down.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = middle(harness, offset);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(button(at, PointerButton::Primary, true, held));
    harness.event(button(at, PointerButton::Primary, false, held));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

/// A click of the other button on the photo at `offset`, and the frame its menu is
/// first drawn in.
fn right_click(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = middle(harness, offset);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(harness);
    // A new area is measured in one frame and shown in the next.
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key, held: Modifiers) {
    harness.key_press_modifiers(held, key);
    driver.act(harness);
}

/// The words of the messages on screen.
fn said(harness: &Harness<'_>) -> Vec<String> {
    (harness.state().toasts().iter())
        .map(|toast| toast.message.clone())
        .collect()
}

/// A folder called Pictures under `dir` with two folders of real photos in it, `coast`
/// with three and `hills` with two, each a minute after the one before and `hills` a
/// year the newer: in the grid `hills` comes first.
fn pictures(dir: &Path) -> PathBuf {
    let root = dir.join("Pictures");
    let picture = std::fs::read(&sources(dir)[0]).unwrap();
    let day = Duration::from_secs(86_400);
    let start = SystemTime::UNIX_EPOCH + 19_000 * day;
    for (folder, names, days) in [
        ("coast", &["a.jpg", "b.jpg", "c.jpg"][..], 0),
        ("hills", &["d.jpg", "e.jpg"][..], 365),
    ] {
        let folder = root.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (n, name) in names.iter().enumerate() {
            let path = folder.join(name);
            // A byte more for each, so that none is another's copy.
            let mut bytes = picture.clone();
            bytes.extend(std::iter::repeat_n(0, n + 1));
            std::fs::write(&path, bytes).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(start + days * day + Duration::from_secs(60 * n as u64))
                .unwrap();
        }
    }
    root
}

/// The application over a library of its own making that watches `pictures`, once its
/// five photos are shown: `d`, `e`, then `a`, `b`, `c`.
fn over_files<'a>(dir: &Path, pictures: &Path) -> (Harness<'a>, Driver, Dirs) {
    let made = dirs::within(&dir.join("data"), &dir.join("cache"));
    let (dirs, pictures) = (made.clone(), pictures.to_owned());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| App::new(cc, dirs, Some(pictures)).unwrap());
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the five photos shown", |app| {
        app.photos() == 5 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    let names: Vec<String> = {
        let lib = Library::open(&made.db_path).unwrap();
        (harness.state().shown().iter())
            .map(|id| {
                let path = PathBuf::from(lib.item(*id).unwrap().unwrap().path);
                path.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect()
    };
    assert_eq!(names, ["d.jpg", "e.jpg", "a.jpg", "b.jpg", "c.jpg"]);
    (harness, driver, made)
}

/// The lines of a folder's `.picasa.ini`, or none when it has no such file.
fn ini(folder: &Path) -> Vec<String> {
    std::fs::read_to_string(folder.join(".picasa.ini"))
        .map(|text| text.lines().map(|line| line.trim().to_owned()).collect())
        .unwrap_or_default()
}

/// Whether `ini` stars the file called `name`.
fn stars(ini: &[String], name: &str) -> bool {
    let header = format!("[{name}]");
    (ini.iter().skip_while(|line| **line != header).skip(1))
        .take_while(|line| !line.starts_with('['))
        .any(|line| line == "star=yes")
}

/// Waits until the photos starred in the grid on screen are those at `offsets`.
fn until_starred(driver: &mut Driver, harness: &mut Harness<'_>, offsets: &[usize]) {
    let shown = harness.state().shown();
    let wanted: Vec<i64> = offsets.iter().map(|offset| shown[*offset]).collect();
    driver.until(harness, &format!("starred: {offsets:?}"), |app| {
        app.starred() == wanted
    });
}

// The one write photon makes outside its own library: a `star=` line in the folder's
// own `.picasa.ini`, under the file's name. From the menu, and taken off by the key.
#[test]
fn a_star_from_the_menu_is_in_the_folders_ini_and_on_the_tile_and_the_key_takes_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let root = pictures(dir.path());
    let (mut harness, mut driver, _) = over_files(dir.path(), &root);
    assert_eq!(ini(&root.join("coast")), Vec::<String>::new());

    // The photo `b`, which nothing had selected: the menu is of that photo.
    right_click(&mut driver, &mut harness, 3);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), [harness.state().shown()[3]]);
    harness.get_by_label("Star photo").click();
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    until_starred(&mut driver, &mut harness, &[3]);
    let coast = ini(&root.join("coast"));
    assert!(stars(&coast, "b.jpg"), "{coast:?}");
    assert!(
        !stars(&coast, "a.jpg") && !stars(&coast, "c.jpg"),
        "{coast:?}"
    );
    // The other folder was not written to.
    assert_eq!(ini(&root.join("hills")), Vec::<String>::new());
    assert_eq!(said(&harness), Vec::<String>::new());

    // Its menu now offers to take the star off, and the key does that.
    right_click(&mut driver, &mut harness, 3);
    assert!(harness.query_by_label("Unstar photo").is_some());
    assert!(harness.query_by_label("Star photo").is_none());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[]);
    assert!(!stars(&ini(&root.join("coast")), "b.jpg"));
    // And sets it again.
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[3]);
    assert!(stars(&ini(&root.join("coast")), "b.jpg"));
}

// One key for the menu's two items. Which it does is decided by one photo: the lead
// while it is selected, else the selected photo that comes first in the grid.
#[test]
fn the_star_key_takes_several_photos_the_way_of_the_deciding_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = pictures(dir.path());
    // Picasa starred `a` before photon ever saw the folder.
    std::fs::write(
        root.join("coast").join(".picasa.ini"),
        "[a.jpg]\r\nstar=yes\r\nkeywords=sea\r\n",
    )
    .unwrap();
    let (mut harness, mut driver, _) = over_files(dir.path(), &root);
    until_starred(&mut driver, &mut harness, &[2]);

    // From `e` in the other folder to `b`: three photos in two folders, `a` among them
    // and starred - but the lead is on `b`, which is not, and the key stars all three.
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    pick(&mut driver, &mut harness, 3, Modifiers::SHIFT);
    assert_eq!(harness.state().lead(), Some(harness.state().shown()[3]));
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[1, 2, 3]);
    // Back to `a` alone starred, by the menu's own item for one photo.
    for offset in [1, 3] {
        pick(&mut driver, &mut harness, offset, Modifiers::NONE);
        right_click(&mut driver, &mut harness, offset);
        harness.get_by_label("Unstar photo").click();
        driver.act(&mut harness);
    }
    until_starred(&mut driver, &mut harness, &[2]);

    // The same three, the lead on `a`, which is starred. The menu offers both, and the
    // key takes the stars off.
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    pick(&mut driver, &mut harness, 3, Modifiers::SHIFT);
    // Out and in again: the lead is on the photo last clicked.
    pick(&mut driver, &mut harness, 2, Modifiers::COMMAND);
    pick(&mut driver, &mut harness, 2, Modifiers::COMMAND);
    assert_eq!(harness.state().selected().len(), 3);
    assert_eq!(harness.state().lead(), Some(harness.state().shown()[2]));
    right_click(&mut driver, &mut harness, 2);
    assert!(harness.query_by_label("Star 3 photos").is_some());
    assert!(harness.query_by_label("Unstar 3 photos").is_some());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[]);
    let coast = ini(&root.join("coast"));
    assert!(!stars(&coast, "a.jpg"), "{coast:?}");
    // Every other byte of Picasa's file is kept.
    assert!(coast.contains(&"keywords=sea".to_owned()), "{coast:?}");

    // Now the deciding photo has none, and the same key stars all three, in both
    // folders' files.
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[1, 2, 3]);
    let (coast, hills) = (ini(&root.join("coast")), ini(&root.join("hills")));
    assert!(
        stars(&coast, "a.jpg") && stars(&coast, "b.jpg"),
        "{coast:?}"
    );
    assert!(!stars(&coast, "c.jpg"), "{coast:?}");
    assert!(
        stars(&hills, "e.jpg") && !stars(&hills, "d.jpg"),
        "{hills:?}"
    );
    assert_eq!(said(&harness), Vec::<String>::new());
}

// A folder photon may not write to: its photos keep no star, the others take theirs,
// and a message says how many did not.
#[cfg(unix)]
#[test]
fn a_folder_whose_ini_cannot_be_written_costs_a_message_and_not_the_other_photos() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = pictures(dir.path());
    let (mut harness, mut driver, _) = over_files(dir.path(), &root);
    let hills = root.join("hills");
    let allow = |mode| std::fs::set_permissions(&hills, PermissionsExt::from_mode(mode)).unwrap();
    allow(0o555);
    // Someone who may write anywhere is refused nothing: nothing to see.
    if std::fs::write(hills.join("probe"), b"").is_ok() {
        std::fs::remove_file(hills.join("probe")).unwrap();
        allow(0o755);
        return;
    }
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 4, Modifiers::SHIFT);
    assert_eq!(harness.state().selected().len(), 5);
    key(&mut driver, &mut harness, Key::Period, Modifiers::NONE);
    until_starred(&mut driver, &mut harness, &[2, 3, 4]);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    allow(0o755);
    assert_eq!(said(&harness), ["2 photos of 5 could not be starred"]);
    assert_eq!(ini(&hills), Vec::<String>::new());
    // The selection stands, for another try.
    assert_eq!(harness.state().selected().len(), 5);
}

// A star nothing could be written for: the engine's own words, and the selection kept.
// (A fixture's folders are on a drive that is not there.)
#[test]
fn a_write_the_engine_refuses_is_a_message_and_the_selection_stands() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 4);
    // From the keyboard: down past "Copy photo", the first item that works, to the
    // star, and Enter.
    key(&mut driver, &mut harness, Key::ArrowDown, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::ArrowDown, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    let said = said(&harness);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains(".picasa.ini"), "{said:?}");
    assert_eq!(harness.state().selected(), [ids[4]]);
    assert_eq!(harness.state().starred(), Vec::<i64>::new());
}

// The photos leave the grid, and the selection lands on the photo after them: `H` again
// goes on from there. Nothing is told to the grid: its rebuild arrives like any other.
#[test]
fn a_hide_takes_the_photos_out_and_lands_on_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);

    // From the menu.
    right_click(&mut driver, &mut harness, 4);
    harness.get_by_label("Hide photo").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "one hidden", |app| app.photos() == 899);
    assert_eq!(harness.state().selected(), [ids[5]]);
    assert_eq!(harness.state().lead(), Some(ids[5]));
    assert!(!harness.state().shown().contains(&ids[4]));
    // From the key, twice: each from where the last one landed.
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    driver.until(&mut harness, "two hidden", |app| app.photos() == 898);
    assert_eq!(harness.state().selected(), [ids[6]]);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    driver.until(&mut harness, "three hidden", |app| app.photos() == 897);
    assert_eq!(harness.state().selected(), [ids[7]]);
    // Several at once, and the last photo of the library among them: the one before.
    key(&mut driver, &mut harness, Key::End, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(harness.state().selected(), ids[898..]);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    driver.until(&mut harness, "five hidden", |app| app.photos() == 895);
    assert_eq!(harness.state().selected(), [ids[897]]);
    assert_eq!(said(&harness), Vec::<String>::new());

    let lib = Library::open(&library.dirs().db_path).unwrap();
    let hidden = |id: i64| lib.item(id).unwrap().unwrap().hidden;
    assert!([4, 5, 6, 898, 899].iter().all(|n| hidden(ids[*n])));
    assert!(!hidden(ids[7]) && !hidden(ids[897]));
}

// A view is switched while a hide is on its way: the write is made all the same, says
// nothing, and nothing of it is left in the view that was gone to.
#[test]
fn a_write_whose_view_was_left_meanwhile_is_made_and_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    harness.key_press_modifiers(Modifiers::NONE, Key::H);
    driver.act(&mut harness);
    harness.state_mut().go(Step::View(GridView::Recent));
    driver.until(
        &mut harness,
        "the other view, and the hide counted",
        |app| {
            app.settled() == &Place::of(GridView::Recent) && !app.busy() && app.counts().hidden == 1
        },
    );
    driver.settle(&mut harness);
    assert!(harness.state().selected().is_empty());
    assert_eq!(said(&harness), Vec::<String>::new());
    assert!(!harness.state().shown().contains(&ids[4]));
    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "the library without it", |app| {
        app.settled() == &Place::of(GridView::All) && app.photos() == 899
    });
    assert!(harness.state().selected().is_empty());
}

// In Hidden the same key and the same place in the menu bring a photo back.
#[test]
fn in_hidden_the_key_and_the_menu_bring_photos_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 3);
    let (mut harness, mut driver) = opened(&library, 897);
    click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photos shown", |app| {
        app.settled() == &Place::of(GridView::Hidden) && app.photos() == 3
    });
    let hidden = harness.state().shown();
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    driver.until(&mut harness, "one brought back", |app| app.photos() == 2);
    assert_eq!(harness.state().selected(), [hidden[1]]);
    right_click(&mut driver, &mut harness, 0);
    assert!(harness.query_by_label("Hide photo").is_none());
    harness.get_by_label("Unhide photo").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "two brought back", |app| app.photos() == 1);
    assert_eq!(harness.state().selected(), [hidden[2]]);
    let lib = Library::open(&library.dirs().db_path).unwrap();
    assert_eq!(lib.grid_entries().unwrap().len(), 899);
}

/// An album of the user's own in `library`, made before the application is launched.
fn album(library: &photon_ui::fixture::Fixture, name: &str) -> i64 {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.create_album(name, 1_700_000_000_000).unwrap().id
}

fn album_count(app: &App, id: i64) -> Option<i64> {
    (app.collections().albums.iter())
        .find(|album| album.id == id)
        .map(|album| album.count)
}

// The albums of the user's own are at the end of the menu; a photo put into one shows
// in the sidebar's count, the engine announcing nothing for an album that is not on
// screen. In the album, the menu takes photos out again, and the selection lands.
#[test]
fn photos_are_put_into_an_album_from_the_menu_and_taken_out_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let lisbon = album(&library, "Lisbon");
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_count(app, lisbon) == Some(0)
    });
    pick(&mut driver, &mut harness, 2, Modifiers::NONE);
    pick(&mut driver, &mut harness, 4, Modifiers::SHIFT);
    right_click(&mut driver, &mut harness, 3);
    assert_eq!(harness.state().selected(), ids[2..=4]);
    // The sidebar has a row of that name too: the menu's is the one over the photos.
    let rows = harness.query_all_by_label("Lisbon").collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    (rows.iter())
        .max_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap()
        .click();
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    driver.until(&mut harness, "three in the album", |app| {
        album_count(app, lisbon) == Some(3)
    });
    // The photos are where they were, selected as they were.
    assert_eq!(harness.state().photos(), 900);
    assert_eq!(harness.state().selected(), ids[2..=4]);

    // In the album.
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    harness.get_by_label("Lisbon").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the album shown", |app| {
        app.settled().view == GridView::Album && app.photos() == 3
    });
    let shown = harness.state().shown();
    right_click(&mut driver, &mut harness, 0);
    harness.get_by_label("Remove photo from “Lisbon”").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "two left in it", |app| app.photos() == 2);
    assert_eq!(harness.state().selected(), [shown[1]]);
    driver.until(&mut harness, "two in the album", |app| {
        album_count(app, lisbon) == Some(2)
    });
    assert_eq!(said(&harness), Vec::<String>::new());
}

// In a person's view the menu takes photos off them: "Not this person" for every face
// of theirs on each, and a message saying what came of it.
#[test]
fn photos_are_taken_from_a_person_in_their_view() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 6, 0, 0);
    let ids = photo_ids(&library);
    {
        // photon has looked at every photo, found a face on the first two, and the user
        // has named both Anna.
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.set_face_detection(true).unwrap();
        let looked = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
        assert_eq!(looked.len(), 6);
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
                let faces = if ids[..2].contains(&photo.id) {
                    vec![face.clone()]
                } else {
                    Vec::new()
                };
                (photo, faces)
            })
            .collect();
        lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
        assert_eq!(lib.name_items(&ids[..2], "Anna").unwrap().named, 2);
    }
    let (mut harness, mut driver) = opened(&library, 6);
    driver.until(&mut harness, "the people read", |app| {
        !app.collections().people.is_empty()
    });
    click(&mut driver, &mut harness, "Show people");
    driver.act(&mut harness);
    click(&mut driver, &mut harness, "Anna");
    driver.until(&mut harness, "Anna's photos shown", |app| {
        app.settled().view == GridView::Person && app.photos() == 2
    });
    let shown = harness.state().shown();
    right_click(&mut driver, &mut harness, 0);
    harness.get_by_label("Remove photo from “Anna”").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "one photo left with her", |app| {
        app.photos() == 1 && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Removed 1 photo from Anna."]);
    assert_eq!(harness.state().selected(), [shown[1]]);
}

// The other button going down on a photo takes the keyboard off whatever had it, as a
// click on a photo does, and the keys are the grid's again when the menu has closed.
// At the press: egui lets go of a text field at a click elsewhere, and a button that is
// held, or let go of after the pointer has moved, is none.
#[test]
fn a_right_click_takes_the_keyboard_from_the_search_box() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    key(&mut driver, &mut harness, Key::F, Modifiers::COMMAND);
    driver.act(&mut harness);
    assert!(harness.ctx.memory(|memory| memory.focused()).is_some());
    let at = middle(&harness, 4);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.ctx.memory(|memory| memory.focused()), None);
    // Let go of somewhere else on the menu's own side of the pointer: no click.
    let away = at + egui::vec2(60.0, 60.0);
    harness.event(egui::Event::PointerMoved(away));
    harness.event(button(
        away,
        PointerButton::Secondary,
        false,
        Modifiers::NONE,
    ));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.ctx.memory(|memory| memory.focused()), None);
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    key(&mut driver, &mut harness, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[5]]);
    assert_eq!(harness.state().search_text(), "");
}

// A video is no picture for the clipboard: its menu has no "Copy photo". And the menu
// of several photos is told which of them decides, not that it is one photo's.
#[test]
fn the_menu_is_of_the_photo_it_was_opened_on() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    {
        let made = rusqlite::Connection::open(&library.dirs().db_path).unwrap();
        let changed = made
            .execute("UPDATE items SET kind = 1 WHERE id = ?1", [ids[1]])
            .unwrap();
        assert_eq!(changed, 1);
    }
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 0);
    assert!(harness.query_by_label("Copy photo").is_some());
    assert!(harness.query_by_label("Reveal in file manager").is_some());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    right_click(&mut driver, &mut harness, 1);
    assert_eq!(harness.state().selected(), [ids[1]]);
    assert!(harness.query_by_label("Copy photo").is_none());
    assert!(harness.query_by_label("Reveal in file manager").is_some());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    // Both, and a third: no item of one photo's.
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 2, Modifiers::SHIFT);
    right_click(&mut driver, &mut harness, 1);
    assert_eq!(harness.state().selected(), ids[0..=2]);
    assert!(harness.query_by_label("Reveal in file manager").is_none());
    assert!(harness.query_by_label("Star 3 photos").is_some());
    assert!(harness.query_by_label("Unstar 3 photos").is_some());
    assert!(harness.query_by_label("Hide 3 photos").is_some());
}

// The menu is drawn in the frame after the one it was asked for in, and nothing else
// asks for that frame: the pointer rests and no key is down.
#[test]
fn a_menu_asked_for_is_drawn_without_anything_more_happening() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    let at = middle(&harness, 4);
    harness.event(egui::Event::PointerMoved(at));
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert!(harness.query_by_label("Hide photo").is_none());
    driver.frames_in(&mut harness, 1.0);
    assert!(harness.query_by_label("Hide photo").is_some());
}

// While a menu is open the keys are its own: nothing behind it hears one.
#[test]
fn no_key_reaches_what_lies_behind_an_open_menu() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 4);
    let hidden = harness.state().layout().sidebar_hidden;
    for (pressed, held) in [
        (Key::H, Modifiers::NONE),
        (Key::Period, Modifiers::NONE),
        (Key::ArrowRight, Modifiers::NONE),
        (Key::End, Modifiers::SHIFT),
        (Key::A, Modifiers::COMMAND),
        (Key::B, Modifiers::COMMAND),
        (Key::F, Modifiers::COMMAND),
        (Key::Tab, Modifiers::NONE),
    ] {
        key(&mut driver, &mut harness, pressed, held);
        assert!(harness.state().menu_open(), "{pressed:?}");
        assert_eq!(harness.state().selected(), [ids[4]], "{pressed:?}");
    }
    harness.event(egui::Event::Text("mountains".to_owned()));
    driver.act(&mut harness);
    assert_eq!(harness.state().search_text(), "");
    assert_eq!(harness.state().layout().sidebar_hidden, hidden);
    driver.settle(&mut harness);
    assert_eq!(harness.state().photos(), 900);
    assert_eq!(said(&harness), Vec::<String>::new());
    // Escape closes the menu, and is not the grid's as well: the photo stays selected.
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[4]]);
    // And the keys are the grid's again.
    key(&mut driver, &mut harness, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[5]]);
}

// A second right-click moves the menu to another photo; a press of the first button
// beside it closes it and is that photo's click as well.
#[test]
fn a_press_elsewhere_closes_the_menu_and_a_right_click_elsewhere_moves_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 0);
    assert_eq!(harness.state().selected(), [ids[0]]);
    // The other button on the menu itself is nobody's: not the photo's under it.
    let on_menu = harness.get_by_label("Hide photo").rect().center();
    harness.event(egui::Event::PointerMoved(on_menu));
    harness.event(button(
        on_menu,
        PointerButton::Secondary,
        true,
        Modifiers::NONE,
    ));
    harness.event(button(
        on_menu,
        PointerButton::Secondary,
        false,
        Modifiers::NONE,
    ));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[0]]);
    assert!(harness.query_by_label("Hide photo").is_some());
    // The keys go into it, and a menu opened somewhere else starts over: Enter picks
    // nothing in it.
    key(&mut driver, &mut harness, Key::ArrowDown, Modifiers::NONE);
    // The last photo of the first row: beside the first menu, which opened down and to
    // the right of its photo.
    let top = harness.state().tile_rect(0).unwrap().top();
    let far = (1..20)
        .take_while(|offset| harness.state().tile_rect(*offset).unwrap().top() == top)
        .last()
        .unwrap();
    assert!(far >= 2);
    right_click(&mut driver, &mut harness, far);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[far]]);
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().menu_open());
    driver.settle(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[0]]);
}

// A menu is of the photos selected when it was opened. A grid of other results closes
// it; so does the photo it was opened on leaving the grid, when the selection lands on
// its neighbour - the items would act on a photo nobody asked about. A grid that only
// loses another photo leaves it open.
#[test]
fn a_menu_does_not_outlive_the_photos_it_is_of() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    let engine = harness.state().engine().clone();
    right_click(&mut driver, &mut harness, 4);
    // Another photo is hidden from elsewhere: a scan would do the same.
    commands::set_items_hidden(&engine, &[ids[8]], true).unwrap();
    driver.until(&mut harness, "one hidden", |app| app.photos() == 899);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[4]]);
    // The photo itself.
    commands::set_items_hidden(&engine, &[ids[4]], true).unwrap();
    driver.until(&mut harness, "two hidden", |app| app.photos() == 898);
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[5]]);

    // Another view.
    right_click(&mut driver, &mut harness, 2);
    assert!(harness.state().menu_open());
    harness.state_mut().go(Step::View(GridView::Recent));
    driver.until(&mut harness, "the other view shown", |app| {
        app.settled() == &Place::of(GridView::Recent)
    });
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    assert!(harness.state().selected().is_empty());
}

// The grid loses every photo the menu was of and has none to land on: nothing is
// selected, and a menu of nothing is no menu.
#[test]
fn a_menu_closes_when_nothing_is_left_selected() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 1);
    let (mut harness, mut driver) = opened(&library, 899);
    let engine = harness.state().engine().clone();
    click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photo shown", |app| {
        app.settled() == &Place::of(GridView::Hidden) && app.photos() == 1
    });
    let hidden = harness.state().shown();
    right_click(&mut driver, &mut harness, 0);
    assert!(harness.state().menu_open());
    commands::set_items_hidden(&engine, &hidden, false).unwrap();
    driver.until(&mut harness, "none hidden", |app| app.photos() == 0);
    driver.act(&mut harness);
    assert!(harness.state().selected().is_empty());
    assert!(!harness.state().menu_open());
}

// ---- from the review ----

fn key_down(key: Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

// The menu opens with its corner at the pointer, and the pointer rests there. A press
// of the first button at that very point is inside the menu, by the menu's own rule,
// and was the photo's under it as well: egui lets a press through to the layer below
// wherever no widget of the layer above covers all of the five points round the pointer,
// which is all along the inside of a menu's edge.
#[test]
fn a_click_where_the_menu_opened_is_not_also_the_photos_under_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    pick(&mut driver, &mut harness, 3, Modifiers::SHIFT);
    right_click(&mut driver, &mut harness, 2);
    driver.settle(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), ids[1..=3]);

    // The pointer has not moved since the menu opened under it.
    let at = middle(&harness, 2);
    harness.event(button(at, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), ids[1..=3]);
    // Nor the other button's: the menu is not opened again on the photo under it.
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().selected(), ids[1..=3]);
}

// The same, as a drag: a band under an open menu selects other photos than the ones
// the menu is about.
#[test]
fn a_drag_from_where_the_menu_opened_draws_no_band_under_the_open_menu() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 2);
    driver.settle(&mut harness);
    assert!(harness.state().menu_open());

    let (from, to) = (middle(&harness, 2), middle(&harness, 0));
    harness.event(button(from, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(egui::Event::PointerMoved(to));
    driver.act(&mut harness);
    assert!(!harness.state().last_frame().unwrap().banding);
    assert_eq!(harness.state().selected(), [ids[2]]);
    harness.event(button(to, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[2]]);
}

// The keys of a frame are answered in order, and a key that writes acts on what was
// selected when it was pressed: `H` and then an arrow in one frame - a long frame, the
// one that puts a rebuilt grid of a large library on screen - hid the photo the arrow
// went to, and left the one `H` was pressed on.
#[test]
fn a_key_that_writes_acts_on_what_was_selected_when_it_was_pressed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[4]]);
    // Both in one frame's input, `H` first.
    harness.input_mut().events.push(key_down(Key::H));
    harness.input_mut().events.push(key_down(Key::ArrowRight));
    driver.act(&mut harness);
    driver.until(&mut harness, "one hidden", |app| app.photos() == 899);
    let shown = harness.state().shown();
    assert!(!shown.contains(&ids[4]) && shown.contains(&ids[5]));
}

// Which way `H` goes, and what a menu offers, is decided by the view of the grid on
// screen, whose photos are the ones selected - not by the view a step on its way leads
// to. The seam is the library's write lock: a sort is rebuilt and then stored, so it
// stands at its store, and the switch to Hidden waits its turn behind it.
#[test]
fn the_hide_key_and_the_menu_go_by_the_view_on_screen_not_by_the_one_being_gone_to() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    driver.settle(&mut harness);

    let other = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let sort = photon_core::sort::Sort {
        reverse: true,
        ..harness.state().sort()
    };
    harness.state_mut().go(Step::Sort(sort));
    harness.state_mut().go(Step::View(GridView::Hidden));
    driver.act(&mut harness);
    // All photos is on screen, with its photo selected; Hidden is where the user is going.
    assert_eq!(harness.state().settled(), &Place::of(GridView::All));
    assert_eq!(harness.state().place(), Place::of(GridView::Hidden));
    assert_eq!(harness.state().selected(), [ids[4]]);
    right_click(&mut driver, &mut harness, 4);
    assert!(harness.query_by_label("Hide photo").is_some());
    assert!(harness.query_by_label("Unhide photo").is_none());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    other.execute_batch("ROLLBACK").unwrap();

    driver.until(&mut harness, "Hidden shown", |app| {
        app.settled() == &Place::of(GridView::Hidden) && !app.busy()
    });
    driver.settle(&mut harness);
    // The photo was hidden: it was a photo of All photos that `H` was pressed on.
    assert_eq!(harness.state().shown(), [ids[4]]);
}

// A hide pressed in the instant before the window is closed is made: the window waits
// for its writes. The first of two stands at the library's write lock, held from
// outside, and the second waits its turn behind it; the window is closed over both.
#[test]
fn a_write_asked_for_just_before_the_window_closes_is_made() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    let other = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    other.busy_timeout(Duration::from_secs(20)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    pick(&mut driver, &mut harness, 8, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::H, Modifiers::NONE);
    // The lock is let go of a moment after the window has begun to close. (Should this
    // thread be held up for longer than that moment, the writes are made before the
    // window closes, and the test says less: it does not fail for it.)
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        other.execute_batch("ROLLBACK").unwrap();
    });
    drop(harness);
    release.join().unwrap();
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let hidden = |id: i64| lib.item(id).unwrap().unwrap().hidden;
    assert!(hidden(ids[4]) && hidden(ids[8]));
}

// A menu taller than the window: the wheel over it scrolls the menu, and the photos
// under it stay where they are.
#[test]
fn the_wheel_over_a_menu_does_not_move_the_photos_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    for n in 0..60 {
        album(&library, &format!("Album {n:02}"));
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        app.collections().albums.len() == 60
    });
    right_click(&mut driver, &mut harness, 1);
    driver.settle(&mut harness);
    assert!(harness.query_by_label("Hide photo").is_some());
    let on_screen = harness.state().last_frame().unwrap().on_screen.clone();
    let over = middle(&harness, 1) + egui::vec2(40.0, 40.0);
    harness.event(egui::Event::PointerMoved(over));
    for _ in 0..5 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -300.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert!(harness.state().menu_open());
    assert_eq!(harness.state().last_frame().unwrap().on_screen, on_screen);
    // Beside the menu the wheel is the photos'.
    let beside = middle(&harness, 0);
    harness.event(egui::Event::PointerMoved(beside));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_ne!(harness.state().last_frame().unwrap().on_screen, on_screen);
}

#[test]
fn a_window_with_a_menu_open_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    right_click(&mut driver, &mut harness, 4);
    driver.settle(&mut harness);
    // The pointer's own echo, and the report of what is in view.
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.state().menu_open());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
