//! The engine as a source of thumbnails, against a real library.

use eframe::egui;
use photon_engine::engine::{Engine, EngineConfig};
use photon_ui::{
    events::UiEvents,
    thumbs::{
        loader::{LoadError, ThumbSource, Want},
        source::EngineThumbs,
    },
};
use std::{
    future::Future,
    io::Cursor,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
};

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `future` to its end on this thread.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

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

// `request_async` answers with the photo's thumbnail as it is now, whatever key was asked
// for, and keys recur - "Original", a fourth quarter turn. A picture stored under a key it
// is not would be drawn for the other picture when the photo comes back to that key: the
// hazard `protocol.rs` answers with `immutable` only for the URL key's own file.
#[test]
fn a_built_thumbnail_is_handed_back_only_under_its_own_key() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("a.jpg"), jpeg(64, 32)).unwrap();
    let (events, _receiver) = UiEvents::new(egui::Context::default());
    let config = EngineConfig {
        db_path: dir.path().join("data").join("library.db"),
        cache_dir: dir.path().join("cache").join("thumbs"),
        workers: 1,
    };
    let engine = Engine::open(config, Arc::new(events)).unwrap();
    engine.add_folder(&photos).unwrap();
    engine.wait_for_scans();
    let (_, grid) = engine.grid();
    let entry = grid.rows(0, 1)[0];
    let source = EngineThumbs(engine.clone());

    // Under its own key the build answers with the picture, and the cache then has it.
    // (Whether it was cached before is the engine's own queue's timing.)
    let own = Want {
        id: entry.id,
        key: entry.thumb_key,
    };
    let built = block_on(source.build(own)).unwrap();
    assert_eq!((built.width, built.height), (64, 32));
    assert_eq!(source.cached(entry.thumb_key), Some(built));

    // The same photo under a key it does not have - the one it had before an edit, say.
    let stale = Want {
        id: entry.id,
        key: entry.thumb_key ^ 1,
    };
    assert_eq!(
        block_on(source.build(stale)).err(),
        Some(LoadError::Unavailable)
    );
    engine.shutdown();
}
