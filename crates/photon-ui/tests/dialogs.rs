//! The dialogs, through the whole application and the real engine: the question asked
//! before an album or a saved search is deleted, and the keyword picker.
//!
//! Every write here goes into a library the test made, in a temporary directory, and
//! none of them outside `library.db`: a keyword is the library's, never the photo's.

mod common;

use common::{Driver, Harness, library, opened, photo_ids};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_core::{grid::GridView, library::Library};
use photon_engine::commands;
use photon_ui::{app::App, dialogs::Dialog, fixture::Fixture, nav::Place};

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

/// The other button at `at`, and the frame its menu is first drawn in.
fn right_click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Secondary, true, Modifiers::NONE));
    harness.event(button(at, PointerButton::Secondary, false, Modifiers::NONE));
    driver.act(harness);
    driver.act(harness);
}

/// Picks the item of the open menu called `label`, and draws the dialog it opens until
/// it stands: an area is measured in one frame and shown in the next, and what lies
/// under it is kept from the pointer from the one after.
fn pick_item(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
    for _ in 0..3 {
        driver.act(harness);
    }
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key, held: Modifiers) {
    harness.key_press_modifiers(held, key);
    driver.act(harness);
}

fn typed(driver: &mut Driver, harness: &mut Harness<'_>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    driver.act(harness);
}

fn said(harness: &Harness<'_>) -> Vec<String> {
    (harness.state().toasts().iter())
        .map(|toast| toast.message.clone())
        .collect()
}

fn album(library: &Fixture, name: &str, ids: &[i64]) -> i64 {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    let id = lib.create_album(name, 1_700_000_000_000).unwrap().id;
    lib.add_to_album(id, ids, 1_700_000_000_000).unwrap();
    id
}

fn album_names(app: &App) -> Vec<String> {
    (app.collections().albums.iter())
        .map(|album| album.name.clone())
        .collect()
}

/// The library's keywords as the sidebar holds them: each with how many photos carry it.
fn keywords(app: &App) -> Vec<(String, i64)> {
    (app.collections().tags.iter())
        .map(|tag| (tag.tag.clone(), tag.count))
        .collect()
}

/// The other button on the sidebar's row called `label`, then its menu's "Delete…".
fn ask_to_delete(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    right_click_at(driver, harness, at);
    pick_item(driver, harness, "Delete…");
}

// "Delete…" asks first, and says what is not lost. "Cancel", Escape and a press beside
// the dialog delete nothing; "Delete" deletes the album, and its photos stay.
#[test]
fn an_album_is_deleted_only_after_the_question_is_answered_with_yes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let lisbon = album(&library, "Lisbon", &ids[..3]);
    album(&library, "Porto", &[]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);

    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    assert!(!harness.state().menu_open());
    assert!(matches!(harness.state().dialog(), Some(Dialog::Confirm(_))));
    assert!(
        harness
            .query_by_label("Delete the album “Lisbon”? The photos stay in your library.")
            .is_some()
    );
    // The keyboard is on "Cancel": Enter deletes nothing.
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().dialog().is_none());
    // Nor does Escape, nor a press on the scrim, nor the button itself.
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert!(harness.state().dialog().is_none());
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    let beside = Pos2::new(990.0, 690.0);
    harness.event(egui::Event::PointerMoved(beside));
    harness.event(button(
        beside,
        PointerButton::Primary,
        true,
        Modifiers::NONE,
    ));
    harness.event(button(
        beside,
        PointerButton::Primary,
        false,
        Modifiers::NONE,
    ));
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    harness.get_by_label("Cancel").click();
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Lisbon", "Porto"]);

    // Yes.
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    driver.until(&mut harness, "the album gone", |app| {
        album_names(app) == ["Porto"]
    });
    assert_eq!(harness.state().photos(), 900);
    assert_eq!(said(&harness), Vec::<String>::new());
    let lib = Library::open(&library.dirs().db_path).unwrap();
    assert!(lib.album(lisbon).unwrap().is_none());
    assert_eq!(lib.grid_entries().unwrap().len(), 900);
}

// The album on screen deleted: the engine has emptied its grid, and All photos is the
// place to land.
#[test]
fn deleting_the_album_on_screen_goes_to_all_photos() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    album(&library, "Lisbon", &ids[..3]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    harness.get_by_label("Lisbon").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the album shown", |app| {
        app.settled().view == GridView::Album && app.photos() == 3
    });
    driver.act(&mut harness);
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    // By the keyboard: Tab to "Delete", Enter.
    key(&mut driver, &mut harness, Key::Tab, Modifiers::NONE);
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "All photos shown", |app| {
        app.settled() == &Place::of(GridView::All) && app.photos() == 900 && !app.busy()
    });
    assert!(album_names(harness.state()).is_empty());
}

