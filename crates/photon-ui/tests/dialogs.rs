//! The dialogs, through the whole application and the real engine: the question asked
//! before an album or a saved search is deleted, and the keyword picker.
//!
//! Every write here goes into a library the test made, in a temporary directory, and
//! none of them outside `library.db`: a keyword is the library's, never the photo's.

use crate::common::{Driver, Harness, library, opened, photo_ids};
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
    // And until a press in it is no longer the second of a double click on the item.
    driver.frames_in(harness, 0.4);
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
    // The lists are read after the delete's answer, which can land after All photos has:
    // asserted at once, Windows' runner found the album still listed.
    driver.until(&mut harness, "the album gone from the lists", |app| {
        album_names(app).is_empty()
    });
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
    // The field has a name, for whoever reads the window without seeing it.
    assert!(harness.query_by_label("Keyword to add").is_some());
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
    // Waited for by what is said, not by how many messages stand: where the write takes
    // longer than a moment the clock goes on to the end of the first message's time, and
    // two were never there together (Windows' CI, once).
    driver.until(&mut harness, "the keyword on one photo", |app| {
        keywords(app) == [("lisbon".to_owned(), 1)]
            && (app.toasts().iter()).any(|toast| toast.message == "Removed “lisbon” from 2 photos")
    });
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
    // The count and the message come by two roads - the lists read after the engine's
    // rebuild, and the write's own answer - in either order: waited for together.
    driver.until(&mut harness, "the keyword on two photos, and said", |app| {
        keywords(app).contains(&("beach".to_owned(), 2)) && !app.toasts().is_empty()
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

// What a review of the dialogs found, each with the test that showed it.

/// The library's write lock, taken from outside: the engine's next write waits for it.
/// It stands in for a write that takes a while - every write waits for those queued
/// before it, and a folder's for a rebuild of the whole grid.
fn locked(library: &Fixture) -> rusqlite::Connection {
    let other = rusqlite::Connection::open(&library.dirs().db_path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    other
}

/// Several events in one frame, through the whole application: put into the frame's
/// input beside the queue, which is drawn a frame an event.
fn frame_with(driver: &mut Driver, harness: &mut Harness<'_>, events: Vec<egui::Event>) {
    harness.input_mut().events.extend(events);
    driver.act(harness);
}

fn search_names(app: &App) -> Vec<String> {
    (app.collections().searches.iter())
        .map(|search| search.name.clone())
        .collect()
}

/// The other button on the sidebar's row called `label`.
fn right_click_row(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    right_click_at(driver, harness, at);
}

// An album's number is handed out again once it is free. "Delete…" answered with yes
// is a write on its way, and until the lists have been read after it the row is still
// there: asked again of that row, a second delete of the same number waited behind
// whatever was made meanwhile - and deleted it. The row of something that is going has
// no menu.
#[test]
fn an_album_being_deleted_has_no_menu_until_it_has_left_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    album(&library, "Lisbon", &ids[..3]);
    let porto = album(&library, "Porto", &ids[..2]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);

    // A write that takes a while: every write queued waits behind it.
    let other = locked(&library);
    ask_to_delete(&mut driver, &mut harness, "Porto");
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    // A new album, typed in the sidebar's field: queued behind the delete.
    harness.get_by_label("New album…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "Paris");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.act(&mut harness);
    // Porto is still listed: nothing has landed. Its row has no menu to ask again with.
    assert_eq!(album_names(harness.state()), ["Lisbon", "Porto"]);
    right_click_row(&mut driver, &mut harness, "Porto");
    assert!(
        !harness.state().menu_open(),
        "a menu of an album that is going"
    );
    // Nor after the lists have been read again for another reason - a scan's rebuild -
    // while the delete is still on its way: they hold Porto still.
    harness.state().engine().refresh_grid().unwrap();
    driver.settle(&mut harness);
    driver.act(&mut harness);
    assert_eq!(album_names(harness.state()), ["Lisbon", "Porto"]);
    right_click_row(&mut driver, &mut harness, "Porto");
    assert!(!harness.state().menu_open(), "the lists read meanwhile");
    // Lisbon's has, as ever.
    right_click_row(&mut driver, &mut harness, "Lisbon");
    assert!(harness.state().menu_open());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);

    // The writes land: Porto goes, and Paris is made under the number it had.
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the lists read again", |app| {
        album_names(app) == ["Lisbon", "Paris"]
    });
    driver.settle(&mut harness);
    let paris = harness.state().collections().albums[1].id;
    assert_eq!(paris, porto, "the number is handed out again");
    // And Paris, which is not going anywhere, has its menu and its own question.
    ask_to_delete(&mut driver, &mut harness, "Paris");
    assert!(
        harness
            .query_by_label("Delete the album “Paris”? The photos stay in your library.")
            .is_some()
    );
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Lisbon", "Paris"]);
}

