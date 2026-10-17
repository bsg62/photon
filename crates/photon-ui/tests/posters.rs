//! Videos' posters, made by the window's own worker (`posters.rs`) through a stand-in for
//! the system's framework: a test cannot count on a decoder (this machine has none), and
//! the backends are tested on their own (`tests/video`). The stand-in answers by the file's
//! name, so each video in a folder is one case.

use crate::common::{Driver, Harness, REFRESH};
use eframe::egui;
use photon_core::{library::Library, media::ThumbState, thumbs::ThumbSize};
use photon_engine::commands;
use photon_ui::{
    app::App,
    video::{Failure, Picture, Stream, Videos},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// The system's framework as a test has it: what it was asked, and an answer by name.
struct StandIn {
    plays: bool,
    patience: Duration,
    asked: Mutex<Vec<(PathBuf, f64)>>,
    /// Held until set: `held.mp4` waits for it.
    released: AtomicBool,
    /// How many posters are being made now, and the most there ever were at once.
    making: AtomicUsize,
    most: AtomicUsize,
}

impl StandIn {
    /// With a patience short enough that a `slow` video runs out of it.
    fn new(plays: bool) -> Arc<Self> {
        Self::patient(plays, Duration::from_millis(300))
    }

    fn patient(plays: bool, patience: Duration) -> Arc<Self> {
        Arc::new(StandIn {
            plays,
            patience,
            asked: Mutex::new(Vec::new()),
            released: AtomicBool::new(false),
            making: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> Vec<String> {
        (self.asked.lock().unwrap().iter())
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }
}

impl Videos for StandIn {
    fn can_play(&self) -> bool {
        self.plays
    }

    fn poster(&self, path: &Path, at: f64) -> Result<Picture, Failure> {
        self.asked.lock().unwrap().push((path.to_owned(), at));
        let now = self.making.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let made = self.make(path);
        self.making.fetch_sub(1, Ordering::SeqCst);
        made
    }

    fn patience(&self) -> Duration {
        self.patience
    }

    fn open(&self, _: &Path) -> Result<Box<dyn Stream>, Failure> {
        Err(Failure::Unsupported("posters only".into()))
    }
}

impl StandIn {
    fn make(&self, path: &Path) -> Result<Picture, Failure> {
        match path.file_name().unwrap().to_str().unwrap() {
            "none.mp4" => Err(Failure::Unsupported("no decoder".into())),
            "bad.mp4" => Err(Failure::Broken("a broken file".into())),
            name if name.starts_with("slow") => {
                std::thread::sleep(Duration::from_secs(2));
                Ok(red())
            }
            "held.mp4" => {
                while !self.released.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(red())
            }
            _ => Ok(red()),
        }
    }
}

fn red() -> Picture {
    Picture {
        width: 320,
        height: 240,
        at: 0.0,
        rgba: [220, 20, 20, 255].repeat(320 * 240),
    }
}

/// A watched folder of one photo and the videos `names` - no video at all inside, which
/// the library does not look at, and the stand-in does not either.
fn folder(dir: &Path, names: &[&str]) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let mut photo = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        64,
        48,
        image::Rgb([20, 90, 200]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut photo),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    std::fs::write(folder.join("a.jpg"), photo).unwrap();
    for name in names {
        std::fs::write(folder.join(name), format!("not a video: {name}")).unwrap();
    }
    folder
}

/// The application over `folder`, its posters made by `posters`, once its photos and
/// videos are shown.
fn launched<'a>(
    dir: &Path,
    folder: &Path,
    posters: Arc<StandIn>,
    count: usize,
) -> (Harness<'a>, Driver, PathBuf) {
    let dirs = photon_ui::dirs::within(&dir.join("data"), &dir.join("cache"));
    let db = dirs.db_path.clone();
    let pictures = Some(folder.to_owned());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            App::new(cc, dirs, pictures)
                .unwrap()
                .without_animations()
                .with_videos(posters)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the folder shown", |app| {
        app.photos() == count
    });
    (harness, driver, db)
}

/// The id of the file called `name` in the grid on screen.
fn id_of(harness: &Harness<'_>, name: &str) -> i64 {
    let engine = harness.state().engine().clone();
    (harness.state().shown().into_iter())
        .find(|&id| commands::viewer_item(&engine, id).unwrap().file_name == name)
        .unwrap_or_else(|| panic!("{name} is not in the grid"))
}

fn state(db: &Path, id: i64) -> ThumbState {
    Library::open(db)
        .unwrap()
        .item(id)
        .unwrap()
        .unwrap()
        .thumb_state
}

