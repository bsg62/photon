//! The scroll programme the gate measures, and what it writes down.
//!
//! The same programme is run by the Svelte grid (a patch kept beside the gate's harness)
//! and by this one, fullscreen on the same monitor, over the same library; the two reports
//! are laid side by side. A frame here is a call of `Probe::frame`, which the application
//! makes once for every frame it draws, and the intervals between those calls are the
//! cadence the compositor allowed: with vertical sync on, a frame that took too long shows
//! up as an interval of two refreshes.
//!
//! Every move is a place worked out from the time, never a distance per frame, so the grid
//! covers the same ground at 60Hz and at 144Hz.
//!
//! No egui here: the programme is a state machine over what a frame came to, driven in
//! tests by a clock that is a number.

use serde::Serialize;

/// The steady scroll: a wheel held, about two screens a second.
pub const STEADY_PX_PER_S: f64 = 3_000.0;
pub const STEADY_SECS: f64 = 10.0;
/// The fast scroll: a flick. Under a viewport a frame at 60Hz, so still a scroll and not a
/// jump (`motion::Motion`).
pub const FAST_PX_PER_S: f64 = 30_000.0;
pub const FAST_SECS: f64 = 5.0;
/// The sweep: top to bottom by position, as a scrollbar drag is. In a large library every
/// frame of it is a jump.
pub const SWEEP_SECS: f64 = 5.0;
pub const IDLE_SECS: f64 = 5.0;
/// The rest between the last sweep showing its pictures and the idle step. A scroll has
/// business after its last move, none of which is a still grid repainting: the frame in
/// which it counts as over and the one that tells the engine what is on screen
/// (`motion::SCROLL_SETTLE_MS`, `visible::VISIBLE_DEBOUNCE_MS`), and the second frame the
/// toolkit draws after every frame asked for at once.
pub const STILL_SECS: f64 = 1.0;
/// How long before the end of a rest a frame is taken to be the one that ends it. A frame
/// asked for at a time is drawn up to a frame before it, so as not to miss the refresh it
/// is meant for: a frame of a 30Hz screen, and some.
pub const EARLY_MS: f64 = 50.0;
/// How long the grid is left alone after it first shows its pictures, before the first
/// measured step: launch has work of its own that is not a scroll's.
pub const CALM_SECS: f64 = 1.0;
/// How long a step waits for every tile in view to have its picture before it gives up
/// and says so. A grid that never settles must not hang the gate.
pub const SETTLE_TIMEOUT_SECS: f64 = 30.0;

/// What a frame came to, as the programme needs to know it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Facts {
    /// The frame's time, in milliseconds from any fixed start.
    pub now_ms: f64,
    /// The furthest the grid scrolls.
    pub max: f64,
    /// Whether every tile in view has its picture, or the mark that it will not.
    pub settled: bool,
    pub photos: usize,
    /// Whether something outside the grid asked for this frame: the engine reporting, in
    /// this frame or the one before it (the toolkit's second frame).
    pub outside: bool,
}

/// Which frames something outside the grid asked for, from which frames the engine
/// reported in.
#[derive(Debug, Default)]
pub struct Outside {
    before: bool,
}

impl Outside {
    /// One frame, and whether the engine reported in it.
    pub fn frame(&mut self, reported: bool) -> bool {
        let outside = reported || self.before;
        self.before = reported;
        outside
    }
}

/// What the programme asks of the grid before the next frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    /// Nothing, and no frame either unless something else asks for one: the idle step.
    Rest {
        for_ms: f64,
    },
    /// Nothing, but another frame at once.
    Wait,
    /// Be at this position, and another frame at once.
    To(f64),
    Done,
}

/// The intervals between a step's frames, in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Cadence {
    pub frames: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub longest_ms: f64,
}

