//! The People page, through the whole application and the real engine: opened from the
//! sidebar, its sections read from a library the test made, its crops cut from the cached
//! previews, a group named, a face's photo opened, and the page left.
//!
//! Every write here goes into a library the test made, in a temporary directory, and none
//! of them leaves `library.db`: a name is never written to a photo or an INI.

use crate::common::{Driver, Harness, click as row, library, opened, photo_ids, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{
    face_detect::{DETECTOR_VERSION, Detection, Rect},
    face_embed::{DIM, EMBEDDER_VERSION},
    grid::GridView,
    library::Library,
};
use photon_ui::{app::App, dialogs::Dialog, fixture::Fixture, settings::Section};

/// photon has looked at every photo of `library` and found one face on each photo of
/// `groups`, embedded so that the library's own grouping puts each group's faces
/// together: each group's a vector of its own, at right angles to every other's.
fn grouped(library: &Fixture, groups: &[&[i64]]) -> Library {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.set_face_detection(true).unwrap();
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
    let with_face: Vec<i64> = groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .collect();
    let photos = lib.face_candidates(0, 1000, DETECTOR_VERSION).unwrap();
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let faces = if with_face.contains(&photo.id) {
                vec![face.clone()]
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    let candidates = lib.embed_candidates(0, 1000, EMBEDDER_VERSION).unwrap();
    let batch: Vec<_> = (candidates.into_iter())
        .map(|candidate| {
            let group = groups
                .iter()
                .position(|group| group.contains(&candidate.item_id))
                .unwrap();
            let mut vector = [0.0_f32; DIM];
            vector[group] = 1.0;
            let faces = (candidate.faces.iter())
                .map(|(id, _)| (*id, Some(vector)))
                .collect();
            (candidate, faces)
        })
        .collect();
    lib.write_embeddings(&batch, EMBEDDER_VERSION).unwrap();
    lib.group_ungrouped_faces(&|| false).unwrap();
    lib
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

/// Opens the page from the label beside People's fold, and waits for it to be read.
fn open_page(driver: &mut Driver, harness: &mut Harness<'_>) {
    row(driver, harness, "People");
    assert!(harness.state().people_open(), "the page is open");
    driver.until(harness, "the page read", |app| {
        app.people_page().is_some_and(|page| page.page().is_some())
    });
    driver.act(harness);
}

/// Whether the sidebar's row called `label` says it is where the user is.
fn marked(harness: &Harness<'_>, label: &str) -> bool {
    (harness.query_all_by_label(label))
        .any(|node| node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True))
}

/// The first unnamed group's faces, as the page lists them.
fn first_group(app: &App) -> Vec<(i64, i64, String)> {
    let page = app.people_page().unwrap();
    let group = page.unnamed()[0];
    (group.faces.iter())
        .map(|face| (face.id, face.item_id, face.thumb_key.clone()))
        .collect()
}

// The page opens from the label beside People's fold: its sections, read from the
// library, and each face's crop, cut from the cached preview. People's heading is where
// the user is, and no view's row.
#[test]
fn the_page_opens_from_the_sidebar_with_its_groups_and_their_crops() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5], &ids[5..6]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    assert!(marked(&harness, "All photos"));
    open_page(&mut driver, &mut harness);
    driver.seen(&mut harness, "Unnamed");
    driver.seen(&mut harness, "2 groups");
    driver.seen(&mut harness, "1 single face");
    assert!(
        marked(&harness, "People"),
        "People's heading is where the user is"
    );
    assert!(!marked(&harness, "All photos"), "no view's row is");
    let faces = first_group(harness.state());
    assert_eq!(faces.len(), 3);
    driver.until(&mut harness, "the first group's crops", |app| {
        (faces.iter()).all(|(face, _, key)| app.people_crop(*face, key))
    });
    // The grid's own controls are not drawn over the page.
    assert!(harness.query_by_label_contains("Sort by").is_none());
}

