//! The selection, through the whole application: photos picked with the mouse and the
//! keys, over the real engine, and kept on their photos when the grid under them changes.

use crate::common::{Driver, Harness, click, library, opened, photo_ids};
use eframe::egui::{self, Key, Modifiers, PointerButton, pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::grid::GridView;
use photon_engine::commands;
use photon_ui::{
    nav::Place,
    window_layout::{SIDEBAR_DEFAULT, SIDEBAR_STEP},
};

/// Clicks the photo at grid offset `offset` with `held` down, and draws the frames the
/// click is in.
fn pick(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center();
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(egui::Event::PointerMoved(at));
    driver.act(harness);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: held,
        });
        driver.act(harness);
    }
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, held: Modifiers, key: Key) {
    harness.key_press_modifiers(held, key);
    driver.act(harness);
}

#[test]
fn photos_are_picked_with_a_click_a_ctrl_click_and_a_shift_click() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    assert!(harness.state().selected().is_empty());

    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[1]]);
    assert_eq!(harness.state().lead(), Some(ids[1]));
    // One photo is its ring; the status bar counts from two.
    assert!(harness.query_by_label_contains("selected").is_none());
    pick(&mut driver, &mut harness, 3, Modifiers::SHIFT);
    assert_eq!(harness.state().selected(), ids[1..=3]);
    assert_eq!(harness.state().lead(), Some(ids[3]));
    assert!(harness.query_by_label("3 selected").is_some());
    pick(&mut driver, &mut harness, 6, Modifiers::COMMAND);
    assert_eq!(harness.state().selected(), [ids[1], ids[2], ids[3], ids[6]]);
    pick(&mut driver, &mut harness, 2, Modifiers::COMMAND);
    assert_eq!(harness.state().selected(), [ids[1], ids[3], ids[6]]);
    // A plain click is one photo again.
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[0]]);
}

// A write this window did not make - Picasa's, another photon's, a scan's - rebuilds the
// grid under the selection, with every later photo at another offset.
#[test]
fn the_selection_stays_on_its_photos_when_one_of_them_leaves_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 3, Modifiers::NONE);
    pick(&mut driver, &mut harness, 5, Modifiers::SHIFT);
    assert!(harness.query_by_label("3 selected").is_some());

    // One before the selection, which moves each of its photos an offset up, and one of
    // the selection itself.
    commands::set_items_hidden(harness.state().engine(), &[ids[0], ids[4]], true).unwrap();
    driver.until(&mut harness, "the grid without them", |app| {
        app.photos() == 898
    });
    driver.act(&mut harness);
    // The one that left is not counted, and the lead is still on its photo.
    assert_eq!(harness.state().selected(), [ids[3], ids[5]]);
    assert_eq!(harness.state().lead(), Some(ids[5]));
    assert!(harness.query_by_label("2 selected").is_some());
    // Shift+click still ranges from the anchor's photo, where that now is: the grid is
    // 1, 2, 3, 5, 6 and on, and the fifth of them is clicked.
    pick(&mut driver, &mut harness, 4, Modifiers::SHIFT);
    assert_eq!(harness.state().selected(), [ids[3], ids[5], ids[6]]);
}

// As a file manager does after a delete: what is acted on next is the photo beside.
#[test]
fn a_lead_whose_photo_leaves_alone_is_on_the_next_photo_and_that_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);

    commands::set_items_hidden(harness.state().engine(), &[ids[4]], true).unwrap();
    driver.until(&mut harness, "the grid without it", |app| {
        app.photos() == 899
    });
    assert_eq!(harness.state().selected(), [ids[5]]);
    assert_eq!(harness.state().lead(), Some(ids[5]));
    // And the arrows go on from there.
    key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(harness.state().selected(), [ids[6]]);
}

/// The grid offset the lead is on.
fn lead_at(harness: &Harness<'_>, ids: &[i64]) -> usize {
    let lead = harness.state().lead().expect("a lead");
    ids.iter().position(|id| *id == lead).unwrap()
}

