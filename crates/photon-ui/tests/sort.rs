//! The controls at the right of the top bar and the year strip, through the whole
//! application: a sort chosen and made by the engine, a size stored, a strip pressed.

mod common;

use common::{Driver, Harness, click, launch, library, opened, photo_ids};
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::{
    grid::GridView,
    library::{GridTile, Library},
    sort::{Grouping, Sort, SortKey},
};
use photon_engine::commands;
use photon_ui::app::App;

/// Opens the list of the control called `control` and takes `option` from it.
fn choose(driver: &mut Driver, harness: &mut Harness<'_>, control: &str, option: &str) {
    click(driver, harness, control);
    // A list is shown in the frame after the one it is first measured in.
    driver.act(harness);
    click(driver, harness, option);
}

/// Draws frames until the grid on screen is sorted as `sort`.
fn sorted(driver: &mut Driver, harness: &mut Harness<'_>, sort: Sort) {
    driver.until(harness, "the sort shown", |app| {
        app.settled_sort() == sort && app.sort() == sort
    });
}

fn frame(app: &App) -> &photon_ui::grid::view::GridOutput {
    app.last_frame().expect("a frame has been drawn")
}

/// Whether the control called `label` says it is pressed, or chosen.
fn on(harness: &Harness<'_>, label: &str) -> bool {
    let node = harness.get_by_label(label);
    node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

fn by(key: SortKey) -> Sort {
    Sort {
        key,
        ..Sort::default()
    }
}

// Another order is another list: shown from its top, in that order, with the sidebar's
// folders in it too - by name they are no longer filed under years.
#[test]
fn a_sort_chosen_is_shown_from_its_top_and_the_folders_follow_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        !app.folders().is_empty()
    });
    driver.settle(&mut harness);
    let top_of = |harness: &Harness<'_>, label: &str| harness.get_by_label(label).rect().top();
    // By date the newest folder is first, in the grid and in the list.
    assert!(top_of(&harness, "folder-0002") < top_of(&harness, "folder-0000"));
    harness.key_press_modifiers(Modifiers::NONE, Key::End);
    driver.act(&mut harness);
    assert!(frame(harness.state()).position > 1000.0);

    choose(&mut driver, &mut harness, "Sort by: Date taken", "Name");
    // The control says so at once, and the grid when the engine has made it.
    assert_eq!(harness.state().sort(), by(SortKey::Name));
    sorted(&mut driver, &mut harness, by(SortKey::Name));
    driver.settle(&mut harness);
    let shown = frame(harness.state());
    assert_eq!(shown.position, 0.0);
    // IMG_000000 is the first photo of the oldest folder, which by date comes last.
    assert_eq!(shown.on_screen[0], ids[600]);
    assert!(harness.query_by_label("Sort by: Name").is_some());
    // The folders by name.
    assert!(top_of(&harness, "folder-0000") < top_of(&harness, "folder-0001"));
    assert!(top_of(&harness, "folder-0001") < top_of(&harness, "folder-0002"));
    // And the library keeps it.
    assert_eq!(
        Library::open(&library.dirs().db_path)
            .unwrap()
            .grid_sort()
            .unwrap(),
        by(SortKey::Name)
    );
}

// Under a sort that ignores it the grouping is dimmed and kept, and comes back with the
// sort by date.
#[test]
fn the_grouping_waits_dimmed_under_another_sort_and_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    choose(&mut driver, &mut harness, "Group by: By folder", "By month");
    let by_month = Sort {
        group: Grouping::Month,
        ..Sort::default()
    };
    sorted(&mut driver, &mut harness, by_month);
    assert_eq!(frame(harness.state()).top_folder, None);

    choose(&mut driver, &mut harness, "Sort by: Date taken", "Size");
    let by_size = Sort {
        key: SortKey::Size,
        ..by_month
    };
    sorted(&mut driver, &mut harness, by_size);
    // Still there, still saying what it holds, and a press opens nothing.
    click(&mut driver, &mut harness, "Group by: By month");
    driver.act(&mut harness);
    assert!(harness.query_by_label("By year").is_none());
    assert_eq!(harness.state().sort(), by_size);

    choose(&mut driver, &mut harness, "Sort by: Size", "Date taken");
    sorted(&mut driver, &mut harness, by_month);
    choose(&mut driver, &mut harness, "Group by: By month", "By year");
    sorted(
        &mut driver,
        &mut harness,
        Sort {
            group: Grouping::Year,
            ..Sort::default()
        },
    );
}