// A name typed for a group says what Enter will do, and Enter names the group: it leaves
// Unnamed at once, and the page read again lists the person. A name someone has is said
// to merge before it is committed.
#[test]
fn a_group_is_named_from_its_box() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let field = |harness: &Harness<'_>| {
        let node = harness
            .query_all_by_label("Name this group")
            .next()
            .unwrap();
        node.rect().center()
    };
    let at = field(&harness);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "Anna");
    driver.seen(&mut harness, "New person “Anna” · Enter");
    key(&mut driver, &mut harness, Key::Enter);
    // At once, before the write has landed.
    assert_eq!(harness.state().people_page().unwrap().unnamed().len(), 1);
    driver.until(&mut harness, "Anna listed", |app| {
        let page = app.people_page().unwrap();
        (page.people().iter()).any(|person| person.name.as_deref() == Some("Anna"))
            && page.unnamed().len() == 1
    });
    driver.seen(&mut harness, "People · 1");
    // The other group, named "anna": a merge, said before Enter.
    driver.act(&mut harness);
    let at = field(&harness);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "anna");
    driver.seen(&mut harness, "Add to Anna · Enter");
    // Nothing leaves `library.db`.
    let lib = Library::open(&library.dirs().db_path).unwrap();
    assert_eq!(lib.named_people().unwrap().len(), 1);
}

// A double click on a face opens its photo in the viewer, over the page; closed, the
// viewer leaves the page where it was. From a grid whose view lacks the photo - a search
// that finds nothing - the grid is switched to All photos for it.
#[test]
fn a_faces_photo_opens_in_the_viewer_over_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    // A search that finds nothing, then the page over it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "nothing-is-called-this");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the search's empty grid", |app| {
        app.settled().view == GridView::Search && app.photos() == 0
    });
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let (_, photo, _) = first_group(harness.state())[0].clone();
    let face = harness
        .query_all_by_label_contains("Unnamed group: face 1 of")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(face));
    for _ in 0..2 {
        harness.event(button(face, PointerButton::Primary, true));
        harness.event(button(face, PointerButton::Primary, false));
        driver.act(&mut harness);
    }
    driver.until(&mut harness, "the photo in the viewer", |app| {
        app.viewer().is_some_and(|viewer| viewer.id() == photo)
    });
    assert_eq!(harness.state().settled().view, GridView::All);
    assert!(
        harness.state().people_open(),
        "the page stays under the viewer"
    );
    driver.until(&mut harness, "the photo's facts", |app| {
        app.viewer_item().is_some()
    });
    key(&mut driver, &mut harness, Key::Escape);
    assert!(harness.state().viewer().is_none());
    assert!(harness.state().people_open(), "back on the page");
}

// Any row of the sidebar leaves the page for the grid, and so does typing in the search
// box. The grid is where it was.
#[test]
fn a_row_of_the_sidebar_or_typing_a_search_leaves_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    row(&mut driver, &mut harness, "All photos");
    assert!(!harness.state().people_open());
    driver.act(&mut harness);
    assert!(marked(&harness, "All photos"));
    open_page(&mut driver, &mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "a");
    assert!(!harness.state().people_open());
}

// With Find faces off the page says so, and its button opens Settings at People.
#[test]
fn switched_off_the_page_says_so_and_opens_settings() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    row(&mut driver, &mut harness, "People");
    driver.seen(
        &mut harness,
        "photon finds and groups faces only while Find faces is on.",
    );
    harness.get_by_label("Open Settings").click();
    driver.act(&mut harness);
    assert!(matches!(
        harness.state().dialog(),
        Some(Dialog::Settings(settings)) if settings.section == Section::People
    ));
}

// A row whose view is another than the grid's, as much as All photos: the page is left.
#[test]
fn another_views_row_leaves_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    row(&mut driver, &mut harness, "Recent");
    assert!(!harness.state().people_open());
    driver.until(&mut harness, "Recent shown", |app| {
        app.settled().view == GridView::Recent
    });
}

// A face whose photo has left the library since the page was read opens nothing, and
// says so.
#[test]
fn a_face_whose_photo_is_gone_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let (_, photo, _) = first_group(harness.state())[0].clone();
    // Gone from the grid, and the page not read again - its read fails, the faces' table
    // renamed out from under it by a second connection - so its face is still listed.
    let engine = harness.state().engine().clone();
    engine.lib.set_hidden(&[photo], true).unwrap();
    let other = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    other
        .execute_batch("ALTER TABLE detected_faces RENAME TO detected_faces_away")
        .unwrap();
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "the photo gone from the grid", |app| {
        !app.shown().contains(&photo)
    });
    let face = harness
        .query_all_by_label_contains("Unnamed group: face 1 of")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(face));
    for _ in 0..2 {
        harness.event(button(face, PointerButton::Primary, true));
        harness.event(button(face, PointerButton::Primary, false));
        driver.act(&mut harness);
    }
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none());
    assert!(said(&harness).contains(&"That photo is no longer in the library.".to_owned()));
}

