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

/// The library of `library`, with Find faces on.
fn faced(library: &Fixture) -> Library {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    lib.set_face_detection(true).unwrap();
    lib
}

/// One face on each photo of `photos`, embedded as a vector of its own for each number
/// given beside the photo - faces of one number are alike, of two numbers at right angles -
/// and grouped by the library's own rule. Photos not given are not looked at yet.
fn faces_on(lib: &Library, photos: &[(i64, usize)]) {
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
    let of = |id: i64| {
        photos
            .iter()
            .find(|(photo, _)| *photo == id)
            .map(|(_, n)| *n)
    };
    let batch: Vec<_> = (lib
        .face_candidates(0, 1000, DETECTOR_VERSION)
        .unwrap()
        .into_iter())
    .filter(|photo| of(photo.id).is_some())
    .map(|photo| (photo, vec![face.clone()]))
    .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    let batch: Vec<_> = (lib
        .embed_candidates(0, 1000, EMBEDDER_VERSION)
        .unwrap()
        .into_iter())
    .map(|candidate| {
        let mut vector = [0.0_f32; DIM];
        vector[of(candidate.item_id).unwrap()] = 1.0;
        let faces = (candidate.faces.iter())
            .map(|(id, _)| (*id, Some(vector)))
            .collect();
        (candidate, faces)
    })
    .collect();
    lib.write_embeddings(&batch, EMBEDDER_VERSION).unwrap();
    lib.group_ungrouped_faces(&|| false).unwrap();
}

/// Every photo not looked at yet, looked at and found to have no face: the launch's pass
/// then has nothing to do.
fn look_rest(lib: &Library) {
    let batch: Vec<_> = (lib
        .face_candidates(0, 1000, DETECTOR_VERSION)
        .unwrap()
        .into_iter())
    .map(|photo| (photo, Vec::new()))
    .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
}

/// photon has looked at every photo of `library` and found one face on each photo of
/// `groups`, embedded so that the library's own grouping puts each group's faces
/// together: each group's a vector of its own, at right angles to every other's.
fn grouped(library: &Fixture, groups: &[&[i64]]) -> Library {
    let lib = faced(library);
    let photos: Vec<(i64, usize)> = (groups.iter().enumerate())
        .flat_map(|(n, group)| group.iter().map(move |id| (*id, n)))
        .collect();
    faces_on(&lib, &photos);
    look_rest(&lib);
    lib
}

/// The unnamed groups, largest first, as the library lists them.
fn unnamed(lib: &Library) -> Vec<i64> {
    let page = lib.people_page(12).unwrap();
    page.unnamed.iter().map(|group| group.id).collect()
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

/// A press on what AccessKit calls `label`, where it was last drawn - once a double
/// click's time has passed since the last, as a user's next click does: a press within it
/// of the page moving presses nothing (`PeopleView`'s `deaf`).
fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    driver.frames_in(harness, 0.4);
    driver.seen(harness, label);
    let at = harness.get_by_label(label).rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

/// The faces a strip of the page shows.
fn strip(app: &App, strip: photon_ui::people_page::Strip) -> Vec<i64> {
    let page = app.people_page().unwrap();
    page.faces(strip).iter().map(|face| face.id).collect()
}

/// The named people the page lists.
fn named(app: &App) -> Vec<String> {
    let page = app.people_page().unwrap();
    (page.people().iter())
        .filter_map(|person| person.name.clone())
        .collect()
}

// A face picked shows its strip's bar; "Not this person" takes it out of the strip at once,
// and the library has it out of the group.
#[test]
fn a_face_picked_is_taken_out_of_its_group_from_the_bar() {
    use photon_ui::people_page::Strip;
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    let group = unnamed(&lib)[0];
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let before = strip(harness.state(), Strip::Unnamed(group));
    press(&mut driver, &mut harness, "Unnamed group: face 1 of 3");
    driver.seen(&mut harness, "1 selected");
    driver.seen(&mut harness, photon_ui::people_view::OPEN_HINT);
    press(&mut driver, &mut harness, "Not this person");
    assert_eq!(strip(harness.state(), Strip::Unnamed(group)), before[1..]);
    driver.act(&mut harness);
    assert!(harness.query_by_label("1 selected").is_none());
    let face = before[0];
    driver.until(&mut harness, "the face out of the group", |_| {
        let page = lib.people_page(12).unwrap();
        (page.unnamed.iter())
            .find(|g| g.id == group)
            .is_none_or(|g| !g.faces.iter().any(|f| f.id == face))
    });
}

// "Show more" loads the group's next faces and "Show fewer" folds them back.
#[test]
fn show_more_and_show_fewer() {
    use photon_ui::people_page::Strip;
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 20, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..15]]);
    let group = unnamed(&lib)[0];
    let (mut harness, mut driver) = opened(&library, 20);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    assert_eq!(strip(harness.state(), Strip::Unnamed(group)).len(), 12);
    press(&mut driver, &mut harness, "Show more (3 left)");
    driver.until(&mut harness, "the rest of the group", |app| {
        strip(app, Strip::Unnamed(group)).len() == 15
    });
    driver.act(&mut harness);
    press(&mut driver, &mut harness, "Show fewer");
    assert_eq!(strip(harness.state(), Strip::Unnamed(group)).len(), 12);
}

