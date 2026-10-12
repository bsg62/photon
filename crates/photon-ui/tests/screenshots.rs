//! The window as it is drawn, written to PNG without a window: the application itself,
//! over a library made of the CC0 photos the Svelte screenshots use (credited in their
//! `CREDITS.md`), rendered off screen through wgpu.
//!
//! Ignored by default - it needs a GPU adapter, which a CI runner need not have - and run
//! by `cargo run -p xtask -- native-shot`. Like `xtask screenshots` it replaces no item of
//! the smoke checklist: it is one renderer's picture, read by whoever runs it.
//!
//! Three of the folders are named in other scripts on purpose. What their headers show is
//! what the pull request reports about text (`src/text.rs`).

use eframe::egui::{self, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use photon_core::{
    grid::GridView,
    library::{Library, ThemeChoice},
};
use photon_engine::commands;
use photon_ui::{app::App, dirs, export_dialog::Dest};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The folders of the library, how many of the photos each takes, and the day its photos
/// are dated: the files carry no capture date, so photon reads the day they were last
/// written, and copied here that would be today for all of them - one month, one year,
/// and no year strip to draw.
const FOLDERS: [(&str, usize, (i16, i8, i8)); 4] = [
    ("2026-07 Coast", 7, (2026, 7, 14)),
    ("東京 2024 桜", 5, (2024, 4, 3)),
    ("رحلة-الصيف (2024)", 5, (2024, 8, 21)),
    ("🎉 Party שלום-עולם abc", 6, (2025, 12, 31)),
];

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Copies the photos into the four folders under `pictures`.
fn library(pictures: &Path) {
    let source = manifest().join("../xtask/screenshots/photos");
    let mut photos: Vec<PathBuf> = std::fs::read_dir(&source)
        .unwrap_or_else(|err| panic!("{}: {err}", source.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    photos.sort();
    let mut photos = photos.into_iter();
    for (folder, count, (year, month, day)) in FOLDERS {
        let folder = pictures.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let noon = jiff::civil::date(year, month, day)
            .at(12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp();
        for (n, photo) in photos.by_ref().take(count).enumerate() {
            let copy = folder.join(photo.file_name().unwrap());
            std::fs::copy(&photo, &copy).unwrap();
            // A photo every ten minutes from noon.
            let taken = std::time::SystemTime::from(noon) + Duration::from_secs(600 * n as u64);
            std::fs::File::options()
                .write(true)
                .open(&copy)
                .unwrap()
                .set_modified(taken)
                .unwrap();
        }
    }
}

type Harness<'a> = egui_kittest::Harness<'a, App>;

/// Held while a picture's GPU device is made. Every picture is drawn on a thread of its
/// own, and the Vulkan loader is not safe to enter from many at once: with two dozen
/// devices being made in the same moment the run crashed inside the loader
/// (`vkEnumerateInstanceExtensionProperties`, a jump to nought) about once in four. One
/// device is made at a time; the pictures are drawn side by side all the same.
static DEVICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A window of the pictures' size that renders through a GPU adapter, off screen.
fn window(theme: Option<egui::Theme>) -> egui_kittest::HarnessBuilder<App> {
    let builder = egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 1000.0))
        .with_pixels_per_point(1.0);
    let builder = match theme {
        Some(theme) => builder.with_theme(theme),
        None => builder,
    };
    let _one_at_a_time = DEVICES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    builder.wgpu()
}

/// Steps the application until `shown`, giving its other threads the time they take.
fn until(harness: &mut Harness<'_>, what: &str, shown: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        harness.step();
        if shown(harness.state()) {
            break;
        }
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Two albums, a saved search and three keywords, as a library someone has used has
/// them. One keyword is in Hebrew: the sidebar's names go through `text.rs` too.
fn collect(harness: &mut Harness<'_>, photos: usize) {
    let engine = harness.state().engine().clone();
    let ids: Vec<i64> = (engine.grid().1.rows(0, photos).iter())
        .map(|entry| entry.id)
        .collect();
    let best = commands::create_album(&engine, "Best of the Alps").unwrap();
    commands::add_to_album(&engine, best.id, &ids[..9]).unwrap();
    let print = commands::create_album(&engine, "To print").unwrap();
    commands::add_to_album(&engine, print.id, &ids[4..7]).unwrap();
    commands::save_search(&engine, "Taken in July", "2026-07").unwrap();
    commands::add_items_tag(&engine, &ids[..12], "mountains").unwrap();
    commands::add_items_tag(&engine, &ids[3..8], "hiking").unwrap();
    commands::add_items_tag(&engine, &ids[10..12], "פרחים").unwrap();
    // The lists are read when the library says its data changed.
    engine.refresh_grid().unwrap();
    until(harness, "the collections read", |app| {
        let read = app.collections();
        read.albums.len() == 2 && read.searches.len() == 1 && read.tags.len() == 3
    });
}

/// The library opened in `theme`, then whatever `then` does to it, as `name`.
fn shot(theme: ThemeChoice, name: &str, then: impl Fn(&mut Harness<'_>)) {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    library(&pictures);
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    // The theme twice. Stored, which is where the application reads it from before its
    // first frame; and given to the harness, which sets one of its own - dark, unless told -
    // after the application has been made, and would undo the first.
    std::fs::create_dir_all(dirs.db_path.parent().unwrap()).unwrap();
    Library::open(&dirs.db_path)
        .unwrap()
        .set_theme(theme)
        .unwrap();
    let harness_theme = match theme {
        ThemeChoice::Light => egui::Theme::Light,
        ThemeChoice::Dark | ThemeChoice::System => egui::Theme::Dark,
    };

    let mut harness =
        window(Some(harness_theme)).build_eframe(|cc| App::new(cc, dirs, Some(pictures)).unwrap());

    let total: usize = FOLDERS.iter().map(|(_, count, _)| count).sum();
    until(&mut harness, "the library shown", |app| {
        let shown = app.last_frame().is_some_and(|frame| frame.settled);
        app.photos() == total && app.folders().len() >= FOLDERS.len() && shown
    });
    collect(&mut harness, total);
    then(&mut harness);
    // The icons are rasterised a frame after they are first asked for.
    for _ in 0..5 {
        harness.step();
    }

    let out = manifest().join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join(name);
    harness.render().unwrap().save(&path).unwrap();
    println!("wrote {}", path.display());
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_main_light() {
    shot(ThemeChoice::Light, "native-main-light.png", |_| {});
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_main_dark() {
    shot(ThemeChoice::Dark, "native-main-dark.png", |_| {});
}

// A view with nothing in it: the row marked, the line in the middle, the count at none.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_starred_empty_light() {
    shot(
        ThemeChoice::Light,
        "native-starred-empty-light.png",
        |harness| {
            harness.get_by_label("Starred").click();
            until(harness, "the starred view shown", |app| {
                app.settled().view == GridView::Starred
            });
        },
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_sidebar_hidden_dark() {
    shot(
        ThemeChoice::Dark,
        "native-sidebar-hidden-dark.png",
        |harness| {
            harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
            until(harness, "the sidebar hidden", |app| {
                app.layout().sidebar_hidden
            });
        },
    );
}

// The lists a window does not open with: People, which nobody has named anyone in, and
// the keywords.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_lists_open_light() {
    shot(
        ThemeChoice::Light,
        "native-lists-open-light.png",
        |harness| {
            harness.get_by_label("Show people").click();
            until(harness, "people unfolded", |app| app.layout().open.people);
            // Drawn again before the next click: Tags is further down now.
            harness.step();
            harness.get_by_label("Tags").click();
            until(harness, "tags unfolded", |app| app.layout().open.tags);
        },
    );
}

// An album shown: its row filled, its photos under their folders' headers.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_album_dark() {
    shot(ThemeChoice::Dark, "native-album-dark.png", |harness| {
        harness.get_by_label("Best of the Alps").click();
        until(harness, "the album shown", |app| {
            app.settled().view == GridView::Album
                && app.last_frame().is_some_and(|frame| frame.settled)
        });
    });
}

// A search typed, with what the box understands open under it.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_search_help_light() {
    shot(
        ThemeChoice::Light,
        "native-search-help-light.png",
        |harness| {
            harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
            harness.step();
            harness.event(egui::Event::Text("mountains".to_owned()));
            until(harness, "the search shown", |app| {
                app.settled().view == GridView::Search
                    && app.last_frame().is_some_and(|frame| frame.settled)
            });
            harness.get_by_label("What you can search for").click();
            until(harness, "the help open", |app| app.search_help_open());
        },
    );
}

// The controls at the right of the top bar, with the sort's list open over the photos.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_sort_list_dark() {
    shot(ThemeChoice::Dark, "native-sort-list-dark.png", |harness| {
        harness.get_by_label("Sort by: Date taken").click();
        harness.step();
        harness.step();
        harness.get_by_label("Date modified").hover();
    });
}

// By month, at the largest size: a header for each month, and - the photos being of
// several years and more than a screen of them - the year strip beside the scrollbar.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_by_month_large_light() {
    shot(
        ThemeChoice::Light,
        "native-by-month-large-light.png",
        |harness| {
            harness.get_by_label("Large").click();
            harness.step();
            harness.get_by_label("Group by: By folder").click();
            harness.step();
            harness.step();
            harness.get_by_label("By month").click();
            until(harness, "the months shown", |app| {
                app.settled_sort().group == photon_core::sort::Grouping::Month
                    && app.last_frame().is_some_and(|frame| frame.settled)
            });
            // Somewhere in the middle, with the pointer on the strip: the line where
            // the grid is, and the year under the pointer.
            harness.key_press_modifiers(Modifiers::NONE, Key::PageDown);
            harness.step();
            if let Some(strip) = harness.query_by_label("Timeline") {
                strip.hover();
            }
        },
    );
}

/// Clicks the photo at grid offset `offset` with `held` down.
fn pick(harness: &mut Harness<'_>, offset: usize, held: Modifiers) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center();
    harness.event(egui::Event::ModifiersChanged(held));
    harness.event(egui::Event::PointerMoved(at));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: held,
        });
        harness.step();
    }
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.step();
}

