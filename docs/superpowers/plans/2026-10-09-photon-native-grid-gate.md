# The native grid's gate - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One command, `cargo run -p xtask -- grid-gate`, that runs the native grid and the Svelte grid through the same scroll programme over the same 300,000-photo library and says, line by line, whether the native grid passes the gate the umbrella design set.

**Architecture:** The programme is a state machine in `photon-ui` (`probe.rs`, a state module: no egui) that the application feeds one frame at a time and obeys; `photon-native --probe FILE` runs it and writes a JSON report. The Svelte grid gets the same programme from a patch file, applied to a throwaway worktree and built apart, never merged. A fixture builder writes the library both run over: rows on a drive recorded as unplugged, every thumbnail already cached. `xtask grid-gate` builds all three, runs the two applications one after the other, and judges the two reports.

**Tech Stack:** Rust 2024, `eframe`/`egui` 0.36.2, `egui_kittest`, `serde`/`serde_json`, `photon-core`'s `Library` and `ThumbCache`; for the Svelte half, Svelte 5, `@tauri-apps/api`, two Tauri commands.

**Spec:** `docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md`, section "The gate" (and the umbrella, `2026-10-09-photon-native-ui-design.md`, which sets the pass line). Task 5 records in the spec what this plan decided that the spec did not.

## How this plan was written

As the first plan of this sub-project was: every file below was **written, compiled, run and probed before the plan was**, in a scratch worktree at `native-ui` (9893bf1), on 2026-10-09. There, `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` were clean, the workspace's tests passed, every probe listed failed as stated, and three things were run that no test runs:

- `cargo run -p xtask -- grid-gate --dry-run` built the fixture, `photon-native` and the patched Svelte photon, and launched nothing.
- The patched Svelte grid ran the whole programme in headless Chromium against the screenshot harness's `mock.js` at 300,000 photos and wrote a complete report. Its numbers mean nothing (software rendering, a mock); what it shows is that the script's selectors match the grid's markup and that every step ends.
- The native application ran the programme over the real 300,000-photo fixture off screen (`the_work_of_a_frame_over_the_gate_fixture`): its own pass per frame was 0.05 ms at the median and 1.35 ms at the 95th percentile.

**The gate itself has not been run.** It opens two fullscreen windows, and that is for a person to do, or to ask for at the time.

So the tasks are: write the file, run its tests, then **prove each rule by its probe** - an exact replacement that breaks the rule, after which the named test must fail. A probe that passes is a finding: stop and report it. (Two did while this was being written, and each changed the code: a second fixture build was refused only by accident, half-way, and a counter was reset where nothing had counted.)

**After every probe: put the original text back, then `touch` the file.**

## What the scratch build found

Three things about frames, each of which would have failed the gate on a perfectly still grid, and none of which a harness that draws a frame at every step can see:

1. egui draws a **second frame after every frame asked for at once** (`request_repaint`), so the frame after the last one the programme asks for is the toolkit's.
2. A frame **asked for after a delay comes a frame early** (egui takes its predicted frame time off the delay), so the frame that ends a five-second rest arrives sixteen milliseconds before the five seconds are up, and then again.
3. A scroll **leaves two frames of its own behind**: the one in which it counts as over (`SCROLL_SETTLE_MS`) and the one that tells the engine what is on screen (`VISIBLE_DEBOUNCE_MS`).

Hence the rest before the idle step (`STILL_SECS`), `EARLY_MS`, and `Facts::outside` for a frame the engine's own report asked for - it looks for the fixture's unplugged drive twice a minute and reports each time. And hence the end-to-end test drives the application **as an event loop does**: a frame only when egui's repaint callback asked for one, on a clock that jumps over the time nothing did.

## Global Constraints

- Read `CLAUDE.md` before starting, the section "The native UI" above all. Its rules bind every task.
- **Branch `native-ui-grid-gate`, off `native-ui`.** The pull request goes into `native-ui`.
- **Never launch the GUI, and never run `grid-gate --go`.** Not `photon-native`, not the Tauri photon, not `npm run dev`. `--dry-run` is the furthest this plan goes.
- **The Rust gate before every commit:** `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. Task 4 also runs the UI gate (`npm run check`, `npm test`), because its patch is against `ui/`.
- `probe.rs` is a **state module**: it names no egui type, and it is in `STATE_MODULES` (`lib.rs`), whose tripwire holds that.
- The patch is **never applied to the working tree and left there**. `ui/` and `photon-app` are unchanged by this plan.
- Nothing here writes outside `target/`, a test's temporary directory, or the files the tasks name.
- eframe is pinned to `=0.36.2`: read that version's source for anything about repaints, not memory.
- Comments carry the reasoning, in the surrounding code's density and voice. No em dashes; the codebase uses " - ".
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

What the spec implies and no test here exercises, most likely to bite first:

1. **The windowed run.** No test measures an interval between two frames under a compositor: how eframe paces frames on Wayland, or a window that reaches its fullscreen size some frames in. The first `--go` is the first evidence. A scroll line that FAILs there is the gate answering, not the gate broken - unless `idle_frames` is not 0 on a still grid, which would be a fourth thing the scratch build did not find.
2. **The Svelte probe inside the real web view.** `probe_config`, `probe_done`, the fullscreen call and the exit were compiled, and the script ran in headless Chromium against the mock; it has never run in WebKitGTK over a real engine. If it hangs, the gate kills it at `RUN_TIMEOUT` and says no report was written.
3. **An engine report landing in the idle step.** `Outside` is tested; the line in `App::ui` that feeds it (`take_events` -> `run_probe`) is not exercised by the end-to-end test, where no report arrives in its second of real time. Read those eight lines.
4. **A fixture from an older builder.** `target/gate-fixture` is used as it is when its `library.db` exists. After a change to `fixture.rs`, delete it.
5. **A refresh rate that is not the screen's.** `--refresh-hz` is trusted, and a screen with variable refresh has no one answer: the scroll's pass line is then wrong in either direction.

None of the five can be given a test without a window; 3 is the one a reviewer can settle by reading.

## File Structure

```
crates/photon-ui/
  Cargo.toml                 + serde, serde_json
  src/lib.rs                 + pub mod probe, pub mod fixture; probe.rs in STATE_MODULES
  src/probe.rs               NEW  the scroll programme and its report (state module)
  src/fixture.rs             NEW  the library both applications are measured over
  src/args.rs                + --probe FILE
  src/main.rs                + the programme's run, PHOTON_PROBE_T0
  src/app.rs                 + with_probe, run_probe, adapter; take_events says if it took any
  examples/fixture.rs        NEW  the fixture builder as a command
  tests/probe.rs             NEW  the fixture; the programme end to end; a measurement (ignored)
crates/xtask/
  src/main.rs                + fixture-library, grid-gate
  src/fixture_library.rs     NEW  runs photon-ui's fixture example, release
  src/grid_gate.rs           NEW  builds, runs, judges
  gate/svelte-probe.patch    NEW  the programme for the Svelte grid; applied to a throwaway worktree