// A scan purges a photo, or another window hides it, while the user is elsewhere in the
// grid: the lead goes on to the photo beside it, and the grid stays where the user is.
#[test]
fn the_grid_stays_where_it_is_when_a_lead_out_of_sight_loses_its_photo() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    harness.event(egui::Event::PointerMoved(egui::pos2(700.0, 400.0)));
    for _ in 0..3 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(&mut harness);
    }
    driver.settle(&mut harness);
    let before = harness.state().last_frame().unwrap().clone();
    assert!(before.position > 3000.0, "{}", before.position);
    assert!(!before.on_screen.contains(&ids[4]));

    commands::set_items_hidden(harness.state().engine(), &[ids[4]], true).unwrap();
    driver.until(&mut harness, "the grid without it", |app| {
        app.photos() == 899
    });
    driver.settle(&mut harness);
    assert_eq!(harness.state().lead(), Some(ids[5]));
    assert_eq!(harness.state().selected(), [ids[5]]);
    let after = harness.state().last_frame().unwrap();
    assert!(
        (after.position - before.position).abs() < 400.0,
        "the grid went from {} to {}",
        before.position,
        after.position
    );
}

// Working through a view with the keys: the photo taken out was the last of its folder,
// on the row at the bottom edge, and the next photo is under the next folder's header,
// below the edge. It is brought into sight, as the lead is after any key.
#[test]
fn a_lead_in_sight_that_loses_its_photo_is_followed_to_where_it_lands() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    let rect = |harness: &Harness<'_>, offset: usize| harness.state().tile_rect(offset).unwrap();
    // The last photo of a folder is followed by one more than a row further down.
    let ends = |harness: &Harness<'_>, offset: usize| {
        let (here, next) = (rect(harness, offset), rect(harness, offset + 1));
        next.top() - here.top() > here.height() + 9.0
    };
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    // Down until the grid has had to follow, and on to the last row of a folder: every
    // row the lead arrives in from above then stands at the bottom edge.
    let mut walked = 0;
    loop {
        key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowDown);
        let at = lead_at(&harness, &ids);
        let row_ends = (at..at + 12)
            .take_while(|offset| {
                (rect(&harness, *offset).top() - rect(&harness, at).top()).abs() < 1.0
            })
            .any(|offset| ends(&harness, offset));
        if harness.state().last_frame().unwrap().position > 0.0 && row_ends {
            break;
        }
        walked += 1;
        assert!(walked < 200, "no folder ends below the first screen");
    }
    while !ends(&harness, lead_at(&harness, &ids)) {
        key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowRight);
    }
    let last = lead_at(&harness, &ids);
    driver.settle(&mut harness);
    let edge = rect(&harness, last).bottom();

    commands::set_items_hidden(harness.state().engine(), &[ids[last]], true).unwrap();
    driver.until(&mut harness, "the grid without it", |app| {
        app.photos() == 899
    });
    driver.act(&mut harness);
    // The next folder's first photo, where the hidden one's offset now is.
    assert_eq!(harness.state().lead(), Some(ids[last + 1]));
    let landed = rect(&harness, last);
    assert!(
        landed.bottom() <= edge + 0.5,
        "the lead is at {landed:?}, below the edge at {edge}"
    );
}

#[test]
fn another_view_is_other_photos_and_nothing_of_them_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    // The first four hundred photos in the grid are starred: a folder and a part of
    // the next, in both views.
    let library = library(dir.path(), 900, 400, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 0, Modifiers::NONE);
    pick(&mut driver, &mut harness, 2, Modifiers::SHIFT);
    assert_eq!(harness.state().selected().len(), 3);

    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        app.settled() == &Place::of(GridView::Starred)
    });
    assert!(harness.state().selected().is_empty());
    assert_eq!(harness.state().lead(), None);
    assert!(harness.query_by_label_contains("selected").is_none());
    // Here Ctrl+A is the view, which the user asked for, and not a folder of it.
    key(&mut driver, &mut harness, Modifiers::COMMAND, Key::A);
    assert_eq!(harness.state().selected().len(), 400);
}

