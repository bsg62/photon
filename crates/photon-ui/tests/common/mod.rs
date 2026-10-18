//! A window's event loop, without the window, for tests of the whole application.
//!
//! A frame is drawn when one was asked for and not otherwise, no sooner than a refresh
//! after the last, on a clock that jumps over the time in which nothing was. egui tells
//! whatever runs it of every request through one callback - it is how eframe learns of
//! them - so the frames drawn here are the ones a window would draw. Stepping the harness
//! blindly draws a frame at every step, which says nothing about a still window, and hid
//! three things the gate's first count of idle frames got wrong (docs/architecture/ui-testing.md, "A test that
//! counts frames").
//!
//! A clock that jumps leaves the application's other threads behind: a second of it passes
//! in no time at all, and what a decoder or the engine was still doing then arrives
//! "seconds later". So before the clock is taken over a stretch nothing asked for, the
//! others are waited for, in real time and none of this clock's (`settle`).

#![allow(dead_code)]

use eframe::egui;
use egui_kittest::kittest::Queryable;
use photon_core::library::Library;
use photon_ui::{app::App, fixture};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub type Harness<'a> = egui_kittest::Harness<'a, App>;

/// The screen: sixty frames a second.
pub const REFRESH: f64 = 1.0 / 60.0;
/// How long no other thread must have asked for a frame before they are taken to be done.
const QUIET: Duration = Duration::from_millis(250);
/// How long anything is waited for before the test fails saying what it waited for.
const PATIENCE: Duration = Duration::from_secs(30);

pub struct Driver {
    clock: Arc<Mutex<f64>>,
    due: Arc<Mutex<Option<f64>>>,
    /// When another thread last asked for a frame: the engine, a decoder, a task.
    stray: Arc<Mutex<Instant>>,
    pub now: f64,
    /// When the last frame was drawn: no frame is drawn before it.
    drawn_at: f64,
    /// How many frames have been drawn.
    pub frames: usize,
    /// What the system says is held over the window. It says so in every frame until it
    /// says otherwise, as `egui-winit` keeps it: `hold_over` and `drop_on` are its word.
    pub held: Vec<egui::HoveredFile>,
}

impl Driver {
    pub fn new(harness: &Harness<'_>) -> Self {
        let clock = Arc::new(Mutex::new(0.0_f64));
        // The application was made before anyone listened: its first frame is due.
        let due = Arc::new(Mutex::new(Some(0.0_f64)));
        let stray = Arc::new(Mutex::new(Instant::now()));
        {
            let (clock, due, stray) = (clock.clone(), due.clone(), stray.clone());
            let ui_thread = std::thread::current().id();
            harness.ctx.set_request_repaint_callback(move |asked| {
                if std::thread::current().id() != ui_thread {
                    *stray.lock().unwrap() = Instant::now();
                }
                let at = *clock.lock().unwrap() + asked.delay.as_secs_f64();
                let mut due = due.lock().unwrap();
                *due = Some(due.map_or(at, |before| before.min(at)));
            });
        }
        Self {
            clock,
            due,
            stray,
            now: 0.0,
            drawn_at: 0.0,
            frames: 0,
            held: Vec::new(),
        }
    }

    fn due(&self) -> Option<f64> {
        *self.due.lock().unwrap()
    }

    /// Draws the frame at `at`. Whatever the user was made to do since the last one
    /// (`Harness::event`, a node's `click`) is drawn with it - one frame for each event
    /// queued, which is how `egui_kittest` steps, and counted here as one: two events in
    /// one frame are a test in whole frames of a view's, not this driver's.
    ///
    /// Never before the last frame: `frames_in` draws a frame asked for within its span a
    /// refresh after the one before, which can be past the span's end, and a test's frame at
    /// a time of its own after it (`pressed + 0.2`, after six refreshes from `pressed + 0.1`)
    /// came a rounding before it, and egui's pointer history panicked ("Time shouldn't move
    /// backwards", macOS' CI, on whether another thread had asked for that frame).
    pub fn frame(&mut self, harness: &mut Harness<'_>, at: f64) {
        let at = at.max(self.drawn_at);
        self.drawn_at = at;
        *self.due.lock().unwrap() = None;
        self.now = at;
        *self.clock.lock().unwrap() = at;
        harness.input_mut().time = Some(at);
        harness.input_mut().hovered_files = self.held.clone();
        hand_over(harness);
        harness.step();
        self.frames += 1;
    }

    /// The next frame, a refresh on, asked for or not: the one that carries what the user
    /// just did.
    pub fn act(&mut self, harness: &mut Harness<'_>) {
        self.frame(harness, self.now + REFRESH);
    }