// A person is renamed in their row: the field opens with the name chosen, what is typed
// replaces it, and Enter stores it.
#[test]
fn a_person_is_renamed_in_their_row() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    lib.name_group(unnamed(&lib)[0], "Anna").unwrap();
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Rename");
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "Anne");
    driver.seen(&mut harness, "Rename to “Anne” · Enter");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "Anne", |app| named(app) == ["Anne"]);
}

// "Merge into…" lists the others; picked, the merge is asked before it is done, and done.
#[test]
fn a_person_is_merged_into_another_once_asked() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let groups = unnamed(&lib);
    lib.name_group(groups[0], "Anna").unwrap();
    lib.name_group(groups[1], "Ben").unwrap();
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    // Anna's row comes first, by name.
    let merge = harness
        .query_all_by_label("Merge into…")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(merge));
    harness.event(button(merge, PointerButton::Primary, true));
    harness.event(button(merge, PointerButton::Primary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    // A frame more: egui draws a new area's first frame unseen, to measure it, and at a
    // place of its own (pushed in from the window's edge by its default size), where
    // AccessKit is told its items are.
    driver.act(&mut harness);
    // Anna is not offered herself: her buttons are the sidebar's row and nothing more.
    let buttons = |harness: &Harness<'_>, name: &str| {
        (harness.query_all_by_label(name))
            .filter(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .count()
    };
    assert_eq!(buttons(&harness, "Anna"), 1);
    assert_eq!(buttons(&harness, "Ben"), 2, "Ben's row and the menu's Ben");
    // The menu's Ben, under the button: the sidebar and the page name him too.
    let item = (harness.query_all_by_label("Ben"))
        .map(|node| node.rect())
        .filter(|rect| rect.top() >= merge.y)
        .min_by(|a, b| (a.top() - merge.y).total_cmp(&(b.top() - merge.y)))
        .unwrap()
        .center();
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true));
    harness.event(button(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(matches!(
        harness.state().dialog(),
        Some(Dialog::Confirm(photon_ui::dialogs::Confirm::Merge { .. }))
    ));
    driver.seen(
        &mut harness,
        "Merge “Anna” into “Ben”? Their faces become Ben's. This cannot be undone.",
    );
    driver.frames_in(&mut harness, 0.5);
    harness.get_by_label("Merge").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "Anna merged into Ben", |app| {
        named(app) == ["Ben"]
    });
}

// Delete is asked before it is done; the person's faces stay, a group with no name.
#[test]
fn a_person_is_deleted_once_asked() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    lib.name_group(unnamed(&lib)[0], "Anna").unwrap();
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    let row = harness.get_by_label("Delete").rect();
    press(&mut driver, &mut harness, "Delete");
    driver.seen(
        &mut harness,
        "Delete “Anna”? Their faces stay together as a group with no name.",
    );
    driver.frames_in(&mut harness, 0.5);
    // The question's Delete, not the row's under it.
    (harness.query_all_by_label("Delete"))
        .find(|node| node.rect() != row)
        .unwrap()
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "Anna gone, her faces a group", |app| {
        let page = app.people_page().unwrap();
        named(app).is_empty() && page.unnamed().len() == 1
    });
}

// A group ignored leaves Unnamed at once and is listed under Ignored, folded; unfolded,
// "Stop ignoring" brings it back.
#[test]
fn a_group_is_ignored_and_brought_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Ignore");
    assert!(harness.state().people_page().unwrap().unnamed().is_empty());
    press(&mut driver, &mut harness, "Ignored · 1 group, 0 faces");
    press(&mut driver, &mut harness, "Stop ignoring");
    driver.until(&mut harness, "the group back in Unnamed", |app| {
        app.people_page().unwrap().unnamed().len() == 1
    });
}