CLAUDE.md                    + the gate, and how a test that counts frames is driven
docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md   + what this plan decided
```

Shell lines below assume the repository root and, for cargo, the usual `target/`.

---
### Task 1: The scroll programme

**Files:**
- Modify: `crates/photon-ui/Cargo.toml`, `crates/photon-ui/src/lib.rs`
- Create: `crates/photon-ui/src/probe.rs` (its tests are in the file)

**Interfaces:**
- Consumes: nothing of this plan's.
- Produces, in `photon_ui::probe`:
  - constants `STEADY_PX_PER_S`, `STEADY_SECS`, `FAST_PX_PER_S`, `FAST_SECS`, `SWEEP_SECS`, `IDLE_SECS`, `STILL_SECS`, `EARLY_MS`, `CALM_SECS`, `SETTLE_TIMEOUT_SECS` (all `f64`);
  - `struct Facts { now_ms: f64, max: f64, settled: bool, photos: usize, outside: bool }`;
  - `enum Move { Rest { for_ms: f64 }, Wait, To(f64), Done }`;
  - `struct Outside` (`Default`) with `fn frame(&mut self, reported: bool) -> bool`;
  - `struct Cadence { frames, median_ms, p95_ms, p99_ms, longest_ms }` with `Cadence::of(&[f64])`;
  - `struct Report { photos, launch_ms, steady, fast, jump_end_ms, jump_middle_ms, sweep, sweep_settle_ms, idle_frames }`, `Serialize`;
  - `Probe::new(started_epoch_ms: Option<f64>)`, `fn frame(&mut self, facts: Facts, epoch_ms: f64) -> Move`, `fn report(&self) -> &Report`, `fn done(&self) -> bool`.

The programme, in the spec's numbering: launch (1); a second of calm; the steady scroll (2) and the fast one (3), each a position worked out from the time; a jump to the end and one to the middle (4), each waited on until the grid shows its pictures or thirty seconds have passed; back to the top; the sweep (5) and its settling; a second of rest; five idle seconds (6).

- [ ] **Step 1: The dependencies and the module's place**

Change `crates/photon-ui/Cargo.toml`:

````diff
--- a/crates/photon-ui/Cargo.toml
+++ b/crates/photon-ui/Cargo.toml
@@ -32,6 +32,9 @@
 # The viewer's time zone, for the month a folder's header names. photon-core's version.
 jiff = "0.2.35"
 parking_lot = "0.12.5"
+# The gate's report (`probe.rs`), in the shape the Svelte probe writes its own.
+serde = { version = "1.0.229", features = ["derive"] }
+serde_json = "1"
 tracing = "0.1.44"
 tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
 
````

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -20,6 +20,7 @@
     pub mod visible;
 }
 pub mod icons;
+pub mod probe;
 pub mod tasks;
 pub mod text;
 pub mod theme {
@@ -37,7 +38,7 @@
 #[cfg(test)]
 mod tests {
     /// The state modules, with their source.
-    const STATE_MODULES: [(&str, &str); 11] = [
+    const STATE_MODULES: [(&str, &str); 12] = [
         ("args.rs", include_str!("args.rs")),
         ("dirs.rs", include_str!("dirs.rs")),
         ("tasks.rs", include_str!("tasks.rs")),
@@ -47,6 +48,7 @@
         ("grid/motion.rs", include_str!("grid/motion.rs")),
         ("grid/scroll.rs", include_str!("grid/scroll.rs")),
         ("grid/visible.rs", include_str!("grid/visible.rs")),
+        ("probe.rs", include_str!("probe.rs")),
         ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
         ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
     ];
````

- [ ] **Step 2: The programme and its tests**

Create `crates/photon-ui/src/probe.rs`:

````rust
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
````

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib probe`
Expected: 12 tests of `probe::tests` pass (the filter also matches one of `args`' in Task 3, not yet there).

Run: `cargo test -p photon-ui --lib state_modules_name_no_egui_type`
Expected: PASS, with `probe.rs` among the twelve it reads.

- [ ] **Step 4: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/probe.rs` | `self.to((self.from + STEADY_PX_PER_S * elapsed / 1000.0).min(facts.max))` | `self.to((self.position + 50.0).min(facts.max))` | `a_scroll_covers_the_same_ground_at_any_frame_rate` (`cargo test -p photon-ui --lib`) |
| 2 | `src/probe.rs` | `} else if waited >= SETTLE_TIMEOUT_SECS * 1000.0 {` | `} else if false && waited >= SETTLE_TIMEOUT_SECS * 1000.0 {` | `a_grid_that_never_settles_after_a_jump_is_given_up_on` (`cargo test -p photon-ui --lib`) |
| 3 | `src/probe.rs` | `self.report.launch_ms = self.started_epoch_ms.map(\|start\| epoch_ms - start);` | `self.report.launch_ms = self.started_epoch_ms.map(\|_\| 0.0 * epoch_ms);` | `the_launch_is_from_the_time_the_harness_gives_to_the_first_settled_frame` (`cargo test -p photon-ui --lib`) |
| 4 | `src/probe.rs` | `p95_ms: Self::at(&sorted, 0.95),` | `p95_ms: Self::at(&sorted, 0.5),` | `the_cadence_of_no_frames_is_nothing_and_of_some_is_their_quantiles` (`cargo test -p photon-ui --lib`) |
| 5 | `src/probe.rs` | `self.idle_frames += 1;` | `self.idle_frames += 0;` | `frames_drawn_while_idle_are_counted` (`cargo test -p photon-ui --lib`) |
| 6 | `src/probe.rs` | `if left > EARLY_MS && !facts.outside {` | `if !facts.outside {` | `the_frame_that_ends_a_rest_may_come_early` (`cargo test -p photon-ui --lib`) |
| 7 | `src/probe.rs` | `if left > EARLY_MS && !facts.outside {` | `if left > EARLY_MS {` | `a_frame_something_outside_the_grid_asked_for_is_not_counted` (`cargo test -p photon-ui --lib`) |
| 8 | `src/probe.rs` | `self.begin(Step::Still, now);` | `self.begin(Step::Idle, now);` | `what_a_scroll_leaves_behind_is_not_counted_as_idle` (`cargo test -p photon-ui --lib`) |
| 9 | `src/probe.rs` | `let outside = reported \|\| self.before;` | `let outside = reported;` | `a_report_explains_its_own_frame_and_the_one_after` (`cargo test -p photon-ui --lib`) |

Each must fail its test. Restore and `touch` after each.

- [ ] **Step 5: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-ui/Cargo.toml crates/photon-ui/src/lib.rs crates/photon-ui/src/probe.rs Cargo.lock
git commit -m "feat(ui): the scroll programme the gate measures

A state machine over what a frame came to: the steps of the spec's programme, each
move a place worked out from the time, and the intervals between frames as a
cadence. No egui in it.

Idle frames are the grid's own: a second of rest lets a scroll's last business end,
the frame that ends the step may come a frame early, and a frame the engine's
report asked for is not the grid repainting.

Nine probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---
### Task 2: The fixture

**Files:**
- Modify: `crates/photon-ui/src/lib.rs`, `crates/xtask/src/main.rs`
- Create: `crates/photon-ui/src/fixture.rs`, `crates/photon-ui/examples/fixture.rs`, `crates/photon-ui/tests/probe.rs`, `crates/xtask/src/fixture_library.rs`

**Interfaces:**
- Consumes: `photon_ui::dirs::{self, Dirs, IDENTIFIER}`; `photon_core::library::{Library, NewItem}` (`add_watched_folder`, `upsert_folder`, `insert_items`, `grid_entries`, `set_thumb_state`, `set_watched_online`, `thumb_gc_due`, `thumb_gc_done`); `photon_core::thumbs::{ThumbCache, ThumbSize}`.
- Produces, in `photon_ui::fixture`: `PER_FOLDER: usize`; `struct Fixture { data_home, cache_home, data_dir, cache_dir: PathBuf, photos: usize }` with `Fixture::at(out: &Path, photos: usize)` and `fn dirs(&self) -> Dirs`; `fn build(out: &Path, photos: usize, sources: &[PathBuf]) -> Result<Fixture, Box<dyn Error>>`. And the command `cargo run -p xtask -- fixture-library [--photos N] [--out DIR]`.

Why the builder is in `photon-ui` and not in xtask: xtask depends on neither `photon-core` nor `photon-ui`, and a builder there would be the first reason it had to.

- [ ] **Step 1: The test**

Create `crates/photon-ui/tests/probe.rs`:

````rust
//! The gate's fixture: a library built to be measured.

use photon_core::{library::Library, media::ThumbState};
use photon_ui::fixture;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

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

    // A second build into the same place is refused before it writes anything, not
    // layered over the first: without the check it fails too, but half-way, on a folder
    // the library already watches, having made the folder again.
    let again = fixture::build(&dir.path().join("fixture"), 10, &sources(dir.path()));
    let refused = again.expect_err("a second build is refused").to_string();
    assert!(refused.contains("already holds a library"), "{refused}");
    assert!(!dir.path().join("fixture/unplugged-drive").exists());
}
````

Run: `cargo test -p photon-ui --test probe`
Expected: does not compile - `photon_ui::fixture` is not there.

- [ ] **Step 2: The builder, and its command**

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -9,6 +9,7 @@
 pub mod args;
 pub mod dirs;
 pub mod events;
+pub mod fixture;
 pub mod grid {
     pub mod header;
     pub mod labels;
````

Create `crates/photon-ui/src/fixture.rs`:

````rust
//! A library made to be measured: any number of photos, every one with its grid thumbnail
//! already in the cache, and nothing for the engine to do when it opens it.
//!
//! The gate runs two applications over the same library and compares them, so the library
//! has to be the same every time and has to keep the engine quiet. Three things do that:
//!
//! - **The watched folder does not exist.** It is made, watched, and removed again. To the
//!   engine that is an unplugged drive: its scan finds no root and stops, nothing is marked
//!   missing or purged, and since a folder is watched, no Pictures folder is added.
//! - **Every row is `Ready`.** Left `Pending`, the engine would queue every photo for a
//!   render at launch and fail each one, the files not being there, all through the
//!   measurement.
//! - **The cache is recorded as clean**, so the launch does not walk every file in it.
//!
//! The thumbnails are a few real pictures, each hard-linked under many keys: 300,000
//! thumbnails take the room of a few dozen. That makes them cheaper to read than a real
//! library's, for both applications alike, which the gate's write-up says.

use crate::dirs::{self, Dirs, IDENTIFIER};
use photon_core::{
    library::{Library, NewItem},
    media::{MediaKind, ThumbState},
    metadata::CameraMeta,
    now_ms,
    thumbs::{ThumbCache, ThumbSize},
};
use std::{
    collections::HashSet,
    error::Error,
    path::{Path, PathBuf},
    time::Duration,
};

/// Photos in each folder, and so under each header.
pub const PER_FOLDER: usize = 300;

/// Where a fixture is, in both spellings: the directories photon-native is given, and the
/// two homes the Tauri photon is pointed at (`XDG_DATA_HOME`, `XDG_CACHE_HOME`), under
/// which it resolves the same directories by its identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub data_home: PathBuf,
    pub cache_home: PathBuf,
    /// `<data_home>/<identifier>`: what `--data-dir` names.
    pub data_dir: PathBuf,
    /// `<cache_home>/<identifier>`: what `--cache-dir` names.
    pub cache_dir: PathBuf,
    pub photos: usize,
}

impl Fixture {
    pub fn at(out: &Path, photos: usize) -> Self {
        let (data_home, cache_home) = (out.join("data"), out.join("cache"));
        Self {
            data_dir: data_home.join(IDENTIFIER),
            cache_dir: cache_home.join(IDENTIFIER),
            data_home,
            cache_home,
            photos,
        }
    }

    pub fn dirs(&self) -> Dirs {
        dirs::within(&self.data_dir, &self.cache_dir)
    }
}

/// Builds a library of `photos` photos under `out`, their thumbnails made from the
/// pictures in `sources`. `out` must not hold a fixture already.
pub fn build(out: &Path, photos: usize, sources: &[PathBuf]) -> Result<Fixture, Box<dyn Error>> {
    if sources.is_empty() {
        return Err("a fixture needs at least one picture to make thumbnails from".into());
    }
    let fixture = Fixture::at(out, photos);
    let dirs = fixture.dirs();
    if dirs.db_path.exists() {
        return Err(format!("{} already holds a library", out.display()).into());
    }
    std::fs::create_dir_all(&fixture.data_dir)?;

    // The folder is real for as long as the library needs it to be: a folder that is not
    // there cannot be watched.
    let root = out.join("unplugged-drive");
    std::fs::create_dir_all(&root)?;
    let lib = Library::open(&dirs.db_path)?;
    let watched = lib.add_watched_folder(&root, &[])?;
    let root_path = watched.path.clone();
    let root_id = lib.upsert_folder(watched.id, None, &root_path, 1)?;

    let mut items = Vec::with_capacity(photos);
    for folder in 0..photos.div_ceil(PER_FOLDER) {
        let folder_path = format!("{root_path}/folder-{folder:04}");
        let folder_id = lib.upsert_folder(watched.id, Some(root_id), &folder_path, 1)?;
        let first = folder * PER_FOLDER;
        for n in first..photos.min(first + PER_FOLDER) {
            let file_name = format!("IMG_{n:06}.jpg");
            // Landscape, portrait and wide, so tiles crop as a real library's do.
            let (width, height) = [(4000, 3000), (3000, 4000), (6000, 4000)][n % 3];
            items.push(NewItem {
                folder_id,
                path: format!("{folder_path}/{file_name}"),
                file_name,
                kind: MediaKind::Image,
                size: 2_000_000 + (n as i64 % 4_000) * 1_000,
                mtime_ms: 1_600_000_000_000 + n as i64,
                width,
                height,
                orientation: 1,
                // A folder every three days from 2017 on, a photo a minute within it.
                taken_at: 1_500_000_000 + folder as i64 * 259_200 + (n - first) as i64 * 60,
                rating: None,
                camera: CameraMeta::default(),
                tags: Vec::new(),
                caption: None,
                duration_ms: None,
            });
        }
    }
    for batch in items.chunks(10_000) {
        lib.insert_items(batch)?;
    }

    // A few real thumbnails, made the way the engine makes them, under keys of their own.
    let made = ThumbCache::new(out.join("made-thumbnails"));
    let mut pictures = Vec::with_capacity(sources.len());
    for (n, source) in sources.iter().enumerate() {
        made.generate(source, 1, n as u64)
            .map_err(|err| format!("{}: {err}", source.display()))?;
        pictures.push(made.path_for(n as u64, ThumbSize::Grid));
    }

    // Each photo's grid thumbnail is one of them, under the photo's own key.
    let cache = ThumbCache::new(&dirs.cache_dir);
    let entries = lib.grid_entries()?;
    let mut shards = HashSet::new();
    for (n, entry) in entries.iter().enumerate() {
        let path = cache.path_for(entry.thumb_key, ThumbSize::Grid);
        let shard = path.parent().expect("a thumbnail is in a directory");
        if shards.insert(shard.to_path_buf()) {
            std::fs::create_dir_all(shard)?;
        }
        let picture = &pictures[n % pictures.len()];
        // A link where the filesystem has room for one more to this file; a copy where it
        // has not, or `out` spans two filesystems.
        if std::fs::hard_link(picture, &path).is_err() {
            std::fs::copy(picture, &path)?;
        }
        lib.set_thumb_state(entry.id, ThumbState::Ready, None)?;
    }

    // Nothing in the cache is garbage, and the library says so: no walk of it at launch.
    let now = now_ms();
    if let Some(epoch) = lib.thumb_gc_due(now, Duration::from_secs(1))? {
        lib.thumb_gc_done(epoch, now)?;
    }
    // The drive is gone and the library knows: found out by the first scan instead, that
    // is a grid rebuilt in whichever application opens the fixture first.
    lib.set_watched_online(watched.id, false)?;
    drop(lib);
    std::fs::remove_dir_all(&root)?;
    Ok(fixture)
}
````

Create `crates/photon-ui/examples/fixture.rs`:

````rust
//! Builds the library the gate measures both applications over (`photon_ui::fixture`).
//!
//!   cargo run --release -p photon-ui --example fixture -- --photos 300000 --out target/gate-fixture
//!
//! The thumbnails are made from the CC0 photos the screenshots use, unless `--sources`
//! names another directory of JPEGs.

use photon_ui::fixture;
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut photos = 300_000usize;
    let mut out = None;
    let mut sources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../xtask/screenshots/photos");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next();
        match (arg.as_str(), value) {
            ("--photos", Some(value)) => match value.parse() {
                Ok(number) => photos = number,
                Err(_) => return usage(&format!("--photos {value} is not a number")),
            },
            ("--out", Some(value)) => out = Some(PathBuf::from(value)),
            ("--sources", Some(value)) => sources = PathBuf::from(value),
            (other, _) => return usage(&format!("unknown or incomplete argument {other}")),
        }
    }
    let Some(out) = out else {
        return usage("--out is needed");
    };
    let mut pictures: Vec<PathBuf> = match std::fs::read_dir(&sources) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
            .collect(),
        Err(err) => return usage(&format!("{}: {err}", sources.display())),
    };
    pictures.sort();
    match fixture::build(&out, photos, &pictures) {
        Ok(fixture) => {
            println!("{} photos in {}", fixture.photos, out.display());
            println!("  --data-dir {}", fixture.data_dir.display());
            println!("  --cache-dir {}", fixture.cache_dir.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("could not build the fixture: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!("{problem}\n\nusage: fixture --out DIR [--photos N] [--sources DIR]");
    ExitCode::from(2)
}
````

Create `crates/xtask/src/fixture_library.rs`:

````rust
//! `cargo run -p xtask -- fixture-library [--photos N] [--out DIR]`: the library the gate
//! measures both applications over, by itself.
//!
//! The builder is photon-ui's (`fixture.rs`, run as its `fixture` example), so that xtask
//! does not itself depend on photon-core. Release, because 300,000 rows in a debug build
//! is a wait.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let mut passed: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    if !passed.contains(&"--out") {
        passed.extend(["--out", "target/gate-fixture"]);
    }
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "run",
            "--release",
            "-p",
            "photon-ui",
            "--example",
            "fixture",
            "--",
        ])
        .args(passed)
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("cannot run cargo: {err}");
            ExitCode::FAILURE
        }
    }
}
````

Change `crates/xtask/src/main.rs`:

````diff
--- a/crates/xtask/src/main.rs
+++ b/crates/xtask/src/main.rs
@@ -6,8 +6,10 @@
 //!   cargo run -p xtask -- screenshots [--out <dir>] [--only <shot>] [--photos <dir>] [--no-build]
 //!   cargo run -p xtask -- scroll-probe [--no-build]
 //!   cargo run -p xtask -- native-shot
+//!   cargo run -p xtask -- fixture-library [--photos <n>] [--out <dir>]
 
 mod checks;
+mod fixture_library;
 mod native_shot;
 mod screenshots;
 mod scroll_probe;
@@ -28,9 +30,10 @@
         Some("screenshots") => screenshots::run(&repo_root(), &args),
         Some("scroll-probe") => scroll_probe::run(&repo_root(), &args),
         Some("native-shot") => native_shot::run(&repo_root()),
+        Some("fixture-library") => fixture_library::run(&repo_root(), &args),
         other => {
             eprintln!(
-                "unknown command {other:?}; expected `versions`, `metadata`, `screenshots`, `scroll-probe` or `native-shot`"
+                "unknown command {other:?}; expected `versions`, `metadata`, `screenshots`, `scroll-probe`, `native-shot` or `fixture-library`"
             );
             ExitCode::FAILURE
         }
````

- [ ] **Step 3: Run the test**

Run: `cargo test -p photon-ui --test probe`
Expected: `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` passes.

- [ ] **Step 4: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/fixture.rs` | `lib.set_thumb_state(entry.id, ThumbState::Ready, None)?;` | `let _ = ThumbState::Ready;` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |
| 2 | `src/fixture.rs` | `std::fs::remove_dir_all(&root)?;` | `let _ = &root;` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |
| 3 | `src/fixture.rs` | `lib.set_watched_online(watched.id, false)?;` | `let _ = watched.id;` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |
| 4 | `src/fixture.rs` | `lib.thumb_gc_done(epoch, now)?;` | `let _ = epoch;` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |
| 5 | `src/fixture.rs` | `if dirs.db_path.exists() {` | `if false && dirs.db_path.exists() {` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |
| 6 | `src/fixture.rs` | `if std::fs::hard_link(picture, &path).is_err() { ⏎             std::fs::copy(picture, &path)?; ⏎         }` | `let _ = (picture, &path);` | `a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan` (`cargo test -p photon-ui --test probe`) |

- [ ] **Step 5: The fixture the gate will use**

```bash
rm -rf target/gate-fixture
cargo run -p xtask -- fixture-library
```

Expected: `300000 photos in target/gate-fixture`, in about half a minute, and `du -sh target/gate-fixture` near 120 MB. It is git-ignored with the rest of `target/`.

- [ ] **Step 6: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-ui/src/lib.rs crates/photon-ui/src/fixture.rs crates/photon-ui/examples/fixture.rs crates/photon-ui/tests/probe.rs crates/xtask/src/fixture_library.rs crates/xtask/src/main.rs
git commit -m "feat(ui): a library made to be measured

300,000 rows written through Library's own writers on a drive recorded as
unplugged, every row Ready with a real thumbnail hard-linked under its key, and
the cache recorded as clean: an engine that opens it scans nothing, renders
nothing and walks nothing.

Six probes, each failing the fixture's test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---
### Task 3: The application runs the programme

**Files:**
- Modify: `crates/photon-ui/src/args.rs`, `crates/photon-ui/src/main.rs`, `crates/photon-ui/src/app.rs`, `crates/photon-ui/tests/probe.rs`

**Interfaces:**
- Consumes: Task 1's `Facts`, `Move`, `Outside`, `Probe`, `Report`; Task 2's `fixture::build`, `Fixture::dirs`; the slice's `GridOutput { settled, position, .. }`, `GridView::{scroll_to, max_position}`, `App::{new, photos, last_frame}`; `photon_engine::commands::memory_usage`.
- Produces: `Args.probe: Option<PathBuf>` (`--probe FILE`); `App::with_probe(self, out: PathBuf, started_epoch_ms: Option<f64>) -> Self`; `App::adapter(&self) -> Option<&str>`; the report file, the programme's `Report` flattened beside `app: "native"`, `window: [f32; 2]`, `scale`, `adapter`, `memory_bytes`; the environment variable `PHOTON_PROBE_T0` (epoch milliseconds at which the harness started the process).

- [ ] **Step 1: The tests**

`--probe`'s, in `args.rs`, arrives with Step 2's change. The end-to-end one:

Change `crates/photon-ui/tests/probe.rs`:

````diff
--- a/crates/photon-ui/tests/probe.rs
+++ b/crates/photon-ui/tests/probe.rs
@@ -1,12 +1,22 @@
-//! The gate's fixture: a library built to be measured.
-
+//! The gate's fixture and its scroll programme, end to end and without a window: a library
+//! built to be measured, the application over it, and the programme run to its report on a
+//! clock that is the harness's own.
+//!
+//! What this cannot say is how long a frame takes on a screen. That is the gate itself
+//! (`cargo run -p xtask -- grid-gate`), which needs a compositor.
+
+use eframe::egui::vec2;
 use photon_core::{library::Library, media::ThumbState};
-use photon_ui::fixture;
+use photon_ui::{app::App, fixture};
 use std::{
     io::Cursor,
     path::{Path, PathBuf},
-    time::Duration,
+    sync::{Arc, Mutex},
+    time::{Duration, Instant},
 };
+
+/// The harness's screen: sixty frames a second.
+const REFRESH: f64 = 1.0 / 60.0;
 
 /// Three small JPEGs to make thumbnails from.
 fn sources(dir: &Path) -> Vec<PathBuf> {
@@ -75,3 +85,162 @@
     assert!(refused.contains("already holds a library"), "{refused}");
     assert!(!dir.path().join("fixture/unplugged-drive").exists());
 }
+
+#[test]
+fn the_programme_runs_over_a_fixture_to_its_report() {
+    let dir = tempfile::tempdir().unwrap();
+    let fixture = fixture::build(&dir.path().join("fixture"), 900, &sources(dir.path())).unwrap();
+    let out = dir.path().join("report.json");
+    let report = out.clone();
+
+    let mut harness = egui_kittest::Harness::builder()
+        .with_size(vec2(800.0, 600.0))
+        .with_step_dt(REFRESH as f32)
+        .build_eframe(|cc| {
+            App::new(cc, fixture.dirs(), None)
+                .unwrap()
+                .with_probe(report, Some(0.0))
+        });
+
+    // A window's event loop, without the window. A frame is drawn when one was asked for
+    // and not otherwise, no sooner than a refresh after the last, on a clock that jumps
+    // over the time in which nothing was. egui tells whatever runs it of every request
+    // through this callback - it is how eframe learns of them - so the frames drawn here
+    // are the ones a window would draw: stepping the harness blindly draws three hundred
+    // in the idle step, which says nothing about a still grid.
+    let clock = Arc::new(Mutex::new(0.0_f64));
+    let due = Arc::new(Mutex::new(Some(0.0_f64)));
+    {
+        let (clock, due) = (clock.clone(), due.clone());
+        harness.ctx.set_request_repaint_callback(move |asked| {
+            let at = *clock.lock().unwrap() + asked.delay.as_secs_f64();
+            let mut due = due.lock().unwrap();
+            *due = Some(due.map_or(at, |before| before.min(at)));
+        });
+    }
+    let mut now = 0.0;
+    let mut unasked = false;
+    let deadline = Instant::now() + Duration::from_secs(120);
+    while !out.exists() {
+        assert!(
+            Instant::now() < deadline,
+            "the programme never wrote its report"
+        );
+        let Some(asked) = *due.lock().unwrap() else {
+            // Nothing is asked for: the engine or a decoder has yet to answer.
+            std::thread::sleep(Duration::from_millis(1));
+            continue;
+        };
+        now = if asked - now > 3.0 && !unasked {
+            // The only rest this long is the idle step's. One frame nobody asked for, in
+            // the middle of it, as a window uncovered would draw.
+            unasked = true;
+            now + (asked - now) / 2.0
+        } else {
+            *due.lock().unwrap() = None;
+            asked.max(now + REFRESH)
+        };
+        *clock.lock().unwrap() = now;
+        harness.input_mut().time = Some(now);
+        harness.step();
+    }
+
+    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
+    assert_eq!(json["app"], "native");
+    assert_eq!(json["photos"], 900);
+    assert_eq!(json["window"][0], 800.0);
+    // Ten seconds of the harness's clock at sixty steps a second.
+    let frames = json["steady"]["frames"].as_u64().unwrap();
+    assert!((595..=605).contains(&frames), "{frames}");
+    // The library is three screens of rows: the jumps land somewhere, and it shows its
+    // pictures there, every one of them from the cache.
+    for step in ["jump_end_ms", "jump_middle_ms", "sweep_settle_ms"] {
+        assert!(json[step].is_number(), "{step}: {}", json[step]);
+    }
+    // The idle step: the one frame nobody asked for, and nothing else. Not what the last
+    // scroll left to be drawn, not the second frame the toolkit draws after one asked for
+    // at once, and not the frame that ends the step, which comes a little early.
+    assert_eq!(json["idle_frames"], 1);
+    // The programme moved the grid and left it where its last step ends: at the far end.
+    let last = harness.state().last_frame().unwrap();
+    assert!(last.position > 10_000.0, "the grid is at {}", last.position);
+    // The app's library was the fixture's, and nothing in it was rendered or marked
+    // missing by being opened.
+    assert_eq!(harness.state().photos(), 900);
+    drop(harness);
+    let lib = Library::open(&fixture.dirs().db_path).unwrap();
+    assert_eq!(lib.grid_entries().unwrap().len(), 900);
+}
+
+/// The value at `quantile` of `sorted`, by the nearest rank.
+fn at(sorted: &[f64], quantile: f64) -> f64 {
+    let rank = (quantile * sorted.len() as f64).ceil() as usize;
+    sorted[rank.clamp(1, sorted.len()) - 1]
+}
+
+// Not a test of anything: a measurement, of the one half of a frame that can be measured
+// without a screen. It runs the gate's programme over the gate's own fixture on the
+// harness's clock and times what the application does for each frame - its own pass, and,
+// for every tenth frame, the pass with the frame drawn off screen and read back, which is
+// more than a window ever does. It says nothing about the cadence a compositor allows;
+// that is `cargo run -p xtask -- grid-gate`.
+//
+//   cargo run --release -p photon-ui --example fixture -- --out target/gate-fixture
+//   PHOTON_GATE_FIXTURE=$PWD/target/gate-fixture \
+//     cargo test --release -p photon-ui --test probe -- --ignored --nocapture
+//
+// The whole path: a test runs in its crate's directory, not where cargo was called.
+#[test]
+#[ignore = "a measurement over a fixture built beforehand; see the comment"]
+fn the_work_of_a_frame_over_the_gate_fixture() {
+    let Some(out) = std::env::var_os("PHOTON_GATE_FIXTURE") else {
+        panic!("PHOTON_GATE_FIXTURE names no fixture");
+    };
+    let out = std::fs::canonicalize(out).unwrap();
+    let fixture = fixture::Fixture::at(&out, 0);
+    let dir = tempfile::tempdir().unwrap();
+    let report = dir.path().join("report.json");
+    let written = report.clone();
+
+    let mut harness = egui_kittest::Harness::builder()
+        .with_size(vec2(2560.0, 1440.0))
+        .with_pixels_per_point(1.0)
+        .with_step_dt(1.0 / 60.0)
+        .wgpu()
+        .build_eframe(|cc| {
+            App::new(cc, fixture.dirs(), None)
+                .unwrap()
+                .with_probe(written, None)
+        });
+
+    let (mut passes, mut drawn) = (Vec::new(), Vec::new());
+    let deadline = Instant::now() + Duration::from_secs(600);
+    let mut frame = 0usize;
+    while !report.exists() {
+        let start = Instant::now();
+        harness.step();
+        passes.push(start.elapsed().as_secs_f64() * 1000.0);
+        if frame.is_multiple_of(10) {
+            harness.render().unwrap();
+            drawn.push(start.elapsed().as_secs_f64() * 1000.0);
+        }
+        frame += 1;
+        assert!(Instant::now() < deadline, "the programme never ended");
+    }
+    let photos = harness.state().photos();
+    for (name, times) in [
+        ("its own pass", &mut passes),
+        ("drawn and read back", &mut drawn),
+    ] {
+        times.sort_by(f64::total_cmp);
+        println!(
+            "{photos} photos at 2560x1440, {name}: {} frames, median {:.2} ms, p95 {:.2}, p99 {:.2}, longest {:.2}",
+            times.len(),
+            at(times, 0.5),
+            at(times, 0.95),
+            at(times, 0.99),
+            times[times.len() - 1]
+        );
+    }
+    println!("{}", std::fs::read_to_string(&report).unwrap());
+}
````

Run: `cargo test -p photon-ui --test probe`
Expected: does not compile - `App::with_probe` is not there.

- [ ] **Step 2: The argument, the wiring**

Change `crates/photon-ui/src/args.rs`:

````diff
--- a/crates/photon-ui/src/args.rs
+++ b/crates/photon-ui/src/args.rs
@@ -3,11 +3,13 @@
 use crate::dirs::{self, Dirs};
 use std::path::PathBuf;
 
-pub const USAGE: &str = "usage: photon-native [--data-dir DIR] [--cache-dir DIR] [--fullscreen]
+pub const USAGE: &str =
+    "usage: photon-native [--data-dir DIR] [--cache-dir DIR] [--fullscreen] [--probe FILE]
 
   --data-dir DIR    the directory that holds library.db
   --cache-dir DIR   the directory that holds thumbs/
   --fullscreen      open fullscreen
+  --probe FILE      run the gate's scroll programme, write its report to FILE, and quit
 
 Without --data-dir and --cache-dir the library the Tauri photon opens is opened.";
 
@@ -16,6 +18,8 @@
     pub data_dir: Option<PathBuf>,
     pub cache_dir: Option<PathBuf>,
     pub fullscreen: bool,
+    /// Where the gate's report goes; the scroll programme runs when this is given.
+    pub probe: Option<PathBuf>,
 }
 
 impl Args {
@@ -25,14 +29,15 @@
         let mut parsed = Self::default();
         let mut args = args.into_iter();
         while let Some(arg) = args.next() {
-            let mut value = |name: &str| {
+            let mut value = |name: &str, what: &str| {
                 args.next()
                     .map(PathBuf::from)
-                    .ok_or_else(|| format!("{name} needs a directory"))
+                    .ok_or_else(|| format!("{name} needs a {what}"))
             };
             match arg.as_str() {
-                "--data-dir" => parsed.data_dir = Some(value("--data-dir")?),
-                "--cache-dir" => parsed.cache_dir = Some(value("--cache-dir")?),
+                "--data-dir" => parsed.data_dir = Some(value("--data-dir", "directory")?),
+                "--cache-dir" => parsed.cache_dir = Some(value("--cache-dir", "directory")?),
+                "--probe" => parsed.probe = Some(value("--probe", "file")?),
                 "--fullscreen" => parsed.fullscreen = true,
                 other => return Err(format!("unknown argument {other}")),
             }
@@ -106,4 +111,12 @@
             Err("unknown argument --frobnicate".to_owned())
         );
     }
+
+    #[test]
+    fn the_probe_is_given_the_file_its_report_goes_to() {
+        let args = parse(&["--probe", "/tmp/report.json", "--fullscreen"]).unwrap();
+        assert_eq!(args.probe.as_deref(), Some(Path::new("/tmp/report.json")));
+        assert_eq!(parse(&[]).unwrap().probe, None);
+        assert_eq!(parse(&["--probe"]), Err("--probe needs a file".to_owned()));
+    }
 }
````

Change `crates/photon-ui/src/main.rs`:

````diff
--- a/crates/photon-ui/src/main.rs
+++ b/crates/photon-ui/src/main.rs
@@ -27,6 +27,7 @@
         return ExitCode::FAILURE;
     };
 
+    let probe = args.probe.clone();
     let options = eframe::NativeOptions {
         viewport: egui::ViewportBuilder::default()
             .with_title("photon")
@@ -41,12 +42,17 @@
         "photon",
         options,
         Box::new(move |cc| {
-            tracing::info!(
-                adapter = ?cc.wgpu_render_state.as_ref().map(|state| state.adapter.get_info()),
-                "started"
-            );
             match App::new(cc, dirs, ::dirs::picture_dir()) {
-                Ok(app) => Ok(Box::new(app) as Box<dyn eframe::App>),
+                Ok(app) => {
+                    tracing::info!(adapter = ?app.adapter(), "started");
+                    let app = match probe {
+                        // When the harness started this process, for the launch time; a
+                        // run by hand has none and the report says so.
+                        Some(out) => app.with_probe(out, started_epoch_ms()),
+                        None => app,
+                    };
+                    Ok(Box::new(app) as Box<dyn eframe::App>)
+                }
                 Err(message) => Err(message.into()),
             }
         }),
@@ -60,3 +66,9 @@
         }
     }
 }
+
+/// `PHOTON_PROBE_T0`: the epoch time, in milliseconds, at which the gate's harness started
+/// this process.
+fn started_epoch_ms() -> Option<f64> {
+    std::env::var("PHOTON_PROBE_T0").ok()?.parse().ok()
+}
````

Change `crates/photon-ui/src/app.rs`:

````diff
--- a/crates/photon-ui/src/app.rs
+++ b/crates/photon-ui/src/app.rs
@@ -8,6 +8,7 @@
         visible::VisibleReport,
     },
     icons,
