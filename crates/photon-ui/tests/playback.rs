//! A video played in the viewer, through a stand-in for the system's framework whose
//! player the test drives: it is told to play, and hands over the frames the test gives
//! it. The backends themselves are tested on their own (`tests/video`).

use crate::common::{Driver, Harness, REFRESH, said};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_engine::commands;
use photon_ui::{
    app::App,
    playback::FAILED,
    video::{Failure, Picture, Stream, Videos},
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// What the test and the stand-in's players share.
#[derive(Default)]
struct Told {
    /// The files players were opened for.
    opened: Vec<PathBuf>,
    played: bool,
    /// What the players were told of their sound and their playing, in order.
    log: Vec<String>,
    /// How many players have been let go of.
    dropped: usize,
    frames: VecDeque<Picture>,
    failure: Option<Failure>,
}

struct StandIn(Arc<Mutex<Told>>);

impl Videos for StandIn {
    fn can_play(&self) -> bool {
        true
    }

    fn poster(&self, _: &Path, _: f64) -> Result<Picture, Failure> {
        Ok(frame(0.0, [200, 30, 30]))
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Stream>, Failure> {
        self.0.lock().unwrap().opened.push(path.to_owned());
        Ok(Box::new(Player(self.0.clone())))
    }
}

struct Player(Arc<Mutex<Told>>);

impl Drop for Player {
    fn drop(&mut self) {
        self.0.lock().unwrap().dropped += 1;
    }
}

impl Stream for Player {
    fn ready(&self) -> bool {
        true
    }
    fn play(&self) {
        let mut told = self.0.lock().unwrap();
        told.played = true;
        told.log.push("play".into());
    }
    fn pause(&self) {
        self.0.lock().unwrap().log.push("pause".into());
    }
    fn seek(&self, at: f64) {
        self.0.lock().unwrap().log.push(format!("seek {at}"));
    }
    fn set_volume(&self, volume: f64) {
        self.0.lock().unwrap().log.push(format!("volume {volume}"));
    }
    fn set_muted(&self, muted: bool) {
        self.0.lock().unwrap().log.push(format!("muted {muted}"));
    }
    fn position(&self) -> Option<f64> {
        None
    }
    fn duration(&self) -> Option<f64> {
        Some(4.0)
    }
    fn ended(&self) -> bool {
        false
    }
    fn failure(&self) -> Option<Failure> {
        self.0.lock().unwrap().failure.clone()
    }
    fn frame(&self) -> Option<Picture> {
        self.0.lock().unwrap().frames.pop_front()
    }
}

fn frame(at: f64, colour: [u8; 3]) -> Picture {
    let [r, g, b] = colour;
    Picture {
        width: 64,
        height: 48,
        at,
        rgba: [r, g, b, 255].repeat(64 * 48),
    }
}

/// A watched folder of `a.jpg`, `v.mp4`, `w.mp4` and `c.jpg`, in the grid's order - no
/// video inside an `.mp4`, which neither the library nor the stand-in looks at.
fn folder(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, name) in ["a.jpg", "v.mp4", "w.mp4", "c.jpg"].into_iter().enumerate() {
        let bytes = if name.ends_with(".mp4") {
            b"not a video".to_vec()
        } else {
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                160,
                120,
                image::Rgb([20, 90, 200]),
            ))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
            bytes
        };
        let path = folder.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(start + Duration::from_secs(60 * n as u64))
            .unwrap();
    }
    folder
}

/// The application over `folder(dir)` with the stand-in, once its four are shown.
fn launched<'a>(dir: &Path, told: &Arc<Mutex<Told>>) -> (Harness<'a>, Driver, PathBuf) {
    let folder = folder(dir);
    let dirs = photon_ui::dirs::within(&dir.join("data"), &dir.join("cache"));
    let (pictures, videos) = (Some(folder.clone()), Arc::new(StandIn(told.clone())));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            App::new(cc, dirs, pictures)
                .unwrap()
                .without_animations()
                .with_videos(videos)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the four shown", |app| {
        app.photos() == 4 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver, folder)
}