// A named person's suggestions - faces like theirs, found after they were named - are
// confirmed together.
#[test]
fn a_persons_suggestions_are_confirmed_together() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = faced(&library);
    faces_on(&lib, &[(ids[0], 0), (ids[1], 0), (ids[2], 0)]);
    lib.name_group(unnamed(&lib)[0], "Anna").unwrap();
    faces_on(&lib, &[(ids[3], 0), (ids[4], 0)]);
    look_rest(&lib);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    driver.seen(&mut harness, "Suggestions");
    driver.seen(&mut harness, "Anna: 2 to check");
    press(&mut driver, &mut harness, "Confirm all");
    driver.until(&mut harness, "the suggestions confirmed", |app| {
        let page = app.people_page().unwrap();
        page.suggestions().is_empty()
            && page.count(photon_ui::people_page::Strip::Person(page.people()[0].id)) == 5
    });
}

// Single faces picked are named together from their bar.
#[test]
fn single_faces_are_named_from_their_bar() {
    use photon_ui::people_page::Strip;
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..1], &ids[1..2]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "2 single faces");
    let faces = strip(harness.state(), Strip::Single);
    assert_eq!(faces.len(), 2);
    press(&mut driver, &mut harness, "Single faces: face 1 of 2");
    // A double click's time apart, or the second click opens the photo of the first.
    driver.frames_in(&mut harness, 0.5);
    press(&mut driver, &mut harness, "Single faces: face 2 of 2");
    driver.seen(&mut harness, "2 selected");
    press(&mut driver, &mut harness, "Name the selected faces");
    typed(&mut driver, &mut harness, "Cara");
    key(&mut driver, &mut harness, Key::Enter);
    assert!(strip(harness.state(), Strip::Single).is_empty());
    driver.until(&mut harness, "Cara with both faces", |app| {
        let page = app.people_page().unwrap();
        named(app) == ["Cara"] && page.count(Strip::Person(page.people()[0].id)) == 2
    });
}

// A question about a person is about the person the page listed under that name: renamed
// from elsewhere while it stands - the viewer's dialog cannot be open under it, but a
// second window of photon on the same library, or a rename finishing, can - it closes,
// rather than delete someone under a name they no longer have. The album's and the saved
// search's question have the same rule (`App::listed`).
#[test]
fn a_question_about_a_person_renamed_meanwhile_closes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let ids = photo_ids(&library);
    let lib = grouped(&library, &[&ids[0..3]]);
    let anna = unnamed(&lib)[0];
    lib.name_group(anna, "Anna").unwrap();
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Delete");
    driver.seen(
        &mut harness,
        "Delete “Anna”? Their faces stay together as a group with no name.",
    );
    let engine = harness.state().engine().clone();
    photon_engine::commands::rename_person(&engine, anna, "Anne").unwrap();
    driver.until(&mut harness, "the question gone", |app| {
        app.dialog().is_none()
    });
    driver.settle(&mut harness);
    assert_eq!(named(harness.state()), ["Anne"], "and Anne is still there");
}