+    probe::{Facts, Move, Outside, Probe, Report},
     tasks::Latest,
     theme::{
         self,
@@ -49,7 +50,33 @@
     zone: TimeZone,
     visible: VisibleReport,
     last: Option<GridOutput>,
+    /// The gate's scroll programme, when this run is a measurement.
+    probe: Option<ProbeRun>,
+    /// The GPU the window is drawn with, for the log and the gate's report.
+    adapter: Option<String>,
     closed: bool,
+}
+
+/// The gate's programme, where its report goes, and what the report says besides.
+struct ProbeRun {
+    probe: Probe,
+    outside: Outside,
+    out: PathBuf,
+}
+
+/// The report as it is written: the programme's, and what the application knows about
+/// the run it was measured in.
+#[derive(serde::Serialize)]
+struct Written<'a> {
+    app: &'static str,
+    /// The window's size in points, and how many device pixels a point is.
+    window: [f32; 2],
+    scale: f32,
+    adapter: Option<&'a str>,
+    /// photon's own memory at the end of the run (`commands::memory_usage`).
+    memory_bytes: Option<u64>,
+    #[serde(flatten)]
+    report: &'a Report,
 }
 
 impl App {
@@ -121,8 +148,30 @@
             zone: TimeZone::system(),
             visible: VisibleReport::default(),
             last: None,
+            probe: None,
+            adapter: cc
+                .wgpu_render_state
+                .as_ref()
+                .map(|state| format!("{:?}", state.adapter.get_info())),
             closed: false,
         })