// Three changes before the engine has made the first: each builds on the one before, and
// the grid ends on all three. Built on the grid on screen, the second undid the first.
#[test]
fn a_change_made_before_the_last_has_landed_builds_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12_000, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12_000);
    driver.settle(&mut harness);

    choose(&mut driver, &mut harness, "Group by: By folder", "By day");
    click(&mut driver, &mut harness, "Reverse order");
    click(&mut driver, &mut harness, "Reverse order");
    click(&mut driver, &mut harness, "Reverse order");
    let wanted = Sort {
        key: SortKey::Date,
        reverse: true,
        group: Grouping::Day,
    };
    assert_eq!(harness.state().sort(), wanted);
    // Drawn as it is wanted, in the frame after the press.
    driver.act(&mut harness);
    assert!(on(&harness, "Reverse order"));
    assert!(harness.query_by_label("Group by: By day").is_some());
    sorted(&mut driver, &mut harness, wanted);
    driver.settle(&mut harness);
    assert_eq!(harness.state().engine().sort(), wanted);
    assert!(on(&harness, "Reverse order"));
}

// A view clicked behind a sort keeps the sort, and a sort chosen behind a view keeps the
// view: both are steps on one queue, and neither takes the other's place.
#[test]
fn a_sort_and_a_view_asked_together_are_both_made() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12_000, 6_000, 0);
    let (mut harness, mut driver) = opened(&library, 12_000);
    driver.settle(&mut harness);

    click(&mut driver, &mut harness, "Reverse order");
    click(&mut driver, &mut harness, "Starred");
    click(&mut driver, &mut harness, "Large");
    choose(
        &mut driver,
        &mut harness,
        "Sort by: Date taken",
        "Date modified",
    );
    let wanted = Sort {
        key: SortKey::Modified,
        reverse: true,
        group: Grouping::Folder,
    };
    sorted(&mut driver, &mut harness, wanted);
    driver.until(&mut harness, "the starred photos shown", |app| {
        app.settled().view == GridView::Starred && app.photos() == 6_000
    });
    driver.settle(&mut harness);
    assert_eq!(harness.state().engine().sort(), wanted);
    assert_eq!(frame(harness.state()).position, 0.0);
}

// The size is applied at the press and is the one the next launch opens with.
#[test]
fn a_size_chosen_is_drawn_at_once_and_kept_for_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    {
        let (mut harness, mut driver) = opened(&library, 900);
        driver.settle(&mut harness);
        assert!(on(&harness, "Medium"));
        let medium = frame(harness.state()).on_screen.len();

        click(&mut driver, &mut harness, "Small");
        driver.act(&mut harness);
        assert_eq!(harness.state().tile_size(), GridTile::Small);
        assert!(on(&harness, "Small") && !on(&harness, "Medium"));
        assert!(frame(harness.state()).on_screen.len() > medium);
        driver.until(&mut harness, "the size stored", |app| {
            commands::grid_tile(app.engine()).ok() == Some(GridTile::Small)
        });
        // No grid was built for it: the photos are the ones there were.
        assert_eq!(harness.state().settled_sort(), Sort::default());
    }
    let mut harness = launch(&library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| app.photos() == 900);
    assert_eq!(harness.state().tile_size(), GridTile::Small);
    assert!(on(&harness, "Small"));
}

// Whatever reads the window for its user presses a control without a pointer.
#[test]
fn a_list_is_opened_and_chosen_from_without_a_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    harness
        .get_by_label("Sort by: Date taken")
        .click_accesskit();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_some());
    harness.get_by_label("Size").click_accesskit();
    driver.act(&mut harness);
    sorted(&mut driver, &mut harness, by(SortKey::Size));
}

// The keys of an open list are the list's: the arrow that moves through it does not
// scroll the photos under it, and Escape closes it without a choice.
#[test]
fn the_keys_of_an_open_list_do_not_reach_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.act(&mut harness);
    for key in [Key::ArrowDown, Key::ArrowDown, Key::End, Key::PageDown] {
        harness.key_press_modifiers(Modifiers::NONE, key);
        driver.act(&mut harness);
    }
    assert_eq!(frame(harness.state()).position, 0.0);
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_none());
    assert_eq!(harness.state().sort(), Sort::default());
}

