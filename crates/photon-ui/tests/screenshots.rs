//! The window as it is drawn, written to PNG without a window: the application itself,
//! over a library made of the CC0 photos in `crates/xtask/screenshots/photos` (credited in
//! their `CREDITS.md`), rendered off screen through wgpu. The website's pictures are five
//! of them (`native-site-*`).
//!
//! Ignored by default - it needs a GPU adapter, which a CI runner need not have - and run
//! by `cargo run -p xtask -- native-shot`. It replaces no item of the smoke checklist: it
//! is one renderer's picture, read by whoever runs it.
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

/// What a picture's library and window hold beyond the four folders, at the size most
/// pictures are drawn at.
#[derive(Clone, Copy)]
struct Setup {
    size: egui::Vec2,
    /// The folders, as `FOLDERS` holds them.
    folders: &'static [(&'static str, usize, (i16, i8, i8))],
    /// Two videos at the end of the first folder, played by `Films`.
    videos: bool,
    /// A copy of the first folder's first photo, the last of the second folder's.
    copy: bool,
}

const WINDOW: Setup = Setup {
    size: vec2(1280.0, 1000.0),
    folders: &FOLDERS,
    videos: false,
    copy: false,
};

/// Where the pictures' library is shown to be: someone's own Pictures folder.
const SHOWN_PICTURES: &str = "/home/anna/Pictures";

/// The website's pictures (`site/img`), which stand 1280 by 800 in its page: a library
/// as someone's is, with folders named as people name them - not the other scripts the
/// rest are named in to test the text - its videos and a copy.
const SITE: Setup = Setup {
    size: vec2(1280.0, 800.0),
    folders: &SITE_FOLDERS,
    videos: true,
    copy: true,
};

const SITE_FOLDERS: [(&str, usize, (i16, i8, i8)); 4] = [
    ("Dolomites, July 2026", 7, (2026, 7, 14)),
    ("Spring in the Alps", 5, (2024, 4, 3)),
    ("Austria in August", 5, (2024, 8, 21)),
    ("Winter lakes", 6, (2025, 12, 31)),
];

impl Setup {
    /// How many photos and videos the library holds.
    fn total(&self) -> usize {
        let photos: usize = self.folders.iter().map(|(_, count, _)| count).sum();
        photos + 2 * usize::from(self.videos) + usize::from(self.copy)
    }
}

