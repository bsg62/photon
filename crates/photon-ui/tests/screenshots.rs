//! The grid as it is drawn, written to PNG without a window: the application itself, over
//! a library made of the CC0 photos the Svelte screenshots use (credited in their
//! `CREDITS.md`), rendered off screen through wgpu.
//!
//! Ignored by default - it needs a GPU adapter, which a CI runner need not have - and run
//! by `cargo run -p xtask -- native-shot`. Like `xtask screenshots` it replaces no item of
//! the smoke checklist: it is one renderer's picture, read by whoever runs it.
//!
//! Three of the folders are named in other scripts on purpose. What their headers show is
//! what the pull request reports about text (`src/text.rs`).

use eframe::egui::vec2;
use photon_core::library::{Library, ThemeChoice};
use photon_ui::{app::App, dirs};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The folders of the library, and how many of the photos each takes.
const FOLDERS: [(&str, usize); 4] = [
    ("2026-07 Coast", 7),
    ("東京 2024 桜", 5),
    ("رحلة 2024 الصيف", 5),
    ("🎉 Party שלום abc", 6),
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
    for (folder, count) in FOLDERS {
        let folder = pictures.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for photo in photos.by_ref().take(count) {
            std::fs::copy(&photo, folder.join(photo.file_name().unwrap())).unwrap();
        }
    }
}

fn shot(theme: ThemeChoice, name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    library(&pictures);
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    // The theme is the stored one, read the way the application reads it: a harness has
    // no desktop to follow.
    std::fs::create_dir_all(dirs.db_path.parent().unwrap()).unwrap();
    Library::open(&dirs.db_path)
        .unwrap()
        .set_theme(theme)
        .unwrap();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 1000.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(|cc| App::new(cc, dirs, Some(pictures)).unwrap());

    let total: usize = FOLDERS.iter().map(|(_, count)| count).sum();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        harness.step();
        let app = harness.state();
        let shown = app.last_frame().is_some_and(|frame| frame.settled);
        if app.photos() == total && app.folders().len() >= FOLDERS.len() && shown {
            break;
        }
        assert!(Instant::now() < deadline, "the library never came up");
        std::thread::sleep(Duration::from_millis(10));
    }
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
fn native_grid_light() {
    shot(ThemeChoice::Light, "native-grid-light.png");
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_grid_dark() {
    shot(ThemeChoice::Dark, "native-grid-dark.png");
}