+    }
+
+    /// Makes this run a measurement: the gate's scroll programme drives the grid from the
+    /// first frame, writes its report to `out` and closes the window.
+    /// `started_epoch_ms` is when the harness started the process, for the launch time.
+    pub fn with_probe(mut self, out: PathBuf, started_epoch_ms: Option<f64>) -> Self {
+        self.probe = Some(ProbeRun {
+            probe: Probe::new(started_epoch_ms),
+            outside: Outside::default(),
+            out,
+        });
+        self
+    }
+
+    /// The GPU the window is drawn with, as wgpu describes it.
+    pub fn adapter(&self) -> Option<&str> {
+        self.adapter.as_deref()
     }
 
     /// How many photos the grid holds.
@@ -145,12 +194,15 @@
         &self.folders
     }
 
-    /// Takes what the engine has reported since the last frame. Only a changed library
-    /// is acted on in this slice; the rest is taken so the channel stays empty.
-    fn take_events(&mut self) {
+    /// Takes what the engine has reported since the last frame, and says whether it had
+    /// reported anything. Only a changed library is acted on in this slice; the rest is
+    /// taken so the channel stays empty.
+    fn take_events(&mut self) -> bool {
+        let mut reported = false;
         let mut changed = false;
         let mut data_changed = false;
         for event in self.events.try_iter() {
+            reported = true;
             if let Event::Library(library) = event {
                 changed = true;
                 data_changed |= library.data_changed;
@@ -165,6 +217,7 @@
         if data_changed {
             self.folder_list.ask(());
         }
+        reported
     }
 
     fn take_answers(&mut self) {
@@ -176,6 +229,55 @@
         }
     }
 
+    /// One frame of the gate's programme, after the frame has been drawn: tells it what
+    /// the frame came to and does what it asks before the next.
+    fn run_probe(&mut self, ctx: &egui::Context, output: &GridOutput, reported: bool) {
+        let Some(run) = &mut self.probe else {
+            return;
+        };
+        if run.probe.done() {
+            return;
+        }
+        let facts = Facts {
+            now_ms: ctx.input(|input| input.time) * 1000.0,
+            max: self.view.max_position(),
+            settled: output.settled,
+            photos: self.index.len(),
+            outside: run.outside.frame(reported),
+        };
+        let epoch_ms = std::time::SystemTime::now()
+            .duration_since(std::time::UNIX_EPOCH)
+            .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
+        match run.probe.frame(facts, epoch_ms) {
+            Move::To(position) => {
+                self.view.scroll_to(position);
+                ctx.request_repaint();
+            }
+            Move::Wait => ctx.request_repaint(),
+            // Nothing is asked for but the frame that ends the rest: what is drawn before
+            // it is what the grid draws by itself.
+            Move::Rest { for_ms } => {
+                ctx.request_repaint_after(Duration::from_secs_f64(for_ms.max(0.0) / 1000.0));
+            }
+            Move::Done => {
+                let written = Written {
+                    app: "native",
+                    window: ctx.input(|input| input.content_rect().size()).into(),
+                    scale: ctx.pixels_per_point(),
+                    adapter: self.adapter.as_deref(),
+                    // The measurement is over: this read of /proc costs it nothing.
+                    memory_bytes: commands::memory_usage().ok().map(|usage| usage.bytes),
+                    report: run.probe.report(),
+                };
+                let json = serde_json::to_string_pretty(&written).unwrap_or_default();
+                if let Err(err) = std::fs::write(&run.out, json) {
+                    tracing::error!(%err, out = %run.out.display(), "could not write the probe's report");
+                }
+                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
+            }
+        }
+    }
+
     fn close(&mut self) {
         if !std::mem::replace(&mut self.closed, true) {
             self.engine.shutdown();
@@ -185,7 +287,7 @@
 
 impl eframe::App for App {
     fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
-        self.take_events();
+        let reported = self.take_events();
         self.take_answers();
 
         let data = GridData {
@@ -212,6 +314,9 @@
                 ui.ctx().request_repaint_after(wait);
             }
         }
+        if let Some(output) = &output {
+            self.run_probe(ui.ctx(), output, reported);
+        }
         self.last = output;
     }
 
````

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui`
Expected: every test passes; `tests/probe.rs` has 2 passed and 1 ignored. The end-to-end test takes about a second: its clock is not the wall's.

- [ ] **Step 4: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/args.rs` | `"--probe" => parsed.probe = Some(value("--probe", "file")?),` | `"--probe" => drop(value("--probe", "file")?),` | `the_probe_is_given_the_file_its_report_goes_to` (`cargo test -p photon-ui --lib`) |
| 2 | `src/app.rs` | `self.view.scroll_to(position); ⏎                 ctx.request_repaint();` | `let _ = position; ⏎                 ctx.request_repaint();` | `the_programme_runs_over_a_fixture_to_its_report` (`cargo test -p photon-ui --test probe`) |
| 3 | `src/app.rs` | `outside: run.outside.frame(reported),` | `outside: true,` | `the_programme_runs_over_a_fixture_to_its_report` (`cargo test -p photon-ui --test probe`) |
| 4 | `src/app.rs` | `ctx.request_repaint_after(Duration::from_secs_f64(for_ms.max(0.0) / 1000.0));` | `let _ = for_ms; ⏎                 ctx.request_repaint();` | `the_programme_runs_over_a_fixture_to_its_report` (`cargo test -p photon-ui --test probe`) |
| 5 | `src/probe.rs` | `if left > EARLY_MS && !facts.outside {` | `if !facts.outside {` | `the_programme_runs_over_a_fixture_to_its_report` (`cargo test -p photon-ui --test probe`) |
| 6 | `src/probe.rs` | `pub const STILL_SECS: f64 = 1.0;` | `pub const STILL_SECS: f64 = 0.0;` | `the_programme_runs_over_a_fixture_to_its_report` (`cargo test -p photon-ui --test probe`) |

Probes 5 and 6 are Task 1's rules again, seen through the whole application: they are what shows the event loop of the test is one, since stepped blindly neither would fail.

No test holds the `false` half of `outside` in `App::ui`: no engine report arrives in the end-to-end test's second of real time. Say so in the commit.

- [ ] **Step 5: The measurement that needs no screen (optional, not a test)**

```bash
PHOTON_GATE_FIXTURE=$PWD/target/gate-fixture \
  cargo test --release -p photon-ui --test probe -- --ignored --nocapture
```

Expected: two lines of timings and the report. In the scratch build, at 2560x1440 over 300,000 photos: its own pass 0.05 ms at the median, 1.35 at the 95th percentile, 6.16 at the longest.

- [ ] **Step 6: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-ui/src/args.rs crates/photon-ui/src/main.rs crates/photon-ui/src/app.rs crates/photon-ui/tests/probe.rs
git commit -m "feat(ui): photon-native --probe runs the gate's programme

After each frame the application tells the programme what the frame came to and
does what it asks: a position, another frame, or a rest. At the end it writes the
report with the window, the scale, the adapter and its memory, and closes.

The end-to-end test drives the application as an event loop does - a frame only
when egui's repaint callback asked for one, on a clock that jumps - because a
harness stepped blindly draws three hundred frames in the idle step and says
nothing about a still grid. Six probes. The one line with no test: that a frame
with no engine report in it or before it is passed as the grid's own, since no
report arrives in the test's second of real time.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---
### Task 4: The gate

**Files:**
- Modify: `crates/xtask/src/main.rs`
- Create: `crates/xtask/src/grid_gate.rs`, `crates/xtask/gate/svelte-probe.patch`

**Interfaces:**
- Consumes: Task 2's fixture (`target/gate-fixture`, `cargo run --release -p photon-ui --example fixture`); Task 3's `photon-native --data-dir D --cache-dir C --fullscreen --probe FILE`, `PHOTON_PROBE_T0`, and the report's keys (`steady.p95_ms`, `fast.p95_ms`, `sweep.p95_ms`, `steady.longest_ms`, `jump_end_ms`, `jump_middle_ms`, `sweep_settle_ms`, `launch_ms`, `memory_bytes`, `idle_frames`, `photos`, `window`, `scale`, `adapter`).
- Produces: `cargo run -p xtask -- grid-gate (--go --refresh-hz HZ | --dry-run) [--native-only] [--photos N]`; reports in `target/gate/native.json` and `target/gate/svelte.json`; exit status 0 only when no line fails.

The patch gives the Svelte grid the programme as `ui/src/lib/probe.ts`, started once from `Grid.svelte` when the harness set `PHOTON_PROBE_OUT`, and two Tauri commands (`probe_config`, `probe_done`). It is a file because the spec keeps it out of `ui/`. It was produced by `git diff` from a tree with those four changes and is applied with `git apply`; do not write it by hand - copy it exactly.

What `judge` passes, which the spec left to this plan (Task 5 records it): a scroll with its 95th-percentile interval at or under 1.5 refreshes; a time or the memory within 5% of the Svelte grid's, a time with one refresh of grace besides; `idle_frames` of exactly 0.

- [ ] **Step 1: The harness, with its tests, and the patch**

Create `crates/xtask/src/grid_gate.rs`:

````rust
//! `cargo run -p xtask -- grid-gate`: the native grid against the Svelte grid, over the same
//! 300,000-photo library, on this machine's screen.
//!
//! It is the gate of the native UI (spec `2026-10-09-photon-native-grid-slice-design.md`):
//! whether egui draws photon's grid well enough to build the rest on. Both applications run
//! the same scroll programme fullscreen, one after the other, and write what they measured;
//! this lays the two reports side by side and says which lines pass.
//!
//! **It opens two fullscreen windows for about two minutes**, which is why it does nothing
//! without `--go`: photon's conventions forbid launching the application to verify a change,
//! and this is the one exception, because the interval between two frames does not exist
//! without a compositor. It needs a desktop that is awake and unlocked for that long.
//! `--dry-run` builds everything and launches nothing.
//!
//! The Svelte grid has no probe of its own. `gate/svelte-probe.patch` gives it one, applied
//! to a throwaway worktree (`target/gate-svelte`) that is built apart from everything else
//! (`target/gate-svelte-target`): the patch is never merged and the working tree is never
//! touched.
//!
//! The pure part - reading two reports and judging them - is tested. Building and running
//! two applications is not.

use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PATCH: &str = "crates/xtask/gate/svelte-probe.patch";
/// How long one application may take over the programme before it is given up on: the
/// programme is half a minute, a launch over 300,000 photos a few seconds more.
const RUN_TIMEOUT: Duration = Duration::from_secs(360);
/// A measure within this share of the other's is a tie. The two clocks are not one
/// instrument: an interval read in a frame callback and one read in the application's own
/// frame are both the compositor's cadence, seen from two places.
const TIE: f64 = 0.05;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    go: bool,
    dry_run: bool,
    native_only: bool,
    refresh_hz: Option<f64>,
    photos: usize,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            go: false,
            dry_run: false,
            native_only: false,
            refresh_hz: None,
            photos: 300_000,
        };
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            let mut number = |name: &str| -> Result<f64, String> {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{name} needs a number"))?;
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|number| *number > 0.0)
                    .ok_or_else(|| format!("{name} {value} is not a number above zero"))
            };
            match arg.as_str() {
                "--go" => options.go = true,
                "--dry-run" => options.dry_run = true,
                "--native-only" => options.native_only = true,
                "--refresh-hz" => options.refresh_hz = Some(number("--refresh-hz")?),
                "--photos" => options.photos = number("--photos")? as usize,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if options.go == options.dry_run {
            return Err(
                "say which: --go opens two fullscreen windows for about two minutes; \
                 --dry-run builds everything and launches nothing"
                    .to_owned(),
            );
        }
        if options.go && options.refresh_hz.is_none() {
            return Err("--go needs --refresh-hz, the screen's refresh rate \
                 (hyprctl monitors, xrandr, or the display settings say it)"
                .to_owned());
        }
        Ok(options)
    }
}