// The Escape that leaves the search box is the box's. The grid is drawn after the box,
// in the same frame, and would clear the selection with it.
#[test]
fn escape_in_the_search_box_leaves_the_selection_and_escape_in_the_grid_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);

    key(&mut driver, &mut harness, Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    // With the caret in the box the keys are the box's: Ctrl+A is its text's.
    key(&mut driver, &mut harness, Modifiers::COMMAND, Key::A);
    assert_eq!(harness.state().selected(), [ids[1]]);
    // An empty box outside a search: Escape only leaves it.
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    assert_eq!(harness.state().selected(), [ids[1]]);
    assert_eq!(harness.state().place(), Place::of(GridView::All));
    // And now the key is the grid's.
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.state().selected().is_empty());
}

// Escape closes the nearest thing, and that is all it does: the panel that lists what
// the search box understands is open, the keys are not in the box, and the grid under
// the panel keeps what is selected in it.
#[test]
fn escape_closes_the_search_help_before_it_clears_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    click(&mut driver, &mut harness, "What you can search for");
    driver.until(&mut harness, "the help open", |app| app.search_help_open());

    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    assert!(!harness.state().search_help_open());
    assert_eq!(harness.state().selected(), [ids[1]]);
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.state().selected().is_empty());
}

#[test]
fn the_keys_move_the_lead_through_the_library_and_the_grid_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    let in_sight = |harness: &Harness<'_>, id: i64| {
        let frame = harness.state().last_frame().unwrap();
        frame.on_screen.contains(&id)
    };

    key(&mut driver, &mut harness, Modifiers::NONE, Key::End);
    assert_eq!(harness.state().selected(), [ids[899]]);
    driver.act(&mut harness);
    assert!(in_sight(&harness, ids[899]));
    assert!(!in_sight(&harness, ids[0]));

    key(&mut driver, &mut harness, Modifiers::SHIFT, Key::ArrowLeft);
    key(&mut driver, &mut harness, Modifiers::SHIFT, Key::ArrowLeft);
    assert_eq!(harness.state().selected(), ids[897..]);
    assert_eq!(harness.state().lead(), Some(ids[897]));

    key(&mut driver, &mut harness, Modifiers::NONE, Key::Home);
    assert_eq!(harness.state().selected(), [ids[0]]);
    driver.act(&mut harness);
    assert!(in_sight(&harness, ids[0]));
    assert_eq!(harness.state().last_frame().unwrap().position, 0.0);

    // In All photos, Ctrl+A is the folder the lead is in, not the library.
    key(&mut driver, &mut harness, Modifiers::COMMAND, Key::A);
    let picked = harness.state().selected().len();
    assert!((2..900).contains(&picked), "{picked}");
    assert_eq!(harness.state().lead(), Some(ids[0]));
}

#[test]
fn a_window_with_photos_selected_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    pick(&mut driver, &mut harness, 6, Modifiers::SHIFT);
    assert_eq!(harness.state().selected().len(), 6);
    driver.settle(&mut harness);
    // The pointer's own echo, a visible report: what any move of the mouse leaves.
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// ---- whose key it is: from the review ----
//
// The grid is drawn after everything else in the frame, and read the frame's keys
// whenever no text field had the keyboard. Each of these keys was answered twice: by
// what had the keyboard, and by the grid behind it.

