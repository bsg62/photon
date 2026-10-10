//! A window's event loop, without the window, for tests of the whole application.
//!
//! A frame is drawn when one was asked for and not otherwise, no sooner than a refresh
//! after the last, on a clock that jumps over the time in which nothing was. egui tells
//! whatever runs it of every request through one callback - it is how eframe learns of
//! them - so the frames drawn here are the ones a window would draw. Stepping the harness
//! blindly draws a frame at every step, which says nothing about a still window, and hid
//! three things the gate's first count of idle frames got wrong (CLAUDE.md, "A test that
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
    /// How many frames have been drawn.
    pub frames: usize,
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
            frames: 0,
        }
    }

    fn due(&self) -> Option<f64> {
        *self.due.lock().unwrap()
    }

    /// Draws one frame at `at`. Whatever the user was made to do since the last one
    /// (`Harness::event`, a node's `click`) is in it.
    pub fn frame(&mut self, harness: &mut Harness<'_>, at: f64) {
        *self.due.lock().unwrap() = None;
        self.now = at;
        *self.clock.lock().unwrap() = at;
        harness.input_mut().time = Some(at);
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
    let dirs = library.dirs();
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(move |cc| App::new(cc, dirs, None).unwrap())
}

/// Clicks the sidebar row called `label`, and draws the frame the click is in.
pub fn click(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// Launches over `library` and waits until its grid is there.
pub fn opened<'a>(library: &fixture::Fixture, photos: usize) -> (Harness<'a>, Driver) {
    let mut harness = launch(library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == photos && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver)
}

/// The ids of `library`'s photos, in grid order: the newest folder first.
pub fn photo_ids(library: &fixture::Fixture) -> Vec<i64> {
    let lib = Library::open(&library.dirs().db_path).unwrap();
    (lib.grid_entries().unwrap().iter())
        .map(|entry| entry.id)
        .collect()
}