/// How one line of the comparison came out.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Pass,
    Fail,
    /// Reported, and judged by a person: the spec gives it no pass line.
    Shown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub what: &'static str,
    pub native: Option<f64>,
    pub svelte: Option<f64>,
    pub outcome: Outcome,
    pub why: String,
}

fn number(report: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(report, |value, key| value.get(key))?
        .as_f64()
}

/// The comparison the spec sets: in the two scrolls the native grid's frames are within one
/// refresh at the 95th percentile, and it is no worse than the Svelte grid in time to
/// picture, in memory and in launch.
pub fn judge(native: &Value, svelte: Option<&Value>, refresh_hz: f64) -> Vec<Line> {
    let refresh_ms = 1000.0 / refresh_hz;
    let of = |report: Option<&Value>, path: &[&str]| report.and_then(|r| number(r, path));
    let mut lines = Vec::new();

    // A frame that missed its refresh shows as an interval of two. Half a refresh of
    // slack is the jitter of a frame that did not.
    for (what, step) in [
        ("steady scroll, p95 frame", "steady"),
        ("fast scroll, p95 frame", "fast"),
    ] {
        let p95 = number(native, &[step, "p95_ms"]);
        let (outcome, why) = match p95 {
            Some(p95) if p95 <= refresh_ms * 1.5 => (
                Outcome::Pass,
                format!("within a refresh ({refresh_ms:.1} ms)"),
            ),
            Some(_) => (
                Outcome::Fail,
                format!("a frame in twenty misses its refresh ({refresh_ms:.1} ms)"),
            ),
            None => (Outcome::Fail, "not measured".to_owned()),
        };
        lines.push(Line {
            what,
            native: p95,
            svelte: of(svelte, &[step, "p95_ms"]),
            outcome,
            why,
        });
    }

    for (what, key) in [
        ("jump to the end, to pictures", "jump_end_ms"),
        ("jump to the middle, to pictures", "jump_middle_ms"),
        ("after a sweep, to pictures", "sweep_settle_ms"),
        ("launch, to pictures", "launch_ms"),
        ("memory", "memory_bytes"),
    ] {
        let (ours, theirs) = (number(native, &[key]), of(svelte, &[key]));
        // A frame's grace on a time: both are read once a frame.
        let grace = if key == "memory_bytes" {
            0.0
        } else {
            refresh_ms
        };
        let (outcome, why) = match (ours, theirs) {
            (None, _) => (
                Outcome::Fail,
                "the native grid did not get there".to_owned(),
            ),
            (Some(_), None) => (Outcome::Shown, "nothing to compare with".to_owned()),
            (Some(ours), Some(theirs)) if ours <= theirs * (1.0 + TIE) + grace => {
                let tie = ours >= theirs * (1.0 - TIE) - grace;
                (
                    Outcome::Pass,
                    if tie { "a tie" } else { "better" }.to_owned(),
                )
            }
            (Some(_), Some(_)) => (Outcome::Fail, "worse than the Svelte grid".to_owned()),
        };
        lines.push(Line {
            what,
            native: ours,
            svelte: theirs,
            outcome,
            why,
        });
    }

    // A still grid that repaints is a bug in the wiring: a finding, the spec says, that
    // weighs as much as a failed number.
    let idle = number(native, &["idle_frames"]);
    lines.push(Line {
        what: "frames drawn in five idle seconds",
        native: idle,
        svelte: None,
        outcome: if idle == Some(0.0) {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        why: "a still grid draws nothing".to_owned(),
    });
    for (what, step) in [
        ("sweep, p95 frame", "sweep"),
        ("steady scroll, longest frame", "steady"),
    ] {
        let key = if what.contains("longest") {
            "longest_ms"
        } else {
            "p95_ms"
        };
        lines.push(Line {
            what,
            native: number(native, &[step, key]),
            svelte: of(svelte, &[step, key]),
            outcome: Outcome::Shown,
            why: String::new(),
        });
    }
    lines
}

fn shown(value: Option<f64>, what: &str) -> String {
    match value {
        None => "-".to_owned(),
        Some(bytes) if what == "memory" => format!("{:.0} MB", bytes / 1_048_576.0),
        Some(frames) if what.starts_with("frames") => format!("{frames:.0}"),
        Some(ms) => format!("{ms:.1} ms"),
    }
}

pub fn table(lines: &[Line]) -> String {
    let mut out = format!("{:<36}{:>12}{:>12}   \n", "", "native", "svelte");
    for line in lines {
        let mark = match line.outcome {
            Outcome::Pass => "pass",
            Outcome::Fail => "FAIL",
            Outcome::Shown => "",
        };
        out.push_str(&format!(
            "{:<36}{:>12}{:>12}   {mark:<5}{}\n",
            line.what,
            shown(line.native, line.what),
            shown(line.svelte, line.what),
            line.why
        ));
    }
    out
}

fn step(what: &str, command: &mut Command) -> Result<(), String> {
    println!("== {what}");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{what}: {status}")),
        Err(err) => Err(format!("{what}: {err}")),
    }
}

