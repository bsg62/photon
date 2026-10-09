//! The whole slice without a window: a library on disk, the engine, and frames of the app.

use eframe::egui::vec2;
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
