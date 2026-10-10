//! The sidebar's lists, through the whole application: the collections read from a library
//! on disk, a folder gone to in the grid, and the folder photon remembers.

mod common;

use common::{Driver, Harness, click, launch, library, opened, photo_ids};
use eframe::egui::{self, Modifiers, pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{
    grid::GridView,
    library::Library,
    now_ms,
    sort::{Grouping, Sort, SortKey},
};
use photon_engine::commands;
use photon_ui::{app::App, fixture::Fixture, nav::Place, nav::Step, window_layout::Layout};

/// An album of the first seven photos, an album of none, a saved search for ten of them
/// and a keyword on four. Answers the first album's id.
fn collect(library: &Fixture) -> i64 {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let ids = photo_ids(library);
    let album = lib.create_album("Best of", now_ms()).unwrap();
    lib.add_to_album(album.id, &ids[..7], now_ms()).unwrap();
    lib.create_album("Empty", now_ms()).unwrap();
    // Ten file names begin so: IMG_000000 to IMG_000009.
    lib.create_saved_search("Lakes", "IMG_00000", now_ms())
        .unwrap();
    lib.add_items_tag(&ids[..4], "coast").unwrap();
    album.id
}

/// The folder called `name`.
fn folder(app: &App, name: &str) -> i64 {
    (app.folders().values())
        .find(|folder| folder.name == name)
        .unwrap_or_else(|| panic!("no folder {name}"))
        .id
}

/// Whether the sidebar's row called `label` says it is where the user is.
fn current(harness: &Harness<'_>, label: &str) -> bool {
    let row = harness.get_by_label(label);
    row.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

fn top_folder(app: &App) -> Option<i64> {
    app.last_frame().and_then(|frame| frame.top_folder)
}

/// Launches over `library` and waits for its lists as well as its grid.
fn opened_with_lists<'a>(library: &Fixture, photos: usize) -> (Harness<'a>, Driver) {
    let (mut harness, mut driver) = opened(library, photos);
    driver.until(&mut harness, "the folders read", |app| {
        !app.folders().is_empty()
    });
    driver.settle(&mut harness);
    (harness, driver)
}

/// Draws frames until the folder photon remembers is `folder`.
fn remembered(driver: &mut Driver, harness: &mut Harness<'_>, folder: i64) {
    driver.until(harness, "the folder remembered", |app| {
        commands::last_folder(app.engine()).ok().flatten() == Some(folder)
    });
}

#[test]
fn the_lists_are_read_and_a_click_on_one_of_them_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let album = collect(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the collections read", |app| {
        app.collections().albums.len() == 2
    });
    driver.settle(&mut harness);
    let read = harness.state().collections();
    assert_eq!(read.searches.len(), 1);
    assert_eq!(read.tags.len(), 1);
    // Albums and Searches are open in a window nobody has laid out; Tags are folded.
    assert!(harness.query_by_label("Best of").is_some());
    assert!(harness.query_by_label("Lakes").is_some());
    assert!(harness.query_by_label("coast").is_none());
    // And the folders that hold photos are listed, under the year of their photos.
    for name in ["folder-0000", "folder-0001", "folder-0002"] {
        assert!(harness.query_by_label(name).is_some(), "{name}");
    }

    click(&mut driver, &mut harness, "Best of");
    let shown = Place {
        view: GridView::Album,
        arg: album.to_string(),
    };
    assert_eq!(harness.state().place(), shown, "at once");
    driver.until(&mut harness, "the album shown", |app| {
        *app.settled() == shown
    });
    assert_eq!(harness.state().photos(), 7);
    driver.settle(&mut harness);
    assert!(current(&harness, "Best of"));
    assert!(!current(&harness, "Empty") && !current(&harness, "All photos"));

    click(&mut driver, &mut harness, "Lakes");
    driver.until(&mut harness, "the saved search run", |app| {
        *app.settled() == Place::search("IMG_00000")
    });
    assert_eq!(harness.state().photos(), 10);

    // An album with nothing in it says so, by its name.
    click(&mut driver, &mut harness, "Empty");
    driver.until(&mut harness, "the empty album shown", |app| {
        app.settled().view == GridView::Album && app.photos() == 0
    });
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("“Empty” is empty. Right-click a photo to add it.")
    );
}