/// cargo in the repository, building into its own `target`: the binaries are looked for
/// there, so a `CARGO_TARGET_DIR` this was started under must not send them elsewhere.
fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(root).env_remove("CARGO_TARGET_DIR");
    command
}

/// The throwaway worktree with the probe patched in, built: the path of its binary.
fn build_svelte(root: &Path) -> Result<PathBuf, String> {
    let tree = root.join("target/gate-svelte");
    let head = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|err| format!("git: {err}"))?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let marker = tree.join(".gate-built-from");
    // Made from another commit, or half made: start again.
    if tree.exists() && std::fs::read_to_string(&marker).ok().as_deref() != Some(head.as_str()) {
        let _ = Command::new("git")
            .current_dir(root)
            .args(["worktree", "remove", "--force"])
            .arg(&tree)
            .status();
        let _ = std::fs::remove_dir_all(&tree);
    }
    if !tree.exists() {
        step(
            "a worktree for the Svelte grid",
            Command::new("git")
                .current_dir(root)
                .args(["worktree", "add", "--detach"])
                .arg(&tree)
                .arg("HEAD"),
        )?;
        step(
            "the probe patched into it",
            Command::new("git")
                .current_dir(&tree)
                .arg("apply")
                .arg(root.join(PATCH)),
        )?;
        step(
            "its packages",
            Command::new("npm").current_dir(&tree).arg("ci"),
        )?;
        std::fs::write(&marker, &head).map_err(|err| format!("{}: {err}", marker.display()))?;
    }
    let target = root.join("target/gate-svelte-target");
    step(
        "the Svelte photon, release",
        Command::new("npm")
            .current_dir(&tree)
            .env("CARGO_TARGET_DIR", &target)
            .args(["run", "tauri", "build", "--", "--no-bundle"]),
    )?;
    Ok(target.join("release/photon"))
}

/// Reads the library into the page cache. The two applications run one after the other
/// over the same file, and the first would otherwise pay for reading it from the disk in
/// its launch while the second found it in memory.
fn warm(data: &Path) {
    for name in ["library.db", "library.db-wal"] {
        let _ = std::fs::read(data.join(name));
    }
}