// The list is the keyboard's while the control has it, and closed when it goes elsewhere
// by a key - Ctrl+F, which no press outside announces.
#[test]
fn an_open_list_closes_when_the_keyboard_goes_to_the_search_box() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_some());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_none());
    assert_eq!(harness.state().sort(), Sort::default());
}

// Tab in an open list takes the option the list is on, and the keyboard goes on to the
// next control - not into the list, which is gone, and not nowhere.
#[test]
fn tab_in_an_open_list_chooses_and_moves_on_to_the_next_control() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowDown);
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowDown);
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::Tab);
    driver.act(&mut harness);
    assert_eq!(harness.state().sort(), by(SortKey::Name));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        harness
            .get_by_label("Reverse order")
            .accesskit_node()
            .is_focused()
    );
    sorted(&mut driver, &mut harness, by(SortKey::Name));
}

// A sort moves no view and clears no query: chosen in a search, the search is still
// shown, in the new order, and the box still holds its words.
#[test]
fn a_sort_chosen_in_a_search_keeps_the_search_and_its_words() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    // Ten file names begin so: IMG_000000 to IMG_000009.
    harness.event(egui::Event::Text("IMG_00000".to_owned()));
    driver.act(&mut harness);
    driver.until(&mut harness, "the search shown", |app| {
        app.settled().view == GridView::Search && app.photos() == 10
    });
    driver.settle(&mut harness);
    assert_eq!(frame(harness.state()).on_screen[0], ids[600]);

    click(&mut driver, &mut harness, "Reverse order");
    let reversed = Sort {
        reverse: true,
        ..Sort::default()
    };
    sorted(&mut driver, &mut harness, reversed);
    driver.settle(&mut harness);
    let app = harness.state();
    assert_eq!(app.search_text(), "IMG_00000");
    assert_eq!(app.settled().view, GridView::Search);
    assert_eq!(app.photos(), 10);
    assert_eq!(frame(app).on_screen[0], ids[609]);
}

// Sixty folders, three days apart from the middle of July 2017: the last of them are in
// 2018. The strip stands beside the grid, a press on it is a place in the grid, and by
// name, where there are no years, it is gone.
#[test]
fn the_year_strip_stands_beside_a_grid_of_two_years_and_takes_the_grid_to_a_place() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 18_000, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 18_000);
    driver.settle(&mut harness);
    assert!(frame(harness.state()).strip);
    let on_screen = frame(harness.state()).on_screen.len();

    // Half way down the strip, which is at the window's right edge.
    let strip = harness.get_by_label("Timeline");
    // Not a stop of the Tab key: the sidebar's years are the keyboard's way to a year.
    let focusable = (strip.accesskit_node().data()).supports_action(egui::accesskit::Action::Focus);
    assert!(!focusable);
    let strip = strip.rect();
    assert_eq!((strip.right(), strip.width()), (1000.0, 44.0));
    let middle = strip.center();
    harness.event(egui::Event::PointerMoved(middle));
    harness.event(egui::Event::PointerButton {
        pos: middle,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    harness.event(egui::Event::PointerButton {
        pos: middle,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.settle(&mut harness);
    // The strip is a scale model of the whole layout: half way down it is half way
    // through the library, give or take the headers.
    let shown = frame(harness.state());
    assert!(shown.position > 50_000.0, "{}", shown.position);
    let top = ids.iter().position(|id| *id == shown.on_screen[0]).unwrap();
    assert!((8_500..9_500).contains(&top), "photo {top} of 18,000");

    // By name there are no headers and no years: the strip goes and the tiles have its
    // room.
    choose(&mut driver, &mut harness, "Sort by: Date taken", "Name");
    sorted(&mut driver, &mut harness, by(SortKey::Name));
    driver.settle(&mut harness);
    assert!(!frame(harness.state()).strip);
    assert!(harness.query_by_label("Timeline").is_none());
    assert!(frame(harness.state()).on_screen.len() >= on_screen);
}

// With its lists closed the bar is as still as the rest: nothing in it asks for a frame.
#[test]
fn a_window_with_its_controls_used_and_closed_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    choose(&mut driver, &mut harness, "Group by: By folder", "By month");
    sorted(
        &mut driver,
        &mut harness,
        Sort {
            group: Grouping::Month,
            ..Sort::default()
        },
    );
    click(&mut driver, &mut harness, "Large");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
    // And with a list left open: it is a picture, not a movie.
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}
