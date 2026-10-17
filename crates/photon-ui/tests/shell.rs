//! The shell and the views, through the whole application: a library on disk, the engine,
//! and the frames a window would draw (`crate::common::Driver`).

use crate::common::{Driver, Harness, click, library, opened, opened_animated};
use eframe::egui::{self, Key, Modifiers, pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use photon_core::grid::GridView;
use photon_ui::{
    app::App,
    fixture::Fixture,
    grid::scroll::{Notch, wheel_notch},
    nav::Place,
    sidebar::rows::Counts,
    window_layout::{Layout, SIDEBAR_DEFAULT, SPLITTER},
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

#[test]
fn a_click_on_a_view_shows_it_and_the_row_follows_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 3);
    let (mut harness, mut driver) = opened(&library, 897);
    driver.until(&mut harness, "the counts read", |app| {
        *app.counts()
            == Counts {
                starred: 5,
                hidden: 3,
                ..Counts::default()
            }
    });
    assert_eq!(harness.state().photo_count().as_deref(), Some("897 photos"));
    // The rows a library has only while it holds something of the kind.
    assert!(harness.query_by_label("Hidden").is_some());
    assert!(harness.query_by_label("Videos").is_none());
    assert!(harness.query_by_label("Duplicates").is_none());

    click(&mut driver, &mut harness, "Starred");
    // Where the user is going, before the rebuild has landed - and the row says so in
    // the very next frame, which is not the one the grid arrives in.
    assert_eq!(harness.state().place(), Place::of(GridView::Starred));
    driver.act(&mut harness);
    let current = |harness: &Harness<'_>, label: &str| {
        let row = harness.get_by_label(label);
        row.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(current(&harness, "Starred") && !current(&harness, "All photos"));
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    assert_eq!(harness.state().photos(), 5);
    assert_eq!(harness.state().photo_count().as_deref(), Some("5 photos"));

    click(&mut driver, &mut harness, "Hidden");
    driver.until(&mut harness, "the hidden photos shown", |app| {
        *app.settled() == Place::of(GridView::Hidden)
    });
    assert_eq!(harness.state().photos(), 3);

    click(&mut driver, &mut harness, "All photos");
    driver.until(&mut harness, "every photo shown again", |app| {
        *app.settled() == Place::of(GridView::All)
    });
    assert_eq!(harness.state().photos(), 897);
    assert_eq!(harness.state().notice(), None);
}

// Several clicks before the first has landed: the last is where the user ends, and the
// row follows it from the click. That each step is made, and in the order asked, is what
// `tasks::Queue`'s own tests hold; with views alone, which each replace the last, a queue
// that kept only the newest would end in the same place.
#[test]
fn the_last_of_several_clicks_is_where_the_user_ends() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    for _ in 0..4 {
        click(&mut driver, &mut harness, "Starred");
        click(&mut driver, &mut harness, "Recent");
        click(&mut driver, &mut harness, "All photos");
        click(&mut driver, &mut harness, "Starred");
        assert_eq!(harness.state().place(), Place::of(GridView::Starred));
        driver.until(&mut harness, "the last click shown", |app| {
            *app.settled() == app.place()
        });
        driver.settle(&mut harness);
        assert_eq!(harness.state().settled(), &Place::of(GridView::Starred));
        assert_eq!(harness.state().photos(), 5);
        click(&mut driver, &mut harness, "All photos");
        driver.until(&mut harness, "back in all photos", |app| {
            *app.settled() == Place::of(GridView::All) && app.photos() == 900
        });
    }
}

// All photos most of all: the click means "back to where I was", and the user is there.
// Asked of the engine anyway it is a rebuild of the whole library for nothing.
#[test]
fn a_click_on_the_row_that_is_shown_asks_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 2, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    driver.settle(&mut harness);
    let version = harness.state().version();
    click(&mut driver, &mut harness, "All photos");
    click(&mut driver, &mut harness, "All photos");
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().version(), version, "no grid was built");

    // The same of a row clicked twice before its view has landed: one rebuild, not two.
    click(&mut driver, &mut harness, "Starred");
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(harness.state().version(), version + 1);
}

/// What the application says of the grid it has just drawn.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    photos: usize,
    settled: Place,
    notice: Option<String>,
    position: f64,
}

