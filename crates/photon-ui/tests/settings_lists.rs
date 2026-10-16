//! Settings' Folders and Tags, through the whole application and the real engine, over a
//! library of real photos the test made in a temporary directory. A folder removed keeps
//! its files, and a keyword renamed or removed is a rule in `library.db`: nothing here
//! writes outside it.

use crate::common::{Driver, Harness, Picker, drop_on, hold_over, over_with, photos_in};
use eframe::egui::{self, Key, Modifiers, accesskit::Role};
use egui_kittest::kittest::Queryable;
use photon_engine::commands;
use photon_ui::{
    app::App,
    dialogs::Dialog,
    export_dialog::Dest,
    settings::{Question, Settings},
    tag_manager, watched_list,
};
use std::path::{Path, PathBuf};

fn settings(app: &App) -> Option<&Settings> {
    match app.dialog() {
        Some(Dialog::Settings(settings)) => Some(&**settings),
        _ => None,
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

/// A library watching a folder of two real photos, shown; Settings open at `section`. No
/// folder picker can be opened there, so "Add folder…" opens the field a folder's path is
/// typed in (`add_field`).
fn opened_at<'a>(dir: &Path, section: &str) -> (Harness<'a>, Driver, PathBuf) {
    let pictures = photos_in(dir, "Pictures", &["a.jpg", "b.jpg"]);
    let picker = std::sync::Arc::new(Picker::default());
    *picker.answer.lock().unwrap() = photon_ui::platform::Picked::Unavailable;
    let (mut harness, mut driver, _) = over_with(dir, Some(&pictures), picker);
    driver.until(&mut harness, "the photos shown", |app| {
        app.photos() == 2 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.get_by_label("Settings").click();
    driver.act(&mut harness);
    driver.until(&mut harness, "Settings read", |app| {
        settings(app).is_some_and(|settings| settings.rules.is_some())
    });
    driver.settle(&mut harness);
    // Past a double click's time of its first frame (`DialogView::deaf`).
    driver.frames_in(&mut harness, 0.5);
    press(&mut driver, &mut harness, section);
    (harness, driver, pictures)
}

/// The dialog's button called `label`: the sidebar and the top bar under it may have one
/// of that name, and the dialog stands in the middle of the window, 720 by 520 in a window
/// 1000 by 700. With several in the dialog, the one nearest the words `near`.
fn press_near(driver: &mut Driver, harness: &mut Harness<'_>, label: &str, near: Option<&str>) {
    let y = near.map(|words| harness.get_by_label(words).rect().center().y);
    let nodes = harness.query_all_by_role_and_label(Role::Button, label);
    let node = (nodes.into_iter())
        .filter(|node| node.rect().left() > 140.0 && node.rect().top() > 80.0)
        .min_by(|a, b| {
            let off = |node: &egui_kittest::Node<'_>| {
                y.map_or(0.0, |y| (node.rect().center().y - y).abs())
            };
            off(a).total_cmp(&off(b))
        })
        .unwrap_or_else(|| panic!("no button {label} in the dialog"));
    node.click();
    driver.act(harness);
    driver.settle(harness);
}

fn press(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    press_near(driver, harness, label, None);
}

/// "Add folder…" pressed where no picker opens, and the field it then opens.
fn add_field(driver: &mut Driver, harness: &mut Harness<'_>) {
    press(driver, harness, watched_list::ADD_FOLDER);
    driver.until(harness, "the field open", |app| {
        settings(app).is_some_and(|settings| settings.adding.is_some())
    });
}

/// A folder's path as the library stores it: on a Mac a temporary directory is under
/// `/var`, which is a link to `/private/var`, and the library keeps where a path leads.
fn stored(path: &std::path::Path) -> String {
    let path = photon_core::paths::canonicalize(path).unwrap();
    path.to_str().unwrap().to_owned()
}

fn watched(app: &App) -> Vec<String> {
    app.watched()
        .iter()
        .map(|folder| folder.path.clone())
        .collect()
}

// A folder's path typed into the field "Add folder…" opens where no picker can be
// opened, asked about as it is typed,
// and added by Enter: watched, scanned, and the field closed.
#[test]
fn a_folder_is_added_by_its_typed_path() {
    let dir = tempfile::tempdir().unwrap();
    let more = photos_in(dir.path(), "More", &["c.jpg"]);
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    add_field(&mut driver, &mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, more.to_str().unwrap());
    driver.until(&mut harness, "the path passed", |app| {
        settings(app)
            .and_then(|settings| settings.adding.as_ref())
            .is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_)))
    });
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(
        &mut harness,
        "the folder watched and its photos shown",
        |app| app.photos() == 3 && settings(app).is_some_and(|settings| settings.adding.is_none()),
    );
    assert!(watched(harness.state()).contains(&stored(&more)));
}