// The steps a person's and a tag's rows ask for, asked as they ask: each reaches the
// engine as that view with that argument.
#[test]
fn a_person_and_a_tag_are_views_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 600, 0, 0);
    collect(&library);
    let (mut harness, mut driver) = opened(&library, 600);

    harness.state_mut().go(Step::Tag("coast".to_owned()));
    driver.until(&mut harness, "the keyword's photos shown", |app| {
        app.settled().view == GridView::Tag
    });
    assert_eq!(harness.state().settled().arg, "coast");
    assert_eq!(harness.state().photos(), 4);
    assert_eq!(
        harness.state().engine().view_and_arg(),
        (GridView::Tag, "coast".to_owned())
    );

    harness.state_mut().go(Step::Tag("nothing".to_owned()));
    driver.until(&mut harness, "an empty keyword shown", |app| {
        app.settled().arg == "nothing"
    });
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("No photos tagged “nothing”.")
    );

    harness.state_mut().go(Step::Person("p:99".to_owned()));
    driver.until(&mut harness, "a person shown", |app| {
        app.settled().view == GridView::Person
    });
    assert_eq!(
        harness.state().engine().view_and_arg(),
        (GridView::Person, "p:99".to_owned())
    );
    // Nobody is called that: there is no name to read.
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("No photos of this person.")
    );
}

// The last folder in the grid, two screens down: its header at the top of the grid, and
// its row marked - in the frames a window would draw, with nobody moving the pointer.
#[test]
fn a_folder_clicked_is_at_the_top_of_the_grid_and_marked_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    let oldest = folder(harness.state(), "folder-0000");
    let newest = folder(harness.state(), "folder-0002");
    assert_eq!(top_folder(harness.state()), Some(newest));
    let version = harness.state().version();

    click(&mut driver, &mut harness, "folder-0000");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(top_folder(app), Some(oldest));
    assert!(app.last_frame().unwrap().position > 1000.0);
    assert_eq!(
        app.marked(),
        Some(oldest),
        "the list marks where the grid is"
    );
    assert_eq!(
        app.version(),
        version,
        "All photos was shown: no grid was built"
    );

    // And back to the top, where every picture is already in hand and no decoder asks
    // for a frame. The sidebar is drawn before the grid has moved, so the mark is a frame
    // behind it: the frame that follows the jump's own is what makes that up, and it is
    // among the frames asked for at once - not the one the grid asks for a moment later,
    // to say what is in view.
    click(&mut driver, &mut harness, "folder-0002");
    driver.frames_in(&mut harness, 0.05);
    let app = harness.state();
    assert_eq!(top_folder(app), Some(newest));
    assert_eq!(app.marked(), Some(newest));
}

// From an excursion the folder is gone to in All photos: asked of Starred's grid, its
// offset would name another photo, or none.
#[test]
fn a_folder_clicked_from_another_view_is_gone_to_in_all_photos() {
    let dir = tempfile::tempdir().unwrap();
    // Every photo of the newest folder is starred, and every photo of the oldest: in
    // Starred the oldest folder is a screen and more down.
    let library = library(dir.path(), 900, 300, 0);
    let ids = photo_ids(&library);
    let stars: Vec<(i64, u8)> = ids[600..].iter().map(|id| (*id, 1)).collect();
    Library::open(&library.dirs().db_path)
        .unwrap()
        .set_ratings(&stars)
        .unwrap();
    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    let oldest = folder(harness.state(), "folder-0000");

    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred) && app.photos() == 600
    });
    driver.settle(&mut harness);
    // Starred's folders are the ones its photos are in.
    assert!(harness.query_by_label("folder-0001").is_none());

    click(&mut driver, &mut harness, "folder-0000");
    assert_eq!(harness.state().place(), Place::of(GridView::All));
    // Until All photos is on screen the grid is Starred's, and stays where it was: the
    // folder's place is looked up in the grid it was asked of.
    let moved = std::cell::Cell::new(false);
    driver.until(&mut harness, "all photos shown", |app| {
        let position = app.last_frame().map_or(0.0, |frame| frame.position);
        if app.settled().view == GridView::Starred && position != 0.0 {
            moved.set(true);
        }
        *app.settled() == Place::of(GridView::All)
    });
    assert!(
        !moved.get(),
        "the starred photos were scrolled to the folder"
    );
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(app.photos(), 900);
    assert_eq!(
        top_folder(app),
        Some(oldest),
        "not at the top of All photos"
    );
    assert_eq!(app.marked(), Some(oldest));
}

