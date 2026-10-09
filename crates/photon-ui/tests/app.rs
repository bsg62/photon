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