// A folder the library will not watch - inside one it watches already - keeps its path in
// the field, the library's words under it.
#[test]
fn a_folder_the_library_refuses_keeps_its_path_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, pictures) = opened_at(dir.path(), "Folders");
    let inner = pictures.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    add_field(&mut driver, &mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, inner.to_str().unwrap());
    driver.until(&mut harness, "the path passed", |app| {
        settings(app)
            .and_then(|settings| settings.adding.as_ref())
            .is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_)))
    });
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(
        &mut harness,
        "the library's refusal under the field",
        |app| {
            settings(app)
                .and_then(|settings| settings.adding.as_ref())
                .is_some_and(|adding| adding.problem().is_some() && !adding.busy())
        },
    );
    let adding = settings(harness.state())
        .and_then(|s| s.adding.clone())
        .unwrap();
    assert_eq!(adding.path(), inner.to_str().unwrap());
    assert_eq!(watched(harness.state()).len(), 1);
}

// A folder dropped on the window while the field is open is put in the field, and
// nothing is watched until it is added.
#[test]
fn a_folder_dropped_under_the_field_is_put_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let more = photos_in(dir.path(), "More", &["c.jpg"]);
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    add_field(&mut driver, &mut harness);
    drop_on(&mut driver, &mut harness, &[&more]);
    driver.until(&mut harness, "the path asked about", |app| {
        settings(app)
            .and_then(|settings| settings.adding.as_ref())
            .is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_)))
    });
    let adding = settings(harness.state())
        .and_then(|s| s.adding.clone())
        .unwrap();
    assert_eq!(adding.path(), more.to_str().unwrap());
    driver.settle(&mut harness);
    assert_eq!(
        watched(harness.state()).len(),
        1,
        "nothing watched by the drop"
    );
}

// Remove… asks in the folder's row; the answer takes the folder out of the library and
// its files stay where they are.
#[test]
fn a_folder_is_removed_after_its_row_asks_and_its_files_stay() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, pictures) = opened_at(dir.path(), "Folders");
    press(&mut driver, &mut harness, watched_list::REMOVE);
    assert!(matches!(
        settings(harness.state()).and_then(Settings::question),
        Some(Question::RemoveFolder { .. })
    ));
    assert!(!watched(harness.state()).is_empty(), "asked, not removed");
    press(&mut driver, &mut harness, watched_list::REMOVE_YES);
    driver.until(&mut harness, "the folder no longer watched", |app| {
        app.watched().is_empty() && app.photos() == 0
    });
    assert!(pictures.join("a.jpg").is_file() && pictures.join("b.jpg").is_file());
}

/// Keywords put on the library's photos: `hiking` on both, `lake` on one.
fn keyworded(driver: &mut Driver, harness: &mut Harness<'_>) {
    let engine = harness.state().engine().clone();
    let ids: Vec<i64> = (engine.grid().1.rows(0, 2).iter())
        .map(|entry| entry.id)
        .collect();
    commands::add_items_tag(&engine, &ids, "hiking").unwrap();
    commands::add_items_tag(&engine, &ids[..1], "lake").unwrap();
    driver.until(harness, "the keywords listed", |app| {
        app.collections().tags.len() == 2
    });
    driver.settle(harness);
}

fn keywords(app: &App) -> Vec<String> {
    (app.collections().tags.iter())
        .map(|tag| tag.tag.clone())
        .collect()
}

// A keyword renamed in its row is renamed in the library, listed under Changes, and
// restored from there.
#[test]
fn a_keyword_is_renamed_and_restored() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    keyworded(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    press_near(
        &mut driver,
        &mut harness,
        tag_manager::RENAME,
        Some("hiking"),
    );
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "walks");
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "renamed, and listed under Changes", |app| {
        keywords(app).contains(&"walks".to_owned())
            && settings(app).is_some_and(|settings| {
                settings.renamer.editing().is_none()
                    && settings
                        .rules
                        .as_ref()
                        .is_some_and(|rules| rules.len() == 1)
            })
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("hiking → walks").is_some());
    press(&mut driver, &mut harness, tag_manager::RESTORE);
    driver.until(&mut harness, "restored", |app| {
        keywords(app).contains(&"hiking".to_owned())
            && settings(app)
                .is_some_and(|settings| settings.rules.as_ref().is_some_and(Vec::is_empty))
    });
}