impl Cadence {
    /// The value at `quantile` (0 to 1) of `sorted`, by the nearest rank: the smallest
    /// value that at least that share of the intervals are at or below.
    fn at(sorted: &[f64], quantile: f64) -> f64 {
        let rank = (quantile * sorted.len() as f64).ceil() as usize;
        sorted[rank.clamp(1, sorted.len()) - 1]
    }

    pub fn of(intervals: &[f64]) -> Self {
        if intervals.is_empty() {
            return Self::default();
        }
        let mut sorted = intervals.to_vec();
        sorted.sort_by(f64::total_cmp);
        Self {
            frames: sorted.len(),
            median_ms: Self::at(&sorted, 0.5),
            p95_ms: Self::at(&sorted, 0.95),
            p99_ms: Self::at(&sorted, 0.99),
            longest_ms: sorted[sorted.len() - 1],
        }
    }
}

/// What the programme measured. Written as JSON; the Svelte probe writes the same shape.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub photos: usize,
    /// From the process being started (the harness says when) to the first frame in which
    /// every tile in view has its picture. `None` when nobody said when it was started.
    pub launch_ms: Option<f64>,
    pub steady: Cadence,
    pub fast: Cadence,
    /// From a jump to the end of the library to every tile there having its picture;
    /// `None` when that did not happen within `SETTLE_TIMEOUT_SECS`.
    pub jump_end_ms: Option<f64>,
    pub jump_middle_ms: Option<f64>,
    pub sweep: Cadence,
    /// From the sweep's last frame to every tile having its picture.
    pub sweep_settle_ms: Option<f64>,
    /// Frames the grid drew by itself in `IDLE_SECS` of nothing happening. A still grid
    /// draws none. Not counted: the frame that ends the step, and a frame something
    /// outside the grid asked for (`Facts::outside`).
    pub idle_frames: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Until the grid has photos and shows them.
    Launch,
    Calm,
    Steady,
    Fast,
    JumpEnd,
    JumpMiddle,
    /// Back at the top, until it shows its pictures, so the sweep starts from rest.
    SweepReady,
    Sweep,
    SweepSettle,
    /// What the sweep left behind, left to end.
    Still,
    Idle,
    Done,
}

pub struct Probe {
    step: Step,
    /// When the step began, and where the grid was then.
    since_ms: f64,
    from: f64,
    /// The frame before this one, for the interval; `None` on a step's first frame.
    last_ms: Option<f64>,
    intervals: Vec<f64>,
    /// The frames of the idle step that were the grid's own.
    idle_frames: usize,
    /// Where the programme last put the grid.
    position: f64,
    /// The epoch time the harness says the process was started at, in milliseconds.
    started_epoch_ms: Option<f64>,
    report: Report,
}

impl Probe {
    pub fn new(started_epoch_ms: Option<f64>) -> Self {
        Self {
            step: Step::Launch,
            since_ms: 0.0,
            from: 0.0,
            last_ms: None,
            intervals: Vec::new(),
            idle_frames: 0,
            position: 0.0,
            started_epoch_ms,
            report: Report::default(),
        }
    }

    pub fn report(&self) -> &Report {
        &self.report
    }

    pub fn done(&self) -> bool {
        self.step == Step::Done
    }

    fn begin(&mut self, step: Step, now_ms: f64) {
        self.step = step;
        self.since_ms = now_ms;
        self.from = self.position;
        self.last_ms = None;
        self.intervals.clear();
    }

    fn to(&mut self, position: f64) -> Move {
        self.position = position;
        Move::To(position)
    }

    /// A step that waits for the grid to show its pictures: how long it has waited once it
    /// does, `Some(None)` once it has waited too long, `None` while it still waits.
    fn settled_after(&self, facts: Facts) -> Option<Option<f64>> {
        let waited = facts.now_ms - self.since_ms;
        if facts.settled {
            Some(Some(waited))
        } else if waited >= SETTLE_TIMEOUT_SECS * 1000.0 {
            Some(None)
        } else {
            None
        }
    }