/// Draws frames until `done`, and answers every different thing the application said
/// of the grid on the way.
fn watched(
    driver: &mut Driver,
    harness: &mut Harness<'_>,
    what: &str,
    done: impl Fn(&photon_ui::app::App) -> bool,
) -> Vec<Shown> {
    let seen = std::cell::RefCell::new(Vec::<Shown>::new());
    driver.until(harness, what, |app| {
        let now = Shown {
            photos: app.photos(),
            settled: app.settled().clone(),
            notice: app.notice(),
            position: app.last_frame().map_or(0.0, |frame| frame.position),
        };
        let mut seen = seen.borrow_mut();
        if seen.last() != Some(&now) {
            seen.push(now);
        }
        done(app)
    });
    seen.into_inner()
}

// Two steps on their way: the first's grid is published while the second's is being
// built, and the engine's own view is by then the second's. Read from the engine at that
// moment, "the view on screen" was the one still to come: the starred photos were drawn
// as All photos, where the grid had been - not from their top.
#[test]
fn a_grid_is_shown_as_the_view_it_was_built_for_with_another_step_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12_000, 6_000, 0);
    let (mut harness, mut driver) = opened(&library, 12_000);
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the grid scrolled", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.position > 1000.0)
    });
    driver.settle(&mut harness);

    for round in 0..6 {
        let version = harness.state().version();
        click(&mut driver, &mut harness, "Starred");
        click(&mut driver, &mut harness, "All photos");
        let seen = watched(&mut driver, &mut harness, "both steps landed", |app| {
            app.version() >= version + 2 && *app.settled() == Place::of(GridView::All)
        });
        let wrong: Vec<&Shown> = seen
            .iter()
            .filter(|shown| {
                shown.photos == 6_000
                    && (shown.settled != Place::of(GridView::Starred) || shown.position != 0.0)
            })
            .collect();
        assert!(wrong.is_empty(), "round {round}: {wrong:?}\nof {seen:?}");
        // Back down All photos for the next round: the pointer is on the row it clicked.
        harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -4000.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        driver.act(&mut harness);
        driver.until(&mut harness, "the grid scrolled again", |app| {
            app.last_frame()
                .is_some_and(|frame| frame.position > 1000.0)
        });
        driver.settle(&mut harness);
    }
}

#[test]
fn an_empty_view_says_what_it_is_empty_of() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    assert_eq!(harness.state().notice(), None);
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred view shown", |app| {
        *app.settled() == Place::of(GridView::Starred)
    });
    assert_eq!(harness.state().photos(), 0);
    assert_eq!(
        harness.state().notice().as_deref(),
        Some("No starred photos. Star one in the viewer, or in Picasa.")
    );
    assert_eq!(harness.state().photo_count().as_deref(), Some("0 photos"));
}

#[test]
fn on_this_day_is_a_search_for_todays_date() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened(&library, 300);
    click(&mut driver, &mut harness, "On this day");
    driver.until(&mut harness, "the search shown", |app| {
        app.settled().view == GridView::Search
    });
    let query = harness.state().settled().arg.clone();
    assert!(
        query.starts_with("on:") && query.len() == "on:MM-DD".len(),
        "{query}"
    );
}

// The photos are another list: a place in the old one names nothing in this one.
#[test]
fn another_view_is_shown_from_its_top() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    // The wheel, over the grid.
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the grid scrolled", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.position > 1000.0)
    });

    click(&mut driver, &mut harness, "Recent");
    driver.until(&mut harness, "the recent photos shown", |app| {
        *app.settled() == Place::of(GridView::Recent)
    });
    driver.until(&mut harness, "the grid drawn again", |app| {
        app.last_frame().is_some_and(|frame| frame.position == 0.0)
    });
    assert_eq!(harness.state().photos(), 500);
}

