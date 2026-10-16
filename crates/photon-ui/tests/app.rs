//! The whole slice without a window: a library on disk, the engine, and frames of the app.

use eframe::egui::{ThemePreference, vec2};
use photon_core::library::{GridTile, Library, ThemeChoice};
use photon_ui::{app::App, dirs};
use std::{
    io::Cursor,
    path::Path,
    time::{Duration, Instant},
};

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([90, 120, 200]),
    ));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();
    bytes
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Steps the app until `done`, or panics after thirty seconds saying what it waited for.
fn until(harness: &mut egui_kittest::Harness<'_, App>, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        harness.step();
        if done(harness.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_library_is_opened_scanned_and_shown_with_its_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("Pictures");
    // Different sizes, so the three files differ in bytes and none is another's copy.
    write(&photos.join("coast").join("a.jpg"), &jpeg(64, 32));
    write(&photos.join("coast").join("b.jpg"), &jpeg(48, 32));
    write(&photos.join("hills").join("c.jpg"), &jpeg(32, 64));
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .build_eframe(|cc| App::new(cc, dirs.clone(), Some(photos.clone())).unwrap());

    // The engine watches the folder it was given, scans it and publishes a grid; the app
    // hears of it and reads it.
    until(&mut harness, "three photos in the grid", |app| {
        app.photos() == 3
    });
    until(&mut harness, "the folder list read", |app| {
        app.folders().len() >= 2
    });
    let names: Vec<&str> = {
        let mut names: Vec<&str> = harness
            .state()
            .folders()
            .values()
            .map(|folder| folder.name.as_str())
            .collect();
        names.sort_unstable();
        names
    };
    assert!(
        names.contains(&"coast") && names.contains(&"hills"),
        "{names:?}"
    );

    // Nothing was cached: each thumbnail is waited for, built by the engine and uploaded.
    until(&mut harness, "every tile has its picture", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.settled && frame.on_screen.len() == 3)
    });
    assert!(dirs.db_path.is_file());
}

// A theme pinned against the desktop's must not show the desktop's for a frame at every
// launch, nor the grid lay itself out twice: what is stored is read before the first
// frame, as the Tauri shell reads the theme in `setup` for its title bar.
#[test]
fn the_stored_theme_and_tile_size_are_in_force_before_the_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("Pictures");
    std::fs::create_dir_all(&photos).unwrap();
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    std::fs::create_dir_all(dirs.db_path.parent().unwrap()).unwrap();
    {
        let library = Library::open(&dirs.db_path).unwrap();
        library.set_theme(ThemeChoice::Light).unwrap();
        library.set_grid_tile(GridTile::Large).unwrap();
    }

    let mut app = None;
    let ctx = eframe::egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    app.replace(App::new(&cc, dirs, Some(photos)).unwrap());
    assert_eq!(
        ctx.options(|options| options.theme_preference),
        ThemePreference::Light
    );
    assert_eq!(app.as_ref().unwrap().tile_size(), GridTile::Large);
}

// eframe keeps the window's size and place for photon, and with them egui's own memory
// unless the application says not to: photon keeps what it remembers itself, and egui's
// copy of an area's place or a field's text would be read back at the next launch.
#[test]
fn the_window_keeps_no_memory_of_egui() {
    let dir = tempfile::tempdir().unwrap();
    let library = crate::common::library(dir.path(), 1, 0, 0);
    let (harness, _driver) = crate::common::opened(&library, 1);
    assert!(!eframe::App::persist_egui_memory(harness.state()));
}

// What eframe writes of a window closed while minimized is put right by the application
// as it is saved: the size and place it was last seen at in its normal state, not the
// 0 by 0 at -32000 that Windows says of a minimized window.
#[test]
fn a_window_saved_while_minimized_is_kept_at_its_normal_size() {
    let dir = tempfile::tempdir().unwrap();
    let library = crate::common::library(dir.path(), 1, 0, 0);
    let (mut harness, mut driver) = crate::common::opened(&library, 1);
    driver.settle(&mut harness);
    let root = eframe::egui::ViewportId::ROOT;
    let info = |minimized| {
        let mut info = eframe::egui::ViewportInfo {
            native_pixels_per_point: Some(1.0),
            inner_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(40.0, 60.0),
                vec2(1000.0, 700.0),
            )),
            minimized: Some(minimized),
            maximized: Some(false),
            fullscreen: Some(false),
            ..Default::default()
        };
        if minimized {
            info.inner_rect = Some(eframe::egui::Rect::ZERO);
        }
        info
    };
    harness.input_mut().viewports.insert(root, info(false));
    harness.step();
    harness.input_mut().viewports.insert(root, info(true));
    harness.step();
    #[derive(Default)]
    struct Memory(std::collections::HashMap<String, String>);
    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }
    let mut memory = Memory::default();
    let minimized = "(inner_position_pixels:Some((x:-32000.0,y:-32000.0)),outer_position_pixels:Some((x:-32000.0,y:-32000.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:0.0,y:0.0)))";
    memory.0.insert("window".to_owned(), minimized.to_owned());
    eframe::App::save(harness.state_mut(), &mut memory);
    let kept = memory.0.get("window").unwrap();
    assert!(
        kept.contains("inner_size_points:Some((x:1000.0,y:700.0))"),
        "kept as {kept}"
    );
    assert!(
        kept.contains("inner_position_pixels:Some((x:40.0,y:60.0))"),
        "kept as {kept}"
    );
}
