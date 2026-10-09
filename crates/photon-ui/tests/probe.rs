//! The gate's fixture and its scroll programme, end to end and without a window: a library
//! built to be measured, the application over it, and the programme run to its report on a
//! clock that is the harness's own.
//!
//! What this cannot say is how long a frame takes on a screen. That is the gate itself
//! (`cargo run -p xtask -- grid-gate`), which needs a compositor.

use eframe::egui::{self, pos2, vec2};
use photon_core::{library::Library, media::ThumbState};
use photon_ui::{app::App, fixture};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The harness's screen: sixty frames a second.
const REFRESH: f64 = 1.0 / 60.0;
/// How long no other thread must have asked for a frame before they are taken to be done.
const QUIET: Duration = Duration::from_millis(250);

/// Three small JPEGs to make thumbnails from.
fn sources(dir: &Path) -> Vec<PathBuf> {
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

#[test]
fn a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = fixture::build(&dir.path().join("fixture"), 700, &sources(dir.path())).unwrap();
    let dirs = fixture.dirs();

    let lib = Library::open(&dirs.db_path).unwrap();
    let entries = lib.grid_entries().unwrap();
    assert_eq!(entries.len(), 700);
    // Three folders: two of 300 and one of 100.
    assert_eq!(
        lib.folders().unwrap().len(),
        4,
        "the root and three under it"
    );
    let cache = photon_core::thumbs::ThumbCache::new(&dirs.cache_dir);
    for entry in &entries {
        let item = lib.item(entry.id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Ready);
        let thumbnail = cache.path_for(entry.thumb_key, photon_core::thumbs::ThumbSize::Grid);
        assert!(thumbnail.is_file(), "{}", thumbnail.display());
        // Both sizes, which is what the engine asks before it leaves a photo alone: with
        // the grid's alone it tried to make the other for every photo that came to rest
        // in view, from a file that is not there - four hundred failed jobs in a run of
        // the gate, in each of the two applications.
        assert!(cache.is_complete(entry.thumb_key), "photo {}", entry.id);
    }
    // The drive is unplugged: its folder is watched and is not there.
    let watched = lib.watched_folders().unwrap();
    assert_eq!(watched.len(), 1);
    assert!(!Path::new(&watched[0].path).exists());
    // And recorded as gone, as the engine's first scan would record it: left for that scan
    // to find, the first application to open the fixture rebuilds its grid for the change
    // and the second does not, and the two are compared.
    assert!(!watched[0].online);
    // And the cache needs no walk.
    assert_eq!(
        lib.thumb_gc_due(photon_core::now_ms(), Duration::from_secs(3600))
            .unwrap(),
        None
    );

    // It says of itself that it was finished, and by which builder: the last thing
    // written. A build that was interrupted has a library and no such word, and one from
    // an older builder is not the library this one makes.
    let out = dir.path().join("fixture");
    assert_eq!(fixture::complete(&out), Some(700));
    let marker = out.join(fixture::MARKER);
    let said = std::fs::read_to_string(&marker).unwrap();
    let ours = format!("\"builder\":{}", fixture::BUILDER);
    assert!(said.contains(&ours), "{said}");
    std::fs::write(&marker, said.replace(&ours, "\"builder\":0")).unwrap();
    assert_eq!(fixture::complete(&out), None, "an older builder's");
    std::fs::remove_file(&marker).unwrap();
    assert_eq!(fixture::complete(&out), None, "an unfinished one");
    assert_eq!(fixture::complete(&dir.path().join("nowhere")), None);

    // A second build into the same place is refused before it writes anything, not
    // layered over the first: without the check it fails too, but half-way, on a folder
    // the library already watches, having made the folder again.
    let again = fixture::build(&dir.path().join("fixture"), 10, &sources(dir.path()));
    let refused = again.expect_err("a second build is refused").to_string();
    assert!(refused.contains("already holds a library"), "{refused}");
    assert!(!dir.path().join("fixture/unplugged-drive").exists());
}