/// Waits, in real time, until the layout's file holds what the application does: it is
/// written off the UI's thread.
fn stored(harness: &Harness<'_>, path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let wanted = harness.state().layout().written();
    while std::fs::read_to_string(path).ok().as_deref() != Some(wanted.as_str()) {
        assert!(Instant::now() < deadline, "the layout was never stored");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_sidebar_is_as_wide_at_the_next_launch_as_it_was_dragged() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let path = library.dirs().layout_path;
    {
        let (mut harness, mut driver) = opened(&library, 300);
        assert_eq!(*harness.state().layout(), Layout::default());
        // The splitter, dragged eighty points to the right.
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 300.0);
        harness.event(egui::Event::PointerMoved(hold));
        harness.event(egui::Event::PointerButton {
            pos: hold,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        let to = pos2(hold.x + 80.0, 300.0);
        harness.event(egui::Event::PointerMoved(to));
        driver.act(&mut harness);
        // Followed while it is held.
        assert_eq!(
            harness.state().layout().sidebar_width,
            SIDEBAR_DEFAULT + 80.0
        );
        harness.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        driver.act(&mut harness);
        driver.act(&mut harness);
        stored(&harness, &path);
    }
    let (harness, _) = opened(&library, 300);
    assert_eq!(
        harness.state().layout().sidebar_width,
        SIDEBAR_DEFAULT + 80.0
    );
    assert!(!harness.state().layout().sidebar_hidden);
}

#[test]
fn a_sidebar_hidden_is_hidden_at_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let path = library.dirs().layout_path;
    {
        let (mut harness, mut driver) = opened(&library, 300);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
        driver.act(&mut harness);
        assert!(harness.state().layout().sidebar_hidden);
        stored(&harness, &path);
    }
    let (harness, _) = opened(&library, 300);
    assert!(harness.state().layout().sidebar_hidden);
    // Hidden, its rows are not there to be clicked.
    assert!(harness.query_by_label("Starred").is_none());
    assert!(harness.query_by_label("Show sidebar").is_some());
}

// The counts are read once at launch and again whenever the library says it has changed:
// a photo hidden is one more in Hidden and one fewer in the grid.
#[test]
fn the_counts_follow_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 2);
    let (mut harness, mut driver) = opened(&library, 298);
    driver.until(&mut harness, "the counts read", |app| {
        app.counts().hidden == 2
    });
    let first = harness.state().last_frame().unwrap().on_screen[0];
    let engine = harness.state().engine().clone();
    photon_engine::commands::set_items_hidden(&engine, &[first], true).unwrap();
    driver.until(&mut harness, "the count moved", |app| {
        app.counts().hidden == 3 && app.photos() == 297
    });
}

// The shell around a still grid is as still as the grid: no frame in five seconds that
// nothing asked for. A bar that repainted by itself would be drawn sixty times a second
// for as long as the window is open.
#[test]
fn a_still_window_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 5, 3);
    let (mut harness, mut driver) = opened(&library, 897);
    click(&mut driver, &mut harness, "Starred");
    driver.until(&mut harness, "the starred photos shown", |app| {
        *app.settled() == Place::of(GridView::Starred) && app.counts().starred == 5
    });
    driver.settle(&mut harness);
    // What the click and the scroll back to the top left to be drawn, drawn.
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// A notch of the mouse wheel moves the grid as far as the web view of this system moved
// it: a hundred points on Windows, forty elsewhere. The first person to run the native
// grid on Windows found it very slow beside the Tauri photon, at egui's forty.
#[test]
fn a_notch_of_the_wheel_moves_the_grid_as_far_as_the_web_view_did() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened(&library, 900);
    driver.settle(&mut harness);
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, -3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let moved = harness.state().last_frame().unwrap().position;
    // What this system says: on Windows its own lines a notch, whatever the machine
    // has them set to, and a page of the grid where that is the setting.
    let notch = wheel_notch(cfg!(target_os = "windows"), photon_ui::wheel::lines()).points(IN_VIEW);
    assert!(
        (moved - 3.0 * notch).abs() < 1.0,
        "three notches moved {moved}"
    );
}

/// What is in view of the grid in the tests' window: its height less the two bars.
const IN_VIEW: f64 = 700.0 - 46.0 - 23.0;

/// A library of nine hundred photos in a window where `wheel` says what a notch moves.
fn wheeled<'a>(dir: &std::path::Path, wheel: fn() -> Notch) -> (Harness<'a>, Driver, Fixture) {
    let built = library(dir, 900, 0, 0);
    let dirs = built.dirs();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(1000.0, 700.0))
        .with_step_dt(crate::common::REFRESH as f32)
        .build_eframe(move |cc| {
            App::new(cc, dirs, None)
                .unwrap()
                .without_animations()
                .with_wheel(wheel)
        });
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == 900 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    driver.act(&mut harness);
    (harness, driver, built)
}