fn offset_of(harness: &Harness<'_>, name: &str) -> usize {
    let engine = harness.state().engine().clone();
    (harness.state().shown().into_iter())
        .position(|id| commands::viewer_item(&engine, id).unwrap().file_name == name)
        .unwrap_or_else(|| panic!("{name} is not in the grid"))
}

fn button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// Opens the viewer on `name` by a double click, and waits for the library to have said
/// what it is.
fn open(driver: &mut Driver, harness: &mut Harness<'_>, name: &str) {
    let offset = offset_of(harness, name);
    let at = harness.state().tile_rect(offset).unwrap().center();
    for _ in 0..2 {
        harness.event(egui::Event::PointerMoved(at));
        harness.event(button(at, true));
        harness.event(button(at, false));
        driver.act(harness);
    }
    driver.until(harness, "the video's facts", |app| {
        app.viewer_item().is_some()
    });
}

#[test]
fn a_video_plays_as_it_opens_its_frames_in_its_posters_place() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, folder) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        !told.lock().unwrap().opened.is_empty()
    });
    // Its own file, told to play at once.
    let path = photon_core::paths::canonicalize(folder.join("v.mp4")).unwrap();
    assert_eq!(told.lock().unwrap().opened, [path]);
    assert!(told.lock().unwrap().played);
    // Its poster until a frame comes, and then the frame, and the next.
    assert_eq!(harness.state().video_frame(), None);
    told.lock()
        .unwrap()
        .frames
        .push_back(frame(0.5, [20, 200, 20]));
    driver.until(&mut harness, "the first frame drawn", |app| {
        app.video_frame() == Some(0.5)
    });
    told.lock()
        .unwrap()
        .frames
        .push_back(frame(0.6, [20, 20, 200]));
    driver.until(&mut harness, "the next frame drawn", |app| {
        app.video_frame() == Some(0.6)
    });
    // And nothing is said over it.
    assert!(harness.query_by_label(FAILED).is_none());
}

#[test]
fn leaving_a_video_lets_its_player_go() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, folder) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        told.lock().unwrap().opened.len() == 1
    });
    // A step to the next video: its player goes, and the next one's is its own.
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    driver.act(&mut harness);
    driver.until(&mut harness, "the next one's player", |_| {
        told.lock().unwrap().opened.len() == 2
    });
    let next = photon_core::paths::canonicalize(folder.join("w.mp4")).unwrap();
    assert_eq!(told.lock().unwrap().opened[1], next);
    assert_eq!(told.lock().unwrap().dropped, 1);
    // A step to a photo, and the viewer closed: no player is left.
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    driver.act(&mut harness);
    driver.until(&mut harness, "the second let go of", |_| {
        told.lock().unwrap().dropped == 2
    });
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowLeft);
    driver.act(&mut harness);
    driver.until(&mut harness, "a third player", |_| {
        told.lock().unwrap().opened.len() == 3
    });
    harness.key_press_modifiers(Modifiers::NONE, Key::Escape);
    driver.act(&mut harness);
    driver.until(&mut harness, "the third let go of", |_| {
        told.lock().unwrap().dropped == 3
    });
}

#[test]
fn a_video_that_fails_shows_its_poster_says_so_and_lets_its_player_go() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, _) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        !told.lock().unwrap().opened.is_empty()
    });
    told.lock()
        .unwrap()
        .frames
        .push_back(frame(0.5, [20, 200, 20]));
    driver.until(&mut harness, "a frame drawn", |app| {
        app.video_frame() == Some(0.5)
    });
    told.lock().unwrap().failure = Some(Failure::Broken("a broken file".into()));
    // Its player let go of - on Linux it would play its sound on - and its poster, not
    // the last frame it drew, under what is said.
    driver.until(&mut harness, "its player let go of", |_| {
        told.lock().unwrap().dropped == 1
    });
    driver.act(&mut harness);
    assert_eq!(harness.state().video_frame(), None);
    assert!(
        harness.query_by_label(FAILED).is_some(),
        "{:?}",
        said(&harness)
    );
}