#[test]
fn a_videos_poster_is_made_by_the_system_and_stored_as_its_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let folder = folder(dir.path(), &["red.mp4"]);
    let posters = StandIn::new(true);
    let (mut harness, mut driver, _) = launched(dir.path(), &folder, posters.clone(), 2);
    let video = id_of(&harness, "red.mp4");
    let engine = harness.state().engine().clone();
    driver.until(&mut harness, "the poster stored", |_| {
        (engine.lib.item(video).unwrap().unwrap()).thumb_state == ThumbState::Ready
    });
    // Asked of the file itself, at its start: the library knows no length for it.
    let asked = posters.asked.lock().unwrap().clone();
    // (In the form the library keeps a path in, which on Windows is not std's.)
    let path = photon_core::paths::canonicalize(folder.join("red.mp4")).unwrap();
    assert_eq!(asked, [(path, 0.0)]);
    // And the grid's thumbnail is the picture the system made.
    let key = engine.lib.item(video).unwrap().unwrap().thumb_key();
    let tile = engine.thumbs.decoded(key, ThumbSize::Grid).unwrap();
    let [r, g, b, _] = tile.get_pixel(tile.width() / 2, tile.height() / 2).0;
    assert!(r > 180 && g < 70 && b < 70, "{:?}", (r, g, b));
}

// The web view's rule: what the system lacks keeps a video waiting for a session that can
// decode it; a file that fails, and one that takes too long, are failed.
#[test]
fn a_video_the_system_cannot_decode_waits_and_one_that_fails_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = folder(dir.path(), &["none.mp4", "bad.mp4", "slow.mp4"]);
    let posters = StandIn::new(true);
    let (mut harness, mut driver, db) = launched(dir.path(), &folder, posters.clone(), 4);
    let ids = ["none.mp4", "bad.mp4", "slow.mp4"].map(|name| id_of(&harness, name));
    let engine = harness.state().engine().clone();
    let failed = |id: i64| engine.lib.item(id).unwrap().unwrap().thumb_state == ThumbState::Failed;
    driver.until(&mut harness, "every video asked about", |_| {
        posters.asked().len() == 3 && failed(ids[1]) && failed(ids[2])
    });
    // Closed, the window waits for the worker to answer the job it has: the last answer
    // has landed.
    drop(harness);
    assert_eq!(state(&db, ids[0]), ThumbState::Pending);
    assert_eq!(state(&db, ids[1]), ThumbState::Failed);
    assert_eq!(state(&db, ids[2]), ThumbState::Failed);
}

#[test]
fn a_system_that_plays_nothing_is_asked_for_no_poster() {
    let dir = tempfile::tempdir().unwrap();
    let folder = folder(dir.path(), &["red.mp4"]);
    let posters = StandIn::new(false);
    let (mut harness, mut driver, db) = launched(dir.path(), &folder, posters.clone(), 2);
    let video = id_of(&harness, "red.mp4");
    driver.settle(&mut harness);
    drop(harness);
    assert!(posters.asked().is_empty(), "{:?}", posters.asked());
    assert_eq!(state(&db, video), ThumbState::Pending);
}

// A window closed while a poster is being made answers the job as not made this
// session, at once: unanswered, it is counted at the next launch as a window that died
// opening the video, and the video is failed for good after two.
#[test]
fn a_poster_being_made_as_the_window_closes_leaves_its_video_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let folder = folder(dir.path(), &["held.mp4"]);
    // Patient enough that only the closing ends the wait, on a runner however slow.
    let posters = StandIn::patient(true, Duration::from_secs(60));
    let (mut harness, mut driver, db) = launched(dir.path(), &folder, posters.clone(), 2);
    let video = id_of(&harness, "held.mp4");
    driver.until(&mut harness, "the poster asked for", |_| {
        !posters.asked().is_empty()
    });
    // The engine is kept past the window: let go of, it takes every marker with it,
    // answered or not, and only an answer is to be seen to remove this one.
    let engine = harness.state().engine().clone();
    drop(harness);
    posters.released.store(true, Ordering::SeqCst);
    assert_eq!(state(&db, video), ThumbState::Pending);
    // And its claim was answered: the engine's marker of a video being opened is gone,
    // which left behind is what the next launch counts as a death.
    let markers = in_flight(&dir.path().join("cache"));
    assert!(
        !markers
            .iter()
            .any(|path| path.ends_with(format!("video/in-flight/{video}"))),
        "{markers:?}"
    );
    drop(engine);
}

// One poster at a time, a decoder left behind at its patience included: beside it, every
// video of a share that answers nothing would leave one more open for the session.
#[test]
fn a_decoder_left_behind_is_waited_for_before_the_next_poster() {
    let dir = tempfile::tempdir().unwrap();
    let folder = folder(dir.path(), &["slow1.mp4", "slow2.mp4"]);
    let posters = StandIn::new(true);
    let (mut harness, mut driver, _) = launched(dir.path(), &folder, posters.clone(), 3);
    let ids = ["slow1.mp4", "slow2.mp4"].map(|name| id_of(&harness, name));
    let engine = harness.state().engine().clone();
    let failed = |id: i64| engine.lib.item(id).unwrap().unwrap().thumb_state == ThumbState::Failed;
    driver.until(&mut harness, "both timed out", |_| {
        failed(ids[0]) && failed(ids[1])
    });
    assert_eq!(posters.most.load(Ordering::SeqCst), 1);
}

/// Every file under `dir` whose parent is called `in-flight`.
fn in_flight(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(in_flight(&path));
        } else if path
            .parent()
            .is_some_and(|parent| parent.ends_with("in-flight"))
        {
            found.push(path);
        }
    }
    found
}