// The same of a saved search.
#[test]
fn a_saved_search_being_deleted_has_no_menu_until_it_has_left_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Tens", "IMG_00000", 1_700_000_000_000)
            .unwrap();
        lib.create_saved_search("Ones", "IMG_00001", 1_700_000_000_001)
            .unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the lists read", |app| {
        search_names(app).len() == 2
    });
    driver.act(&mut harness);
    let other = locked(&library);
    ask_to_delete(&mut driver, &mut harness, "Ones");
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Ones");
    assert!(!harness.state().menu_open());
    right_click_row(&mut driver, &mut harness, "Tens");
    assert!(harness.state().menu_open());
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the lists read again", |app| {
        search_names(app) == ["Tens"]
    });
}

// A question is about one album, under the name it was asked under. Deleted from
// elsewhere while the question stands - the other photon, open on the same library -
// and another album made at once, the number is the new album's: "Delete the album
// “Porto”?" answered with yes deleted Paris. A question whose album is no longer
// listed under its name is no question, and closes.
#[test]
fn a_question_whose_album_has_gone_closes_and_deletes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    album(&library, "Lisbon", &[]);
    let porto = album(&library, "Porto", &[]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);
    ask_to_delete(&mut driver, &mut harness, "Porto");
    assert!(harness.state().dialog().is_some());

    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, porto).unwrap();
    let paris = commands::create_album(&engine, "Paris").unwrap().id;
    assert_eq!(paris, porto, "the number is handed out again");
    // The lists are read when the library says its data changed.
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "the lists read again", |app| {
        album_names(app) == ["Lisbon", "Paris"]
    });
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none(), "a question about Porto");
    driver.settle(&mut harness);
    assert_eq!(album_names(harness.state()), ["Lisbon", "Paris"]);
}

// The same of a saved search, which the library deletes by its number alone.
#[test]
fn a_question_whose_saved_search_has_gone_closes_and_deletes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let tens = {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.create_saved_search("Tens", "IMG_00000", 1_700_000_000_000)
            .unwrap()
            .id
    };
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the lists read", |app| {
        search_names(app) == ["Tens"]
    });
    driver.act(&mut harness);
    ask_to_delete(&mut driver, &mut harness, "Tens");
    let engine = harness.state().engine().clone();
    commands::delete_saved_search(&engine, tens).unwrap();
    let ones = commands::save_search(&engine, "Ones", "IMG_00001")
        .unwrap()
        .id;
    assert_eq!(ones, tens, "the number is handed out again");
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "the lists read again", |app| {
        search_names(app) == ["Ones"]
    });
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_none());
    driver.settle(&mut harness);
    assert_eq!(search_names(harness.state()), ["Ones"]);
}

// A delete the library refuses - the album has gone from elsewhere, and the lists have
// not been read since - is a message, the lists read again, and a row that is a row
// again: left as "going", nothing listed under that number could be opened a menu on
// for the rest of the session.
#[test]
fn a_delete_the_library_refuses_leaves_the_rows_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    album(&library, "Lisbon", &[]);
    let porto = album(&library, "Porto", &[]);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the albums read", |app| {
        album_names(app).len() == 2
    });
    driver.act(&mut harness);
    ask_to_delete(&mut driver, &mut harness, "Porto");
    // Gone from elsewhere, and Paris made under its number, with nothing announced.
    let engine = harness.state().engine().clone();
    commands::delete_album(&engine, porto).unwrap();
    commands::delete_album(&engine, porto).unwrap_err();
    harness.get_by_label("Delete").click();
    driver.act(&mut harness);
    driver.until(
        &mut harness,
        "the refusal, and the lists read again",
        |app| !app.toasts().is_empty() && album_names(app) == ["Lisbon"],
    );
    assert_eq!(said(&harness).len(), 1);
    // An album made now takes the number, and has its menu.
    let paris = commands::create_album(&engine, "Paris").unwrap().id;
    assert_eq!(paris, porto);
    engine.refresh_grid().unwrap();
    driver.until(&mut harness, "Paris listed", |app| {
        album_names(app) == ["Lisbon", "Paris"]
    });
    driver.act(&mut harness);
    right_click_row(&mut driver, &mut harness, "Paris");
    assert!(harness.state().menu_open());
}