/// One notch of the wheel down, and where the grid has come to rest after it.
fn notch_down(driver: &mut Driver, harness: &mut Harness<'_>) -> f64 {
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(harness);
    driver.settle(harness);
    driver.frames_in(harness, 1.0);
    harness.state().last_frame().unwrap().position
}

// Windows' lines a notch are set in a window of the system's, and photon is not told when
// they change: it asks when it is made and again when it comes back to the front, which
// is where the user is coming from. Six lines are two hundred points, as in the web view.
#[test]
fn what_a_notch_moves_is_asked_again_when_the_window_comes_back_to_the_front() {
    static LINES: AtomicU32 = AtomicU32::new(3);
    fn said() -> Notch {
        wheel_notch(true, Some(LINES.load(Ordering::Relaxed)))
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = wheeled(dir.path(), said);
    assert!((notch_down(&mut driver, &mut harness) - 100.0).abs() < 1.0);
    // Changed, and not asked about yet: nothing says so.
    LINES.store(6, Ordering::Relaxed);
    assert!((notch_down(&mut driver, &mut harness) - 200.0).abs() < 1.0);
    harness.event(egui::Event::WindowFocused(true));
    driver.act(&mut harness);
    let moved = notch_down(&mut driver, &mut harness);
    assert!((moved - 400.0).abs() < 1.0, "{moved}");
    // Leaving the window asks nothing.
    LINES.store(1, Ordering::Relaxed);
    harness.event(egui::Event::WindowFocused(false));
    driver.act(&mut harness);
    let moved = notch_down(&mut driver, &mut harness);
    assert!((moved - 600.0).abs() < 1.0, "{moved}");
}

// "One screen at a time": a notch turns a page of the grid, all but forty points of what
// is in view, as the web view turns it.
#[test]
fn a_wheel_set_to_a_screen_at_a_time_turns_a_page_of_the_grid() {
    fn page() -> Notch {
        Notch::Page
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _library) = wheeled(dir.path(), page);
    let moved = notch_down(&mut driver, &mut harness);
    assert!((moved - (IN_VIEW - 40.0)).abs() < 1.0, "{moved}");
}

/// Where the content begins: what the sidebar and its splitter leave of the window.
fn content_left(harness: &Harness<'_>) -> f32 {
    harness
        .state()
        .content_rect()
        .expect("the shell drawn")
        .left()
}

/// Ctrl+B, in the frame drawn next.
fn toggle(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    driver.act(harness);
}

// The toggle slides the sidebar in over its time, the content's room changing with it in
// every frame - and once it has got there the window is still again.
#[test]
fn the_sidebar_slides_open_and_then_asks_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened_animated(&library, 300);
    driver.settle(&mut harness);
    let open = content_left(&harness);
    assert_eq!(open, SIDEBAR_DEFAULT + SPLITTER);
    toggle(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(content_left(&harness), 0.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 1.0), 0);

    // The key going down alone, in a frame of its own - `key_press` would draw a second
    // frame at the same moment for the key coming up. The slide is timed from the press,
    // not from the frame after it.
    harness.event(egui::Event::Key {
        key: Key::B,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    driver.act(&mut harness);
    let pressed = driver.now;
    driver.frame(&mut harness, pressed + 0.1);
    let part = content_left(&harness);
    assert!(
        0.0 < part && part < open,
        "{part} at a hundred milliseconds"
    );
    assert!(harness.state().sidebar_sliding(driver.now * 1000.0));
    driver.frame(&mut harness, pressed + 0.2);
    assert_eq!(content_left(&harness), open);
    assert!(!harness.state().sidebar_sliding(driver.now * 1000.0));
    // What the grid does after a change of its width - what is in view said, the
    // thumbnails of rows come into it - is the grid's, and over in a moment.
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);
}

// Sliding closed, the sidebar is drawn and is nobody's: the toggle has put it away, and
// a press on the part still drawn is no row's.
#[test]
fn a_closing_sidebar_takes_no_press() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 5, 0);
    let (mut harness, mut driver) = opened_animated(&library, 300);
    driver.seen(&mut harness, "Starred");
    driver.settle(&mut harness);
    let row = harness.get_by_label("Starred").rect();
    let at = pos2(row.left() + 20.0, row.center().y);
    let before = harness.state().place();
    toggle(&mut driver, &mut harness);
    let pressed = driver.now;
    driver.frame(&mut harness, pressed + 0.05);
    // Still drawn where the press goes.
    assert!(at.x < content_left(&harness), "{at:?}");
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.frame(&mut harness, pressed + 0.06);
    driver.frame(&mut harness, pressed + 0.07);
    assert_eq!(harness.state().place(), before, "no view was asked for");
}

