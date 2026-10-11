//! The rubber band, through the whole application: a drag over the photos selects what
//! its rectangle touches, over the real engine.

mod common;

use common::{Driver, Harness, library, opened, photo_ids};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_ui::nav::{Place, Step};

fn button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The middle of the photo at grid offset `offset`.
fn middle(harness: &Harness<'_>, offset: usize) -> Pos2 {
    (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center()
}

/// The button down on the photo at `from` and the pointer taken to the one at `to`
/// with it held: a band, left held.
fn drag(driver: &mut Driver, harness: &mut Harness<'_>, from: usize, to: usize) -> Pos2 {
    let (from, to) = (middle(harness, from), middle(harness, to));
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, true));
    harness.event(egui::Event::PointerMoved(to));
    driver.act(harness);
    to
}

fn banding(harness: &Harness<'_>) -> bool {
    harness.state().last_frame().unwrap().banding
}

#[test]
fn a_drag_over_the_photos_selects_them_and_the_status_bar_counts_them() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    // From the first photo to the third of the same row.
    let to = drag(&mut driver, &mut harness, 0, 2);
    assert!(banding(&harness));
    assert_eq!(harness.state().selected(), ids[0..=2]);
    driver.act(&mut harness);
    assert!(harness.query_by_label("3 selected").is_some());
    // Nothing leads until the button comes up; then the band's first photo does.
    assert_eq!(harness.state().lead(), None);
    harness.event(button(to, false));
    driver.act(&mut harness);
    assert!(!banding(&harness));
    assert_eq!(harness.state().selected(), ids[0..=2]);
    assert_eq!(harness.state().lead(), Some(ids[0]));

    // The arrows go on from there.
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[1]]);
}

// A grid of other results arrives under a held band: the photos are another set, and
// the band began in the one before. Nothing of it is selected in the new grid, and the
// button coming up selects nothing either.
#[test]
fn a_band_does_not_outlive_the_grid_it_was_begun_in() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    let to = drag(&mut driver, &mut harness, 0, 2);
    assert!(banding(&harness));
    // Another view, asked for with the button still down: no key or click can ask
    // under a band, but a step asked for a moment before it began can land under it.
    harness.state_mut().go(Step::View(GridView::Recent));
    driver.until(&mut harness, "the other view shown", |app| {
        app.settled() == &Place::of(GridView::Recent)
    });
    driver.act(&mut harness);
    assert!(!banding(&harness));
    assert!(harness.state().selected().is_empty());
    harness.event(egui::Event::PointerMoved(to + egui::vec2(40.0, 40.0)));
    driver.act(&mut harness);
    harness.event(button(to + egui::vec2(40.0, 40.0), false));
    driver.act(&mut harness);
    assert!(harness.state().selected().is_empty());
    assert!(!banding(&harness));
}

#[test]
fn a_window_with_a_band_held_still_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    drag(&mut driver, &mut harness, 0, 6);
    assert!(banding(&harness));
    driver.settle(&mut harness);
    // The pointer's own echo, and the report of what is in view.
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(banding(&harness));
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A band owns the window while it is held: Escape abandons it, and every other key is
// nobody's - not the search box's, not the sidebar's.
#[test]
fn no_key_but_escape_is_answered_while_a_band_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    // One photo selected first, to be put back.
    let at = middle(&harness, 8);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, true));
    harness.event(button(at, false));
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[8]]);

    drag(&mut driver, &mut harness, 0, 2);
    assert_eq!(harness.state().selected(), ids[0..=2]);
    let hidden = harness.state().layout().sidebar_hidden;
    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    driver.act(&mut harness);
    harness.event(egui::Event::Text("mountains".to_owned()));
    driver.act(&mut harness);
    harness.key_press_modifiers(Modifiers::NONE, Key::End);
    driver.act(&mut harness);
    // Tab is read by egui before anything is drawn, and walks the keyboard to the first
    // widget that takes it: the grid's keys would be that widget's after the band.
    harness.key_press_modifiers(Modifiers::NONE, Key::Tab);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(harness.ctx.memory(|memory| memory.focused()), None);
    assert_eq!(harness.state().layout().sidebar_hidden, hidden);
    assert_eq!(harness.state().search_text(), "");
    assert!(banding(&harness));
    assert_eq!(harness.state().selected(), ids[0..=2]);
    // Escape is the band's: what was selected before it is back.
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    assert!(!banding(&harness));
    assert_eq!(harness.state().selected(), [ids[8]]);
}