// Hidden is disjoint from All photos: a folder whose photos are all hidden is not in All
// at all, and the click would land the user at its top with nothing.
#[test]
fn a_folder_clicked_in_hidden_stays_in_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 3);
    let (mut harness, mut driver) = opened_with_lists(&library, 897);
    click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photos shown", |app| {
        *app.settled() == Place::of(GridView::Hidden)
    });
    driver.settle(&mut harness);
    let version = harness.state().version();

    click(&mut driver, &mut harness, "folder-0000");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(app.place(), Place::of(GridView::Hidden));
    assert_eq!(app.version(), version);
    assert_eq!(app.photos(), 3);
}

// An excursion leaves the place in All photos alone, and All photos comes back to it:
// a click on a folder instead lands on that folder's top, which made going back a reset.
#[test]
fn all_photos_comes_back_to_the_folder_that_was_left() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    let oldest = folder(harness.state(), "folder-0000");
    click(&mut driver, &mut harness, "folder-0000");
    remembered(&mut driver, &mut harness, oldest);

    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    driver.settle(&mut harness);
    // The excursion's own folder is not remembered in its place.
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(
        commands::last_folder(harness.state().engine()).unwrap(),
        Some(oldest)
    );

    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "all photos shown", |app| {
        *app.settled() == Place::of(GridView::All)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(top_folder(harness.state()), Some(oldest));
    assert_eq!(
        commands::last_folder(harness.state().engine()).unwrap(),
        Some(oldest)
    );
}

// The folder remembered is a place in the folder order. Under a month grouping a jump to
// it would land on one of its photos wherever the order put it: All photos opens at its
// top, at launch and from an excursion.
#[test]
fn the_folder_left_is_come_back_to_only_where_the_grid_runs_folder_by_folder() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        // The folder of the last photo in the grid: the oldest.
        let oldest = lib.item(ids[ids.len() - 1]).unwrap().unwrap().folder_id;
        lib.set_last_folder(oldest).unwrap();
        lib.set_grid_sort(Sort {
            key: SortKey::Date,
            reverse: false,
            group: Grouping::Month,
        })
        .unwrap();
    }
    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().last_frame().unwrap().position, 0.0);

    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "all photos shown", |app| {
        *app.settled() == Place::of(GridView::All)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().last_frame().unwrap().position, 0.0);
}

// The grid of a launch is at its top until the restore has run. A write before it stores
// the library's first folder over the place the user left.
#[test]
fn the_folder_left_is_where_the_next_launch_begins() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let oldest = {
        let (mut harness, mut driver) = opened_with_lists(&library, 900);
        let oldest = folder(harness.state(), "folder-0000");
        click(&mut driver, &mut harness, "folder-0000");
        remembered(&mut driver, &mut harness, oldest);
        oldest
    };

    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(top_folder(harness.state()), Some(oldest));
    assert_eq!(
        commands::last_folder(harness.state().engine()).unwrap(),
        Some(oldest),
        "the launch did not overwrite it"
    );
    // The list has followed: the folder's row is the marked one.
    assert_eq!(harness.state().marked(), Some(oldest));
}

#[test]
fn a_group_folded_is_folded_at_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    collect(&library);
    {
        let (mut harness, mut driver) = opened(&library, 300);
        driver.until(&mut harness, "the collections read", |app| {
            app.collections().albums.len() == 2
        });
        driver.settle(&mut harness);
        assert!(harness.query_by_label("Best of").is_some());
        assert!(harness.query_by_label("coast").is_none());

        click(&mut driver, &mut harness, "Albums");
        // The list is drawn again before the next click: Tags has moved up by two rows.
        driver.settle(&mut harness);
        click(&mut driver, &mut harness, "Tags");
        driver.settle(&mut harness);
        // In the frames a window would draw: its rows are gone, the other's are there.
        assert!(harness.query_by_label("Best of").is_none());
        assert!(harness.query_by_label("coast").is_some());
        assert!(!harness.state().layout().open.albums);
        let path = library.dirs().layout_path;
        driver.until(&mut harness, "the layout stored", |_| {
            let stored = Layout::load(&path).open;
            !stored.albums && stored.tags
        });
    }
    let mut harness = launch(&library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the collections read", |app| {
        app.collections().albums.len() == 2
    });
    driver.settle(&mut harness);
    assert!(harness.query_by_label("Best of").is_none());
    assert!(harness.query_by_label("coast").is_some());
}