// A group named elsewhere - the viewer, the grid, another window of the library - is a
// change of the library's data, and the page reads itself again.
#[test]
fn the_page_follows_a_change_made_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let group = harness.state().people_page().unwrap().unnamed()[0].id;
    let engine = harness.state().engine().clone();
    photon_engine::commands::name_person(&engine, group, "Ben").unwrap();
    driver.until(&mut harness, "Ben on the page", |app| {
        let page = app.people_page().unwrap();
        (page.people().iter()).any(|person| person.name.as_deref() == Some("Ben"))
    });
}

// a write is not a view asked for. Deleting the album the grid under the page
// shows sends the grid to All photos, as it must - but through `go`, which leaves the
// People page: the user, on the page, deleted an album from the sidebar's menu and was
// taken off the page to a grid they had not asked for. The Svelte page stays (its
// `deleteAlbum` switches no page), and the grid under it is All photos.
#[test]
fn deleting_the_album_under_the_page_leaves_the_page_open() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    let album = lib.create_album("Lisbon", 1_700_000_000_000).unwrap().id;
    lib.add_to_album(album, &ids[..3], 1_700_000_000_000)
        .unwrap();
    drop(lib);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    row(&mut driver, &mut harness, "Lisbon");
    driver.until(&mut harness, "the album shown", |app| {
        app.settled().view == GridView::Album && app.photos() == 3
    });
    open_page(&mut driver, &mut harness);
    // The other button on the album's row, its menu's "Delete…", and yes.
    let at = harness.get_by_label("Lisbon").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true));
    harness.event(button(at, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    harness.get_by_label("Delete…").click();
    for _ in 0..4 {
        driver.act(&mut harness);
    }
    driver.frames_in(&mut harness, 0.4);
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "All photos under the page", |app| {
        app.settled().view == GridView::All && !app.busy()
    });
    assert!(
        harness.state().people_open(),
        "the page the user was on is left by a write"
    );
}

// a folder clicked in the sidebar leaves the page, from All photos too, where the
// click asks for no step and `go` - which leaves the page for every other row - is never
// reached. No test held it: the page stayed up with the line taken out.
#[test]
fn a_folder_clicked_from_all_photos_leaves_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    driver.until(&mut harness, "the folders read", |app| {
        !app.folders().is_empty()
    });
    open_page(&mut driver, &mut harness);
    // A folder of photos, as its row in the sidebar names it.
    let name = (harness.state().folders().values())
        .map(|folder| folder.name.clone())
        .find(|name| harness.query_by_label(name).is_some())
        .unwrap();
    row(&mut driver, &mut harness, &name);
    assert!(!harness.state().people_open(), "the page is left");
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

/// A click at `at`, its move, press and release in one frame.
fn click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(at),
        button(at, PointerButton::Primary, true),
        button(at, PointerButton::Primary, false),
    ]);
    driver.act(harness);
}

// compare asked for in the grid and answered once the People page is open does not
// go up over the page (`free` asks for no page). No test held that line.
#[test]
fn compare_answered_under_the_page_does_not_go_up() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    driver.until(&mut harness, "photos laid out", |app| {
        app.tile_rect(1).is_some()
    });
    let adding = if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    };
    for (offset, held) in [(0, Modifiers::NONE), (1, adding)] {
        let at = harness.state().tile_rect(offset).unwrap().center();
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(at),
            egui::Event::ModifiersChanged(held),
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: held,
            },
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: held,
            },
            egui::Event::ModifiersChanged(Modifiers::NONE),
        ]);
        driver.act(&mut harness);
    }
    harness
        .input_mut()
        .events
        .extend([key_event(Key::C, true), key_event(Key::C, false)]);
    driver.act(&mut harness);
    // The library answers while no frame is drawn; the next frame is the click on People.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let people = harness.get_by_label("People").rect().center();
    click_at(&mut driver, &mut harness, people);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().people_open());
    assert!(
        harness.state().compare().is_none(),
        "compare went up over the page"
    );
}