// A saved search is a name over a query: deleting it leaves the grid alone, the photos
// on screen being still the answer to what was typed.
#[test]
fn a_saved_search_is_deleted_and_the_grid_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Tens", "IMG_00000", 1_700_000_000_000)
            .unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the lists read", |app| {
        app.collections().searches.len() == 1
    });
    driver.act(&mut harness);
    harness.get_by_label("Tens").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the search shown", |app| {
        app.settled().view == GridView::Search && app.photos() == 10
    });
    driver.act(&mut harness);
    ask_to_delete(&mut driver, &mut harness, "Tens");
    assert!(
        harness
            .query_by_label("Delete the saved search “Tens”? Your photos are not affected.")
            .is_some()
    );
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the search gone from the list", |app| {
        app.collections().searches.is_empty()
    });
    driver.settle(&mut harness);
    assert_eq!(harness.state().settled().view, GridView::Search);
    assert_eq!(harness.state().photos(), 10);
}

/// Selects the photos at `from..=to` and opens the keyword picker for them from the
/// tile's menu.
fn open_picker(
    driver: &mut Driver,
    harness: &mut Harness<'_>,
    (from, to): (usize, usize),
    item: &str,
) {
    pick(driver, harness, from, Modifiers::NONE);
    if to != from {
        pick(driver, harness, to, Modifiers::SHIFT);
    }
    let at = middle(harness, from);
    right_click_at(driver, harness, at);
    pick_item(driver, harness, item);
}

// A keyword typed is written to the photos selected, and the Tags list has it with
// their count; the message says what was written. Taken off again from the list.
#[test]
fn a_keyword_is_added_to_the_selection_and_removed_again() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);

    open_picker(
        &mut driver,
        &mut harness,
        (2, 4),
        "Add keyword to 3 photos…",
    );
    assert!(!harness.state().menu_open());
    assert!(matches!(harness.state().dialog(), Some(Dialog::Tags(_))));
    typed(&mut driver, &mut harness, " lisbon ");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    // Closed before its write: the write is the last word on it.
    assert!(harness.state().dialog().is_none());
    driver.until(&mut harness, "the keyword listed", |app| {
        keywords(app) == [("lisbon".to_owned(), 3)] && !app.toasts().is_empty()
    });
    assert_eq!(said(&harness), ["Added “lisbon” to 3 photos"]);
    // The selection is what it was, and the keys are the grid's again.
    assert_eq!(harness.state().selected(), ids[2..=4]);
    driver.settle(&mut harness);
    key(&mut driver, &mut harness, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[5]]);
    let lib = Library::open(&library.dirs().db_path).unwrap();
    assert_eq!(lib.item_tags(ids[3]).unwrap(), ["lisbon"]);
    assert!(lib.item_tags(ids[5]).unwrap().is_empty());

    // Off two of the three, by its row in the list of the library's keywords.
    open_picker(
        &mut driver,
        &mut harness,
        (3, 4),
        "Remove keyword from 2 photos…",
    );
    // A dialog opened after another has the keyboard in its own field, as the first
    // had: what is typed narrows its list.
    typed(&mut driver, &mut harness, "lis");
    assert!(matches!(
        harness.state().dialog(),
        Some(Dialog::Tags(picker)) if picker.draft() == "lis"
    ));
    harness.get_by_label("lisbon").click();
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    driver.until(&mut harness, "the keyword on one photo", |app| {
        keywords(app) == [("lisbon".to_owned(), 1)] && app.toasts().len() == 2
    });
    assert_eq!(said(&harness)[1], "Removed “lisbon” from 2 photos");
}

// The photos are the ones selected when the dialog opened, whatever a scan takes away
// behind it: what its title said it would write to is what it writes to, and the
// message says how many of them it reached.
#[test]
fn a_keyword_is_written_to_the_photos_the_dialog_opened_for() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    open_picker(
        &mut driver,
        &mut harness,
        (2, 4),
        "Add keyword to 3 photos…",
    );
    // One of the three is hidden from elsewhere while the dialog is up: it leaves the
    // grid and the selection, and the dialog still says three.
    let engine = harness.state().engine().clone();
    commands::set_items_hidden(&engine, &[ids[3]], true).unwrap();
    driver.until(&mut harness, "one hidden", |app| app.photos() == 899);
    driver.act(&mut harness);
    assert_eq!(harness.state().selected(), [ids[2], ids[4]]);
    typed(&mut driver, &mut harness, "beach");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    // A hidden photo is a photo still: the keyword is on all three.
    assert_eq!(said(&harness), ["Added “beach” to 3 photos"]);
    let lib = Library::open(&library.dirs().db_path).unwrap();
    for id in &ids[2..=4] {
        assert_eq!(lib.item_tags(*id).unwrap(), ["beach"], "{id}");
    }
    // The sidebar counts the two that are not hidden.
    driver.until(&mut harness, "the keyword listed", |app| {
        keywords(app) == [("beach".to_owned(), 2)]
    });
}