#[test]
fn the_programme_runs_over_a_fixture_to_its_report() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = fixture::build(&dir.path().join("fixture"), 900, &sources(dir.path())).unwrap();
    let out = dir.path().join("report.json");
    let report = out.clone();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(|cc| {
            App::new(cc, fixture.dirs(), None)
                .unwrap()
                .with_probe(report, Some(0.0))
        });

    // A window's event loop, without the window. A frame is drawn when one was asked for
    // and not otherwise, no sooner than a refresh after the last, on a clock that jumps
    // over the time in which nothing was. egui tells whatever runs it of every request
    // through this callback - it is how eframe learns of them - so the frames drawn here
    // are the ones a window would draw: stepping the harness blindly draws three hundred
    // in the idle step, which says nothing about a still grid.
    let clock = Arc::new(Mutex::new(0.0_f64));
    let due = Arc::new(Mutex::new(Some(0.0_f64)));
    // When another thread last asked for a frame: the engine, a decoder, a task.
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
    let mut now = 0.0;
    let (mut unasked, mut nudged) = (false, false);
    // Since when the programme has been at its first long rest, until what the other
    // threads were still doing has come in.
    let mut draining: Option<Instant> = None;
    let mut drained = false;
    let deadline = Instant::now() + Duration::from_secs(120);
    while !out.exists() {
        assert!(
            Instant::now() < deadline,
            "the programme never wrote its report"
        );
        let Some(asked) = *due.lock().unwrap() else {
            // Nothing is asked for: the engine or a decoder has yet to answer.
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        // The programme's first long rest is where this clock first jumps, and a clock
        // that jumps leaves the other threads behind: a second of it passes in no time at
        // all, and the row of thumbnails still being decoded, or the engine's last word
        // on its launch, then arrives "seconds later", in the idle step, as a frame the
        // grid drew by itself. On a screen that second is a second and they are long
        // done. So before the first jump they are waited for, in real time and none of
        // this clock's: until no picture is on its way and no other thread has asked for
        // a frame for a while. One run in fifty failed on one core without it, and the
        // first on a macOS runner.
        if !drained && asked - now > 0.5 {
            let since = *draining.get_or_insert_with(Instant::now);
            let loading = harness.state().last_frame().is_some_and(|f| f.loading);
            let quiet = since.max(*stray.lock().unwrap()).elapsed() >= QUIET;
            if loading || !quiet {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            drained = true;
        }
        now = if asked - now > 3.0 && !unasked {
            // The only rest this long is the idle step's. One frame nobody asked for, in
            // the middle of it: what a grid that repaints by itself would draw.
            unasked = true;
            now + (asked - now) / 2.0
        } else if asked - now > 1.5 && unasked && !nudged {
            // And, later in the same rest, the mouse nudged: the frame that answers it
            // and whatever egui draws after it are the person's, not the grid's.
            nudged = true;
            harness.event(egui::Event::PointerMoved(pos2(300.0, 200.0)));
            now + 1.0
        } else {
            *due.lock().unwrap() = None;
            // While the others are waited for, what they ask for is drawn at once.
            let refresh = if draining.is_some() && !drained {
                0.001
            } else {
                REFRESH
            };
            asked.max(now + refresh)
        };
        *clock.lock().unwrap() = now;
        harness.input_mut().time = Some(now);
        harness.step();
    }

    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(json["app"], "native");
    assert_eq!(json["photos"], 900);
    assert_eq!(json["window"][0], 800.0);
    // Ten seconds of the harness's clock at sixty steps a second.
    let frames = json["steady"]["frames"].as_u64().unwrap();
    assert!((595..=605).contains(&frames), "{frames}");
    // The library is many screens of rows: each jump lands somewhere it has not been,
    // and it shows its pictures there, every one of them from the cache.
    for step in ["jump_end_ms", "jump_middle_ms", "sweep_settle_ms"] {
        assert!(json[step].is_number(), "{step}: {}", json[step]);
    }
    // The idle step: the one frame nobody asked for, and nothing else. Not what the last
    // scroll left to be drawn, not the second frame the toolkit draws after one asked for
    // at once, not the frame that ends the step, which comes a little early, and not
    // the frames a nudged mouse was answered with.
    assert!(unasked && nudged);
    assert_eq!(json["idle_frames"], 1);
    // Every tile was a picture: the fixture's thumbnails are where the application looks.
    assert_eq!(json["marked_tiles"], 0);
    // The programme moved the grid and left it where its last step ends: at the far end.
    let last = harness.state().last_frame().unwrap();
    assert!(last.position > 10_000.0, "the grid is at {}", last.position);
    // The app's library was the fixture's, and nothing in it was rendered or marked
    // missing by being opened.
    assert_eq!(harness.state().photos(), 900);
    drop(harness);
    let lib = Library::open(&fixture.dirs().db_path).unwrap();
    assert_eq!(lib.grid_entries().unwrap().len(), 900);
}

/// Runs the launch the gate throws away over `fixture`, and reads its report.
fn warm_up(fixture: &fixture::Fixture, dir: &Path) -> (serde_json::Value, f64) {
    let out = dir.join("report.json");
    let report = out.clone();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .with_step_dt(REFRESH as f32)
        .build_eframe(|cc| {
            App::new(cc, fixture.dirs(), None)
                .unwrap()
                .with_warm_up(report, Some(0.0))
        });
    let deadline = Instant::now() + Duration::from_secs(60);
    while !out.exists() {
        assert!(Instant::now() < deadline, "the warm-up never ended");
        harness.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    let position = harness.state().last_frame().unwrap().position;
    let json = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    (json, position)
}

// The launch the gate throws away before the one it measures.
#[test]
fn a_warm_up_writes_its_report_at_the_first_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = fixture::build(&dir.path().join("fixture"), 900, &sources(dir.path())).unwrap();
    let (json, position) = warm_up(&fixture, dir.path());
    assert_eq!(json["photos"], 900);
    assert!(json["launch_ms"].is_number());
    assert_eq!(json["steady"]["frames"], 0);
    assert_eq!(json["marked_tiles"], 0);
    // It never moved the grid.
    assert_eq!(position, 0.0);
}

// A tile whose picture cannot be had shows a mark and counts as shown, so a run whose
// thumbnails are not where the application looks "shows its pictures" in a frame. The
// report says how many tiles were marks, and the gate fails on any.
#[test]
fn a_library_without_its_thumbnails_is_reported_as_marks() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = fixture::build(&dir.path().join("fixture"), 900, &sources(dir.path())).unwrap();
    std::fs::remove_dir_all(&fixture.cache_dir).unwrap();
    let (json, _) = warm_up(&fixture, dir.path());
    let marks = json["marked_tiles"].as_u64().unwrap();
    assert!(marks >= 8, "{marks} tiles of a screen of them");
}