    /// Draws the frames that are asked for, as they are, until `done`.
    pub fn until(&mut self, harness: &mut Harness<'_>, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        let mut idle = Instant::now();
        loop {
            if self.frames > 0 && done(harness.state()) {
                return;
            }
            assert!(Instant::now() < deadline, "never: {what}");
            match self.due() {
                // Soon: draw it. Far off - a message's own end, say - and another thread
                // may answer first: give it a moment of real time before the clock jumps.
                Some(asked) if asked - self.now <= 0.1 || idle.elapsed() > QUIET => {
                    self.frame(harness, asked.max(self.now + REFRESH));
                    idle = Instant::now();
                }
                _ => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    /// Waits until the application's other threads are done with what they were doing: no
    /// picture on its way, and no frame asked for by another thread for a while. What
    /// they ask for meanwhile is drawn at once, the clock all but standing.
    pub fn settle(&mut self, harness: &mut Harness<'_>) {
        let since = Instant::now();
        let deadline = since + PATIENCE;
        loop {
            assert!(
                Instant::now() < deadline,
                "the application never came to rest"
            );
            if let Some(asked) = self.due()
                && asked - self.now <= 0.1
            {
                self.frame(harness, self.now + 0.001);
                continue;
            }
            let loading = harness
                .state()
                .last_frame()
                .is_some_and(|frame| frame.loading);
            let quiet = since.max(*self.stray.lock().unwrap()).elapsed() >= QUIET;
            if !loading && quiet {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Draws the frames that are asked for until something called `label` is drawn. A
    /// row of the sidebar that is there only while the library holds something of its
    /// kind - Hidden, Starred - comes with the counts, which are read off the UI thread
    /// a moment after the grid: pressed at once, it was not there yet on Windows' CI.
    pub fn seen(&mut self, harness: &mut Harness<'_>, label: &str) {
        let deadline = Instant::now() + PATIENCE;
        let mut idle = Instant::now();
        while harness.query_by_label(label).is_none() {
            assert!(Instant::now() < deadline, "never drawn: {label}");
            match self.due() {
                Some(asked) if asked - self.now <= 0.1 || idle.elapsed() > QUIET => {
                    self.frame(harness, asked.max(self.now + REFRESH));
                    idle = Instant::now();
                }
                _ => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    /// Waits, drawing nothing, until no other thread has asked for a frame for a while:
    /// for what the engine and the tasks do with something the window is not to hear of
    /// yet.
    pub fn quiet(&mut self) {
        let since = Instant::now();
        let deadline = since + PATIENCE;
        while since.max(*self.stray.lock().unwrap()).elapsed() < QUIET {
            assert!(Instant::now() < deadline, "the other threads never rested");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The next `secs` of the clock: how many frames were drawn in them. Call `settle`
    /// first; what another thread asks for after the clock has jumped is counted.
    pub fn frames_in(&mut self, harness: &mut Harness<'_>, secs: f64) -> usize {
        let end = self.now + secs;
        let before = self.frames;
        while let Some(asked) = self.due()
            && asked <= end
        {
            self.frame(harness, asked.max(self.now + REFRESH));
        }
        self.now = end;
        *self.clock.lock().unwrap() = end;
        self.frames - before
    }
}

/// Three small JPEGs to make thumbnails from.
pub fn sources(dir: &Path) -> Vec<PathBuf> {
    [(64, 48), (48, 64), (96, 64)]
        .iter()
        .enumerate()
        .map(|(n, &(width, height))| {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                width,
                height,
                image::Rgb([40 * n as u8, 120, 200]),
            ));
            let mut bytes = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
                .unwrap();
            let path = dir.join(format!("source-{n}.jpg"));
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect()
}

/// A library of `photos` photos under `dir`, every thumbnail cached and nothing to scan
/// (`fixture::build`), of which the first `starred` in grid order are starred and the
/// last `hidden` are hidden.
pub fn library(dir: &Path, photos: usize, starred: usize, hidden: usize) -> fixture::Fixture {
    let built = fixture::build(&dir.join("library"), photos, &sources(dir)).unwrap();
    let lib = Library::open(&built.dirs().db_path).unwrap();
    let ids: Vec<i64> = lib
        .grid_entries()
        .unwrap()
        .iter()
        .map(|entry| entry.id)
        .collect();
    let stars: Vec<(i64, u8)> = ids.iter().take(starred).map(|id| (*id, 1)).collect();
    lib.set_ratings(&stars).unwrap();
    lib.set_hidden(&ids[ids.len() - hidden..], true).unwrap();
    built
}

/// The application over `library`, in a window 1000 by 700.
pub fn launch<'a>(library: &fixture::Fixture) -> Harness<'a> {
    launch_with(library, no_platform())
}

/// The application over `library`, with `platform` for the system.
pub fn launch_with<'a>(
    library: &fixture::Fixture,
    platform: std::sync::Arc<Picker>,
) -> Harness<'a> {
    let dirs = library.dirs();
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, None).unwrap().without_animations())
                .with_videos(no_videos())
                .with_platform(platform)
        })
}

/// The application over `library` with its animations on, as a user who has not switched
/// them off has it: for the tests of what moves. Every other test launches without them,
/// since a row pressed straight after a toggle would be pressed where it is sliding from.
pub fn launch_animated<'a>(library: &fixture::Fixture) -> Harness<'a> {
    let dirs = library.dirs();
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            (App::new(cc, dirs, None).unwrap())
                .with_videos(no_videos())
                .with_platform(no_platform())
        })
}

/// `opened`, with the animations on (`launch_animated`).
pub fn opened_animated<'a>(library: &fixture::Fixture, photos: usize) -> (Harness<'a>, Driver) {
    let mut harness = launch_animated(library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == photos && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver)
}

/// Clicks the sidebar row called `label`, once it is drawn, and draws the frame the
/// click is in.
pub fn click(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    driver.seen(harness, label);
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// Launches over `library` and waits until its grid is there.
pub fn opened<'a>(library: &fixture::Fixture, photos: usize) -> (Harness<'a>, Driver) {
    opened_with(library, photos, no_platform())
}

/// `opened`, with `platform` for the system.
pub fn opened_with<'a>(
    library: &fixture::Fixture,
    photos: usize,
    platform: std::sync::Arc<Picker>,
) -> (Harness<'a>, Driver) {
    let mut harness = launch_with(library, platform);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == photos && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver)
}