// A held arrow passes videos faster than a player opens: one is opened only once the
// viewer has rested on its video, as a photo's full picture is made.
#[test]
fn a_video_stepped_past_is_not_played() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, folder) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        told.lock().unwrap().opened.len() == 1
    });
    // Over `w.mp4` to `c.jpg`, a frame apart.
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    harness.step();
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    driver.act(&mut harness);
    driver.settle(&mut harness);
    let first = photon_core::paths::canonicalize(folder.join("v.mp4")).unwrap();
    assert_eq!(told.lock().unwrap().opened, [first]);
}

fn log(told: &Arc<Mutex<Told>>) -> Vec<String> {
    std::mem::take(&mut told.lock().unwrap().log)
}

// Space pauses and plays, M mutes - and the sound is carried to the next video, set
// before it plays, for as long as photon is open.
#[test]
fn a_videos_keys_reach_its_player_and_its_sound_goes_on_to_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, _) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        told.lock().unwrap().opened.len() == 1
    });
    // A frame, so that it plays: its strip is drawn while it does.
    told.lock()
        .unwrap()
        .frames
        .push_back(frame(0.5, [20, 200, 20]));
    driver.until(&mut harness, "a frame drawn", |app| {
        app.video_frame() == Some(0.5)
    });
    assert_eq!(log(&told), ["volume 1", "muted false", "play"]);
    for key in [Key::Space, Key::Space, Key::M] {
        harness.key_press_modifiers(Modifiers::NONE, key);
        driver.act(&mut harness);
    }
    assert_eq!(log(&told), ["pause", "play", "muted true"]);
    assert!(
        harness.query_by_label("Unmute").is_some(),
        "{:?}",
        said(&harness)
    );
    // The next video starts muted.
    harness.key_press_modifiers(Modifiers::NONE, Key::ArrowRight);
    driver.act(&mut harness);
    driver.until(&mut harness, "the next one's player", |_| {
        told.lock().unwrap().opened.len() == 2
    });
    assert_eq!(log(&told), ["volume 1", "muted true", "play"]);
}

// A double click on an item of the viewer's menu that lies over the picture: the first
// click picks the item and closes the menu, the second lands on the picture. It pauses
// nothing; a click there a moment later does.
#[test]
fn a_double_click_on_a_menus_item_over_a_video_does_not_pause_it() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(Told::default()));
    let (mut harness, mut driver, _) = launched(dir.path(), &told);
    open(&mut driver, &mut harness, "v.mp4");
    driver.until(&mut harness, "its player opened", |_| {
        told.lock().unwrap().opened.len() == 1
    });
    told.lock()
        .unwrap()
        .frames
        .push_back(frame(0.5, [20, 200, 20]));
    driver.until(&mut harness, "a frame drawn", |app| {
        app.video_frame() == Some(0.5)
    });
    let middle = egui::pos2(500.0, 300.0);
    harness.event(egui::Event::PointerMoved(middle));
    harness.event(press(middle, PointerButton::Secondary, true));
    harness.event(press(middle, PointerButton::Secondary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    let item = harness.get_by_label("Hide photo").rect().center();
    log(&told);
    harness.event(egui::Event::PointerMoved(item));
    harness.event(press(item, PointerButton::Primary, true));
    harness.event(press(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(
        !harness.state().menu_open(),
        "the first click picked the item"
    );
    // The second, a tenth of a second on.
    driver.frames_in(&mut harness, 0.1);
    harness.event(press(item, PointerButton::Primary, true));
    harness.event(press(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(
        !log(&told).contains(&"pause".to_owned()),
        "the second click paused it"
    );
    // Once a double click's time is over, a click on the picture is a click on it.
    driver.frames_in(&mut harness, 1.0);
    harness.event(press(item, PointerButton::Primary, true));
    harness.event(press(item, PointerButton::Primary, false));
    driver.act(&mut harness);
    assert!(log(&told).contains(&"pause".to_owned()));
}

fn press(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}