// The shell is not drawn under the viewer: a slide begun at the press that opened it -
// Ctrl+B and Enter in one frame - asks for no frames there, and is over by the time the
// viewer closes, not begun then.
#[test]
fn a_slide_under_the_viewer_asks_for_no_frames_once_its_time_is_over() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 300, 0, 0);
    let (mut harness, mut driver) = opened_animated(&library, 300);
    driver.settle(&mut harness);
    let at = harness.state().tile_rect(0).unwrap().center();
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    let down = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    // Both in the frame's own input, so that one frame is drawn for the two.
    (harness.input_mut().events).extend([
        down(Key::B, Modifiers::COMMAND),
        down(Key::Enter, Modifiers::NONE),
    ]);
    driver.act(&mut harness);
    assert!(harness.state().layout().sidebar_hidden);
    assert!(harness.state().viewer().is_some());
    driver.frames_in(&mut harness, 0.2);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    driver.settle(&mut harness);
    assert_eq!(driver.frames_in(&mut harness, 5.0), 0);

    harness.event(down(Key::Escape, Modifiers::NONE));
    driver.act(&mut harness);
    assert!(harness.state().viewer().is_none());
    driver.act(&mut harness);
    assert_eq!(
        content_left(&harness),
        0.0,
        "the slide was over under the viewer"
    );
    assert!(!harness.state().sidebar_sliding(driver.now * 1000.0));
}

// The place in the grid is kept by its pin in every frame of a slide, as through a
// splitter dragged: the folder at the top stays at the top while the rows are built again
// at each width. Many folders, so that a place kept as a number would be another's.
#[test]
fn the_top_photo_stays_at_the_top_through_a_slide() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12_000, 0, 0);
    let (mut harness, mut driver) = opened_animated(&library, 12_000);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 40
    });
    driver.settle(&mut harness);
    // Far down, by the wheel over the grid.
    harness.event(egui::Event::PointerMoved(pos2(700.0, 400.0)));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -60_000.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    driver.act(&mut harness);
    driver.until(&mut harness, "the grid scrolled", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.position > 50_000.0)
    });
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    let top = |harness: &Harness<'_>| harness.state().last_frame().unwrap().top_folder;
    let folder = top(&harness);
    assert!(folder.is_some());
    let open = content_left(&harness);

    toggle(&mut driver, &mut harness);
    let pressed = driver.now;
    driver.frame(&mut harness, pressed + 0.04);
    driver.frame(&mut harness, pressed + 0.08);
    let part = content_left(&harness);
    assert!(0.0 < part && part < open, "{part}: sliding");
    assert_eq!(top(&harness), folder);
    driver.frame(&mut harness, pressed + 0.2);
    assert_eq!(content_left(&harness), 0.0);
    assert_eq!(top(&harness), folder);
}

// "Rename in photon…" on a header shows a hidden sidebar at once, not by a slide: the
// name field is to take the keyboard in the frame it opens.
#[test]
fn rename_in_photon_shows_the_sidebar_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 900, 0, 0);
    let (mut harness, mut driver) = opened_animated(&library, 900);
    driver.until(&mut harness, "the folders read", |app| {
        app.folders().len() >= 3
    });
    driver.settle(&mut harness);
    let open = content_left(&harness);
    toggle(&mut driver, &mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 1.0);
    assert_eq!(content_left(&harness), 0.0);

    // The other button on the first folder's header, over its first photo.
    let tile = harness.state().tile_rect(0).unwrap();
    let at = tile.center_top() - vec2(0.0, 14.0);
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().menu_open());
    harness.get_by_label("Rename in photon…").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(content_left(&harness), open);
    harness.event(egui::Event::Text("x".to_owned()));
    driver.act(&mut harness);
    assert!(
        matches!(harness.state().naming(), Some((_, text, false)) if text.ends_with('x')),
        "{:?}",
        harness.state().naming()
    );
}