/// Three photos selected, offsets 3 to 5, the lead on 5.
fn three<'a>(dir: &std::path::Path) -> (Harness<'a>, Driver, Vec<i64>) {
    let library = library(dir, 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    pick(&mut driver, &mut harness, 3, Modifiers::NONE);
    pick(&mut driver, &mut harness, 5, Modifiers::SHIFT);
    assert_eq!(harness.state().selected(), ids[3..=5]);
    (harness, driver, ids)
}

// The splitter keeps the keyboard a press gave it, so that the arrows move it afterwards.
#[test]
fn an_arrow_that_moves_the_splitter_does_not_move_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = three(dir.path());
    // Dragged eighty points to the right with the mouse.
    let hold = pos2(SIDEBAR_DEFAULT + 2.0, 300.0);
    let to = pos2(hold.x + 80.0, 300.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(hold));
    harness.event(button(hold, true));
    harness.event(egui::Event::PointerMoved(to));
    driver.act(&mut harness);
    harness.event(button(to, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    let dragged = SIDEBAR_DEFAULT + 80.0;
    assert_eq!(harness.state().layout().sidebar_width, dragged);
    assert_eq!(harness.state().selected(), ids[3..=5]);

    key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(
        harness.state().layout().sidebar_width,
        dragged + SIDEBAR_STEP
    );
    assert_eq!(harness.state().selected(), ids[3..=5]);
    // A click on a photo gives the keys back to the photos.
    pick(&mut driver, &mut harness, 3, Modifiers::NONE);
    key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(harness.state().selected(), [ids[4]]);
    assert_eq!(
        harness.state().layout().sidebar_width,
        dragged + SIDEBAR_STEP
    );
}

#[test]
fn the_keys_are_an_open_lists_and_not_the_grids_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = three(dir.path());
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_some());
    // Keys that do nothing in the list do nothing at all.
    for (held, pressed) in [
        (Modifiers::NONE, Key::ArrowRight),
        (Modifiers::NONE, Key::ArrowLeft),
        (Modifiers::COMMAND, Key::A),
    ] {
        key(&mut driver, &mut harness, held, pressed);
        assert!(harness.query_by_label("Date modified").is_some());
        assert_eq!(harness.state().selected(), ids[3..=5], "{pressed:?}");
    }
}

// Escape closes the nearest thing, and that is all it does. The first closes the list
// and the control keeps the keyboard; the second takes the keyboard off the control.
#[test]
fn the_escape_that_leaves_a_control_does_not_clear_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = three(dir.path());
    let focused = |harness: &Harness<'_>| {
        (harness.get_by_label("Sort by: Date taken"))
            .accesskit_node()
            .is_focused()
    };
    click(&mut driver, &mut harness, "Sort by: Date taken");
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    assert!(harness.query_by_label("Date modified").is_none());
    assert!(focused(&harness));
    assert_eq!(harness.state().selected(), ids[3..=5]);

    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    assert!(!focused(&harness));
    assert_eq!(harness.state().selected(), ids[3..=5]);
    // And the third, with the keyboard nowhere, is the grid's.
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.state().selected().is_empty());
}

// The keyboard's way round the window: Tab to a button, and the arrows walk on from it.
#[test]
fn an_arrow_that_walks_the_focus_does_not_move_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = three(dir.path());
    key(&mut driver, &mut harness, Modifiers::NONE, Key::Tab);
    driver.act(&mut harness);
    let focused = |harness: &Harness<'_>| {
        harness
            .query_all_by_role(egui::accesskit::Role::Button)
            .find(|node| node.accesskit_node().is_focused())
            .map(|node| node.accesskit_node().label().unwrap_or_default())
    };
    let first = focused(&harness);
    assert!(first.is_some(), "Tab gave no button the keyboard");
    key(&mut driver, &mut harness, Modifiers::NONE, Key::ArrowDown);
    driver.act(&mut harness);
    assert_ne!(focused(&harness), first, "the arrow did not walk on");
    assert_eq!(harness.state().selected(), ids[3..=5]);
}

// The panel lies over the photos: an arrow must not move a ring the user cannot see.
#[test]
fn the_keys_are_not_the_grids_under_the_open_search_help() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids) = three(dir.path());
    click(&mut driver, &mut harness, "What you can search for");
    driver.until(&mut harness, "the help open", |app| app.search_help_open());
    for (held, pressed) in [
        (Modifiers::NONE, Key::ArrowDown),
        (Modifiers::NONE, Key::End),
        (Modifiers::COMMAND, Key::A),
    ] {
        key(&mut driver, &mut harness, held, pressed);
        assert!(harness.state().search_help_open());
        assert_eq!(harness.state().selected(), ids[3..=5], "{pressed:?}");
    }
}