// a search half typed when the page is opened is dropped (`open_people`'s
// `search.cancel()`), or it lands behind the page and switches the grid where nobody is
// looking. No test held that line.
#[test]
fn a_search_half_typed_is_dropped_when_the_page_opens() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "IMG");
    let people = harness.get_by_label("People").rect().center();
    click_at(&mut driver, &mut harness, people);
    assert!(harness.state().people_open());
    driver.frames_in(&mut harness, 2.0);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().settled().view,
        GridView::All,
        "the search landed behind the page"
    );
    assert!(!harness.state().busy());
}

// Escape in the search box clears the search; in the Svelte UI that switches the
// grid behind the People page and leaves the page where it is (`searchBox.clear()` calls
// no `showGrid`, only typing does). Here `SearchAction::Clear` leaves the page, with no
// word of why - an Escape on a box focused by mistake (`/`, Ctrl+F) took the user off the
// page.
#[test]
fn escape_in_the_search_box_keeps_the_page_as_the_svelte_page_does() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "IMG");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the search shown", |app| {
        app.settled().view == GridView::Search && !app.busy()
    });
    open_page(&mut driver, &mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Escape);
    assert!(
        harness.state().people_open(),
        "Escape in the box left the page"
    );
}

// in the frame a dialog is first drawn in, egui keeps nothing under it from the
// pointer, and what the page answers in that frame is dropped (`pressed` under
// `under_dialog`). No test held that line: a face clicked once, a dialog opened from the
// sidebar's menu, and the face's second click in the dialog's first frame opened its photo
// in the viewer under the question.
#[test]
fn a_click_on_the_page_in_a_dialogs_first_frame_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    lib.create_album("Lisbon", 1_700_000_000_000).unwrap();
    drop(lib);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    driver.seen(&mut harness, "Lisbon");
    open_page(&mut driver, &mut harness);
    let face = harness
        .query_all_by_label_contains("Unnamed group: face 1 of")
        .next()
        .unwrap()
        .rect()
        .center();
    click_at(&mut driver, &mut harness, face);
    let album = harness.get_by_label("Lisbon").rect().center();
    harness.event(egui::Event::PointerMoved(album));
    harness.event(button(album, PointerButton::Secondary, true));
    harness.event(button(album, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    let item = harness.get_by_label("Delete…").rect().center();
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true));
    harness.event(button(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_some());
    // The dialog's first frame: the face's second click.
    click_at(&mut driver, &mut harness, face);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert!(
        harness.state().viewer().is_none(),
        "the viewer opened under the dialog"
    );
}

/// Types `name` into the first group's box and presses Enter.
fn name_first_group(driver: &mut Driver, harness: &mut Harness<'_>, name: &str) {
    let at = harness
        .query_all_by_label("Name this group")
        .next()
        .unwrap()
        .rect()
        .center();
    click_at(driver, harness, at);
    typed(driver, harness, name);
    key(driver, harness, Key::Enter);
}

// a name the library refuses puts the group back (`group_named(.., false)`), on the
// application's side as well as the model's. No test of the whole application held it: the
// group stayed hidden, its count one short, until the page was opened again.
#[test]
fn a_name_refused_puts_the_group_back_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    // Every write of a name fails, and so does every read of the page after it.
    let other = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    other
        .execute_batch("ALTER TABLE people RENAME TO people_away")
        .unwrap();
    name_first_group(&mut driver, &mut harness, "Anna");
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(
        harness.state().people_page().unwrap().unnamed().len(),
        2,
        "the group refused is not put back"
    );
}

// a name for a group that has gone since the page was read is said in words the
// user can act on (`STALE_GROUP`), and the page read again no longer lists it. No test of
// the whole application held the mapping of `notAPerson`.
#[test]
fn a_name_for_a_group_gone_says_the_group_changed() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let group = harness.state().people_page().unwrap().unnamed()[0].id;
    // Gone behind the page's back: no change is announced for it.
    let other = rusqlite::Connection::open(library.dirs().db_path).unwrap();
    other
        .execute_batch(&format!(
            "UPDATE detected_faces SET person_id = NULL WHERE person_id = {group};
             DELETE FROM people WHERE id = {group};"
        ))
        .unwrap();
    name_first_group(&mut driver, &mut harness, "Anna");
    driver.until(&mut harness, "the refusal said", |app| {
        !app.toasts().is_empty()
    });
    assert_eq!(
        said(&harness),
        ["That group changed; the page has been refreshed."]
    );
    driver.until(&mut harness, "the page read again", |app| {
        let page = app.people_page().unwrap();
        page.unnamed().iter().all(|g| g.id != group)
    });
}