// A double click on the menu's item: its first click opens the picker, which stands in
// the middle of the window - where a photo's menu usually is - and its second landed
// on whichever keyword the dialog had put under the pointer, which a click writes. The
// dialog had come and gone inside the double click, and a keyword nobody chose was
// taken off the photo.
#[test]
fn the_second_click_of_a_double_click_on_a_menus_item_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        for n in 0..12 {
            lib.add_items_tag(&ids[..3], &format!("keyword {n:02}"))
                .unwrap();
        }
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the keywords read", |app| {
        keywords(app).len() == 12
    });
    driver.settle(&mut harness);
    pick(&mut driver, &mut harness, 1, Modifiers::NONE);
    // Where the item stands under a menu opened at a point: found once, so that the
    // menu can be opened where its item will be over the dialog's list.
    let first = middle(&harness, 1);
    right_click_at(&mut driver, &mut harness, first);
    let item = harness
        .get_by_label("Remove keyword from photo…")
        .rect()
        .center();
    let from_pointer = item - first;
    key(&mut driver, &mut harness, Key::Escape, Modifiers::NONE);
    assert!(!harness.state().menu_open());
    // The middle of the window, a little under it: where the picker lists keywords.
    let at = Pos2::new(500.0, 420.0) - from_pointer;
    right_click_at(&mut driver, &mut harness, at);
    assert_eq!(harness.state().selected().len(), 1);
    let item = harness
        .get_by_label("Remove keyword from photo…")
        .rect()
        .center();
    // A double click on it, as a window hears one: down, up, the frames the window
    // draws by itself, down, up.
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(item, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(matches!(harness.state().dialog(), Some(Dialog::Tags(_))));
    driver.frames_in(&mut harness, 0.15);
    harness.event(button(item, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(item, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.settle(&mut harness);
    assert_eq!(said(&harness), Vec::<String>::new());
    assert!(
        matches!(harness.state().dialog(), Some(Dialog::Tags(_))),
        "and the dialog is there to be used"
    );
    // A click that is one of its own writes, as ever.
    driver.frames_in(&mut harness, 0.4);
    harness.event(button(item, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(item, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    driver.until(&mut harness, "the keyword written", |app| {
        !app.toasts().is_empty()
    });
    assert!(said(&harness)[0].starts_with("Removed “keyword"));
}

// The first row of the list is the library's keyword of that name, whatever its case,
// and Enter writes what the first row would: written as typed, "lisbon" stood beside
// "Lisbon" as a second keyword.
#[test]
fn enter_writes_the_keyword_the_library_has_under_another_case() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.add_items_tag(&ids[..1], "Lisbon").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the keywords read", |app| {
        keywords(app).len() == 1
    });
    open_picker(&mut driver, &mut harness, (5, 5), "Add keyword to photo…");
    typed(&mut driver, &mut harness, "lisbon");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["Added “Lisbon” to 1 photo"]);
    driver.until(&mut harness, "the keyword counted", |app| {
        keywords(app) == [("Lisbon".to_owned(), 2)]
    });
}

// In the frame a dialog is first drawn in, egui keeps nothing under it from the
// pointer yet. A right press on a photo in that frame opened the photo's menu over the
// dialog - the selection behind it another photo's by then - and its "Hide photo" hid
// a photo while the dialog stood.
#[test]
fn nothing_opens_behind_a_dialog_in_the_frame_it_is_first_drawn_in() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    pick(&mut driver, &mut harness, 2, Modifiers::NONE);
    pick(&mut driver, &mut harness, 4, Modifiers::SHIFT);
    let at = middle(&harness, 2);
    right_click_at(&mut driver, &mut harness, at);
    let other = middle(&harness, 0);
    let item = harness
        .get_by_label("Add keyword to 3 photos…")
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(item, PointerButton::Primary, false, Modifiers::NONE));
    // The next frame is the dialog's first.
    harness.event(button(
        other,
        PointerButton::Secondary,
        true,
        Modifiers::NONE,
    ));
    harness.event(button(
        other,
        PointerButton::Secondary,
        false,
        Modifiers::NONE,
    ));
    driver.act(&mut harness);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    assert!(!harness.state().menu_open());
    assert_eq!(harness.state().selected(), ids[2..=4]);
    assert!(matches!(harness.state().dialog(), Some(Dialog::Tags(_))));
}