// A blank name writes nothing and leaves the dialog as it is; and a name that is in
// the library under another case is offered first, so that "Beach" does not come to
// stand beside "beach".
#[test]
fn a_blank_name_writes_nothing_and_the_list_offers_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.add_items_tag(&ids[..1], "beach").unwrap();
        lib.add_items_tag(&ids[..1], "Lisbon").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the keywords read", |app| {
        keywords(app).len() == 2
    });
    open_picker(&mut driver, &mut harness, (5, 5), "Add keyword to photo…");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().dialog().is_some());
    typed(&mut driver, &mut harness, "   ");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().dialog().is_some());
    typed(&mut driver, &mut harness, "BEA");
    driver.act(&mut harness);
    // The list is narrowed to it: "Lisbon" is not offered, in the dialog or anywhere
    // but the sidebar's own row.
    assert!(harness.query_all_by_label("beach").count() >= 1);
    harness
        .query_all_by_label("beach")
        .last()
        .expect("its row in the dialog")
        .click();
    driver.act(&mut harness);
    driver.until(&mut harness, "the keyword on two photos", |app| {
        keywords(app).contains(&("beach".to_owned(), 2))
    });
    assert_eq!(said(&harness), ["Added “beach” to 1 photo"]);
}

// Under a dialog nothing takes a key or a press: not the grid, not the search box, not
// the sidebar.
#[test]
fn nothing_behind_an_open_dialog_answers_a_key_or_a_press() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    album(&library, "Lisbon", &[]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app) == ["Lisbon"]
    });
    driver.act(&mut harness);
    pick(&mut driver, &mut harness, 4, Modifiers::NONE);
    ask_to_delete(&mut driver, &mut harness, "Lisbon");
    let hidden = harness.state().layout().sidebar_hidden;
    for (pressed, held) in [
        (Key::H, Modifiers::NONE),
        (Key::Period, Modifiers::NONE),
        (Key::ArrowRight, Modifiers::NONE),
        (Key::End, Modifiers::SHIFT),
        (Key::A, Modifiers::COMMAND),
        (Key::B, Modifiers::COMMAND),
        (Key::F, Modifiers::COMMAND),
    ] {
        key(&mut driver, &mut harness, pressed, held);
        assert!(harness.state().dialog().is_some(), "{pressed:?}");
        assert_eq!(harness.state().selected(), [ids[4]], "{pressed:?}");
    }
    harness.event(egui::Event::Text("mountains".to_owned()));
    driver.act(&mut harness);
    assert_eq!(harness.state().search_text(), "");
    assert_eq!(harness.state().layout().sidebar_hidden, hidden);
    // The other button on a photo behind it closes the dialog, as any press on the
    // scrim does, and opens no menu and selects nothing.
    let at = middle(&harness, 8);
    right_click_at(&mut driver, &mut harness, at);
    assert!(harness.state().dialog().is_none());
    assert!(!harness.state().menu_open());
    assert_eq!(harness.state().selected(), [ids[4]]);
    driver.settle(&mut harness);
    assert_eq!(harness.state().photos(), 900);
    assert_eq!(album_names(harness.state()), ["Lisbon"]);
    assert_eq!(said(&harness), Vec::<String>::new());
    // Closed, the keys are the grid's.
    key(&mut driver, &mut harness, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(harness.state().selected(), [ids[5]]);
}

#[test]
fn a_window_with_a_dialog_open_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    open_picker(&mut driver, &mut harness, (2, 2), "Add keyword to photo…");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert!(harness.state().dialog().is_some());
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

fn enter_down() -> egui::Event {
    egui::Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

// "Delete…" picked with Enter, and the key held a moment longer: the question opens
// with the keyboard on "Cancel", and egui presses a button the keyboard is on at every
// Enter, repeats among them - the question closed under the key that asked it. And the
// held Enter that answered a question presses nothing after it.
#[test]
fn the_enter_that_asks_the_question_does_not_answer_it_at_its_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    album(&library, "Lisbon", &[]);
    album(&library, "Porto", &[]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);
    let at = harness.get_by_label("Lisbon").rect().center();
    right_click_at(&mut driver, &mut harness, at);
    key(&mut driver, &mut harness, Key::End, Modifiers::NONE);
    // Enter goes down, and stays down.
    harness.event(enter_down());
    driver.act(&mut harness);
    assert!(!harness.state().menu_open());
    driver.act(&mut harness);
    assert!(matches!(harness.state().dialog(), Some(Dialog::Confirm(_))));
    // The keyboard's repeats of the key that is still down.
    for _ in 0..3 {
        harness.event(enter_down());
        driver.act(&mut harness);
    }
    assert!(
        harness.state().dialog().is_some(),
        "the repeat pressed Cancel"
    );
    // Let go of, and pressed: now it is an answer.
    harness.event(egui::Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    assert!(harness.state().dialog().is_none());
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Lisbon", "Porto"]);
}