    /// One frame: what it came to, and the wall clock's epoch time in milliseconds, which
    /// only the launch reads. Answers what the grid should do before the next.
    pub fn frame(&mut self, facts: Facts, epoch_ms: f64) -> Move {
        let now = facts.now_ms;
        if let Some(last) = self.last_ms {
            self.intervals.push(now - last);
        }
        self.last_ms = Some(now);
        let elapsed = now - self.since_ms;

        match self.step {
            Step::Launch => {
                if facts.photos > 0 && facts.settled {
                    self.report.photos = facts.photos;
                    self.report.launch_ms = self.started_epoch_ms.map(|start| epoch_ms - start);
                    self.begin(Step::Calm, now);
                }
                Move::Wait
            }
            Step::Calm => {
                if elapsed >= CALM_SECS * 1000.0 {
                    self.begin(Step::Steady, now);
                }
                Move::Wait
            }
            Step::Steady => {
                if elapsed >= STEADY_SECS * 1000.0 {
                    self.report.steady = Cadence::of(&self.intervals);
                    self.begin(Step::Fast, now);
                    return Move::Wait;
                }
                self.to((self.from + STEADY_PX_PER_S * elapsed / 1000.0).min(facts.max))
            }
            Step::Fast => {
                if elapsed >= FAST_SECS * 1000.0 {
                    self.report.fast = Cadence::of(&self.intervals);
                    self.begin(Step::JumpEnd, now);
                    return self.to(facts.max);
                }
                self.to((self.from + FAST_PX_PER_S * elapsed / 1000.0).min(facts.max))
            }
            Step::JumpEnd => match self.settled_after(facts) {
                // The frame that made the jump cannot be the one that shows its pictures.
                Some(waited) if elapsed > 0.0 => {
                    self.report.jump_end_ms = waited;
                    self.begin(Step::JumpMiddle, now);
                    self.to(facts.max / 2.0)
                }
                _ => Move::Wait,
            },
            Step::JumpMiddle => match self.settled_after(facts) {
                Some(waited) if elapsed > 0.0 => {
                    self.report.jump_middle_ms = waited;
                    self.begin(Step::SweepReady, now);
                    self.to(0.0)
                }
                _ => Move::Wait,
            },
            Step::SweepReady => match self.settled_after(facts) {
                Some(_) if elapsed > 0.0 => {
                    self.begin(Step::Sweep, now);
                    Move::Wait
                }
                _ => Move::Wait,
            },
            Step::Sweep => {
                if elapsed >= SWEEP_SECS * 1000.0 {
                    self.report.sweep = Cadence::of(&self.intervals);
                    self.begin(Step::SweepSettle, now);
                    return self.to(facts.max);
                }
                self.to(facts.max * elapsed / (SWEEP_SECS * 1000.0))
            }
            Step::SweepSettle => match self.settled_after(facts) {
                Some(waited) if elapsed > 0.0 => {
                    self.report.sweep_settle_ms = waited;
                    self.begin(Step::Still, now);
                    Move::Rest {
                        for_ms: STILL_SECS * 1000.0,
                    }
                }
                _ => Move::Wait,
            },
            Step::Still => {
                let left = STILL_SECS * 1000.0 - elapsed;
                if left <= 0.0 {
                    self.begin(Step::Idle, now);
                    return Move::Rest {
                        for_ms: IDLE_SECS * 1000.0,
                    };
                }
                Move::Rest { for_ms: left }
            }
            Step::Idle => {
                let left = IDLE_SECS * 1000.0 - elapsed;
                if left <= 0.0 {
                    self.report.idle_frames = self.idle_frames;
                    self.step = Step::Done;
                    return Move::Done;
                }
                // A frame of the grid's own: not the one that ends the rest, come early,
                // and not one something else asked for.
                if left > EARLY_MS && !facts.outside {
                    self.idle_frames += 1;
                }
                Move::Rest { for_ms: left }
            }
            Step::Done => Move::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid that follows the programme: it is where it was last told to be, and shows
    /// its pictures once `settle_frames` frames have passed since it last moved - on the
    /// frame after those.
    struct Grid {
        probe: Probe,
        now_ms: f64,
        max: f64,
        position: f64,
        still_for: usize,
        settle_frames: usize,
        /// Whether the next frame is one something outside the grid asked for.
        outside: bool,
        /// Every position the grid was told to be at, with the time.
        moves: Vec<(f64, f64)>,
    }

    impl Grid {
        fn new(max: f64, settle_frames: usize) -> Self {
            Self {
                probe: Probe::new(Some(1_000.0)),
                now_ms: 0.0,
                max,
                position: 0.0,
                still_for: 0,
                settle_frames,
                outside: false,
                moves: Vec::new(),
            }
        }

        /// One frame `dt_ms` after the last.
        fn frame(&mut self, dt_ms: f64) -> Move {
            self.now_ms += dt_ms;
            let facts = Facts {
                now_ms: self.now_ms,
                max: self.max,
                settled: self.still_for >= self.settle_frames,
                photos: 300_000,
                outside: std::mem::take(&mut self.outside),
            };
            let asked = self.probe.frame(facts, 1_000.0 + self.now_ms);
            if let Move::To(position) = asked {
                if position != self.position {
                    self.still_for = 0;
                }
                self.position = position;
                self.moves.push((self.now_ms, position));
            } else {
                self.still_for += 1;
            }
            asked
        }

        /// Runs the whole programme at `dt_ms` a frame; an idle rest is slept through in
        /// one frame, as a still grid would.
        fn run(&mut self, dt_ms: f64) -> Report {
            for _ in 0..1_000_000 {
                match self.frame(dt_ms) {
                    Move::Done => return self.probe.report().clone(),
                    Move::Rest { for_ms } => self.now_ms += (for_ms - dt_ms).max(0.0),
                    Move::Wait | Move::To(_) => {}
                }
            }
            panic!("the programme never ended");
        }
    }

    #[test]
    fn the_cadence_of_no_frames_is_nothing_and_of_some_is_their_quantiles() {
        assert_eq!(Cadence::of(&[]), Cadence::default());
        let intervals: Vec<f64> = (1..=100).map(f64::from).collect();
        let cadence = Cadence::of(&intervals);
        assert_eq!(
            (
                cadence.frames,
                cadence.median_ms,
                cadence.p95_ms,
                cadence.p99_ms,
                cadence.longest_ms
            ),
            (100, 50.0, 95.0, 99.0, 100.0)
        );
        // One frame is every quantile of itself.
        assert_eq!(Cadence::of(&[16.0]).p99_ms, 16.0);
    }

    #[test]
    fn the_programme_runs_its_steps_and_ends() {
        let mut grid = Grid::new(4_000_000.0, 3);
        let report = grid.run(1000.0 / 60.0);
        assert_eq!(report.photos, 300_000);
        // Ten seconds and five seconds of frames a sixtieth of a second apart.
        assert!(
            (598..=601).contains(&report.steady.frames),
            "{}",
            report.steady.frames
        );
        assert!(
            (298..=301).contains(&report.fast.frames),
            "{}",
            report.fast.frames
        );
        assert!(
            (298..=301).contains(&report.sweep.frames),
            "{}",
            report.sweep.frames
        );
        assert!((report.steady.median_ms - 16.67).abs() < 0.01);
        // The grid shows its pictures on the fourth frame after it stops.
        for settle in [
            report.jump_end_ms,
            report.jump_middle_ms,
            report.sweep_settle_ms,
        ] {
            let settle = settle.expect("it settled");
            assert!((62.0..71.0).contains(&settle), "{settle}");
        }
        assert_eq!(report.idle_frames, 0);
        assert!(grid.probe.done());
    }

    // The ground covered is the time's, not the frame count's: the same at 60Hz and 144Hz.
    #[test]
    fn a_scroll_covers_the_same_ground_at_any_frame_rate() {
        let furthest_before_the_jump = |dt_ms: f64| {
            let mut grid = Grid::new(4_000_000.0, 3);
            grid.run(dt_ms);
            // The last position of the fast scroll: the move before the jump to the end.
            let jump = grid
                .moves
                .iter()
                .position(|(_, p)| *p == 4_000_000.0)
                .unwrap();
            grid.moves[jump - 1].1
        };
        let expected = STEADY_PX_PER_S * STEADY_SECS + FAST_PX_PER_S * FAST_SECS;
        for dt in [1000.0 / 60.0, 1000.0 / 144.0, 1000.0 / 30.0] {
            let reached = furthest_before_the_jump(dt);
            // Within a frame's travel of the full distance.
            assert!(
                (expected - reached).abs()
                    <= FAST_PX_PER_S * dt / 1000.0 + STEADY_PX_PER_S * dt / 1000.0,
                "{reached} at {dt}ms a frame"
            );
        }
    }

    #[test]
    fn the_launch_is_from_the_time_the_harness_gives_to_the_first_settled_frame() {
        let mut grid = Grid::new(1000.0, 5);
        let report = grid.run(10.0);
        // Settled on the sixth frame, 60ms in; the harness started the process at 1,000
        // and the wall clock then read 1,060.
        assert_eq!(report.launch_ms, Some(60.0));

        let mut unknown = Grid::new(1000.0, 5);
        unknown.probe = Probe::new(None);
        assert_eq!(unknown.run(10.0).launch_ms, None);
    }

    #[test]
    fn a_library_too_short_to_scroll_still_ends() {
        let mut grid = Grid::new(0.0, 1);
        let report = grid.run(1000.0 / 60.0);
        assert!(grid.moves.iter().all(|(_, position)| *position == 0.0));
        assert!(report.jump_end_ms.is_some());
    }

    // A grid that never shows its pictures - thumbnails that never arrive - must not hang
    // the gate: each waiting step gives up and says so.
    #[test]
    fn a_grid_that_never_settles_after_a_jump_is_given_up_on() {
        let mut grid = Grid::new(4_000_000.0, 3);
        // It shows its pictures at launch and at rest, but never after the first jump.
        loop {
            let asked = grid.frame(100.0);
            if asked == Move::To(4_000_000.0) {
                break;
            }
        }
        grid.settle_frames = usize::MAX;
        let report = grid.run(100.0);
        assert_eq!(report.jump_end_ms, None);
        assert_eq!(report.jump_middle_ms, None);
        assert_eq!(report.sweep_settle_ms, None);
        assert!(grid.probe.done());
    }

    impl Grid {
        /// Runs up to the rest after the last sweep, and through it: the next frame is the
        /// first of the idle step. Answers the rest the idle step began with.
        fn run_to_the_idle_step(&mut self) -> Move {
            loop {
                if let Move::Rest { for_ms } = self.frame(1000.0 / 60.0) {
                    assert_eq!(for_ms, STILL_SECS * 1000.0);
                    break;
                }
            }
            self.frame(STILL_SECS * 1000.0)
        }
    }

    #[test]
    fn frames_drawn_while_idle_are_counted() {
        let mut grid = Grid::new(4_000_000.0, 1);
        assert_eq!(
            grid.run_to_the_idle_step(),
            Move::Rest {
                for_ms: IDLE_SECS * 1000.0
            }
        );
        // Four frames nobody asked for, each answered with what is left of the rest, then
        // the one that ends the step.
        for left in [4000.0, 3000.0, 2000.0, 1000.0] {
            assert_eq!(grid.frame(1000.0), Move::Rest { for_ms: left });
        }
        assert_eq!(grid.frame(1000.0), Move::Done);
        assert_eq!(grid.probe.report().idle_frames, 4);
    }

    // A scroll has business of its own after its last move - the frame in which it counts
    // as over, the one that tells the engine what is on screen, and the second frame a
    // toolkit draws after any frame asked for at once. None of it is a still grid
    // repainting, and all of it is over within the rest before the idle step.
    #[test]
    fn what_a_scroll_leaves_behind_is_not_counted_as_idle() {
        let mut grid = Grid::new(4_000_000.0, 1);
        loop {
            if let Move::Rest { .. } = grid.frame(1000.0 / 60.0) {
                break;
            }
        }
        // Three frames in the second after the sweep settled.
        for (dt, left) in [(16.0, 984.0), (134.0, 850.0), (150.0, 700.0)] {
            assert_eq!(grid.frame(dt), Move::Rest { for_ms: left });
        }
        // The rest ends, the idle step begins and nothing is drawn in it.
        assert_eq!(
            grid.frame(700.0),
            Move::Rest {
                for_ms: IDLE_SECS * 1000.0
            }
        );
        assert_eq!(grid.frame(IDLE_SECS * 1000.0), Move::Done);
        assert_eq!(grid.probe.report().idle_frames, 0);
    }

    // A frame asked for at a time is drawn up to a frame before that time, so the vertical
    // sync it is meant for is not missed. That frame is the programme's own, not the
    // grid's: counted, every run of a perfectly still grid would report one.
    #[test]
    fn the_frame_that_ends_a_rest_may_come_early() {
        let mut grid = Grid::new(4_000_000.0, 1);
        grid.run_to_the_idle_step();
        let early = IDLE_SECS * 1000.0 - 16.0;
        assert_eq!(grid.frame(early), Move::Rest { for_ms: 16.0 });
        assert_eq!(grid.frame(16.0), Move::Done);
        assert_eq!(grid.probe.report().idle_frames, 0);

        // Earlier than a frame of the slowest screen is not that frame.
        let mut grid = Grid::new(4_000_000.0, 1);
        grid.run_to_the_idle_step();
        let early = IDLE_SECS * 1000.0 - EARLY_MS - 1.0;
        assert!(matches!(grid.frame(early), Move::Rest { .. }));
        assert_eq!(grid.frame(EARLY_MS + 1.0), Move::Done);
        assert_eq!(grid.probe.report().idle_frames, 1);
    }

    // The engine goes on reporting while the window is still - it looks for an unplugged
    // drive twice a minute, and the fixture's library is on one - and a report is drawn.
    // That is the window answering, not the grid repainting by itself.
    #[test]
    fn a_frame_something_outside_the_grid_asked_for_is_not_counted() {
        let mut grid = Grid::new(4_000_000.0, 1);
        grid.run_to_the_idle_step();
        grid.outside = true;
        assert!(matches!(grid.frame(1000.0), Move::Rest { .. }));
        grid.outside = true;
        assert!(matches!(grid.frame(16.0), Move::Rest { .. }));
        // And one nobody asked for.
        assert!(matches!(grid.frame(1000.0), Move::Rest { .. }));
        assert_eq!(grid.frame(IDLE_SECS * 1000.0), Move::Done);
        assert_eq!(grid.probe.report().idle_frames, 1);
    }

    // The toolkit draws a second frame after every frame asked for at once, so a report
    // from the engine is two frames, and only the first has the report in it.
    #[test]
    fn a_report_explains_its_own_frame_and_the_one_after() {
        let mut outside = Outside::default();
        assert!(!outside.frame(false));
        assert!(outside.frame(true));
        assert!(outside.frame(false), "the second frame of the same report");
        assert!(!outside.frame(false));
    }

    #[test]
    fn a_report_is_written_as_the_svelte_probe_writes_its_own() {
        let json = serde_json::to_value(Report::default()).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "fast",
                "idle_frames",
                "jump_end_ms",
                "jump_middle_ms",
                "launch_ms",
                "photos",
                "steady",
                "sweep",
                "sweep_settle_ms"
            ]
        );
        let mut cadence: Vec<&str> = json["steady"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        cadence.sort_unstable();
        assert_eq!(
            cadence,
            ["frames", "longest_ms", "median_ms", "p95_ms", "p99_ms"]
        );
    }
}