// Nor is a row of the sidebar pressed in that frame: a click that is over in it - down
// and up together, as a long frame hands them over - switched the view behind the
// dialog. egui is told of the dialog in the frame it is opened, a frame before it is
// drawn.
#[test]
fn a_click_in_the_frame_a_dialog_is_first_drawn_in_presses_nothing_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    pick(&mut driver, &mut harness, 2, Modifiers::NONE);
    let at = middle(&harness, 2);
    right_click_at(&mut driver, &mut harness, at);
    let starred = harness.get_by_label("Starred").rect().center();
    let item = harness
        .get_by_label("Add keyword to photo…")
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(item));
    harness.event(button(item, PointerButton::Primary, true, Modifiers::NONE));
    harness.event(button(item, PointerButton::Primary, false, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(harness.state().dialog().is_some());
    frame_with(
        &mut driver,
        &mut harness,
        vec![
            button(starred, PointerButton::Primary, true, Modifiers::NONE),
            button(starred, PointerButton::Primary, false, Modifiers::NONE),
        ],
    );
    driver.settle(&mut harness);
    driver.until(&mut harness, "at rest", |app| !app.busy());
    assert_eq!(harness.state().settled(), &Place::of(GridView::All));
}

// A frame can be long, and a keyword's last letter and its Enter in it together: the
// keyword written is the one with the letter.
#[test]
fn a_letter_and_enter_in_one_frame_write_the_keyword_with_the_letter() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    open_picker(&mut driver, &mut harness, (2, 2), "Add keyword to photo…");
    typed(&mut driver, &mut harness, "lisbo");
    frame_with(
        &mut driver,
        &mut harness,
        vec![egui::Event::Text("n".to_owned()), enter_down()],
    );
    assert!(harness.state().dialog().is_none());
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["Added “lisbon” to 1 photo"]);
}

// A keyword the user has renamed is stored under its new name, whatever was typed, and
// the message names the one stored.
#[test]
fn a_keyword_a_rule_renames_is_written_and_said_under_its_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let ids = photo_ids(&library);
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.add_items_tag(&ids[..1], "holiday").unwrap();
        lib.rename_tag("holiday", "vacation").unwrap();
    }
    let (mut harness, mut driver) = opened(&library, 900);
    driver.until(&mut harness, "the keywords read", |app| {
        keywords(app) == [("vacation".to_owned(), 1)]
    });
    open_picker(&mut driver, &mut harness, (5, 5), "Add keyword to photo…");
    typed(&mut driver, &mut harness, "holiday");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["Added “vacation” to 1 photo"]);
    driver.until(&mut harness, "the keyword counted", |app| {
        keywords(app) == [("vacation".to_owned(), 2)]
    });
}

// The library's count is the truth: a photo that has gone missing since the dialog
// opened is passed over, and the message says how many of those asked for it reached.
#[test]
fn the_message_says_how_many_of_the_photos_the_keyword_reached() {
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
    // Marked missing, as a scan marks a photo whose file is gone; no rebuild is asked
    // for, so the grid and the selection still hold it.
    {
        let lib = Library::open(&library.dirs().db_path).unwrap();
        lib.mark_missing(&ids[3..4], 1_700_000_000_000).unwrap();
    }
    typed(&mut driver, &mut harness, "beach");
    key(&mut driver, &mut harness, Key::Enter, Modifiers::NONE);
    driver.until(&mut harness, "the message", |app| !app.toasts().is_empty());
    assert_eq!(said(&harness), ["Added “beach” to 2 of 3 photos"]);
}