// A keyword renamed onto one that is there asks in its row first: Cancel keeps both, the
// merge makes them one.
#[test]
fn a_rename_onto_another_keyword_is_a_merge_that_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    keyworded(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    press_near(&mut driver, &mut harness, tag_manager::RENAME, Some("lake"));
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "hiking");
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    assert!(matches!(
        settings(harness.state()).and_then(Settings::question),
        Some(Question::Rename(_))
    ));
    // The keyboard is on Cancel: Enter keeps both, and the name typed stays.
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    let renamer = &settings(harness.state()).unwrap().renamer;
    assert_eq!(
        (renamer.editing(), renamer.text()),
        (Some("lake"), "hiking")
    );
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    press(&mut driver, &mut harness, tag_manager::MERGE_YES);
    driver.until(&mut harness, "merged", |app| keywords(app) == ["hiking"]);
}

// Remove… asks in the keyword's row; the answer takes it out of photon - the photos
// keep it - and Changes lists it.
#[test]
fn a_keyword_is_removed_after_its_row_asks() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    keyworded(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    press_near(&mut driver, &mut harness, tag_manager::REMOVE, Some("lake"));
    assert_eq!(
        settings(harness.state()).and_then(Settings::question),
        Some(&Question::RemoveTag("lake".to_owned()))
    );
    press(&mut driver, &mut harness, tag_manager::REMOVE_YES);
    driver.until(&mut harness, "removed, and listed under Changes", |app| {
        keywords(app) == ["hiking"]
            && settings(app).is_some_and(|settings| {
                settings
                    .rules
                    .as_ref()
                    .is_some_and(|rules| rules.len() == 1)
            })
    });
    driver.act(&mut harness);
    assert!(harness.query_by_label("lake — removed").is_some());
}

// What changes while Settings is open from elsewhere - a keyword removed by another
// window of the library's, a scan - is read again with the sidebar's lists.
#[test]
fn settings_follows_a_change_made_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    keyworded(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    let engine = harness.state().engine().clone();
    commands::hide_tag(&engine, "lake").unwrap();
    driver.until(&mut harness, "the change listed", |app| {
        settings(app).is_some_and(|settings| {
            settings
                .rules
                .as_ref()
                .is_some_and(|rules| rules.len() == 1)
        })
    });
}

// What a review of the two sections found.

// "Rename" pressed on another keyword's row while a merge's question stands is that
// question's Cancel: left standing, it was drawn under the new field with the first
// rename's words, and its "Merge" did nothing (`agreed` finds nothing asked). A rename's
// question stands exactly while the renamer is asking.
#[test]
fn a_rename_begun_on_another_row_takes_the_merge_question_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    keyworded(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    press_near(&mut driver, &mut harness, tag_manager::RENAME, Some("lake"));
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, "hiking");
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    assert!(matches!(
        settings(harness.state()).and_then(Settings::question),
        Some(Question::Rename(_))
    ));
    press_near(
        &mut driver,
        &mut harness,
        tag_manager::RENAME,
        Some("hiking"),
    );
    driver.act(&mut harness);
    let settings = settings(harness.state()).unwrap();
    assert_eq!(settings.renamer.editing(), Some("hiking"));
    assert!(
        !matches!(settings.question(), Some(Question::Rename(_)))
            || settings.renamer.asking().is_some(),
        "a merge question about “lake” stands under “hiking”'s field, and its Merge does \
         nothing: {:?}",
        settings.question().map(Question::words)
    );
}

// A question takes Escape only while it is on screen: asked in a folder's row and left
// there for another section, `claims_escape` took the Escape that closes Settings to
// answer a question nobody could see.
#[test]
fn escape_in_another_section_closes_settings_and_answers_no_unseen_question() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    press(&mut driver, &mut harness, watched_list::REMOVE);
    assert!(matches!(
        settings(harness.state()).and_then(Settings::question),
        Some(Question::RemoveFolder { .. })
    ));
    press(&mut driver, &mut harness, "Tags");
    key(&mut driver, &mut harness, Key::Escape);
    driver.act(&mut harness);
    assert!(
        settings(harness.state()).is_none(),
        "Escape in Tags answered the question in Folders' row and left Settings open"
    );
}