/// A library of 900 photos opened and scrolled some screens down, its thumbnails settled:
/// the photo ids, and those on screen.
fn scrolled<'a>(
    library: &photon_ui::fixture::Fixture,
) -> (Harness<'a>, Driver, Vec<i64>, Vec<i64>) {
    let ids = photo_ids(library);
    let (mut harness, mut driver) = opened(library, 900);
    harness.event(egui::Event::PointerMoved(egui::pos2(700.0, 400.0)));
    for _ in 0..3 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -1500.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(&mut harness);
        driver.settle(&mut harness);
        driver.frames_in(&mut harness, 1.0);
        driver.settle(&mut harness);
    }
    let on_screen = harness.state().last_frame().unwrap().on_screen.clone();
    let held = harness.state().grid_textures();
    assert!(
        held > 2 * on_screen.len(),
        "{held} held, {} on screen",
        on_screen.len()
    );
    (harness, driver, ids, on_screen)
}

// Under the viewer the grid draws nothing, and holds only the thumbnails it had on screen:
// those it scrolled past are let go of, and those it shows are there again at once.
#[test]
fn the_viewer_over_the_grid_lets_go_of_the_thumbnails_out_of_sight() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, ids, on_screen) = scrolled(&library);
    let middle = on_screen[on_screen.len() / 2];
    let offset = ids.iter().position(|id| *id == middle).unwrap();
    pick(&mut driver, &mut harness, offset, Modifiers::NONE);
    harness.key_press(Key::Enter);
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_some());
    assert!(harness.state().grid_textures() <= on_screen.len());
    harness.key_press(Key::Escape);
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none());
    driver.act(&mut harness);
    assert_eq!(harness.state().last_frame().unwrap().on_screen, on_screen);
    assert!(harness.state().grid_textures() >= on_screen.len());
}

// And so under compare, and under the People page, which is drawn in the grid's place.
#[test]
fn compare_and_the_people_page_over_the_grid_let_go_of_the_thumbnails_out_of_sight() {
    let dir = tempfile::tempdir().unwrap();
    let photos = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, ids, on_screen) = scrolled(&photos);
    let at = |id: i64| ids.iter().position(|of| *of == id).unwrap();
    let adding = if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    };
    pick(&mut driver, &mut harness, at(on_screen[4]), Modifiers::NONE);
    pick(&mut driver, &mut harness, at(on_screen[5]), adding);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.key_press(Key::C);
    driver.until(&mut harness, "compare open", |app| app.compare().is_some());
    driver.act(&mut harness);
    assert!(harness.state().grid_textures() <= on_screen.len());

    let dir = tempfile::tempdir().unwrap();
    let other = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver, _, on_screen) = scrolled(&other);
    click(&mut driver, &mut harness, "People");
    assert!(harness.state().people_open());
    driver.act(&mut harness);
    assert!(harness.state().grid_textures() <= on_screen.len());
}

// The driver's own clock: a frame asked for a rounding before the last one drawn is drawn
// at the last one's time, as `frames_in` and a test's own time can ask for (`Driver::frame`):
// earlier, egui's pointer history panics.
#[test]
fn a_frame_asked_for_before_the_last_is_drawn_at_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 3, 0, 0);
    let (mut harness, mut driver) = opened(&library, 3);
    let at = driver.now + 0.5396666666666668;
    harness.event(egui::Event::PointerMoved(pos2(300.0, 300.0)));
    driver.frame(&mut harness, at);
    harness.event(egui::Event::PointerMoved(pos2(310.0, 300.0)));
    driver.frame(&mut harness, at - 1e-12);
}