/// A folder called `name` under `dir` with real photos called `names` in it, each a
/// byte longer than the one before so that none is another's copy, and a minute newer.
pub fn photos_in(dir: &Path, name: &str, names: &[&str]) -> PathBuf {
    let folder = dir.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    // The picture they are copies of is made beside the folder, not in it: it is no
    // photo of the library's.
    let made = tempfile::tempdir().unwrap();
    let picture = std::fs::read(&sources(made.path())[0]).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, file) in names.iter().enumerate() {
        let path = folder.join(file);
        let mut bytes = picture.clone();
        bytes.extend(std::iter::repeat_n(0, n + 1));
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

/// The application over a library of its own making under `dir`, which starts by
/// watching `pictures` - or nothing, as a first run with no Pictures folder does. The
/// library is real: its folders are scanned and its files are there to be exported.
pub fn over<'a>(
    dir: &Path,
    pictures: Option<&Path>,
) -> (Harness<'a>, Driver, photon_ui::dirs::Dirs) {
    over_with(dir, pictures, no_platform())
}

/// `over`, with `platform` for the system.
pub fn over_with<'a>(
    dir: &Path,
    pictures: Option<&Path>,
    platform: std::sync::Arc<Picker>,
) -> (Harness<'a>, Driver, photon_ui::dirs::Dirs) {
    over_built(dir, pictures, platform, false)
}

/// `over`, with the animations on (`launch_animated`).
pub fn over_animated<'a>(
    dir: &Path,
    pictures: Option<&Path>,
) -> (Harness<'a>, Driver, photon_ui::dirs::Dirs) {
    over_built(dir, pictures, no_platform(), true)
}

fn over_built<'a>(
    dir: &Path,
    pictures: Option<&Path>,
    platform: std::sync::Arc<Picker>,
    animations: bool,
) -> (Harness<'a>, Driver, photon_ui::dirs::Dirs) {
    let made = photon_ui::dirs::within(&dir.join("data"), &dir.join("cache"));
    let (dirs, pictures) = (made.clone(), pictures.map(Path::to_owned));
    let harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| {
            let app = App::new(cc, dirs, pictures).unwrap();
            let app = if animations {
                app
            } else {
                app.without_animations()
            };
            app.with_videos(no_videos()).with_platform(platform)
        });
    let driver = Driver::new(&harness);
    (harness, driver, made)
}

/// A file as the system hands it to a window it was dropped on.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("a test's dropped file is never read".to_owned())
    }
}

/// Hands the next frame's input to the application before egui reads it, as eframe does
/// before every frame of a window and `egui_kittest` does not (`App::raw_input_hook`).
pub fn hand_over(harness: &mut Harness<'_>) {
    use eframe::App as _;
    let ctx = harness.ctx.clone();
    let mut input = std::mem::take(harness.input_mut());
    harness.state_mut().raw_input_hook(&ctx, &mut input);
    *harness.input_mut() = input;
}