/// Dates a file `minutes` after noon of `day`, UTC.
fn date(path: &Path, (year, month, day): (i16, i8, i8), minutes: u64) {
    let noon = jiff::civil::date(year, month, day)
        .at(12, 0, 0, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap()
        .timestamp();
    let at = std::time::SystemTime::from(noon) + Duration::from_secs(60 * minutes);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// Copies the photos into the four folders under `pictures`, and what `setup` adds.
fn library(pictures: &Path, setup: Setup) {
    let source = manifest().join("../xtask/screenshots/photos");
    let mut photos: Vec<PathBuf> = std::fs::read_dir(&source)
        .unwrap_or_else(|err| panic!("{}: {err}", source.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    photos.sort();
    let mut photos = photos.into_iter();
    for &(folder, count, (year, month, day)) in setup.folders {
        let folder = pictures.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (n, photo) in photos.by_ref().take(count).enumerate() {
            let copy = folder.join(photo.file_name().unwrap());
            std::fs::copy(&photo, &copy).unwrap();
            // A photo every ten minutes from noon.
            date(&copy, (year, month, day), 10 * n as u64);
        }
    }
    let (first, _, first_day) = setup.folders[0];
    if setup.videos {
        // The video tests' own files, so that the library reads a size and a length from
        // each; what they show is `Films`'s, a photo, and not their test patterns.
        for (n, (name, fixture)) in [("Waves.mp4", "h264.mp4"), ("Harbour.mp4", "hevc.mp4")]
            .into_iter()
            .enumerate()
        {
            let path = pictures.join(first).join(name);
            std::fs::copy(manifest().join("tests/video").join(fixture), &path).unwrap();
            date(&path, first_day, 120 + 10 * n as u64);
        }
    }
    if setup.copy {
        let (second, _, second_day) = setup.folders[1];
        let mut firsts: Vec<PathBuf> = std::fs::read_dir(pictures.join(first))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
            .collect();
        firsts.sort();
        let path = pictures.join(second).join("copy of the coast.jpg");
        std::fs::copy(&firsts[0], &path).unwrap();
        date(&path, second_day, 120);
    }
}

/// A stand-in for the system's video: every poster and every frame is a photo of the
/// library's, so that a video looks like one in a picture on any machine - this one has
/// no decoder for an MP4, and a CI runner need not have one either. Four by three and four
/// seconds long, as the video tests' files the library reads are; "Harbour" is another
/// photo than "Waves".
struct Films([egui::ColorImage; 2]);

impl Films {
    fn new() -> std::sync::Arc<Self> {
        let source = manifest().join("../xtask/screenshots/photos");
        let mut photos: Vec<PathBuf> = std::fs::read_dir(&source)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
            .collect();
        photos.sort();
        let frame = |photo: &Path| {
            let photo = image::open(photo).unwrap().to_rgba8();
            let height = (photo.width() * 3 / 4).min(photo.height());
            let top = (photo.height() - height) / 2;
            let frame = image::imageops::crop_imm(&photo, 0, top, photo.width(), height).to_image();
            egui::ColorImage::from_rgba_unmultiplied(
                [frame.width() as usize, frame.height() as usize],
                frame.as_raw(),
            )
        };
        let n = photos.len();
        std::sync::Arc::new(Films([frame(&photos[n - 1]), frame(&photos[n - 2])]))
    }

    fn picture(&self, path: &Path, at: f64) -> photon_ui::video::Picture {
        let harbour = path.to_string_lossy().contains("Harbour");
        let image = &self.0[usize::from(harbour)];
        photon_ui::video::Picture {
            width: image.size[0] as u32,
            height: image.size[1] as u32,
            at,
            rgba: image.as_raw().to_vec(),
        }
    }
}

impl photon_ui::video::Videos for Films {
    fn can_play(&self) -> bool {
        true
    }

    fn poster(
        &self,
        path: &Path,
        at: f64,
    ) -> Result<photon_ui::video::Picture, photon_ui::video::Failure> {
        Ok(self.picture(path, at))
    }

    fn open(
        &self,
        path: &Path,
    ) -> Result<Box<dyn photon_ui::video::Stream>, photon_ui::video::Failure> {
        Ok(Box::new(Reel {
            frame: std::sync::Mutex::new(Some(self.picture(path, 1.5))),
        }))
    }
}

/// A player that stands a second and a half into a video of four, playing, and hands over
/// its one frame once.
struct Reel {
    frame: std::sync::Mutex<Option<photon_ui::video::Picture>>,
}

impl photon_ui::video::Stream for Reel {
    fn ready(&self) -> bool {
        true
    }
    fn play(&self) {}
    fn pause(&self) {}
    fn seek(&self, _: f64) {}
    fn set_volume(&self, _: f64) {}
    fn set_muted(&self, _: bool) {}
    fn position(&self) -> Option<f64> {
        Some(1.5)
    }
    fn duration(&self) -> Option<f64> {
        Some(4.0)
    }
    fn ended(&self) -> bool {
        false
    }
    fn failure(&self) -> Option<photon_ui::video::Failure> {
        None
    }
    fn frame(&self) -> Option<photon_ui::video::Picture> {
        self.frame.lock().unwrap().take()
    }
}

type Harness<'a> = egui_kittest::Harness<'a, App>;

/// Held while a picture's GPU device is made. Every picture is drawn on a thread of its
/// own, and the Vulkan loader is not safe to enter from many at once: with two dozen
/// devices being made in the same moment the run crashed inside the loader
/// (`vkEnumerateInstanceExtensionProperties`, a jump to nought) about once in four. One
/// device is made at a time; the pictures are drawn side by side all the same.
static DEVICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A window of `size` that renders through a GPU adapter, off screen.
fn window(theme: Option<egui::Theme>, size: egui::Vec2) -> egui_kittest::HarnessBuilder<App> {
    let builder = egui_kittest::Harness::builder()
        .with_size(size)
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
    shot_over(theme, name, |harness, _| then(harness));
}

/// The same, for a picture that needs something in the library no command puts there:
/// `then` is handed the library's own file as well.
fn shot_over(theme: ThemeChoice, name: &str, then: impl Fn(&mut Harness<'_>, &Path)) {
    shot_of(theme, name, WINDOW, then);
}

/// The same, over a library and in a window `setup` says.
fn shot_of(theme: ThemeChoice, name: &str, setup: Setup, then: impl Fn(&mut Harness<'_>, &Path)) {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    library(&pictures, setup);
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
    let db = dirs.db_path.clone();

    // Shown where someone's own photos would be, not in the temporary folder they are in.
    let real = std::fs::canonicalize(&pictures).unwrap();
    let mut harness = window(Some(harness_theme), setup.size).build_eframe(|cc| {
        let app = App::new(cc, dirs, Some(pictures))
            .unwrap()
            .without_animations()
            .showing_paths_as(&real, SHOWN_PICTURES);
        if setup.videos {
            app.with_videos(Films::new())
        } else {
            app
        }
    });

    let total = setup.total();
    until(&mut harness, "the library shown", |app| {
        let shown = app.last_frame().is_some_and(|frame| frame.settled);
        app.photos() == total && app.folders().len() >= setup.folders.len() && shown
    });
    collect(&mut harness, total);
    then(&mut harness, &db);
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
    let mut harness = window(Some(egui::Theme::Light), WINDOW.size)
        .build_eframe(|cc| App::new(cc, dirs, None).unwrap().without_animations());
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
    // The zoom outgrows the levels held at the fit, and the whole picture is made again.
    until(harness, "the picture the zoom needs", |app| {
        !app.viewer_asked() && app.viewer_levels().is_some()
    });
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

/// The viewer with its info panel open: the photo beside it.
fn informed(harness: &mut Harness<'_>) {
    viewing(harness);
    harness.key_press(Key::I);
    harness.step();
    until(harness, "the copy for the area beside the panel", |app| {
        app.viewer_info() && app.viewer_fitted().is_some_and(|size| size[0] < 1000)
    });
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_info_light() {
    shot(ThemeChoice::Light, "native-viewer-info-light.png", informed);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_info_dark() {
    shot(ThemeChoice::Dark, "native-viewer-info-dark.png", informed);
}

/// The viewer with the faces of its photo outlined: Anna's, with her name on its plate,
/// and one nobody has named. The rectangles are placed for the picture: the first is on
/// the woman the photo is of, the second on
/// the skyline beside her, where there is no face - it is there to show what an outline
/// that can be pressed looks like.
fn with_faces(harness: &mut Harness<'_>, db: &Path) {
    faces_on(harness, db, true);
}

/// Anna's face on the third photo, and with `unnamed` the outline on the skyline, then
/// the viewer on it with its panel open.
fn faces_on(harness: &mut Harness<'_>, db: &Path, unnamed: bool) {
    use photon_core::face_detect::{DETECTOR_VERSION, Detection, Rect};
    let id = harness.state().shown()[2];
    let lib = Library::open(db).unwrap();
    lib.set_face_detection(true).unwrap();
    // A photo is looked at once its preview is made.
    let deadline = Instant::now() + Duration::from_secs(120);
    let photos = loop {
        let photos = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
        if photos.iter().any(|photo| photo.id == id) {
            break photos;
        }
        assert!(Instant::now() < deadline, "never: the photo's preview");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    };
    let face = |left: f64, top: f64, right: f64, bottom: f64| Detection {
        rect: Rect {
            left,
            top,
            right,
            bottom,
        },
        landmarks: [(0.0, 0.0); 5],
        score: 0.9,
    };
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let faces = if photo.id == id {
                let mut faces = vec![face(0.3, 0.25, 0.42, 0.5)];
                if unnamed {
                    faces.push(face(0.62, 0.36, 0.72, 0.56));
                }
                faces
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    let first: i64 = conn
        .query_row(
            "SELECT id FROM detected_faces WHERE item_id = ?1 ORDER BY left LIMIT 1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(lib.name_faces(&[first], "Anna").unwrap().is_some());
    informed(harness);
    harness.state().engine().refresh_grid().unwrap();
    until(harness, "the faces read", |app| {
        (app.viewer_item()).is_some_and(|item| {
            item.faces.len() == 1 && item.unnamed_faces.len() == usize::from(unnamed)
        })
    });
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_faces_light() {
    shot_over(
        ThemeChoice::Light,
        "native-viewer-faces-light.png",
        with_faces,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_faces_dark() {
    shot_over(
        ThemeChoice::Dark,
        "native-viewer-faces-dark.png",
        with_faces,
    );
}

/// The other button on Anna's face: the viewer's menu, with what it offers for her
/// over what it offers for the photo, and the pointer on an item.
fn viewer_menu(harness: &mut Harness<'_>, db: &Path) {
    with_faces(harness, db);
    let plate = (harness.get_by_role_and_label(egui::accesskit::Role::Label, "Anna")).rect();
    let at = plate.left_top() + vec2(20.0, -40.0);
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
    let item = harness.get_by_label("Not Anna").rect().center();
    harness.event(egui::Event::PointerMoved(item));
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_menu_light() {
    shot_over(
        ThemeChoice::Light,
        "native-viewer-menu-light.png",
        viewer_menu,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_menu_dark() {
    shot_over(
        ThemeChoice::Dark,
        "native-viewer-menu-dark.png",
        viewer_menu,
    );
}

/// The outline of the face nobody has named, pressed: the dialog that asks who it is,
/// over the viewer, with a name half typed and what Enter will do with it.
fn naming_a_face(harness: &mut Harness<'_>, db: &Path) {
    with_faces(harness, db);
    harness.get_by_label("Name this face").click();
    for _ in 0..4 {
        harness.step();
    }
    until(harness, "the people listed", App::people_listed);
    harness.event(egui::Event::Text("an".to_owned()));
    for _ in 0..3 {
        harness.step();
    }
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_name_face_light() {
    shot_over(
        ThemeChoice::Light,
        "native-name-face-light.png",
        naming_a_face,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_name_face_dark() {
    shot_over(
        ThemeChoice::Dark,
        "native-name-face-dark.png",
        naming_a_face,
    );
}

/// The viewer on a photo that has been turned a quarter to the right: its bar, with
/// the turns, the crop and "Original".
fn edited(harness: &mut Harness<'_>) {
    viewing(harness);
    harness.key_press(Key::R);
    harness.step();
    until(harness, "the photo turned, and its picture", |app| {
        (app.viewer_item()).is_some_and(|item| item.edit.is_some()) && app.viewer_levels().is_some()
    });
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_edited_light() {
    shot(ThemeChoice::Light, "native-viewer-edited-light.png", edited);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_edited_dark() {
    shot(ThemeChoice::Dark, "native-viewer-edited-dark.png", edited);
}

/// Where the picture the crop tool draws its rectangle on stands in the window: the
/// photo's, turned and not cropped, fitted.
fn crop_box(harness: &Harness<'_>) -> egui::Rect {
    let item = harness.state().viewer_item().expect("the photo's facts");
    let (width, height) = (item.uncropped_width as f32, item.uncropped_height as f32);
    let scale = (1280.0 / width).min(1000.0 / height);
    egui::Rect::from_center_size(egui::pos2(640.0, 500.0), vec2(width, height) * scale)
}

/// The crop tool, open on the photo in the viewer, with a rectangle drawn: its two
/// opposite corners dragged inwards.
fn cropping(harness: &mut Harness<'_>) {
    viewing(harness);
    harness.key_press(Key::C);
    harness.step();
    assert!(harness.state().cropping().is_some());
    let picture = crop_box(harness);
    let by = picture.size() * 0.18;
    for (from, to) in [
        (picture.left_top(), picture.left_top() + by),
        (picture.right_bottom(), picture.right_bottom() - by * 0.6),
    ] {
        harness.event(egui::Event::PointerMoved(from));
        harness.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        harness.step();
        harness.event(egui::Event::PointerMoved(to));
        harness.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_crop_light() {
    shot(ThemeChoice::Light, "native-viewer-crop-light.png", cropping);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_crop_dark() {
    shot(ThemeChoice::Dark, "native-viewer-crop-dark.png", cropping);
}

/// The same, with the list of ratios open over the tool's bar and the pointer on one.
fn crop_ratios(harness: &mut Harness<'_>) {
    cropping(harness);
    harness.get_by_label_contains("Crop ratio").click();
    // (A new area fades in.)
    for _ in 0..30 {
        harness.step();
    }
    let at = harness.get_by_label("3:2").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_crop_ratios_light() {
    shot(
        ThemeChoice::Light,
        "native-viewer-crop-ratios-light.png",
        crop_ratios,
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_crop_ratios_dark() {
    shot(
        ThemeChoice::Dark,
        "native-viewer-crop-ratios-dark.png",
        crop_ratios,
    );
}

/// The tool on a photo that has a crop already: the picture without it, with the
/// rectangle where the crop is.
fn cropping_again(harness: &mut Harness<'_>) {
    let id = harness.state().shown()[2];
    let engine = harness.state().engine().clone();
    let was = harness.state().version();
    commands::set_item_edit(&engine, id, 0, Some([9000, 12000, 52000, 60000])).unwrap();
    until(harness, "the grid with the photo cropped", |app| {
        app.version() != was
    });
    viewing(harness);
    harness.key_press(Key::C);
    harness.step();
    until(harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_crop_again_dark() {
    shot(
        ThemeChoice::Dark,
        "native-viewer-crop-again-dark.png",
        cropping_again,
    );
}

/// A slideshow, paused, with the pointer over the photo: the bar's button says it will
/// play the show again.
fn showing(harness: &mut Harness<'_>) {
    viewing(harness);
    harness.key_press(Key::S);
    harness.step();
    harness.key_press(Key::Space);
    harness.step();
    harness.event(egui::Event::PointerMoved(egui::pos2(640.0, 400.0)));
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_show_light() {
    shot(ThemeChoice::Light, "native-viewer-show-light.png", showing);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_show_dark() {
    shot(ThemeChoice::Dark, "native-viewer-show-dark.png", showing);
}

/// Compare, on three photos of the second folder: picked, `C`, their previews.
fn comparing(harness: &mut Harness<'_>) {
    let adding = if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    };
    pick(harness, 2, Modifiers::NONE);
    pick(harness, 3, adding);
    pick(harness, 5, adding);
    harness.key_press(Key::C);
    until(harness, "compare open", |app| app.compare().is_some());
    for _ in 0..20 {
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    harness.key_press(Key::Num2);
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_compare_light() {
    shot(ThemeChoice::Light, "native-compare-light.png", comparing);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_compare_dark() {
    shot(ThemeChoice::Dark, "native-compare-dark.png", comparing);
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
    let mut harness = window(None, WINDOW.size).build_eframe(|cc| {
        App::new(cc, dirs, Some(pictures))
            .unwrap()
            .without_animations()
    });
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

/// Settings opened by the gear, at the section called `label`, once what it shows has
/// been read.
fn settings_at(harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label("Settings").click();
    until(harness, "Settings read", |app| {
        matches!(app.dialog(), Some(photon_ui::dialogs::Dialog::Settings(settings))
            if settings.stored().is_some())
    });
    // Past a double click's time of its first frame, which a press in it would be the
    // second press of.
    for _ in 0..30 {
        harness.step();
    }
    let nodes = harness.query_all_by_role_and_label(egui::accesskit::Role::Button, label);
    let node = (nodes.into_iter())
        .find(|node| node.rect().left() > 140.0 && node.rect().top() > 80.0)
        .expect("the section's row");
    node.click();
    for _ in 0..5 {
        harness.step();
    }
    harness.event(egui::Event::PointerGone);
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_appearance_light() {
    shot(
        ThemeChoice::Light,
        "native-settings-appearance-light.png",
        |harness| settings_at(harness, "Appearance"),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_people_dark() {
    shot(
        ThemeChoice::Dark,
        "native-settings-people-dark.png",
        |harness| settings_at(harness, "People"),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_slideshow_dark() {
    shot(
        ThemeChoice::Dark,
        "native-settings-slideshow-dark.png",
        |harness| settings_at(harness, "Slideshow"),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_about_light() {
    shot(
        ThemeChoice::Light,
        "native-settings-about-light.png",
        |harness| settings_at(harness, "About"),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_folders_light() {
    shot(
        ThemeChoice::Light,
        "native-settings-folders-light.png",
        |harness| {
            settings_at(harness, "Folders");
            // Its counts, read a moment after it opens.
            for _ in 0..20 {
                harness.step();
                std::thread::sleep(Duration::from_millis(10));
            }
        },
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_tags_dark() {
    shot(
        ThemeChoice::Dark,
        "native-settings-tags-dark.png",
        |harness| {
            settings_at(harness, "Tags");
            for _ in 0..20 {
                harness.step();
                std::thread::sleep(Duration::from_millis(10));
            }
        },
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_shortcuts_light() {
    shot(
        ThemeChoice::Light,
        "native-shortcuts-light.png",
        |harness| {
            harness.event(egui::Event::Key {
                key: Key::Questionmark,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::SHIFT,
            });
            for _ in 0..5 {
                harness.step();
            }
        },
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_shortcuts_dark() {
    shot(
        ThemeChoice::Dark,
        "native-settings-shortcuts-dark.png",
        |harness| settings_at(harness, "Shortcuts"),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_settings_statistics_light() {
    shot(
        ThemeChoice::Light,
        "native-settings-statistics-light.png",
        |harness| {
            settings_at(harness, "Statistics");
            until(harness, "the library counted", |app| {
                matches!(app.dialog(), Some(photon_ui::dialogs::Dialog::Settings(settings))
                    if matches!(settings.stats, photon_ui::settings::Counted::Counted(_)))
            });
        },
    );
}

/// The People page over a library with two groups to name and a single face: faces placed
/// on six photos through the library's own calls, each group's a vector of its own so that
/// its grouping puts them together, and Find faces switched on through the engine last -
/// every photo is looked at by then, and the pass it asks for finds nothing to do.
fn people(harness: &mut Harness<'_>, db: &Path) {
    use photon_core::{
        face_detect::{DETECTOR_VERSION, Detection, Rect},
        face_embed::{DIM, EMBEDDER_VERSION},
    };
    let shown = harness.state().shown();
    let groups = [&shown[0..3], &shown[3..5], &shown[5..6]];
    let lib = Library::open(db).unwrap();
    lib.set_face_detection(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let photos = loop {
        let photos = lib.face_candidates(0, 100, DETECTOR_VERSION).unwrap();
        let ready = |id: &i64| photos.iter().any(|photo| photo.id == *id);
        if shown[0..6].iter().all(ready) {
            break photos;
        }
        assert!(Instant::now() < deadline, "never: the photos' previews");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    };
    let face = Detection {
        rect: Rect {
            left: 0.35,
            top: 0.3,
            right: 0.55,
            bottom: 0.6,
        },
        landmarks: [(0.0, 0.0); 5],
        score: 0.9,
    };
    let batch: Vec<_> = (photos.into_iter())
        .map(|photo| {
            let faces = if shown[0..6].contains(&photo.id) {
                vec![face.clone()]
            } else {
                Vec::new()
            };
            (photo, faces)
        })
        .collect();
    lib.write_face_batch(&batch, DETECTOR_VERSION).unwrap();
    let candidates = lib.embed_candidates(0, 100, EMBEDDER_VERSION).unwrap();
    let batch: Vec<_> = (candidates.into_iter())
        .map(|candidate| {
            let group = (groups.iter())
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
    let engine = harness.state().engine().clone();
    photon_engine::commands::set_face_detection(&engine, true).unwrap();
    // A grouping run announces a change of the data, which the sidebar's "to name" reads.
    engine.refresh_grid().unwrap();
    until(harness, "the groups to name counted", |app| {
        app.collections().to_name == 2
    });
    harness.get_by_label("People").click();
    harness.step();
    until(harness, "the page read", |app| {
        app.people_page()
            .is_some_and(|page| page.unnamed().len() == 2)
    });
    let faces: Vec<(i64, String)> = (harness.state().people_page().unwrap().unnamed().iter())
        .flat_map(|group| group.faces.iter())
        .map(|face| (face.id, face.thumb_key.clone()))
        .collect();
    until(harness, "the crops", |app| {
        (faces.iter()).all(|(face, key)| app.people_crop(*face, key))
    });
    harness.step();
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_people_light() {
    shot_over(ThemeChoice::Light, "native-people-light.png", people);
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_people_dark() {
    shot_over(ThemeChoice::Dark, "native-people-dark.png", people);
}

/// Steps the application until AccessKit names `label`, which a sidebar row tells it.
fn until_named(harness: &mut Harness<'_>, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while harness.query_by_label(label).is_none() {
        assert!(Instant::now() < deadline, "never: {label}");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Steps until the grid has every thumbnail it shows, and has stopped asking for frames.
fn thumbnails_in(harness: &mut Harness<'_>) {
    until(harness, "the thumbnails", |app| {
        // A tile marked as having no picture counts as settled: a video's poster not
        // made yet is asked for again only seconds later.
        app.last_frame()
            .is_some_and(|frame| frame.settled && !frame.loading && frame.marked == 0)
    });
    for _ in 0..10 {
        harness.step();
    }
}

// The smallest tiles: more of the library in the window, the headers as they are.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_small_light() {
    shot(ThemeChoice::Light, "native-small-light.png", |harness| {
        harness.get_by_label("Small").click();
        harness.step();
        thumbnails_in(harness);
    });
}

// Scrolled into a folder further down: its header pinned over the photos it heads, the
// sidebar marking the folder at the top.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_pinned_header_dark() {
    shot(
        ThemeChoice::Dark,
        "native-pinned-header-dark.png",
        |harness| {
            // At the largest tiles: at Medium the library is hardly taller than the window,
            // and its end comes before the second folder can be scrolled into.
            harness.get_by_label("Large").click();
            harness.step();
            let over = egui::pos2(800.0, 500.0);
            harness.event(egui::Event::PointerMoved(over));
            for _ in 0..4 {
                harness.event(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -275.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                });
                for _ in 0..20 {
                    harness.step();
                }
            }
            harness.event(egui::Event::PointerGone);
            thumbnails_in(harness);
        },
    );
}

// Duplicates: a copy of a photo in another folder, both under their folders' headers,
// the row in the sidebar with its count.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_duplicates_light() {
    let setup = Setup {
        copy: true,
        ..WINDOW
    };
    shot_of(
        ThemeChoice::Light,
        "native-duplicates-light.png",
        setup,
        |harness, _| {
            until_named(harness, "Duplicates");
            harness.get_by_label("Duplicates").click();
            until(harness, "the copies shown", |app| app.photos() == 2);
            thumbnails_in(harness);
        },
    );
}

// A saved search opened from the sidebar: its words in the box, the photos that match.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_search_dark() {
    shot(ThemeChoice::Dark, "native-search-dark.png", |harness| {
        harness.get_by_label("Taken in July").click();
        until(harness, "the search's photos", |app| {
            app.photos() == FOLDERS[0].1
        });
        thumbnails_in(harness);
    });
}

// The Videos view: each video's poster with its badge.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_videos_dark() {
    let setup = Setup {
        videos: true,
        ..WINDOW
    };
    shot_of(
        ThemeChoice::Dark,
        "native-videos-dark.png",
        setup,
        |harness, _| {
            until_named(harness, "Videos");
            harness.get_by_label("Videos").click();
            until(harness, "the videos shown", |app| app.photos() == 2);
            thumbnails_in(harness);
        },
    );
}

// A video playing in the viewer: its frame, and the strip of its controls over the bar.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_viewer_video_light() {
    let setup = Setup {
        videos: true,
        ..WINDOW
    };
    shot_of(
        ThemeChoice::Light,
        "native-viewer-video-light.png",
        setup,
        |harness, _| {
            // The first folder's photos, then its two videos.
            pick(harness, FOLDERS[0].1, Modifiers::NONE);
            harness.key_press(Key::Enter);
            until(harness, "the video's frame drawn", |app| {
                app.video_frame().is_some()
            });
            // The controls stay while the pointer has just moved over the photo.
            harness.event(egui::Event::PointerMoved(egui::pos2(640.0, 400.0)));
            harness.step();
        },
    );
}

// The website's pictures (`site/img`, converted with `magick x.png -quality 85 x.webp`):
// the main window in both themes, the viewer with its panel, and the feature tour's search
// and compare (below), at the page's 1280 by 800.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_site_main_light() {
    shot_of(
        ThemeChoice::Light,
        "native-site-main-light.png",
        SITE,
        |harness, _| lived_in(harness),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_site_main_dark() {
    shot_of(
        ThemeChoice::Dark,
        "native-site-main-dark.png",
        SITE,
        |harness, _| lived_in(harness),
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_site_viewer_info_light() {
    shot_of(
        ThemeChoice::Light,
        "native-site-viewer-info-light.png",
        SITE,
        |harness, db| {
            lived_in(harness);
            // What a camera writes and the scan would have read: these photos carry none.
            let id = harness.state().shown()[2];
            let conn = rusqlite::Connection::open(db).unwrap();
            conn.execute(
                "UPDATE items SET caption = 'Evening over Lisbon', make = 'Canon', \
                 model = 'Canon EOS R6', lens = 'RF24-105mm F4 L IS USM', focal_mm = 50, \
                 aperture = 5.6, exposure_s = 0.004, iso = 200, \
                 gps_lat = 38.7139, gps_lon = -9.1334 WHERE id = ?1",
                [id],
            )
            .unwrap();
            harness.state().engine().refresh_grid().unwrap();
            faces_on(harness, db, false);
        },
    );
}

// The site's feature tour: the search's help over a search typed, and compare.
#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_site_search_light() {
    shot_of(
        ThemeChoice::Light,
        "native-site-search-light.png",
        SITE,
        |harness, _| {
            lived_in(harness);
            harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
            harness.step();
            harness.event(egui::Event::Text("alps".to_owned()));
            until(harness, "the search shown", |app| {
                app.settled().view == GridView::Search
                    && app.last_frame().is_some_and(|frame| frame.settled)
            });
            harness.get_by_label("What you can search for").click();
            until(harness, "the help open", |app| app.search_help_open());
        },
    );
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_site_compare_dark() {
    shot_of(
        ThemeChoice::Dark,
        "native-site-compare-dark.png",
        SITE,
        |harness, _| {
            lived_in(harness);
            comparing(harness);
        },
    );
}

/// The site's library as someone's is: a few photos starred, and the thumbnails in.
fn lived_in(harness: &mut Harness<'_>) {
    let engine = harness.state().engine().clone();
    let shown = harness.state().shown();
    commands::set_stars(&engine, &[shown[0], shown[3], shown[11]], true).unwrap();
    until(harness, "the stars", |app| {
        let (_, index) = app.engine().grid();
        (index.rows(0, index.len()).iter())
            .filter(|entry| entry.starred)
            .count()
            == 3
    });
    thumbnails_in(harness);
}