/// Runs `command` to its end, or kills it at the timeout, and reads the report it wrote.
fn measure(what: &str, command: &mut Command, report: &Path) -> Result<Value, String> {
    println!("== {what}: running");
    let _ = std::fs::remove_file(report);
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
    let mut child = command
        .env("PHOTON_PROBE_T0", format!("{started:.3}"))
        .spawn()
        .map_err(|err| format!("{what}: {err}"))?;
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("{what}: no report after {RUN_TIMEOUT:?}; killed"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(err) => return Err(format!("{what}: {err}")),
        }
    }
    let text = std::fs::read_to_string(report)
        .map_err(|err| format!("{what} wrote no report ({}): {err}", report.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{what}'s report: {err}"))
}

fn gate(root: &Path, options: &Options) -> Result<bool, String> {
    let fixture = root.join("target/gate-fixture");
    let data = fixture.join("data/io.github.bsg62.photon");
    let cache = fixture.join("cache/io.github.bsg62.photon");
    if !data.join("library.db").is_file() {
        step(
            "the fixture library",
            cargo(root)
                .args([
                    "run",
                    "--release",
                    "-p",
                    "photon-ui",
                    "--example",
                    "fixture",
                    "--",
                ])
                .args(["--photos", &options.photos.to_string(), "--out"])
                .arg(&fixture),
        )?;
    }
    step(
        "photon-native, release",
        cargo(root).args(["build", "--release", "-p", "photon-ui"]),
    )?;
    let native = root.join("target/release/photon-native");
    let svelte = if options.native_only {
        None
    } else {
        Some(build_svelte(root)?)
    };
    if options.dry_run {
        println!("== built; --dry-run launches nothing");
        return Ok(true);
    }

    let reports = root.join("target/gate");
    std::fs::create_dir_all(&reports).map_err(|err| format!("{}: {err}", reports.display()))?;
    // The native grid first. Whatever the first run finds cold - a directory entry, a page
    // of the library the warm-up did not reach - then counts against the grid that has
    // to pass, never for it.
    let out = reports.join("native.json");
    warm(&data);
    let native_report = measure(
        "the native grid",
        Command::new(&native)
            .arg("--data-dir")
            .arg(&data)
            .arg("--cache-dir")
            .arg(&cache)
            .arg("--fullscreen")
            .arg("--probe")
            .arg(&out),
        &out,
    )?;

    let svelte_report = match &svelte {
        Some(binary) => {
            let out = reports.join("svelte.json");
            // The Tauri photon finds its library, its cache and its window state under
            // these three; nothing of the user's own photon is read or written.
            warm(&data);
            let report = measure(
                "the Svelte grid",
                Command::new(binary)
                    .env("XDG_DATA_HOME", fixture.join("data"))
                    .env("XDG_CACHE_HOME", fixture.join("cache"))
                    .env("XDG_CONFIG_HOME", fixture.join("config"))
                    .env("PHOTON_PROBE_OUT", &out),
                &out,
            )?;
            Some(report)
        }
        None => None,
    };
    let refresh_hz = options.refresh_hz.unwrap_or(60.0);
    let lines = judge(&native_report, svelte_report.as_ref(), refresh_hz);
    // The fixture's own count: one built before with another `--photos` is used as it is.
    println!("\n{} photos, {refresh_hz} Hz", native_report["photos"]);
    for (name, report) in [
        ("native", Some(&native_report)),
        ("svelte", svelte_report.as_ref()),
    ] {
        if let Some(report) = report {
            println!(
                "{name}: window {} at scale {}, {}",
                report["window"], report["scale"], report["adapter"]
            );
        }
    }
    println!("\n{}", table(&lines));
    println!("reports: {}", reports.display());
    Ok(lines.iter().all(|line| line.outcome != Outcome::Fail))
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!(
                "{problem}\n\nusage: cargo run -p xtask -- grid-gate (--go --refresh-hz HZ | --dry-run) \
                 [--native-only] [--photos N]"
            );
            return ExitCode::from(2);
        }
    };
    match gate(root, &options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            println!("the gate did not pass");
            ExitCode::FAILURE
        }
        Err(problem) => {
            eprintln!("{problem}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("grid-gate")
            .chain(list.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    /// A report in which everything took `ms`, at 60 Hz, in `mb` megabytes.
    fn report(ms: f64, settle: f64, mb: f64) -> Value {
        let cadence =
            json!({ "frames": 600, "median_ms": ms, "p95_ms": ms, "p99_ms": ms, "longest_ms": ms });
        json!({
            "steady": cadence, "fast": cadence, "sweep": cadence,
            "jump_end_ms": settle, "jump_middle_ms": settle, "sweep_settle_ms": settle,
            "launch_ms": 800.0, "memory_bytes": mb * 1_048_576.0, "idle_frames": 0,
        })
    }

    fn outcome(lines: &[Line], what: &str) -> Outcome {
        lines
            .iter()
            .find(|line| line.what == what)
            .unwrap()
            .outcome
            .clone()
    }

    // Opening two fullscreen windows is never something this does by default, or by a
    // flag that could mean something else.
    #[test]
    fn nothing_is_launched_without_being_told_to() {
        assert!(Options::parse(&args(&[])).is_err());
        assert!(Options::parse(&args(&["--refresh-hz", "60"])).is_err());
        assert!(Options::parse(&args(&["--go", "--dry-run", "--refresh-hz", "60"])).is_err());
        let dry = Options::parse(&args(&["--dry-run"])).unwrap();
        assert!(dry.dry_run && !dry.go);
        let go = Options::parse(&args(&["--go", "--refresh-hz", "144", "--native-only"])).unwrap();
        assert!(go.go && go.native_only);
        assert_eq!(go.refresh_hz, Some(144.0));
    }

    // The pass line is against the screen's refresh, so the screen's refresh is not guessed.
    #[test]
    fn a_run_needs_the_screens_refresh_rate() {
        let problem = Options::parse(&args(&["--go"])).unwrap_err();
        assert!(problem.contains("--refresh-hz"), "{problem}");
        assert!(Options::parse(&args(&["--go", "--refresh-hz", "0"])).is_err());
        assert!(Options::parse(&args(&["--go", "--refresh-hz", "fast"])).is_err());
    }

    #[test]
    fn a_grid_on_time_that_is_no_worse_passes_every_line() {
        let lines = judge(
            &report(16.7, 80.0, 300.0),
            Some(&report(16.7, 120.0, 600.0)),
            60.0,
        );
        assert!(
            lines.iter().all(|line| line.outcome != Outcome::Fail),
            "{lines:?}"
        );
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        assert_eq!(outcome(&lines, "memory"), Outcome::Pass);
    }

    // At 60 Hz a refresh is 16.7 ms; a frame that misses one is 33.3 apart from the last.
    #[test]
    fn a_frame_in_twenty_that_misses_its_refresh_fails_the_scroll() {
        let mut native = report(16.7, 80.0, 300.0);
        native["fast"]["p95_ms"] = json!(33.3);
        let lines = judge(&native, Some(&report(16.7, 120.0, 600.0)), 60.0);
        assert_eq!(outcome(&lines, "fast scroll, p95 frame"), Outcome::Fail);
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        // The same intervals at 30 Hz are every frame on time.
        let lines = judge(&native, None, 30.0);
        assert_eq!(outcome(&lines, "fast scroll, p95 frame"), Outcome::Pass);
    }

    #[test]
    fn worse_than_the_svelte_grid_fails_and_within_a_few_percent_is_a_tie() {
        let svelte = report(16.7, 200.0, 600.0);
        // 5% and a frame's grace: 200 * 1.05 + 16.7 = 226.7.
        let lines = judge(&report(16.7, 226.0, 620.0), Some(&svelte), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Pass
        );
        assert_eq!(outcome(&lines, "memory"), Outcome::Pass);
        let lines = judge(&report(16.7, 228.0, 640.0), Some(&svelte), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Fail
        );
        assert_eq!(
            outcome(&lines, "memory"),
            Outcome::Fail,
            "630 MB is the line"
        );
    }

    #[test]
    fn a_jump_that_never_showed_its_pictures_fails() {
        let mut native = report(16.7, 80.0, 300.0);
        native["jump_end_ms"] = Value::Null;
        let lines = judge(&native, Some(&report(16.7, 120.0, 600.0)), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Fail
        );
    }

    #[test]
    fn a_grid_that_draws_while_idle_fails() {
        let mut native = report(16.7, 80.0, 300.0);
        native["idle_frames"] = json!(3);
        let lines = judge(&native, None, 60.0);
        assert_eq!(
            outcome(&lines, "frames drawn in five idle seconds"),
            Outcome::Fail
        );
    }

    // The patch is against files that go on changing, and nothing else reads it until
    // somebody runs the gate. Asked of the index, not of the working tree, so an edit in
    // progress to one of those files does not fail it. Linux only, where the gate is run:
    // a Windows checkout rewrites the patch's line endings.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "the gate is run on Linux")]
    fn the_svelte_probe_still_applies() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let check = Command::new("git")
            .current_dir(&root)
            .args(["apply", "--check", "--cached", PATCH])
            .output()
            .expect("git");
        assert!(
            check.status.success(),
            "{PATCH} no longer applies: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    // `--native-only`, while tuning: the scrolls are still judged, and what has nothing to
    // be compared with is shown.
    #[test]
    fn without_a_svelte_report_only_the_native_lines_are_judged() {
        let lines = judge(&report(16.7, 80.0, 300.0), None, 60.0);
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        assert_eq!(outcome(&lines, "memory"), Outcome::Shown);
        assert!(table(&lines).contains("300 MB"));
    }
}
````

Create `crates/xtask/gate/svelte-probe.patch`:

````diff
diff --git i/crates/photon-app/src/app.rs w/crates/photon-app/src/app.rs
index abdbd95..512150b 100644
--- i/crates/photon-app/src/app.rs
+++ w/crates/photon-app/src/app.rs
@@ -306,6 +306,8 @@ pub fn run() {
             ipc::reveal_watched,
             ipc::reveal_library,
             ipc::media_base,
+            ipc::probe_config,
+            ipc::probe_done,
             ipc::video_session_start,
             ipc::next_video_job,
             ipc::put_video_frame,
diff --git i/crates/photon-app/src/ipc.rs w/crates/photon-app/src/ipc.rs
index 791a864..1781c81 100644
--- i/crates/photon-app/src/ipc.rs
+++ w/crates/photon-app/src/ipc.rs
@@ -700,6 +700,37 @@ pub fn video_frame_failed(
     commands::video_frame_failed(&engine, id, &key, reason)
 }
 
+// THE GATE'S PROBE (`ui/src/lib/probe.ts`): where its report goes and when the harness
+// started this process, and the end of the run. Applied by `xtask grid-gate` to a throwaway
+// worktree; never merged.
+#[derive(serde::Serialize)]
+pub struct ProbeConfig {
+    out: String,
+    t0: Option<f64>,
+}
+
+#[tauri::command(async)]
+pub fn probe_config() -> Option<ProbeConfig> {
+    let out = std::env::var("PHOTON_PROBE_OUT").ok()?;
+    let t0 = std::env::var("PHOTON_PROBE_T0")
+        .ok()
+        .and_then(|t0| t0.parse().ok());
+    Some(ProbeConfig { out, t0 })
+}
+
+#[tauri::command(async)]
+pub fn probe_done(app: tauri::AppHandle, report: String) -> Result<(), AppError> {
+    let out = std::env::var("PHOTON_PROBE_OUT").map_err(AppError::internal)?;
+    let mut report: serde_json::Value =
+        serde_json::from_str(&report).map_err(AppError::internal)?;
+    // photon and its web view's processes, as Settings counts them.
+    report["memory_bytes"] = serde_json::json!(commands::memory_usage().ok().map(|m| m.bytes));
+    let text = serde_json::to_string_pretty(&report).map_err(AppError::internal)?;
+    std::fs::write(out, text).map_err(AppError::internal)?;
+    app.exit(0);
+    Ok(())
+}
+
 #[cfg(test)]
 mod tests {
     use super::*;
diff --git i/ui/src/components/Grid.svelte w/ui/src/components/Grid.svelte
index e576335..e3d30ad 100644
--- i/ui/src/components/Grid.svelte
+++ w/ui/src/components/Grid.svelte
@@ -16,6 +16,7 @@
   import { CAP_FALLBACK, capFrom, createScrollMap, PROBE_HEIGHT } from '../lib/scroll-map';
   import { createScrollSpeed } from '../lib/scroll-speed.svelte';
   import { move, type NavKey } from '../lib/nav';
+  import { runProbe } from '../lib/probe';
   import { keyHint } from '../lib/shortcuts';
   import { decidingPhoto } from '../lib/star-key';
   import { isMac } from '../lib/url';
@@ -300,6 +301,20 @@
     writeDom(map.setVirtual(v));
   }
 
+  // THE GATE'S PROBE (see lib/probe.ts): once, when the grid has its viewport. It does
+  // nothing unless the harness started this process with PHOTON_PROBE_OUT.
+  let probed = false;
+  $effect(() => {
+    if (probed || !viewport) return;
+    probed = true;
+    void runProbe({
+      scrollTo: scrollToVirtual,
+      max: () => Math.max(0, total - height),
+      viewport: () => viewport,
+      photos: () => library.info.len,
+    });
+  });
+
   /** How tall a box this engine will lay out, in CSS px, at this display scale. */
   function measureCap() {
     const probe = document.createElement('div');
diff --git i/ui/src/lib/probe.ts w/ui/src/lib/probe.ts
new file mode 100644
index 0000000..63ba82f
--- /dev/null
+++ w/ui/src/lib/probe.ts
@@ -0,0 +1,170 @@
+/** THE GATE'S PROBE. Applied by `cargo run -p xtask -- grid-gate` to a throwaway worktree
+ *  and never merged: it is how the Svelte grid runs the scroll programme the native grid
+ *  runs (`crates/photon-ui/src/probe.rs`), so the two can be laid side by side.
+ *
+ *  The programme, its constants and the report's shape are that file's, step for step. A
+ *  frame here is a `requestAnimationFrame` callback, and the intervals between callbacks
+ *  are the cadence the compositor allowed. Every move is a place worked out from the time,
+ *  never a distance per frame.
+ *
+ *  One thing differs and cannot be otherwise: `requestAnimationFrame` itself asks for every
+ *  frame, so "frames drawn while idle" has no meaning here and is reported as null. */
+
+import { invoke } from '@tauri-apps/api/core';
+import { getCurrentWindow } from '@tauri-apps/api/window';
+
+const STEADY_PX_PER_S = 3_000;
+const STEADY_SECS = 10;
+const FAST_PX_PER_S = 30_000;
+const FAST_SECS = 5;
+const SWEEP_SECS = 5;
+const STILL_SECS = 1;
+const IDLE_SECS = 5;
+const CALM_SECS = 1;
+const SETTLE_TIMEOUT_SECS = 30;
+
+export interface ProbeHooks {
+  /** Puts the grid at a position of its layout. */
+  scrollTo(position: number): void;
+  /** The furthest the grid scrolls. */
+  max(): number;
+  viewport(): HTMLElement | undefined;
+  photos(): number;
+}
+
+interface Cadence {
+  frames: number;
+  median_ms: number;
+  p95_ms: number;
+  p99_ms: number;
+  longest_ms: number;
+}
+
+/** The value at `quantile` of `sorted`, by the nearest rank. */
+function at(sorted: number[], quantile: number): number {
+  const rank = Math.ceil(quantile * sorted.length);
+  return sorted[Math.min(Math.max(rank, 1), sorted.length) - 1];
+}
+
+export function cadenceOf(intervals: number[]): Cadence {
+  if (intervals.length === 0) return { frames: 0, median_ms: 0, p95_ms: 0, p99_ms: 0, longest_ms: 0 };
+  const sorted = [...intervals].sort((a, b) => a - b);
+  return {
+    frames: sorted.length,
+    median_ms: at(sorted, 0.5),
+    p95_ms: at(sorted, 0.95),
+    p99_ms: at(sorted, 0.99),
+    longest_ms: sorted[sorted.length - 1],
+  };
+}
+
+/** Whether every tile in view has its picture, or the mark that it will not. Asked only
+ *  by the steps that wait for it: it measures every tile on screen, which during a scroll
+ *  would be the probe slowing down what it is timing. */
+function settled(viewport: HTMLElement): boolean {
+  const box = viewport.getBoundingClientRect();
+  let seen = 0;
+  for (const tile of viewport.querySelectorAll<HTMLElement>('.tile')) {
+    const rect = tile.getBoundingClientRect();
+    if (rect.bottom <= box.top || rect.top >= box.bottom) continue;
+    seen++;
+    if (!tile.querySelector('img.loaded') && !tile.querySelector('.broken')) return false;
+  }
+  return seen > 0;
+}
+
+const frame = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));
+const epoch = () => performance.timeOrigin + performance.now();
+
+export async function runProbe(hooks: ProbeHooks): Promise<void> {
+  const config = await invoke<{ out: string; t0: number | null } | null>('probe_config');
+  if (!config) return;
+  await getCurrentWindow().setFullscreen(true);
+
+  const viewport = (): HTMLElement => {
+    const element = hooks.viewport();
+    if (!element) throw new Error('the grid has no viewport');
+    return element;
+  };
+  let position = 0;
+  const to = (next: number) => {
+    position = next;
+    hooks.scrollTo(next);
+  };
+
+  /** Frames until the grid shows its pictures: how long that took, or null after the
+   *  timeout. The frame that made the move is not the one that shows them. */
+  const untilSettled = async (): Promise<number | null> => {
+    const since = await frame();
+    for (;;) {
+      const now = await frame();
+      if (settled(viewport())) return now - since;
+      if (now - since >= SETTLE_TIMEOUT_SECS * 1000) return null;
+    }
+  };
+
+  /** A step that moves the grid for `secs`, to wherever `place(elapsed ms)` says. */
+  const moving = async (secs: number, place: (elapsed: number) => number): Promise<Cadence> => {
+    const intervals: number[] = [];
+    const since = await frame();
+    let last = since;
+    for (;;) {
+      const now = await frame();
+      intervals.push(now - last);
+      last = now;
+      const elapsed = now - since;
+      if (elapsed >= secs * 1000) return cadenceOf(intervals);
+      to(Math.min(place(elapsed), hooks.max()));
+    }
+  };
+
+  // Launch: until the grid has photos and shows them.
+  for (;;) {
+    await frame();
+    if (hooks.photos() > 0 && hooks.viewport() && settled(viewport())) break;
+  }
+  const launch_ms = config.t0 === null ? null : epoch() - config.t0;
+  const photos = hooks.photos();
+  // From the top, whatever the launch showed: this grid goes back to the folder it was
+  // last left in, and a second run over the same fixture was left at the far end.
+  to(0);
+  await untilSettled();
+  await new Promise((resolve) => setTimeout(resolve, CALM_SECS * 1000));
+
+  const from = position;
+  const steady = await moving(STEADY_SECS, (elapsed) => from + (STEADY_PX_PER_S * elapsed) / 1000);
+  const fastFrom = position;
+  const fast = await moving(FAST_SECS, (elapsed) => fastFrom + (FAST_PX_PER_S * elapsed) / 1000);
+
+  to(hooks.max());
+  const jump_end_ms = await untilSettled();
+  to(hooks.max() / 2);
+  const jump_middle_ms = await untilSettled();
+
+  to(0);
+  await untilSettled();
+  const sweep = await moving(SWEEP_SECS, (elapsed) => (hooks.max() * elapsed) / (SWEEP_SECS * 1000));
+  to(hooks.max());
+  const sweep_settle_ms = await untilSettled();
+
+  // The rest before the idle step, and the step: nothing is counted in either here, but
+  // the memory is read at the same point of both programmes.
+  await new Promise((resolve) => setTimeout(resolve, (STILL_SECS + IDLE_SECS) * 1000));
+
+  const report = {
+    app: 'svelte',
+    window: [window.innerWidth, window.innerHeight],
+    scale: window.devicePixelRatio,
+    adapter: null,
+    photos,
+    launch_ms,
+    steady,
+    fast,
+    jump_end_ms,
+    jump_middle_ms,
+    sweep,
+    sweep_settle_ms,
+    idle_frames: null,
+  };
+  await invoke('probe_done', { report: JSON.stringify(report) });
+}
````

Change `crates/xtask/src/main.rs`:

````diff
--- a/crates/xtask/src/main.rs
+++ b/crates/xtask/src/main.rs
@@ -7,9 +7,11 @@
 //!   cargo run -p xtask -- scroll-probe [--no-build]
 //!   cargo run -p xtask -- native-shot
 //!   cargo run -p xtask -- fixture-library [--photos <n>] [--out <dir>]
+//!   cargo run -p xtask -- grid-gate (--go --refresh-hz <hz> | --dry-run) [--native-only] [--photos <n>]
 
 mod checks;
 mod fixture_library;
+mod grid_gate;
 mod native_shot;
 mod screenshots;
 mod scroll_probe;
@@ -31,9 +33,10 @@
         Some("scroll-probe") => scroll_probe::run(&repo_root(), &args),
         Some("native-shot") => native_shot::run(&repo_root()),
         Some("fixture-library") => fixture_library::run(&repo_root(), &args),
+        Some("grid-gate") => grid_gate::run(&repo_root(), &args),
         other => {
             eprintln!(
-                "unknown command {other:?}; expected `versions`, `metadata`, `screenshots`, `scroll-probe`, `native-shot` or `fixture-library`"
+                "unknown command {other:?}; expected `versions`, `metadata`, `screenshots`, `scroll-probe`, `native-shot`, `fixture-library` or `grid-gate`"
             );
             ExitCode::FAILURE
         }
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p xtask grid_gate`
Expected: 9 pass, `the_svelte_probe_still_applies` among them. That one asks git whether the patch applies to the *index*, so stage the patch file's own commit normally; an unstaged edit to `Grid.svelte` does not affect it.

Run: `cargo run -p xtask -- grid-gate`
Expected: exit 2 and "say which: --go opens two fullscreen windows for about two minutes; --dry-run builds everything and launches nothing".

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `xtask/src/grid_gate.rs` | `Some(p95) if p95 <= refresh_ms * 1.5 => (` | `Some(p95) if p95 <= refresh_ms * 3.0 => (` | `a_frame_in_twenty_that_misses_its_refresh_fails_the_scroll` (`cargo test -p xtask`) |
| 2 | `xtask/src/grid_gate.rs` | `(Some(ours), Some(theirs)) if ours <= theirs * (1.0 + TIE) + grace => {` | `(Some(ours), Some(theirs)) if ours <= theirs * (2.0 + TIE) + grace => {` | `worse_than_the_svelte_grid_fails_and_within_a_few_percent_is_a_tie` (`cargo test -p xtask`) |
| 3 | `xtask/src/grid_gate.rs` | `if options.go == options.dry_run {` | `if options.go && options.dry_run {` | `nothing_is_launched_without_being_told_to` (`cargo test -p xtask`) |
| 4 | `xtask/src/grid_gate.rs` | `if options.go && options.refresh_hz.is_none() {` | `if false && options.refresh_hz.is_none() {` | `a_run_needs_the_screens_refresh_rate` (`cargo test -p xtask`) |
| 5 | `xtask/src/grid_gate.rs` | `outcome: if idle == Some(0.0) {` | `outcome: if idle.is_some() {` | `a_grid_that_draws_while_idle_fails` (`cargo test -p xtask`) |
| 6 | `xtask/src/grid_gate.rs` | `(None, _) => ( ⏎                 Outcome::Fail, ⏎                 "the native grid did not get there".to_owned(),` | `(None, _) => ( ⏎                 Outcome::Shown, ⏎                 "the native grid did not get there".to_owned(),` | `a_jump_that_never_showed_its_pictures_fails` (`cargo test -p xtask`) |

And the patch's tripwire, by hand: in `crates/xtask/gate/svelte-probe.patch` change the context line `   import { move, type NavKey } from '../lib/nav';` to end in `'../lib/navigation';`. `the_svelte_probe_still_applies` must fail with "patch failed: ui/src/components/Grid.svelte". Restore the line exactly.

- [ ] **Step 4: The patch typechecks**

```bash
git apply crates/xtask/gate/svelte-probe.patch
npm run check
git apply -R crates/xtask/gate/svelte-probe.patch
git status --short ui crates/photon-app
```

Expected: `0 ERRORS 0 WARNINGS`, and after the reverse nothing listed under `ui` or `crates/photon-app`.

- [ ] **Step 5: Build everything, launch nothing**

```bash
cargo run -p xtask -- grid-gate --dry-run
```

Expected, after some minutes the first time: `== built; --dry-run launches nothing`, with `target/release/photon-native` and `target/gate-svelte-target/release/photon` both there. It makes the worktree `target/gate-svelte` (listed by `git worktree list`; leave it, the gate reuses it for the same commit).

**Do not run `--go`.**

- [ ] **Step 6: Both gates, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check && npm test
git add crates/xtask/src/grid_gate.rs crates/xtask/src/main.rs crates/xtask/gate/svelte-probe.patch
git commit -m "feat(xtask): grid-gate, the native grid against the Svelte grid

Builds the fixture, photon-native and a Svelte photon with the programme patched
in (a throwaway worktree, built apart; the patch is never merged), runs the two
one after the other over the same library, and judges the two reports line by
line. It launches nothing without --go, and --go needs the screen's refresh rate.

The native grid runs first and the library is read into the page cache before
each run, so what is cold counts against the grid that has to pass.

Six probes of the judging and the arguments; a test that fails when the patch no
longer applies. Building and running two applications has no test: --dry-run was
run, --go was not.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---
### Task 5: Say what is here

**Files:**
- Modify: `CLAUDE.md`, `docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md`

- [ ] **Step 1: CLAUDE.md**

Change `CLAUDE.md`:

````diff
--- a/CLAUDE.md
+++ b/CLAUDE.md
@@ -1350,6 +1350,29 @@
 at every launch. A screenshot sets its theme twice, stored in the library and given to the
 `egui_kittest` harness, because the harness sets one of its own (dark) after the
 application has been made.
+
+**`cargo run -p xtask -- grid-gate` is the one command that launches the application**, and
+it does nothing unless told which: `--go --refresh-hz <hz>` opens two fullscreen windows,
+one after the other, for about two minutes on a desktop that has to stay awake; `--dry-run`
+builds everything and launches nothing. It is run by a person, or on their word at the
+time, because the interval between two frames does not exist without a compositor. It
+measures `photon-native --probe` (the scroll programme, `probe.rs`, a state module) against
+the Svelte grid, which is given the same programme by `crates/xtask/gate/svelte-probe.patch`
+in a throwaway worktree (`target/gate-svelte`) and never in `ui/`;
+`the_svelte_probe_still_applies` fails when `Grid.svelte` or `ipc.rs` has moved from under
+the patch. Both run over `target/gate-fixture` (`fixture-library`: 300,000 rows on a drive
+recorded as unplugged, every thumbnail cached, so the engine has nothing to do at launch).
+A fixture is not rebuilt when its builder changes: delete the directory.
+
+**A test that counts frames drives the application as an event loop does**
+(`the_programme_runs_over_a_fixture_to_its_report`): a frame only when egui's repaint
+callback asked for one, on a clock that jumps over the time nothing did. Stepping
+`egui_kittest` draws a frame at every step, asked for or not, and hid three things the
+gate's first count of idle frames got wrong: egui draws a second frame after every frame
+asked for at once, a frame asked for after a delay comes one frame early, and a scroll
+leaves two frames of its own behind (`SCROLL_SETTLE_MS`, `VISIBLE_DEBOUNCE_MS`). Each would
+have failed the gate on a perfectly still grid. Anything that asserts a still interface
+draws nothing is tested through that loop.
 
 ## Conventions
 
````

- [ ] **Step 2: The spec**

Change `docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md`:

````diff
--- a/docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md
+++ b/docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md
@@ -17,6 +17,22 @@
 is the folder list; a thumbnail that is not there yet marks its tile and counts as settled,
 and a frame is requested for its retry; a built thumbnail is handed back only under its own
 key; a right-to-left run is cut at everything that is not a letter, with brackets mirrored.
+
+**Changed by the gate's plan** (2026-10-09), each found while the gate was built and driven
+without a window: the Svelte grid measured is the one on `native-ui` itself, patched in a
+throwaway worktree, not the commit the branch began from (main is merged in as it moves, so
+it is the newer of the two); "time to picture" is the time from a jump, or from the end of
+the sweep, to the first frame in which every tile in view has its picture, not a time per
+tile; the screen's refresh rate is given by whoever runs the gate (`--refresh-hz`) and not
+read; the native grid is run first and the library is read into the page cache before each
+run, so that what is cold counts against the grid that has to pass; the pass lines are
+numbers this spec did not give - a scroll passes with its 95th-percentile interval at or
+under one and a half refreshes (a missed refresh is two), and "no worse" is within 5%, plus
+one refresh on a time; and step 6 counts the frames the grid draws *by itself* - there is a
+second of rest before it, the frame that ends it is not counted, and neither is a frame the
+engine's own report asked for (it looks for the fixture's unplugged drive twice a minute).
+The fixture's builder lives in `photon-ui` (`fixture.rs`, run as an example), so xtask goes
+on depending on neither crate.
 
 ## What this is for
 
````

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md
git commit -m "docs: the gate, and what its plan decided that the spec had not

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

## After the last task

The gate is run by the user:

```bash
cargo run -p xtask -- grid-gate --go --refresh-hz <the screen's refresh rate>
```

on a desktop that is awake and unlocked, with nothing else fullscreen. About two minutes. The table it prints, with the fixture's command and the tuned values, is what the spec asks the pull request to carry; the pull request this plan ends in carries the code and says the run is owed.