// A folder whose removal is on its way offers no Remove… until the list is read again. The
// row stays until then - for a folder of many photos, seconds of cascade - and a watched
// folder's number is handed out again once it is free (no AUTOINCREMENT): a folder added
// meanwhile took it, and a second Remove… on the row, still naming the old path, removed
// the folder just added. The library's write lock is held (`tests/status.rs`'s seam) so
// that the three writes wait on the queue together.
#[test]
fn a_folder_whose_removal_is_on_its_way_is_not_removed_again_under_its_reused_id() {
    let dir = tempfile::tempdir().unwrap();
    let more = photos_in(dir.path(), "More", &["c.jpg"]);
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    let made = photon_ui::dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    let other = rusqlite::Connection::open(&made.db_path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    press(&mut driver, &mut harness, watched_list::REMOVE);
    press(&mut driver, &mut harness, watched_list::REMOVE_YES);
    // A folder added while the removal waits.
    add_field(&mut driver, &mut harness);
    driver.act(&mut harness);
    typed(&mut driver, &mut harness, more.to_str().unwrap());
    driver.until(&mut harness, "the path passed", |app| {
        settings(app)
            .and_then(|settings| settings.adding.as_ref())
            .is_some_and(|adding| matches!(adding.dest(), Dest::Passed(_)))
    });
    key(&mut driver, &mut harness, Key::Enter);
    // The stale row still offers Remove…; pressed again, and agreed to.
    press(&mut driver, &mut harness, watched_list::REMOVE);
    if settings(harness.state()).is_some_and(|settings| settings.question().is_some()) {
        press(&mut driver, &mut harness, watched_list::REMOVE_YES);
    }
    other.execute_batch("ROLLBACK").unwrap();
    driver.until(&mut harness, "the folder added", |app| {
        settings(app).is_some_and(|settings| settings.adding.is_none())
    });
    driver.settle(&mut harness);
    driver.settle(&mut harness);
    let engine = harness.state().engine().clone();
    let watched: Vec<String> = (engine.lib.watched_folders().unwrap().into_iter())
        .map(|folder| folder.path)
        .collect();
    assert_eq!(
        watched,
        [stored(&more)],
        "the folder just added was removed by a second Remove… of the folder before it"
    );
}

// A folder dropped while the field a folder is typed in is open but not on screen -
// another section shown; the field is the dialog's, not the section's - is the window's,
// and watched: put in the field, nothing was watched and nothing said.
#[test]
fn a_folder_dropped_while_the_add_field_is_out_of_sight_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let more = photos_in(dir.path(), "More", &["c.jpg"]);
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    add_field(&mut driver, &mut harness);
    press(&mut driver, &mut harness, "Tags");
    drop_on(&mut driver, &mut harness, &[&more]);
    driver.settle(&mut harness);
    driver.act(&mut harness);
    let app = harness.state();
    let shown = settings(app).map(|settings| settings.section);
    assert!(
        app.watched().len() == 2 || shown == Some(photon_ui::settings::Section::Folders),
        "dropped into a field nobody can see: watched {:?}, section {shown:?}, field {:?}",
        watched(app),
        settings(app).and_then(|settings| settings.adding.as_ref().map(|a| a.path().to_owned()))
    );
}

// The card says what letting go will do (`App::ui`): over Settings' field for a folder to
// add, that its path is put in the field - where it said the folders would be added,
// which letting go there does not do.
#[test]
fn the_card_over_the_add_field_says_what_a_drop_does_there() {
    let dir = tempfile::tempdir().unwrap();
    let more = photos_in(dir.path(), "More", &["c.jpg"]);
    let (mut harness, mut driver, _) = opened_at(dir.path(), "Folders");
    add_field(&mut driver, &mut harness);
    hold_over(&mut driver, &mut harness, &[&more]);
    driver.act(&mut harness);
    assert!(
        harness
            .query_by_label(photon_ui::folder_drop::CARD_ADD)
            .is_some(),
        "the card says what a drop does over the field"
    );
}