/// A run of photos picked with Shift and one more with Ctrl: their rings, and the count
/// of them in the status bar, before the photos'.
fn selection(harness: &mut Harness<'_>) {
    pick(harness, 1, Modifiers::NONE);
    pick(harness, 4, Modifiers::SHIFT);
    pick(harness, 9, Modifiers::COMMAND);
    // The pointer off the photos, as it is not in the picture.
    harness.event(egui::Event::PointerGone);
    harness.step();
}

/// A rubber band half drawn, the button still down: its rectangle over the photos it
/// has taken, added to one picked before it.
fn band(harness: &mut Harness<'_>) {
    pick(harness, 12, Modifiers::NONE);
    let from = harness.state().tile_rect(1).unwrap().center();
    let to = harness.state().tile_rect(9).unwrap().center() + vec2(30.0, 40.0);
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::COMMAND,
    });
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_band_light() {
    shot(ThemeChoice::Light, "native-band-light.png", band);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_band_dark() {
    shot(ThemeChoice::Dark, "native-band-dark.png", band);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_selection_light() {
    shot(ThemeChoice::Light, "native-selection-light.png", selection);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_selection_dark() {
    shot(ThemeChoice::Dark, "native-selection-dark.png", selection);
}

/// A click of the other button on the photo at `offset`, and the frames its menu is
/// measured and shown in.
fn right_click(harness: &mut Harness<'_>, offset: usize) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is laid out")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.step();
}

/// The menu of one photo, which nothing had selected: the photo ringed, what is not
/// there yet dimmed, the albums of the user's own at its end, and the pointer on an item.
fn menu_of_one(harness: &mut Harness<'_>) {
    right_click(harness, 2);
    harness.get_by_label("Hide photo").hover();
    harness.step();
}

/// The menu of several photos of which one is starred: both star items, the key beside
/// the one it would do. Opened near the window's bottom edge, so it stands up from the
/// pointer.
fn menu_of_several(harness: &mut Harness<'_>) {
    pick(harness, 9, Modifiers::NONE);
    harness.key_press_modifiers(Modifiers::NONE, Key::Period);
    until(harness, "the star shown", |app| app.starred().len() == 1);
    pick(harness, 8, Modifiers::NONE);
    pick(harness, 12, Modifiers::SHIFT);
    right_click(harness, 12);
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowDown);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_menu_light() {
    shot(ThemeChoice::Light, "native-menu-light.png", menu_of_one);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_menu_dark() {
    shot(ThemeChoice::Dark, "native-menu-dark.png", menu_of_one);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_menu_several_light() {
    shot(
        ThemeChoice::Light,
        "native-menu-several-light.png",
        menu_of_several,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_menu_several_dark() {
    shot(
        ThemeChoice::Dark,
        "native-menu-several-dark.png",
        menu_of_several,
    );
}

/// The other button at `at`, and the frames a menu is measured and shown in.
fn right_click_at(harness: &mut Harness<'_>, at: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.step();
}

/// A folder's menu, opened on its header in the grid: the folder has a name in photon,
/// so "Use folder name" is in it, and the pointer is on an item.
fn folder_menu(harness: &mut Harness<'_>) {
    let engine = harness.state().engine().clone();
    let party = (harness.state().folders().values())
        .find(|folder| folder.name.contains("Party"))
        .map(|folder| folder.id)
        .expect("the folder is listed");
    commands::set_folder_alias(&engine, party, Some("New Year".to_owned())).unwrap();
    until(harness, "the folder's name shown", |app| {
        app.folders()[&party].alias.is_some()
    });
    harness.step();
    let tile = harness
        .state()
        .tile_rect(7)
        .expect("its first photo is laid out");
    right_click_at(harness, tile.left_top() + vec2(120.0, -14.0));
    harness.get_by_label("Hide folder").hover();
    harness.step();
}

/// A name being typed in an album's row: the field in the row's own place, half typed.
fn naming(harness: &mut Harness<'_>) {
    let at = harness.get_by_label("Best of the Alps").rect().center();
    right_click_at(harness, at);
    harness.get_by_label("Rename…").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.event(egui::Event::Text("Best of 20".to_owned()));
    harness.step();
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_folder_menu_light() {
    shot(
        ThemeChoice::Light,
        "native-folder-menu-light.png",
        folder_menu,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_folder_menu_dark() {
    shot(
        ThemeChoice::Dark,
        "native-folder-menu-dark.png",
        folder_menu,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_naming_light() {
    shot(ThemeChoice::Light, "native-naming-light.png", naming);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_naming_dark() {
    shot(ThemeChoice::Dark, "native-naming-dark.png", naming);
}

/// The question asked before an album is deleted: over the whole window, the keyboard
/// on "Cancel".
fn confirming(harness: &mut Harness<'_>) {
    let at = harness.get_by_label("Best of the Alps").rect().center();
    right_click_at(harness, at);
    harness.get_by_label("Delete…").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.event(egui::Event::PointerGone);
    harness.step();
}

/// The keyword picker for three photos, with a few letters typed: the library's
/// keywords narrowed to those that hold them.
fn picking(harness: &mut Harness<'_>) {
    pick(harness, 2, Modifiers::NONE);
    pick(harness, 4, Modifiers::SHIFT);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    right_click(harness, 3);
    harness.get_by_label("Add keyword to 3 photos…").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.event(egui::Event::Text("in".to_owned()));
    harness.step();
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_confirm_light() {
    shot(ThemeChoice::Light, "native-confirm-light.png", confirming);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_confirm_dark() {
    shot(ThemeChoice::Dark, "native-confirm-dark.png", confirming);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_keywords_light() {
    shot(ThemeChoice::Light, "native-keywords-light.png", picking);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_keywords_dark() {
    shot(ThemeChoice::Dark, "native-keywords-dark.png", picking);
}

/// The export's dialog for three photos, with `path` typed and answered about, and a
/// size chosen.
fn exporting_to(harness: &mut Harness<'_>, path: &str) {
    pick(harness, 2, Modifiers::NONE);
    pick(harness, 4, Modifiers::SHIFT);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    right_click(harness, 3);
    harness.get_by_label("Export 3 photos…").click();
    for _ in 0..4 {
        harness.step();
    }
    harness.event(egui::Event::Text(path.to_owned()));
    until(harness, "the path answered about", |app| {
        matches!(app.dialog(), Some(photon_ui::dialogs::Dialog::Export(export))
            if matches!(export.dest(), Dest::Passed(_) | Dest::Refused(_)))
    });
    // What is said of the path may have made the dialog taller: it stands where it
    // will be pressed a frame or two after.
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("1920 px").click();
    harness.step();
    harness.event(egui::Event::PointerGone);
    harness.step();
}

/// A destination that passes: the folder the test's own library lies under.
fn exporting(harness: &mut Harness<'_>) {
    let temp = std::env::temp_dir();
    exporting_to(harness, &temp.to_string_lossy());
}

/// One photon will not write into, said under the field: the watched folder itself.
fn export_refused(harness: &mut Harness<'_>) {
    let watched = harness.state().watched()[0].path.clone();
    exporting_to(harness, &watched);
}

/// A folder held over the window: the card that says what letting go will do.
fn holding(harness: &mut Harness<'_>) {
    use eframe::App as _;
    // What is held is handed to the application before egui reads it, as eframe does
    // before every frame of a window (`App::raw_input_hook`): once is enough here,
    // the application keeping what it was last told.
    let mut input = egui::RawInput {
        hovered_files: vec![egui::HoveredFile {
            path: Some(std::path::PathBuf::from("/home/ada/Pictures/Holiday")),
            ..Default::default()
        }],
        ..Default::default()
    };
    let ctx = harness.ctx.clone();
    harness.state_mut().raw_input_hook(&ctx, &mut input);
    for _ in 0..3 {
        harness.step();
    }
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_export_light() {
    shot(ThemeChoice::Light, "native-export-light.png", exporting);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_export_dark() {
    shot(ThemeChoice::Dark, "native-export-dark.png", exporting);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_export_refused_light() {
    shot(
        ThemeChoice::Light,
        "native-export-refused-light.png",
        export_refused,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_export_refused_dark() {
    shot(
        ThemeChoice::Dark,
        "native-export-refused-dark.png",
        export_refused,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_drop_light() {
    shot(ThemeChoice::Light, "native-drop-light.png", holding);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_drop_dark() {
    shot(ThemeChoice::Dark, "native-drop-dark.png", holding);
}

// A library that watches nothing, as a first run is where there is no Pictures folder:
// what it says where its photos would be, its buttons drawn and not yet answering, and
// "No folders yet." in the sidebar.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_empty_library_light() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    std::fs::create_dir_all(dirs.db_path.parent().unwrap()).unwrap();
    Library::open(&dirs.db_path)
        .unwrap()
        .set_theme(ThemeChoice::Light)
        .unwrap();
    let mut harness =
        window(Some(egui::Theme::Light)).build_eframe(|cc| App::new(cc, dirs, None).unwrap());
    until(&mut harness, "the empty library's panel", |app| {
        app.panel().is_some()
    });
    for _ in 0..5 {
        harness.step();
    }
    let out = manifest().join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("native-empty-library-light.png");
    harness.render().unwrap().save(&path).unwrap();
    println!("wrote {}", path.display());
}

/// The viewer on the third photo of the grid, once its full picture is made.
fn viewing(harness: &mut Harness<'_>) {
    pick(harness, 2, Modifiers::NONE);
    harness.key_press(Key::Enter);
    until(harness, "the full picture", |app| {
        app.viewer_levels().is_some()
    });
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_light() {
    shot(ThemeChoice::Light, "native-viewer-light.png", viewing);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_dark() {
    shot(ThemeChoice::Dark, "native-viewer-dark.png", viewing);
}

/// The same, zoomed in by the keys and moved by a drag: the slider says how far.
fn zoomed(harness: &mut Harness<'_>) {
    viewing(harness);
    for _ in 0..4 {
        harness.key_press(Key::Plus);
        harness.step();
    }
    let from = egui::pos2(640.0, 500.0);
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    let to = from + vec2(-260.0, 120.0);
    harness.event(egui::Event::PointerMoved(to));
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_zoomed_light() {
    shot(ThemeChoice::Light, "native-viewer-zoomed-light.png", zoomed);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_zoomed_dark() {
    shot(ThemeChoice::Dark, "native-viewer-zoomed-dark.png", zoomed);
}

// The spec's first risk, to be judged by eye: a detailed photo much larger than the
// window, fitted, as photon's own levels and a linear filter draw it
// (`native-viewer-quality.png`), beside the same photo resized to the same size with a
// proper filter (`native-viewer-quality-reference.png`), which is what a web view's
// scaling comes to. The photo is a zone plate: rings that grow finer outwards, on which
// a scaling that skips pixels draws rings of its own. Also says what handing the full
// picture to the GPU costs: the render that uploads it against one that does not.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_quality() {
    let (width, height) = (4000u32, 2667u32);
    let plate = image::RgbImage::from_fn(width, height, |x, y| {
        let (dx, dy) = (
            f64::from(x) - f64::from(width) / 2.0,
            f64::from(y) - f64::from(height) / 2.0,
        );
        // The rings' spacing reaches two pixels at the picture's corners: the finest a
        // picture can hold.
        let phase = (dx * dx + dy * dy) * std::f64::consts::PI / f64::from(width + height);
        let value = (127.5 + 127.5 * phase.cos()) as u8;
        image::Rgb([value, value, value])
    });
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    plate.save(pictures.join("plate.png")).unwrap();
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    let mut harness = window(None).build_eframe(|cc| App::new(cc, dirs, Some(pictures)).unwrap());
    harness.input_mut().max_texture_side = Some(8192);
    until(&mut harness, "the photo shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    pick(&mut harness, 0, Modifiers::NONE);
    harness.key_press(Key::Enter);
    // The frame the full picture arrives in is the one that hands it to the GPU: how
    // long that frame takes, against the frames after it. (An unoptimised build of
    // egui's renderer: more than a release build takes, not less.)
    let deadline = Instant::now() + Duration::from_secs(120);
    let upload = loop {
        let before = Instant::now();
        harness.step();
        let took = before.elapsed();
        if harness.state().viewer_levels().is_some() {
            break took;
        }
        assert!(Instant::now() < deadline, "never: the full picture");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut still = Duration::MAX;
    for _ in 0..5 {
        let before = Instant::now();
        harness.step();
        still = still.min(before.elapsed());
    }
    println!(
        "levels {:?}: the frame that uploads them {upload:?}, a frame after {still:?}",
        harness.state().viewer_levels()
    );
    let drawn = harness.render().unwrap();
    let out = manifest().join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    drawn.save(out.join("native-viewer-quality.png")).unwrap();
    // The same photo as a proper filter fits it into the same window.
    let fitted = image::imageops::resize(
        &plate,
        1280,
        (1280.0 * f64::from(height) / f64::from(width)).round() as u32,
        image::imageops::FilterType::Lanczos3,
    );
    fitted
        .save(out.join("native-viewer-quality-reference.png"))
        .unwrap();
    println!("wrote {}", out.join("native-viewer-quality.png").display());
}