/// Holds `paths` over the window, as a drag from a file manager does, and draws the
/// frame the system asks for. They are held until they are dropped, or the test takes
/// them out of `Driver::held`.
pub fn hold_over(driver: &mut Driver, harness: &mut Harness<'_>, paths: &[&Path]) {
    driver.held = (paths.iter())
        .map(|path| egui::HoveredFile {
            path: Some((*path).to_owned()),
            ..Default::default()
        })
        .collect();
    driver.act(harness);
}

/// Drops `paths` on the window: they are no longer held over it, and are the frame's
/// dropped files.
pub fn drop_on(driver: &mut Driver, harness: &mut Harness<'_>, paths: &[&Path]) {
    driver.held.clear();
    for path in paths {
        let file: egui::DroppedFileHandle = Arc::new(Dropped((*path).to_owned()));
        harness.input_mut().dropped_files.push(file);
    }
    driver.act(harness);
}

/// What the window has said, oldest first.
pub fn said(harness: &Harness<'_>) -> Vec<String> {
    (harness.state().toasts().iter())
        .map(|toast| toast.message.clone())
        .collect()
}

/// The ids of `library`'s photos, in grid order: the newest folder first.
pub fn photo_ids(library: &fixture::Fixture) -> Vec<i64> {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    (lib.grid_entries().unwrap().iter())
        .map(|entry| entry.id)
        .collect()
}

/// A system that plays no video, for the tests that are not about videos (`tests/posters.rs`
/// and `tests/playback.rs` are): over the system's own framework, a file called a video
/// would be decoded on a runner that has decoders and not on one that has none, and each
/// system calls the same unreadable file something else.
/// A stand-in for the system's pickers that chooses nothing, as a cancel does. Every test
/// of the whole application that can press "Choose…" or the empty library's "Add folder…"
/// launches with one (or with a `Picker`): over the system's own, the press would open a
/// real picker on whatever machine the test runs on.
pub fn no_platform() -> std::sync::Arc<Picker> {
    std::sync::Arc::new(Picker::default())
}

/// A stand-in for the system's folder picker and clipboard. The picker answers `answer`
/// (a cancel until told otherwise), says what it was asked under `asked`, and - while
/// `held` holds a receiver - waits until it is sent to. The clipboard keeps the size of
/// every picture put on it in `copied`, or refuses with `refused`. The file manager and the
/// opener say what they were asked in `opened` - "reveal <path>", "open <target>".
pub struct Picker {
    pub opened: std::sync::Mutex<Vec<String>>,
    pub open_refused: std::sync::Mutex<Option<String>>,
    pub answer: std::sync::Mutex<photon_ui::platform::Picked>,
    pub asked: std::sync::Mutex<Vec<String>>,
    pub held: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    pub copied: std::sync::Mutex<Vec<(usize, usize)>>,
    pub refused: std::sync::Mutex<Option<String>>,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            answer: std::sync::Mutex::new(photon_ui::platform::Picked::Cancelled),
            asked: Default::default(),
            held: Default::default(),
            copied: Default::default(),
            refused: Default::default(),
            opened: Default::default(),
            open_refused: Default::default(),
        }
    }
}

impl photon_ui::platform::Platform for Picker {
    fn reveal(&self, path: &Path) -> Result<(), String> {
        if let Some(refused) = self.open_refused.lock().unwrap().clone() {
            return Err(refused);
        }
        (self.opened.lock().unwrap()).push(format!("reveal {}", path.display()));
        Ok(())
    }

    fn open(&self, target: &std::ffi::OsStr) -> Result<(), String> {
        (self.opened.lock().unwrap()).push(format!("open {}", target.to_string_lossy()));
        Ok(())
    }

    fn copy_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        assert_eq!(rgba.len(), width * height * 4, "a picture of its own size");
        if let Some(refused) = self.refused.lock().unwrap().clone() {
            return Err(refused);
        }
        self.copied.lock().unwrap().push((width, height));
        Ok(())
    }

    fn pick_folder(&self, title: &str) -> photon_ui::platform::Picked {
        self.asked.lock().unwrap().push(title.to_owned());
        let held = self.held.lock().unwrap().take();
        if let Some(held) = held {
            let _ = held.recv();
        }
        self.answer.lock().unwrap().clone()
    }
}

pub fn no_videos() -> std::sync::Arc<dyn photon_ui::video::Videos> {
    use photon_ui::video::{Failure, Picture, Stream};
    struct Nothing;
    impl photon_ui::video::Videos for Nothing {
        fn can_play(&self) -> bool {
            false
        }
        fn poster(&self, _: &Path, _: f64) -> Result<Picture, Failure> {
            Err(Failure::Unsupported("plays nothing".into()))
        }
        fn open(&self, _: &Path) -> Result<Box<dyn Stream>, Failure> {
            Err(Failure::Unsupported("plays nothing".into()))
        }
    }
    std::sync::Arc::new(Nothing)
}
