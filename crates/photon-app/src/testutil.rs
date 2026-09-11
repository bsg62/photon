#![allow(dead_code)]

use crate::engine::{Engine, EngineConfig};
use crate::events::Recorder;
use photon_core::library::WatchedFolder;
use std::{io::Cursor, path::PathBuf, sync::Arc};
use tempfile::TempDir;

pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        w,
        h,
        image::Rgb([90, 120, 200]),
    ));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .unwrap();
    buf
}

pub struct Fixture {
    pub dir: TempDir,
    pub photos: PathBuf,
    pub events: Arc<Recorder>,
    pub engine: Arc<Engine>,
}

impl Fixture {
    pub fn config(&self) -> EngineConfig {
        config_in(&self.dir)
    }

    /// Watches `photos` and waits for its first scan to finish.
    pub fn add_photos(&self) -> WatchedFolder {
        let watched = self.engine.add_folder(&self.photos).unwrap();
        self.engine.wait_for_scans();
        watched
    }

    /// Item ids in grid order.
    pub fn ids(&self) -> Vec<i64> {
        let (_, grid) = self.engine.grid();
        grid.rows(0, grid.len()).iter().map(|e| e.id).collect()
    }
}

fn config_in(dir: &TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("data").join("library.db"),
        cache_dir: dir.path().join("cache").join("thumbs"),
        workers: 1,
    }
}

/// A temp dir with `photos/` holding `files` (names may contain '/') and an engine whose
/// data and cache live elsewhere in the same temp dir.
pub fn fixture(files: &[(&str, &[u8])]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    for (name, bytes) in files {
        let path = name.split('/').fold(photos.clone(), |p, part| p.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let events = Arc::new(Recorder::default());
    let engine = Engine::open(config_in(&dir), events.clone()).unwrap();
    Fixture {
        dir,
        photos,
        events,
        engine,
    }
}