// Already there, the user is at their own place: the click must not take them to the top
// of the folder they are in the middle of.
#[test]
fn all_photos_clicked_in_all_photos_moves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened_with_lists(&library, 900);
    click(&mut driver, &mut harness, "folder-0001");
    driver.settle(&mut harness);
    // Into the middle of the folder.
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -700.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let before = harness.state().last_frame().unwrap().position;
    let header = {
        let app = harness.state();
        assert_eq!(top_folder(app), Some(folder(app, "folder-0001")));
        before - 700.0
    };
    assert!(header > 0.0);

    click(&mut driver, &mut harness, "All photos");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().last_frame().unwrap().position, before);
}

// An album made, a face named, a keyword that turned up in a scan: the lists are read
// again when the library says its data changed, and the list drawn is built from them.
#[test]
fn the_lists_follow_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    collect(&library);
    // Tags unfolded, as a user would have left them.
    let mut layout = Layout::default();
    layout.open.tags = true;
    layout.save(&library.dirs().layout_path).unwrap();
    let (mut harness, mut driver) = opened(&library, 300);
    driver.until(&mut harness, "the collections read", |app| {
        app.collections().tags.len() == 1
    });
    driver.settle(&mut harness);
    assert!(harness.query_by_label("coast").is_some());
    assert!(harness.query_by_label("later").is_none());

    let ids = photo_ids(&library);
    Library::open(&library.dirs().db_path)
        .unwrap()
        .add_items_tag(&ids[10..12], "later")
        .unwrap();
    // The library says its data changed, as a scan that found the keyword would have it
    // say, and nothing else about it moves: no count, no folder, no photo.
    harness.state().engine().refresh_grid().unwrap();
    driver.until(&mut harness, "the new keyword read", |app| {
        app.collections().tags.len() == 2
    });
    driver.settle(&mut harness);
    assert!(harness.query_by_label("later").is_some());
}

// The folder list is read again with them, and the list drawn is built from it: a folder
// given a name in photon is listed under that name.
#[test]
fn a_folder_given_a_name_is_listed_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 600, 0, 0);
    let (mut harness, mut driver) = opened_with_lists(&library, 600);
    let renamed = folder(harness.state(), "folder-0001");
    assert!(harness.query_by_label("folder-0001").is_some());
    let engine = harness.state().engine().clone();
    commands::set_folder_alias(&engine, renamed, Some("Roma".to_owned())).unwrap();
    driver.until(&mut harness, "the name read", |app| {
        app.folders()[&renamed].alias.as_deref() == Some("Roma")
    });
    driver.settle(&mut harness);
    assert!(harness.query_by_label("Roma").is_some());
    assert!(harness.query_by_label("folder-0001").is_none());
    assert!(harness.query_by_label("folder-0000").is_some());
}

// Thirty folders are more than the window holds. At launch the grid goes back to the
// oldest, the last in the list, and the list goes with it: its row is in sight, marked.
#[test]
fn the_list_is_where_the_grid_is() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 9000, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        let oldest = lib.item(ids[ids.len() - 1]).unwrap().unwrap().folder_id;
        lib.set_last_folder(oldest).unwrap();
    }
    let (mut harness, mut driver) = opened_with_lists(&library, 9000);
    driver.frames_in(&mut harness, 1.0);
    let app = harness.state();
    assert_eq!(top_folder(app), Some(folder(app, "folder-0000")));
    assert!(app.sidebar_position() > 0.0, "the list did not follow");
    assert!(harness.query_by_label("folder-0000").is_some());
    assert!(
        harness.query_by_label("All photos").is_none(),
        "scrolled out"
    );
}