// The page scrolls as far a notch as the grid does - the system's notch, a hundred points
// at Windows' three lines a notch, where it moved forty whatever the system said and was
// "slow" to the first person to run it on Windows - and its scrollbar is the grid's, which
// a press takes hold of: it was a mark nothing could drag.
#[test]
fn the_page_scrolls_by_the_systems_notch_and_its_bar_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 24, 0, 0);
    let ids = photo_ids(&library);
    let groups: Vec<&[i64]> = ids.chunks(2).collect();
    grouped(&library, &groups);
    let dirs = library.dirs();
    let mut harness: Harness<'_> = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 500.0))
        .with_step_dt(crate::common::REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, None).unwrap().without_animations())
                .with_videos(crate::common::no_videos())
                .with_platform(crate::common::no_platform())
                .with_wheel(|| photon_ui::grid::scroll::Notch::Points(100.0))
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == 24 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    open_page(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let over = egui::pos2(600.0, 250.0);
    harness.event(egui::Event::PointerMoved(over));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    assert_eq!(harness.state().people_scrolled(), (100.0, false));
    // A press at the foot of the bar's track, at the window's right edge: the page at
    // its end, the thumb held.
    let foot = egui::pos2(995.0, 440.0);
    harness.event(egui::Event::PointerMoved(foot));
    driver.act(&mut harness);
    harness.event(button(foot, PointerButton::Primary, true));
    driver.act(&mut harness);
    let (scrolled, held) = harness.state().people_scrolled();
    assert!(held, "the bar takes the press");
    assert!(scrolled > 100.0, "the page went to {scrolled}");
    harness.event(button(foot, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(!harness.state().people_scrolled().1, "let go of");
}

/// A library of two groups, the first named Anna on the page and the caret in the other's
/// box, with `typed` typed in it.
fn typing_after_anna<'a>(
    dir: &std::path::Path,
    typed_in: &str,
) -> (
    crate::common::Harness<'a>,
    Driver,
    photon_ui::fixture::Fixture,
) {
    let library = library(dir, 12, 0, 0);
    let ids = photo_ids(&library);
    grouped(&library, &[&ids[0..3], &ids[3..5]]);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    open_page(&mut driver, &mut harness);
    name_first_group(&mut driver, &mut harness, "Anna");
    driver.until(&mut harness, "Anna listed", |app| {
        let page = app.people_page().unwrap();
        (page.people().iter()).any(|person| person.name.as_deref() == Some("Anna"))
            && page.unnamed().len() == 1
    });
    driver.frames_in(&mut harness, 0.5);
    let at = (harness.query_all_by_label("Name this group"))
        .next()
        .unwrap()
        .rect()
        .center();
    click_at(&mut driver, &mut harness, at);
    typed(&mut driver, &mut harness, typed_in);
    // The list is an area of its own, drawn first unseen to be measured.
    driver.act(&mut harness);
    driver.act(&mut harness);
    (harness, driver, library)
}

/// Whether the other group has gone into Anna: no unnamed group left, Anna with its faces.
fn merged_into_anna(app: &App) -> bool {
    let page = app.people_page().unwrap();
    page.unnamed().is_empty()
        && (page.people().iter())
            .any(|person| person.name.as_deref() == Some("Anna") && person.face_count == 5)
}

// A name half typed for a group offers the people whose name it begins: the arrow takes
// the first, its line says what Enter does, and Enter puts the group's faces with her - a
// name the user did not have to type whole (asked for by the first person to run the
// page).
#[test]
fn a_person_offered_under_a_name_is_picked_by_the_arrow_and_enter() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = typing_after_anna(dir.path(), "an");
    driver.seen(&mut harness, "New person “an” · Enter");
    key(&mut driver, &mut harness, Key::ArrowDown);
    driver.seen(&mut harness, "Add to Anna · Enter");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the group in Anna", merged_into_anna);
}

// And by a click on her row in the list.
#[test]
fn a_person_offered_under_a_name_is_picked_by_a_click() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = typing_after_anna(dir.path(), "AN");
    // Her row in the list, under the box - not her name in the People section above, nor
    // her row in the sidebar.
    let field = (harness.query_all_by_label("Name this group"))
        .next()
        .unwrap()
        .rect();
    let row = (harness.query_all_by_label("Anna"))
        .filter(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .map(|node| node.rect())
        .find(|rect| (rect.left() - field.left()).abs() < 12.0)
        .expect("Anna offered under the box");
    click_at(&mut driver, &mut harness, row.center());
    driver.until(&mut harness, "the group in Anna", merged_into_anna);
}

// What is typed and names nobody offers nobody, and Enter makes a new person of it.
#[test]
fn a_name_nobody_has_offers_nobody() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = typing_after_anna(dir.path(), "Ben");
    key(&mut driver, &mut harness, Key::ArrowDown);
    driver.seen(&mut harness, "New person “Ben” · Enter");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "Ben listed", |app| {
        (app.people_page().unwrap().people().iter())
            .any(|person| person.name.as_deref() == Some("Ben"))
    });
}

// A letter typed after the arrow has taken a person lets go of her, even where she is
// still offered: Enter is then the name as typed, as its line says.
#[test]
fn a_letter_typed_after_the_arrow_lets_go_of_the_person() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = typing_after_anna(dir.path(), "An");
    key(&mut driver, &mut harness, Key::ArrowDown);
    driver.seen(&mut harness, "Add to Anna · Enter");
    typed(&mut driver, &mut harness, "n");
    driver.seen(&mut harness, "New person “Ann” · Enter");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "Ann listed", |app| {
        (app.people_page().unwrap().people().iter())
            .any(|person| person.name.as_deref() == Some("Ann"))
    });
}