/// The value at `quantile` of `sorted`, by the nearest rank.
fn at(sorted: &[f64], quantile: f64) -> f64 {
    let rank = (quantile * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

// Not a test of anything: a measurement, of the one half of a frame that can be measured
// without a screen. It runs the gate's programme over the gate's own fixture on the
// harness's clock and times what the application does for each frame - its own pass, and,
// for every tenth frame, the pass with the frame drawn off screen and read back, which is
// more than a window ever does. It says nothing about the cadence a compositor allows;
// that is `cargo run -p xtask -- grid-gate`.
//
//   cargo run --release -p photon-ui --example fixture -- --out target/gate-fixture
//   PHOTON_GATE_FIXTURE=$PWD/target/gate-fixture \
//     cargo test --release -p photon-ui --test probe -- --ignored --nocapture
//
// The whole path: a test runs in its crate's directory, not where cargo was called.
#[test]
#[ignore = "a measurement over a fixture built beforehand; see the comment"]
fn the_work_of_a_frame_over_the_gate_fixture() {
    let Some(out) = std::env::var_os("PHOTON_GATE_FIXTURE") else {
        panic!("PHOTON_GATE_FIXTURE names no fixture");
    };
    let out = std::fs::canonicalize(out).unwrap();
    let fixture = fixture::Fixture::at(&out, 0);
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("report.json");
    let written = report.clone();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(2560.0, 1440.0))
        .with_pixels_per_point(1.0)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(|cc| {
            App::new(cc, fixture.dirs(), None)
                .unwrap()
                .with_probe(written, None)
        });

    let (mut passes, mut drawn) = (Vec::new(), Vec::new());
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut frame = 0usize;
    while !report.exists() {
        let start = Instant::now();
        harness.step();
        passes.push(start.elapsed().as_secs_f64() * 1000.0);
        if frame.is_multiple_of(10) {
            harness.render().unwrap();
            drawn.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        frame += 1;
        assert!(Instant::now() < deadline, "the programme never ended");
    }
    let photos = harness.state().photos();
    for (name, times) in [
        ("its own pass", &mut passes),
        ("drawn and read back", &mut drawn),
    ] {
        times.sort_by(f64::total_cmp);
        println!(
            "{photos} photos at 2560x1440, {name}: {} frames, median {:.2} ms, p95 {:.2}, p99 {:.2}, longest {:.2}",
            times.len(),
            at(times, 0.5),
            at(times, 0.95),
            at(times, 0.99),
            times[times.len() - 1]
        );
    }
    println!("{}", std::fs::read_to_string(&report).unwrap());
}
