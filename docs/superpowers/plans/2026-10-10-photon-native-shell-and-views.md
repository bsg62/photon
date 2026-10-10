# The native shell and the views - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `photon-native` gets the window around its grid - a top bar, a sidebar with its splitter, a status bar - and the sidebar's first rows take the user to every view a library has without a list of its own: All photos, Starred, Recent, On this day, Videos, Duplicates, Hidden.

**Architecture:** *State modules* hold what is decided and are tested without egui: an ordered task queue for the steps that move the engine's view (`tasks::Queue`), where the user is and is going (`nav.rs`), the window's four areas and what is stored about them (`window_layout.rs`), the sidebar's rows (`sidebar/rows.rs`), what is said where there are no photos (`empty.rs`), and messages that go away (`toasts.rs`). *Views* draw them and answer what the user did as a list of actions (`shell.rs`, `sidebar/view.rs`); the application is the one place an action changes anything (`app.rs`).

**Tech Stack:** Rust 2024, `eframe`/`egui` 0.36.2, `egui_kittest`, `serde_json`, `jiff`, `photon-engine`'s `commands`.

**Spec:** `docs/superpowers/specs/2026-10-10-photon-native-sidebar-views-search-design.md` - this is the first of its five pull requests ("The shell and the views"). The umbrella is `2026-10-09-photon-native-ui-design.md`.

## How this plan was written

As the two plans of sub-project 1 were: every file below was **written, compiled, run and probed before the plan was**, in a scratch worktree at `native-ui` (4c7dd0e), on 2026-10-10. There the Rust gate was green, every probe listed failed as stated, and the window was rendered off screen and looked at (`native-main-light`, `native-main-dark`, `native-starred-empty-light`, `native-sidebar-hidden-dark`).

So the tasks are: write the file, run its tests, then **prove each rule by its probe** - an exact replacement that breaks the rule, after which the named test must fail. A probe that passes is a finding: stop and report it. (Three did while this was written. A toast test dismissed only the oldest message, so "takes no other with it" was held by nothing; a test named for the wrong thing was the only one a rule's probe ran; and nothing showed that a click on the view already shown asks the engine for nothing. Each changed a test.)

**After every probe: put the original text back, then `touch` the file.**

## What the scratch build found

- **egui decides for itself whether a key going down is a repeat**, whatever the event says: it is one when the key has not come up since it last went down. A test that sends `repeat: true` tests nothing.
- **A new `egui::Area` is measured in a frame it does not show** and shown in the next, so a toast is on screen from its second frame.
- **A press becomes a drag only after the pointer has moved.** Where the splitter was taken hold of is where the button went down (`press_origin`), not where the pointer is when `drag_started` says so: taken there, the edge trailed the pointer by the distance it had moved.
- **A click on a row can be made by its name** (`harness.get_by_label("Starred").click()`) once the row tells AccessKit what it is, which costs one line a widget.

## Global Constraints

- Read `CLAUDE.md` before starting, the section "The native UI" above all. Its rules bind every task.
- **Branch `native-ui-shell`, off `native-ui`.** The pull request goes into `native-ui`.
- **Never launch the GUI.** Not `photon-native`, not `npm run dev`, not `grid-gate --go`. What is seen is seen through Task 8's PNGs.
- **The Rust gate before every commit:**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

- **One checkout, one build directory.** Do not point this branch's cargo at another checkout's `target` (CLAUDE.md, "Two checkouts do not share a `CARGO_TARGET_DIR`").
- **State modules name no egui type**, and each new one is added to `STATE_MODULES` in `lib.rs`: `nav.rs`, `window_layout.rs`, `sidebar/rows.rs`, `empty.rs`, `toasts.rs`.
- **Nothing that reads SQLite or the filesystem runs on the UI thread**, except before the first frame in `App::new`. A step that moves the engine's view goes through `tasks::Queue`; the counts and the layout's file through a `Latest`.
- **English throughout**: numbers and dates in English form (the user's decision of 2026-10-10). Every string is the Svelte UI's, but for one: "photon could not change the view.", for a step that panicked, which the Svelte UI has no counterpart of.
- **What needs a later sub-project is drawn and takes no press.** In this plan that is the gear.
- `ui/`, `photon-app`, `photon-core` and `photon-engine` are not changed.
- Comments carry the reasoning, in the surrounding code's density and voice. No em dashes; the codebase uses " - ".
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

What the spec implies and no test here exercises, most likely to bite first:

1. **The row that is marked.** `fixed_rows` is given `nav.target()`, so the row follows the click and not the rebuild. The whole-application tests read `App::place()`, which is the same call, not the row drawn: a reviewer checks the one line in `App::ui` that hands it over.
2. **A step the engine refuses.** `Nav::answered` and the toasts are tested; the three lines in `App::take_answers` that turn a refusal into a toast are not, because nothing in a test can make the engine refuse a view. Read them.
3. **A toast's last frame.** `Toasts::tick` says when the next message ends and `App::ui` asks for a frame then; without it a message would stay on a still window. The frame tests draw every frame, so they cannot see it missing.
4. **Hovering.** A row's hint is egui's tooltip, and a tooltip asks for frames while the pointer rests. `a_still_window_draws_nothing` has no pointer over the window. Whether a pointer resting on the sidebar draws frames for ever is for the smoke checklist.
5. **The look.** Colours, sizes and the focus on the splitter are judged from four pictures, by whoever builds them.

None of the five can be held by a test without a window or a seam the engine does not have; 1 and 2 are settled by reading.

## What this plan leaves to the spec's other pull requests

- The sidebar's lists, the mark of the folder at the top, the jump to a folder, the last folder, the folds: the second. `Layout::open` is stored here and first drawn there.
- The search box and its help, and with them `keys.rs`, the list of the keys answered: the third. This plan answers one key, Ctrl+B.
- The sort, grouping and size controls and the year strip: the fourth.
- The status bar's lines and the panel of an empty library: the fifth. Until then an empty library in All photos or Recent shows nothing where its photos would be, as it does today.

## File Structure

```
crates/photon-ui/
  src/tasks.rs               + Queue: every job, in order (beside Latest)
  src/nav.rs                 NEW  where the user is and is going; view_key
  src/window_layout.rs       NEW  the four areas; Layout and its file
  src/dirs.rs                + layout_path
  src/sidebar/rows.rs        NEW  the sidebar's first rows
  src/sidebar/view.rs        NEW  ...drawn
  src/empty.rs               NEW  what is said where there are no photos
  src/toasts.rs              NEW  messages that go away
  src/icons.rs               + seven icons
  src/theme/tokens.rs        + the shadow under a message
  src/shell.rs               NEW  the top bar, the splitter, the status bar, toasts
  src/grid/view.rs           + to_top; the keys left to a text field
  src/app.rs                 the shell around the grid; the queue, the counts, the layout
  src/lib.rs                 the modules, and five more state modules
  tests/common/mod.rs        NEW  Driver: the application as an event loop runs it
  tests/shell.rs             NEW  the shell and the views through the whole application
  tests/screenshots.rs       four pictures in place of two
crates/xtask/src/native_shot.rs   its description
CLAUDE.md                    the shell, the queue, the layout's file, the Driver
```

---
### Task 1: Every job, in order

**Files:**
- Modify: `crates/photon-ui/src/tasks.rs`

**Interfaces:**
- Consumes: nothing of this plan's. `Panicked` and the module's style are `Latest`'s.
- Produces, in `photon_ui::tasks`: `Queue<J, R>` with `Queue::spawn(name: &str, work: impl Fn(J) -> R + Send + 'static, notify: impl Fn() + Send + 'static) -> Self`, `push(&mut self, job: J) -> u64`, `push_or_replace(&mut self, job: J, replaces: impl FnOnce(&J) -> bool) -> u64`, `answers(&mut self) -> Vec<(u64, Result<R, Panicked>)>`.

`Latest` answers only the latest question and would drop a step that another was asked behind. The view setters must all be made, in the order asked (the Svelte store's `viewChain`).

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/tasks.rs`:

````diff
--- a/crates/photon-ui/src/tasks.rs
+++ b/crates/photon-ui/src/tasks.rs
@@ -4,9 +4,13 @@
 //! it, since its answer is no longer wanted - and hands back only the answer to the latest
 //! question asked. This is the job `LibraryStore`'s generation counters did in the Svelte
 //! UI, where a late answer otherwise overwrote a newer one.
+//!
+//! And one `Queue` for what must not be dropped and must not be reordered: the steps that
+//! move the engine's view. That is the Svelte store's `viewChain`.
 
 use parking_lot::{Condvar, Mutex};
 use std::{
+    collections::VecDeque,
     panic::{AssertUnwindSafe, catch_unwind},
     sync::{
         Arc,
@@ -122,6 +126,121 @@
     }
 }
 
+struct Line<J> {
+    /// The jobs not yet started, oldest first, each with its number.
+    waiting: VecDeque<(u64, J)>,
+    closed: bool,
+}
+
+struct SharedLine<J> {
+    line: Mutex<Line<J>>,
+    wake: Condvar,
+}
+
+/// Every job, in the order it was given.
+///
+/// A step that moves the engine's view rebuilds the grid on the thread that makes it, so
+/// it is made here; and a step already begun cannot be taken back, so the one after it
+/// waits. Answered by the latest alone, a click on Starred straight after a search was
+/// sent would let the search land last, under a box the click had emptied.
+pub struct Queue<J, R> {
+    shared: Arc<SharedLine<J>>,
+    answers: Receiver<(u64, Result<R, Panicked>)>,
+    given: u64,
+}
+
+impl<J: Send + 'static, R: Send + 'static> Queue<J, R> {
+    /// Starts the worker. `work` does one job; `notify` is called after each answer is
+    /// ready, from the worker's thread, so the UI can ask to be drawn again.
+    pub fn spawn(
+        name: &str,
+        work: impl Fn(J) -> R + Send + 'static,
+        notify: impl Fn() + Send + 'static,
+    ) -> Self {
+        let shared = Arc::new(SharedLine {
+            line: Mutex::new(Line {
+                waiting: VecDeque::new(),
+                closed: false,
+            }),
+            wake: Condvar::new(),
+        });
+        let (tx, answers) = mpsc::channel();
+        let theirs = shared.clone();
+        thread::Builder::new()
+            .name(name.to_owned())
+            .spawn(move || {
+                loop {
+                    let (number, job) = {
+                        let mut line = theirs.line.lock();
+                        loop {
+                            if line.closed {
+                                return;
+                            }
+                            if let Some(next) = line.waiting.pop_front() {
+                                break next;
+                            }
+                            theirs.wake.wait(&mut line);
+                        }
+                    };
+                    // A job that panics fails its own answer and no other: a queue that
+                    // ended here would leave every later click unanswered for the session.
+                    let answer = catch_unwind(AssertUnwindSafe(|| work(job))).map_err(|_| Panicked);
+                    if tx.send((number, answer)).is_err() {
+                        return;
+                    }
+                    notify();
+                }
+            })
+            .expect("a worker thread could not be started");
+        Self {
+            shared,
+            answers,
+            given: 0,
+        }
+    }
+
+    /// Adds `job` to the end, and answers its number.
+    pub fn push(&mut self, job: J) -> u64 {
+        self.push_or_replace(job, |_| false)
+    }
+
+    /// Adds `job` to the end - or puts it in the place of the last job, keeping that job's
+    /// number, when that one has not started and `replaces` says it may go. Only the last:
+    /// with another job behind it, replacing it would have the two swap their order.
+    pub fn push_or_replace(&mut self, job: J, replaces: impl FnOnce(&J) -> bool) -> u64 {
+        let mut line = self.shared.line.lock();
+        if let Some((number, last)) = line.waiting.back_mut()
+            && replaces(last)
+        {
+            *last = job;
+            return *number;
+        }
+        self.given += 1;
+        line.waiting.push_back((self.given, job));
+        drop(line);
+        self.shared.wake.notify_one();
+        self.given
+    }
+
+    /// The answers that have come since this was last asked, oldest first, each with its
+    /// job's number.
+    pub fn answers(&mut self) -> Vec<(u64, Result<R, Panicked>)> {
+        self.answers.try_iter().collect()
+    }
+}
+
+/// As `Latest`: the worker ends at its next wait and is not joined. Jobs still waiting
+/// are not done: the window is closing, and each is a rebuild nobody will see.
+impl<J, R> Drop for Queue<J, R> {
+    fn drop(&mut self) {
+        let mut line = self.shared.line.lock();
+        line.closed = true;
+        line.waiting.clear();
+        drop(line);
+        self.shared.wake.notify_all();
+    }
+}
+
 #[cfg(test)]
 mod tests {
     use super::*;
@@ -224,4 +343,131 @@
         told.recv_timeout(Duration::from_secs(10)).unwrap();
         assert_eq!(task.answer(), Some(Ok(7)));
     }
-}
+
+    /// Waits for `count` answers of a queue.
+    fn answers_of<J: Send + 'static, R: Send + 'static>(
+        queue: &mut Queue<J, R>,
+        count: usize,
+    ) -> Vec<(u64, Result<R, Panicked>)> {
+        let deadline = Instant::now() + Duration::from_secs(10);
+        let mut all = Vec::new();
+        while all.len() < count {
+            all.extend(queue.answers());
+            assert!(Instant::now() < deadline, "no answer within ten seconds");
+            thread::sleep(Duration::from_millis(2));
+        }
+        all
+    }
+
+    /// A queue whose worker says when it has started a job and then waits to be let go.
+    fn gated_queue() -> (Queue<u32, u32>, Receiver<u32>, Sender<()>) {
+        let (started_tx, started) = channel();
+        let (go, gate) = channel::<()>();
+        let queue = Queue::spawn(
+            "test",
+            move |job: u32| {
+                started_tx.send(job).unwrap();
+                gate.recv().unwrap();
+                job * 10
+            },
+            || {},
+        );
+        (queue, started, go)
+    }
+
+    // The difference from `Latest`: three steps asked while the first is being made are
+    // all made, in the order asked.
+    #[test]
+    fn every_job_is_done_in_the_order_given() {
+        let (mut queue, started, go) = gated_queue();
+        assert_eq!(queue.push(1), 1);
+        assert_eq!(started.recv().unwrap(), 1);
+        assert_eq!(queue.push(2), 2);
+        assert_eq!(queue.push(3), 3);
+        for expected in [2, 3] {
+            go.send(()).unwrap();
+            assert_eq!(started.recv().unwrap(), expected);
+        }
+        go.send(()).unwrap();
+        assert_eq!(
+            answers_of(&mut queue, 3),
+            [(1, Ok(10)), (2, Ok(20)), (3, Ok(30))]
+        );
+    }
+
+    // A search typed further before it was sent: the later text goes in its place, under
+    // its number, and the rebuild nobody wants is never made.
+    #[test]
+    fn the_last_job_is_replaced_while_it_has_not_started() {
+        let (mut queue, started, go) = gated_queue();
+        queue.push(1);
+        assert_eq!(started.recv().unwrap(), 1);
+        assert_eq!(queue.push(2), 2);
+        assert_eq!(queue.push_or_replace(5, |last| *last == 2), 2);
+        go.send(()).unwrap();
+        assert_eq!(started.recv().unwrap(), 5, "2 was never started");
+        go.send(()).unwrap();
+        assert_eq!(answers_of(&mut queue, 2), [(1, Ok(10)), (2, Ok(50))]);
+    }
+
+    #[test]
+    fn a_job_is_not_replaced_once_it_has_started_or_when_it_may_not_be() {
+        let (mut queue, started, go) = gated_queue();
+        queue.push(1);
+        assert_eq!(started.recv().unwrap(), 1);
+        // 1 has started: there is nothing waiting to replace.
+        assert_eq!(queue.push_or_replace(2, |_| true), 2);
+        // 2 is waiting, and may not go.
+        assert_eq!(queue.push_or_replace(3, |_| false), 3);
+        for expected in [2, 3] {
+            go.send(()).unwrap();
+            assert_eq!(started.recv().unwrap(), expected);
+        }
+        go.send(()).unwrap();
+        assert_eq!(answers_of(&mut queue, 3).len(), 3);
+    }
+
+    // Only the last may be replaced. A search with a view switch asked behind it stays
+    // where it is: put in the switch's place, or taken out, the two would land the other
+    // way round.
+    #[test]
+    fn a_job_with_another_behind_it_is_not_replaced() {
+        let (mut queue, started, go) = gated_queue();
+        queue.push(1);
+        assert_eq!(started.recv().unwrap(), 1);
+        queue.push(2);
+        queue.push(7);
+        // "Replace a 2": the last job is the 7, so this is added.
+        assert_eq!(queue.push_or_replace(9, |last| *last == 2), 4);
+        for expected in [2, 7, 9] {
+            go.send(()).unwrap();
+            assert_eq!(started.recv().unwrap(), expected);
+        }
+        go.send(()).unwrap();
+        assert_eq!(answers_of(&mut queue, 4).len(), 4);
+    }
+
+    #[test]
+    fn a_queued_job_that_panics_fails_its_own_answer_and_the_next_one_runs() {
+        let mut queue = Queue::spawn(
+            "test",
+            |job: u32| {
+                assert!(job != 1, "the job this test breaks on purpose");
+                job
+            },
+            || {},
+        );
+        queue.push(1);
+        queue.push(2);
+        assert_eq!(answers_of(&mut queue, 2), [(1, Err(Panicked)), (2, Ok(2))]);
+    }
+
+    #[test]
+    fn the_ui_is_told_when_a_queued_job_is_done() {
+        let (told_tx, told) = channel();
+        let mut queue = Queue::spawn("test", |job: u32| job, move || told_tx.send(()).unwrap());
+        queue.push(7);
+        told.recv_timeout(Duration::from_secs(10)).unwrap();
+        assert_eq!(queue.answers(), [(1, Ok(7))]);
+    }
+}
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib tasks::`
Expected: 11 pass: `Latest`'s five and the queue's six.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/tasks.rs` | `if let Some(next) = line.waiting.pop_front() {` | `if let Some(next) = line.waiting.pop_back() {` | `every_job_is_done_in_the_order_given` (`cargo test -p photon-ui --lib`) |
| 2 | `src/tasks.rs` | `&& replaces(last)` | `&& replaces(last) ⏎             && false` | `the_last_job_is_replaced_while_it_has_not_started` (`cargo test -p photon-ui --lib`) |
| 3 | `src/tasks.rs` | `&& replaces(last)` | `&& (replaces(last) \|\| true)` | `a_job_is_not_replaced_once_it_has_started_or_when_it_may_not_be` (`cargo test -p photon-ui --lib`) |
| 4 | `src/tasks.rs` | `if let Some((number, last)) = line.waiting.back_mut()` | `if let Some((number, last)) = line.waiting.front_mut()` | `a_job_with_another_behind_it_is_not_replaced` (`cargo test -p photon-ui --lib`) |

Each of the 4 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/tasks.rs
git commit -m "feat(ui): a task queue that does every job, in order

The steps that move the engine's view rebuild the grid where they are made and
cannot be taken back once begun, so they are made one at a time and in the order
asked. Latest would drop one that another was asked behind.

The last job may be replaced while it has not started, and only the last: with
another behind it the two would swap. A job that panics fails its own answer.

4 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Where the user is

**Files:**
- Modify: `crates/photon-ui/src/lib.rs`
- Create: `crates/photon-ui/src/nav.rs`

**Interfaces:**
- Consumes: `photon_core::grid::GridView`, `photon_core::sort::{Sort, SortKey, Grouping}`.
- Produces, in `photon_ui::nav`: `Place { view: GridView, arg: String }` with `Place::of(view)` and `Place::search(query)`; `Step::{View(GridView), Search(String)}` with `leads_to(&self) -> Place`; `Nav::new(settled: Place, sort: Sort)`, `settled()`, `sort()`, `target() -> Place`, `busy()`, `wants(&Step) -> bool`, `asked(number: u64, step: Step)`, `answered(number: u64, refused: Option<String>) -> Option<String>`, `settle(place: Place, sort: Sort) -> bool`; `view_key`, `results_changed`.

`Step` gets its other kinds - an album, a person, a tag, a sort, a folder - in the pull requests that bring what asks for them.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -21,6 +21,7 @@
     pub mod visible;
 }
 pub mod icons;
+pub mod nav;
 pub mod probe;
 pub mod tasks;
 pub mod text;
@@ -39,9 +40,10 @@
 #[cfg(test)]
 mod tests {
     /// The state modules, with their source.
-    const STATE_MODULES: [(&str, &str); 12] = [
+    const STATE_MODULES: [(&str, &str); 13] = [
         ("args.rs", include_str!("args.rs")),
         ("dirs.rs", include_str!("dirs.rs")),
+        ("nav.rs", include_str!("nav.rs")),
         ("tasks.rs", include_str!("tasks.rs")),
         ("theme/tokens.rs", include_str!("theme/tokens.rs")),
         ("grid/labels.rs", include_str!("grid/labels.rs")),
````

Create `crates/photon-ui/src/nav.rs`:

````rust
//! Where the user is in the library, and where they have asked to be.
//!
//! The engine holds one view and one sort, and a step that changes either rebuilds the grid
//! on the thread that makes it. Steps are therefore made on `tasks::Queue`, in the order
//! asked, and this module holds what the interface knows meanwhile: the place the published
//! grid shows, and the steps asked that have not landed. It is the part of the Svelte UI's
//! `LibraryStore` that `switchView`, `viewChain` and `settledView` were.
//!
//! No egui here, and no engine: the application reads the engine's view when a grid is
//! published and tells this module.

use photon_core::{
    grid::GridView,
    sort::{Grouping, Sort, SortKey},
};
use std::collections::VecDeque;

/// A view, and the argument that selects within it: a search's query, an album's id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub view: GridView,
    pub arg: String,
}

impl Place {
    /// A view that takes no argument.
    pub fn of(view: GridView) -> Self {
        Self {
            view,
            arg: String::new(),
        }
    }

    /// The search for `query`. A blank one is All photos, which is what the engine makes of
    /// it (`Engine::set_search_query`).
    pub fn search(query: &str) -> Self {
        if query.trim().is_empty() {
            Self::of(GridView::All)
        } else {
            Self {
                view: GridView::Search,
                arg: query.to_owned(),
            }
        }
    }
}

/// One thing asked of the engine's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// A view that takes no argument: All photos, Starred, Recent, Videos, Duplicates, Hidden.
    View(GridView),
    Search(String),
}

impl Step {
    /// Where the grid is once this step has landed.
    pub fn leads_to(&self) -> Place {
        match self {
            Step::View(view) => Place::of(*view),
            Step::Search(query) => Place::search(query),
        }
    }
}

pub struct Nav {
    /// What the published grid shows.
    settled: Place,
    sort: Sort,
    /// The steps asked and not yet answered, oldest first, each with its number.
    asked: VecDeque<(u64, Step)>,
}

impl Nav {
    pub fn new(settled: Place, sort: Sort) -> Self {
        Self {
            settled,
            sort,
            asked: VecDeque::new(),
        }
    }

    /// What the published grid shows.
    pub fn settled(&self) -> &Place {
        &self.settled
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    /// Where the user is, as they see it: where the last step asked leads, and the settled
    /// place when none is on its way. The sidebar marks this row, so a click is answered
    /// at once and not when a rebuild of the whole library has landed.
    pub fn target(&self) -> Place {
        self.asked
            .back()
            .map_or_else(|| self.settled.clone(), |(_, step)| step.leads_to())
    }

    /// Whether a step is on its way.
    pub fn busy(&self) -> bool {
        !self.asked.is_empty()
    }

    /// Whether `step` leads anywhere the user is not already going. A click on the row of
    /// the view that is shown asks for nothing: All photos most of all, where the click
    /// means "back to where I was" and the user is there.
    pub fn wants(&self, step: &Step) -> bool {
        step.leads_to() != self.target()
    }

    /// `step` was given to the queue under `number`. A number already held is a step the
    /// queue put in another's place (`Queue::push_or_replace`).
    pub fn asked(&mut self, number: u64, step: Step) {
        match self.asked.back_mut() {
            Some((last, held)) if *last == number => *held = step,
            _ => self.asked.push_back((number, step)),
        }
    }

    /// The queue's answer to step `number`: what the engine said when it refused it, or
    /// nothing when it was made. Answers what to tell the user.
    ///
    /// A refused step changes nothing but that: the engine put its own state back
    /// (`rebuild_or_restore`), and the place this holds is the published grid's, which
    /// never moved.
    pub fn answered(&mut self, number: u64, refused: Option<String>) -> Option<String> {
        // Answers come in the order the steps were given, so everything up to this one
        // is done with.
        while self
            .asked
            .front()
            .is_some_and(|(asked, _)| *asked <= number)
        {
            self.asked.pop_front();
        }
        refused
    }

    /// The grid now published shows `place` under `sort`. Answers whether those are other
    /// results than before, in which case a position in the old ones means nothing.
    pub fn settle(&mut self, place: Place, sort: Sort) -> bool {
        let before = view_key(&self.settled, self.sort);
        let changed = results_changed(&before, &view_key(&place, sort));
        self.settled = place;
        self.sort = sort;
        changed
    }
}

/// What `results_changed` compares: the view, its argument, and the order - the same
/// photos in another order are another list, and a scroll position in one is arbitrary in
/// the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewKey {
    place: Place,
    key: SortKey,
    reverse: bool,
    /// The grouping, only where it reorders the grid: by date, and not by folder, which
    /// is the view's own order. Under another key it is kept and ignored, and a change to
    /// it must not throw the position away.
    group: Option<Grouping>,
}

pub fn view_key(place: &Place, sort: Sort) -> ViewKey {
    let grouped = sort.key == SortKey::Date && sort.group != Grouping::Folder;
    ViewKey {
        place: place.clone(),
        key: sort.key,
        reverse: sort.reverse,
        group: grouped.then_some(sort.group),
    }
}

/// Whether the grid shows a different list of photos than it did. Keyed on the view alone
/// it would miss a search refined within Search, or one album replacing another.
pub fn results_changed(before: &ViewKey, now: &ViewKey) -> bool {
    before != now
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> Nav {
        Nav::new(Place::of(GridView::All), Sort::default())
    }

    fn sorted(key: SortKey, group: Grouping) -> Sort {
        Sort {
            key,
            reverse: false,
            group,
        }
    }

    #[test]
    fn with_nothing_asked_the_user_is_where_the_grid_is() {
        let nav = nav();
        assert_eq!(nav.target(), Place::of(GridView::All));
        assert!(!nav.busy());
    }

    // A rebuild of a large library takes a moment, and the click must show before it
    // lands: the row marked is the one gone to.
    #[test]
    fn a_step_asked_is_where_the_user_is_going_before_it_lands() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        assert_eq!(nav.target(), Place::of(GridView::Starred));
        assert_eq!(nav.settled(), &Place::of(GridView::All));
        assert!(nav.busy());

        // A second click before the first has landed: the last asked is where they go.
        nav.asked(2, Step::View(GridView::Recent));
        assert_eq!(nav.target(), Place::of(GridView::Recent));
        assert_eq!(nav.answered(1, None), None);
        assert_eq!(nav.target(), Place::of(GridView::Recent));
        assert_eq!(nav.answered(2, None), None);
        assert!(!nav.busy());
    }

    #[test]
    fn a_step_to_where_the_user_already_is_asks_for_nothing() {
        let mut nav = nav();
        assert!(!nav.wants(&Step::View(GridView::All)));
        assert!(nav.wants(&Step::View(GridView::Starred)));
        // Asked and not yet landed: the user is going there, and a second click on the
        // same row is not a second rebuild.
        nav.asked(1, Step::View(GridView::Starred));
        assert!(!nav.wants(&Step::View(GridView::Starred)));
        assert!(nav.wants(&Step::View(GridView::All)));
        // The same search is not wanted twice, and another one is.
        nav.asked(2, Step::Search("lake".to_owned()));
        assert!(!nav.wants(&Step::Search("lake".to_owned())));
        assert!(nav.wants(&Step::Search("lake 2024".to_owned())));
    }

    #[test]
    fn a_refused_step_is_said_and_leaves_the_user_where_the_grid_is() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        let said = nav.answered(1, Some("the library is locked".to_owned()));
        assert_eq!(said.as_deref(), Some("the library is locked"));
        assert_eq!(nav.target(), Place::of(GridView::All));
        assert!(!nav.busy());
    }

    // Starred refused while Recent is still on its way: the user is going to Recent.
    #[test]
    fn a_refusal_does_not_take_back_a_later_step() {
        let mut nav = nav();
        nav.asked(1, Step::View(GridView::Starred));
        nav.asked(2, Step::View(GridView::Recent));
        assert!(nav.answered(1, Some("no".to_owned())).is_some());
        assert_eq!(nav.target(), Place::of(GridView::Recent));
    }

    #[test]
    fn a_step_the_queue_replaced_is_the_one_held() {
        let mut nav = nav();
        nav.asked(1, Step::Search("lak".to_owned()));
        // The same number: the queue put the later search in the first one's place.
        nav.asked(1, Step::Search("lake".to_owned()));
        assert_eq!(nav.target(), Place::search("lake"));
        nav.answered(1, None);
        assert!(!nav.busy());
    }

    #[test]
    fn a_blank_search_leads_to_all_photos() {
        assert_eq!(
            Step::Search("  ".to_owned()).leads_to(),
            Place::of(GridView::All)
        );
        assert_eq!(
            Step::Search("lake".to_owned()).leads_to(),
            Place {
                view: GridView::Search,
                arg: "lake".to_owned()
            }
        );
    }

    #[test]
    fn other_results_are_another_view_or_another_argument() {
        let mut nav = nav();
        assert!(nav.settle(Place::of(GridView::Starred), Sort::default()));
        // The same view published again - a star, a scan - is the same list.
        assert!(!nav.settle(Place::of(GridView::Starred), Sort::default()));
        assert!(nav.settle(Place::search("lake"), Sort::default()));
        // A search refined within Search.
        assert!(nav.settle(Place::search("lake 2024"), Sort::default()));
        assert!(!nav.settle(Place::search("lake 2024"), Sort::default()));
        assert_eq!(nav.settled(), &Place::search("lake 2024"));
    }

    #[test]
    fn a_new_sort_is_other_results_and_the_same_sort_published_again_is_not() {
        let mut nav = nav();
        let by_name = sorted(SortKey::Name, Grouping::Folder);
        assert!(nav.settle(Place::of(GridView::All), by_name));
        assert!(!nav.settle(Place::of(GridView::All), by_name));
        let reversed = Sort {
            reverse: true,
            ..by_name
        };
        assert!(nav.settle(Place::of(GridView::All), reversed));
        assert_eq!(nav.sort(), reversed);
    }

    // The grouping reorders the grid only by date. Under another key it is ignored, and a
    // change to it must not throw the scroll position away.
    #[test]
    fn a_new_grouping_is_other_results_by_date_and_nothing_under_another_key() {
        let all = Place::of(GridView::All);
        let key = |key, group| view_key(&all, sorted(key, group));
        assert!(results_changed(
            &key(SortKey::Date, Grouping::Folder),
            &key(SortKey::Date, Grouping::Month)
        ));
        assert!(results_changed(
            &key(SortKey::Date, Grouping::Month),
            &key(SortKey::Date, Grouping::None)
        ));
        assert!(!results_changed(
            &key(SortKey::Name, Grouping::Folder),
            &key(SortKey::Name, Grouping::Month)
        ));
        assert!(!results_changed(
            &key(SortKey::Size, Grouping::Day),
            &key(SortKey::Size, Grouping::None)
        ));
    }
}
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib nav::`
Expected: 10 pass.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/nav.rs` | `.map_or_else(\|\| self.settled.clone(), \|(_, step)\| step.leads_to())` | `.map_or_else(\|\| self.settled.clone(), \|_\| self.settled.clone())` | `a_step_asked_is_where_the_user_is_going_before_it_lands` (`cargo test -p photon-ui --lib`) |
| 2 | `src/nav.rs` | `self.asked ⏎             .back()` | `self.asked ⏎             .front()` | `a_step_asked_is_where_the_user_is_going_before_it_lands` (`cargo test -p photon-ui --lib`) |
| 3 | `src/nav.rs` | `.is_some_and(\|(asked, _)\| *asked <= number)` | `.is_some_and(\|(asked, _)\| *asked < number)` | `a_step_asked_is_where_the_user_is_going_before_it_lands` (`cargo test -p photon-ui --lib`) |
| 4 | `src/nav.rs` | `Some((last, held)) if *last == number => *held = step,` | `Some((last, _)) if *last == number => {}` | `a_step_the_queue_replaced_is_the_one_held` (`cargo test -p photon-ui --lib`) |
| 5 | `src/nav.rs` | `step.leads_to() != self.target()` | `step.leads_to() != self.settled` | `a_step_to_where_the_user_already_is_asks_for_nothing` (`cargo test -p photon-ui --lib`) |
| 6 | `src/nav.rs` | `let grouped = sort.key == SortKey::Date && sort.group != Grouping::Folder;` | `let grouped = sort.group != Grouping::Folder;` | `a_new_grouping_is_other_results_by_date_and_nothing_under_another_key` (`cargo test -p photon-ui --lib`) |
| 7 | `src/nav.rs` | `self.settled = place;` | *(nothing: the lines are taken out)* | `other_results_are_another_view_or_another_argument` (`cargo test -p photon-ui --lib`) |
| 8 | `src/nav.rs` | `self.sort = sort; ⏎         changed` | `changed` | `a_new_sort_is_other_results_and_the_same_sort_published_again_is_not` (`cargo test -p photon-ui --lib`) |
| 9 | `src/nav.rs` | `if query.trim().is_empty() {` | `if query.is_empty() {` | `a_blank_search_leads_to_all_photos` (`cargo test -p photon-ui --lib`) |

Each of the 9 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/lib.rs crates/photon-ui/src/nav.rs
git commit -m "feat(ui): where the user is in the library, and where they asked to be

The place the published grid shows, and the steps asked that have not landed.
The sidebar marks where the last step leads, so a click is answered at once and
not when a rebuild of the whole library has landed. A step to where the user is
already going is not asked.

9 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The window's areas, and what is kept about them

**Files:**
- Modify: `crates/photon-ui/src/lib.rs`
- Create: `crates/photon-ui/src/window_layout.rs`
- Modify: `crates/photon-ui/src/dirs.rs`

**Interfaces:**
- Consumes: nothing of this plan's.
- Produces, in `photon_ui::window_layout`: `SIDEBAR_DEFAULT`, `SIDEBAR_MIN`, `SIDEBAR_STEP`, `SPLITTER`, `TOP_BAR`, `STATUS_BAR` (`f32`); `clamp_sidebar_width(width, window) -> f32`; `OpenGroups { albums, searches, people, tags }`; `Layout { sidebar_width: f32, sidebar_hidden: bool, open: OpenGroups }` (`Copy`, `Default`) with `read(&str)`, `written()`, `load(&Path)`, `save(&Path)`, `shown_width(window)`; `Area { left, top, right, bottom }`; `Areas { top_bar, sidebar: Option<Area>, splitter: Option<Area>, content, status_bar }`; `areas(width, height, &Layout) -> Areas`. And `Dirs::layout_path`.

The numbers and the rules of the width and the groups are `ui/src/lib/sidebar.ts`'s. The open groups are stored here and first used by the second pull request.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -36,11 +36,12 @@
     pub mod source;
     pub mod textures;
 }
+pub mod window_layout;
 
 #[cfg(test)]
 mod tests {
     /// The state modules, with their source.
-    const STATE_MODULES: [(&str, &str); 13] = [
+    const STATE_MODULES: [(&str, &str); 14] = [
         ("args.rs", include_str!("args.rs")),
         ("dirs.rs", include_str!("dirs.rs")),
         ("nav.rs", include_str!("nav.rs")),
@@ -54,6 +55,7 @@
         ("probe.rs", include_str!("probe.rs")),
         ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
         ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
+        ("window_layout.rs", include_str!("window_layout.rs")),
     ];
 
     /// The version of the `windows` crate that `package` is locked to, or `None` when it
````

Create `crates/photon-ui/src/window_layout.rs`:

````rust
//! How the window is laid out: its four areas around the grid, and the three things the
//! user sets about them - the sidebar's width, whether it is hidden, which of its groups
//! are open.
//!
//! Those three are kept per machine, in `layout.json` beside `library.db`, and not in the
//! library's settings table: they are how this window is laid out here, not a fact about
//! the library, and that table's accessors are photon-core's, which the Tauri photon reads
//! too. The Svelte UI kept them in the web view's `localStorage` (`lib/sidebar.ts`), which
//! is where the numbers and the rules are from.
//!
//! No egui here: an area is four numbers.

use std::path::Path;

pub const SIDEBAR_DEFAULT: f32 = 260.0;
pub const SIDEBAR_MIN: f32 = 160.0;
/// What an arrow key moves the focused splitter by.
pub const SIDEBAR_STEP: f32 = 16.0;
pub const SPLITTER: f32 = 5.0;
/// The top bar: the search field's thirty points, eight above and below, and its line.
pub const TOP_BAR: f32 = 46.0;
pub const STATUS_BAR: f32 = 23.0;

/// Holds a width to between `SIDEBAR_MIN` and half the window. The minimum wins when the
/// window is too narrow for both, so the list never collapses to nothing.
pub fn clamp_sidebar_width(width: f32, window: f32) -> f32 {
    width.min(window / 2.0).max(SIDEBAR_MIN).round()
}

/// Which of the sidebar's groups are unfolded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenGroups {
    pub albums: bool,
    pub searches: bool,
    pub people: bool,
    pub tags: bool,
}

/// Albums and searches start open because they are the user's own; People and Tags start
/// closed because a real library has hundreds of each, and the years below must stay
/// reachable.
impl Default for OpenGroups {
    fn default() -> Self {
        Self {
            albums: true,
            searches: true,
            people: false,
            tags: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The width last dragged to. It may have been set in a wider window, so it is
    /// clamped where it is used (`shown_width`), never where it is read.
    pub sidebar_width: f32,
    pub sidebar_hidden: bool,
    pub open: OpenGroups,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_hidden: false,
            open: OpenGroups::default(),
        }
    }
}

impl Layout {
    /// The layout `text` holds, each thing that is not there, or not what it should be, at
    /// its default: a file from another version, or one cut short, must not cost the user
    /// the parts of it that can be read.
    pub fn read(text: &str) -> Self {
        let mut layout = Self::default();
        let Ok(stored) = serde_json::from_str::<serde_json::Value>(text) else {
            return layout;
        };
        if let Some(width) = stored["sidebarWidth"].as_f64()
            && width.is_finite()
            && width > 0.0
        {
            layout.sidebar_width = width as f32;
        }
        // Only a stored yes hides it: hidden, the way back is one small button and a key,
        // so anything else leaves it where a new user expects it.
        layout.sidebar_hidden = stored["sidebarHidden"] == true;
        let open = &stored["openGroups"];
        for (name, group) in [
            ("albums", &mut layout.open.albums),
            ("searches", &mut layout.open.searches),
            ("people", &mut layout.open.people),
            ("tags", &mut layout.open.tags),
        ] {
            if let Some(stored) = open[name].as_bool() {
                *group = stored;
            }
        }
        layout
    }

    pub fn written(&self) -> String {
        serde_json::json!({
            "sidebarWidth": self.sidebar_width,
            "sidebarHidden": self.sidebar_hidden,
            "openGroups": {
                "albums": self.open.albums,
                "searches": self.open.searches,
                "people": self.open.people,
                "tags": self.open.tags,
            },
        })
        .to_string()
    }

    /// The layout stored at `path`, or the defaults when there is none to be read.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map_or_else(|_| Self::default(), |text| Self::read(&text))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.written())
    }

    /// The width the sidebar is drawn at in a window `window` wide.
    pub fn shown_width(&self, window: f32) -> f32 {
        clamp_sidebar_width(self.sidebar_width, window)
    }
}

/// A rectangle of the window, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// The window's four areas. The sidebar and its splitter are not there when it is hidden:
/// the content then begins at the window's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    pub top_bar: Area,
    pub sidebar: Option<Area>,
    pub splitter: Option<Area>,
    pub content: Area,
    pub status_bar: Area,
}

pub fn areas(width: f32, height: f32, layout: &Layout) -> Areas {
    let across = |top: f32, bottom: f32| Area {
        left: 0.0,
        top,
        right: width,
        bottom,
    };
    let top = TOP_BAR.min(height);
    let bottom = (height - STATUS_BAR).max(top);
    let side = if layout.sidebar_hidden {
        0.0
    } else {
        layout.shown_width(width)
    };
    let column = |left: f32, right: f32| Area {
        left,
        top,
        right,
        bottom,
    };
    let shown = !layout.sidebar_hidden;
    Areas {
        top_bar: across(0.0, top),
        sidebar: shown.then(|| column(0.0, side)),
        splitter: shown.then(|| column(side, side + SPLITTER)),
        content: column(if shown { side + SPLITTER } else { 0.0 }, width),
        status_bar: across(bottom, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_is_held_between_the_minimum_and_half_the_window() {
        assert_eq!(clamp_sidebar_width(260.0, 1280.0), 260.0);
        assert_eq!(clamp_sidebar_width(900.0, 1280.0), 640.0);
        assert_eq!(clamp_sidebar_width(40.0, 1280.0), SIDEBAR_MIN);
        assert_eq!(clamp_sidebar_width(200.4, 1280.0), 200.0);
        // Too narrow for both: the minimum wins, so the list never collapses to nothing.
        assert_eq!(clamp_sidebar_width(260.0, 300.0), SIDEBAR_MIN);
    }

    #[test]
    fn a_layout_is_read_back_as_it_was_written() {
        let layout = Layout {
            sidebar_width: 312.0,
            sidebar_hidden: true,
            open: OpenGroups {
                albums: false,
                searches: true,
                people: true,
                tags: false,
            },
        };
        assert_eq!(Layout::read(&layout.written()), layout);
        assert_eq!(
            Layout::read(&Layout::default().written()),
            Layout::default()
        );
    }

    // A file from another version, a file cut short, a file someone edited: whatever of it
    // can be read is kept, and the rest is as a new user finds it.
    #[test]
    fn what_cannot_be_read_is_at_its_default() {
        assert_eq!(Layout::read(""), Layout::default());
        assert_eq!(Layout::read("{\"sidebarWidth\": 3"), Layout::default());
        assert_eq!(Layout::read("[1, 2]"), Layout::default());

        let partly = Layout::read(
            r#"{"sidebarWidth": "wide", "sidebarHidden": "yes",
                "openGroups": {"albums": false, "people": 1, "tags": true}}"#,
        );
        assert_eq!(partly.sidebar_width, SIDEBAR_DEFAULT);
        assert!(!partly.sidebar_hidden, "only a stored yes hides it");
        assert_eq!(
            partly.open,
            OpenGroups {
                albums: false,
                searches: true,
                people: false,
                tags: true,
            }
        );
        // A width nobody chose.
        assert_eq!(
            Layout::read(r#"{"sidebarWidth": 0}"#).sidebar_width,
            SIDEBAR_DEFAULT
        );
        assert_eq!(
            Layout::read(r#"{"sidebarWidth": -40}"#).sidebar_width,
            SIDEBAR_DEFAULT
        );
    }

    #[test]
    fn a_layout_that_is_not_stored_is_the_defaults_and_one_saved_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        assert_eq!(Layout::load(&path), Layout::default());
        let layout = Layout {
            sidebar_width: 200.0,
            ..Layout::default()
        };
        layout.save(&path).unwrap();
        assert_eq!(Layout::load(&path), layout);
    }

    // The width is stored as it was dragged, and clamped where it is shown: stored clamped,
    // a window narrowed for a moment would cost the user the width they chose.
    #[test]
    fn a_stored_width_is_clamped_to_the_window_it_is_shown_in() {
        let layout = Layout {
            sidebar_width: 500.0,
            ..Layout::default()
        };
        assert_eq!(layout.shown_width(1600.0), 500.0);
        assert_eq!(layout.shown_width(800.0), 400.0);
        assert_eq!(layout.sidebar_width, 500.0);
    }

    #[test]
    fn the_four_areas_fill_the_window() {
        let found = areas(1280.0, 800.0, &Layout::default());
        assert_eq!(
            found.top_bar,
            Area {
                left: 0.0,
                top: 0.0,
                right: 1280.0,
                bottom: TOP_BAR
            }
        );
        let sidebar = found.sidebar.unwrap();
        assert_eq!((sidebar.left, sidebar.right), (0.0, 260.0));
        assert_eq!((sidebar.top, sidebar.bottom), (TOP_BAR, 800.0 - STATUS_BAR));
        let splitter = found.splitter.unwrap();
        assert_eq!((splitter.left, splitter.right), (260.0, 265.0));
        assert_eq!(found.content.left, 265.0);
        assert_eq!(found.content.right, 1280.0);
        assert_eq!(found.content.height(), 800.0 - TOP_BAR - STATUS_BAR);
        assert_eq!(found.status_bar.top, 800.0 - STATUS_BAR);
        assert_eq!(found.status_bar.height(), STATUS_BAR);
    }

    #[test]
    fn a_hidden_sidebar_gives_its_room_to_the_content() {
        let hidden = Layout {
            sidebar_hidden: true,
            ..Layout::default()
        };
        let found = areas(1280.0, 800.0, &hidden);
        assert_eq!(found.sidebar, None);
        assert_eq!(found.splitter, None);
        assert_eq!(found.content.left, 0.0);
        assert_eq!(found.content.width(), 1280.0);
    }

    // A window being dragged very small must not hand out an area that ends before it
    // begins.
    #[test]
    fn no_area_is_inside_out_in_a_window_too_small_for_the_bars() {
        for (width, height) in [(200.0, 40.0), (0.0, 0.0), (120.0, 60.0)] {
            let found = areas(width, height, &Layout::default());
            for area in [found.top_bar, found.content, found.status_bar] {
                assert!(area.height() >= 0.0, "{area:?} in {width}x{height}");
            }
        }
    }
}
````

Change `crates/photon-ui/src/dirs.rs`:

````diff
--- a/crates/photon-ui/src/dirs.rs
+++ b/crates/photon-ui/src/dirs.rs
@@ -15,6 +15,9 @@
 pub struct Dirs {
     pub db_path: PathBuf,
     pub cache_dir: PathBuf,
+    /// Where the window's layout is kept (`window_layout.rs`): beside the library, and
+    /// this application's alone.
+    pub layout_path: PathBuf,
 }
 
 /// The paths inside an app data directory and an app cache directory. This is what
@@ -23,6 +26,7 @@
     Dirs {
         db_path: app_data.join("library.db"),
         cache_dir: app_cache.join("thumbs"),
+        layout_path: app_data.join("layout.json"),
     }
 }
 
@@ -43,6 +47,7 @@
         let dirs = within(Path::new("/data/app"), Path::new("/cache/app"));
         assert_eq!(dirs.db_path, Path::new("/data/app/library.db"));
         assert_eq!(dirs.cache_dir, Path::new("/cache/app/thumbs"));
+        assert_eq!(dirs.layout_path, Path::new("/data/app/layout.json"));
     }
 
     #[test]
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib window_layout:: && cargo test -p photon-ui --lib dirs::`
Expected: window_layout: 8 pass. dirs: all pass, with `layout_path` named.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/window_layout.rs` | `width.min(window / 2.0).max(SIDEBAR_MIN).round()` | `width.max(SIDEBAR_MIN).min(window / 2.0).round()` | `a_width_is_held_between_the_minimum_and_half_the_window` (`cargo test -p photon-ui --lib`) |
| 2 | `src/window_layout.rs` | `layout.sidebar_hidden = stored["sidebarHidden"] == true;` | `layout.sidebar_hidden = !stored["sidebarHidden"].is_null();` | `what_cannot_be_read_is_at_its_default` (`cargo test -p photon-ui --lib`) |
| 3 | `src/window_layout.rs` | `&& width > 0.0` | *(nothing: the lines are taken out)* | `what_cannot_be_read_is_at_its_default` (`cargo test -p photon-ui --lib`) |
| 4 | `src/window_layout.rs` | `if let Some(stored) = open[name].as_bool() { ⏎                 *group = stored; ⏎             }` | `*group = open[name].as_bool().unwrap_or(false);` | `what_cannot_be_read_is_at_its_default` (`cargo test -p photon-ui --lib`) |
| 5 | `src/window_layout.rs` | `clamp_sidebar_width(self.sidebar_width, window)` | `self.sidebar_width.min(window)` | `a_stored_width_is_clamped_to_the_window_it_is_shown_in` (`cargo test -p photon-ui --lib`) |
| 6 | `src/window_layout.rs` | `content: column(if shown { side + SPLITTER } else { 0.0 }, width),` | `content: column(side + SPLITTER, width),` | `a_hidden_sidebar_gives_its_room_to_the_content` (`cargo test -p photon-ui --lib`) |
| 7 | `src/window_layout.rs` | `let bottom = (height - STATUS_BAR).max(top);` | `let bottom = height - STATUS_BAR;` | `no_area_is_inside_out_in_a_window_too_small_for_the_bars` (`cargo test -p photon-ui --lib`) |
| 8 | `src/dirs.rs` | `layout_path: app_data.join("layout.json"),` | `layout_path: app_cache.join("layout.json"),` | `the_library_and_the_cache_are_named_as_the_tauri_shell_names_them` (`cargo test -p photon-ui --lib`) |

Each of the 8 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/lib.rs crates/photon-ui/src/window_layout.rs crates/photon-ui/src/dirs.rs
git commit -m "feat(ui): the window's four areas, and its layout kept beside the library

The sidebar's width, whether it is hidden and which groups are open are how this
window is laid out on this machine: in layout.json beside library.db, not in the
settings table, which the Tauri photon reads too. A file that cannot be read, or
a part of one, is the defaults. The width is stored as dragged and clamped where
it is shown.

8 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The sidebar's rows, and what is said

**Files:**
- Modify: `crates/photon-ui/src/lib.rs`
- Create: `crates/photon-ui/src/sidebar/rows.rs`
- Create: `crates/photon-ui/src/empty.rs`
- Create: `crates/photon-ui/src/toasts.rs`

**Interfaces:**
- Consumes: Task 2's `Place` and `Step`; `grid::labels::photo_count`; `photon_engine::engine::NOT_BUILT`.
- Produces:
  - `photon_ui::sidebar::rows`: `ROW: f32`; `Fixed::{All, Starred, Recent, OnThisDay, Videos, Duplicates, CopiesOf, Hidden}` (`Hash`) with `step(self, today: Today) -> Option<Step>`; `Row { what, label, count: Option<usize>, active, hint }`; `Counts { starred, duplicates, hidden, videos: usize, copies_of: Option<String> }`; `Today { month, day: u32 }` with `query()` and `label()`; `fixed_rows(&Counts, at: &Place, today) -> Vec<Row>`.
  - `photon_ui::empty`: `GridState { version: u64, len: usize, build_error: Option<String> }`; `grid_built`, `build_failure`, `show_empty_notice`, `photo_count(&GridState) -> Option<String>`, `view_notice(&GridState, &Place) -> Option<String>`.
  - `photon_ui::toasts`: `ERROR_MS`, `DONE_MS`; `Kind::{Error, Done}`; `Toast { id, message, kind, .. }`; `Toasts` (`Default`) with `error`, `done`, `dismiss(id)`, `tick(now_ms) -> Option<f64>`, `showing() -> &[Toast]`.

`view_notice` has the lines of the views this pull request can reach; an album's, a person's and a tag's come with their lists.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -8,6 +8,7 @@
 pub mod app;
 pub mod args;
 pub mod dirs;
+pub mod empty;
 pub mod events;
 pub mod fixture;
 pub mod grid {
@@ -23,6 +24,9 @@
 pub mod icons;
 pub mod nav;
 pub mod probe;
+pub mod sidebar {
+    pub mod rows;
+}
 pub mod tasks;
 pub mod text;
 pub mod theme {
@@ -36,15 +40,18 @@
     pub mod source;
     pub mod textures;
 }
+pub mod toasts;
 pub mod window_layout;
 
 #[cfg(test)]
 mod tests {
     /// The state modules, with their source.
-    const STATE_MODULES: [(&str, &str); 14] = [
+    const STATE_MODULES: [(&str, &str); 17] = [
         ("args.rs", include_str!("args.rs")),
         ("dirs.rs", include_str!("dirs.rs")),
+        ("empty.rs", include_str!("empty.rs")),
         ("nav.rs", include_str!("nav.rs")),
+        ("sidebar/rows.rs", include_str!("sidebar/rows.rs")),
         ("tasks.rs", include_str!("tasks.rs")),
         ("theme/tokens.rs", include_str!("theme/tokens.rs")),
         ("grid/labels.rs", include_str!("grid/labels.rs")),
@@ -55,6 +62,7 @@
         ("probe.rs", include_str!("probe.rs")),
         ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
         ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
+        ("toasts.rs", include_str!("toasts.rs")),
         ("window_layout.rs", include_str!("window_layout.rs")),
     ];
 
````

Create `crates/photon-ui/src/sidebar/rows.rs`:

````rust
//! The rows at the top of the sidebar: the views every library has, with their counts.
//! `FolderTree.svelte`'s first block, as a list the view draws.
//!
//! No egui here: a row is what it says and whether it is where the user is.

use crate::nav::{Place, Step};
use photon_core::grid::GridView;

/// A row's height, in points. Every row of the sidebar is this tall, which is what lets
/// the list be drawn a window at a time.
pub const ROW: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fixed {
    All,
    Starred,
    Recent,
    OnThisDay,
    Videos,
    Duplicates,
    /// The photo whose copies are shown, while they are: under Duplicates, and not a
    /// button - the view is already open, and leaving it removes the row.
    CopiesOf,
    Hidden,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub what: Fixed,
    pub label: String,
    pub count: Option<usize>,
    /// Whether this is where the user is, or is going.
    pub active: bool,
    /// What the row says when the pointer rests on it.
    pub hint: String,
}

/// What the library holds of each kind (`grid_info`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub starred: usize,
    pub duplicates: usize,
    pub hidden: usize,
    pub videos: usize,
    /// The file name of the photo whose copies are shown, while the view is Copies.
    pub copies_of: Option<String>,
}

/// Today, as "On this day" means it: the machine's own day - where the user is - while a
/// capture date is the camera's wall clock, so the two meet as plain calendar days with no
/// zone between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Today {
    pub month: u32,
    pub day: u32,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl Today {
    /// The search for photos taken on this day in any year, in `search::Query`'s grammar.
    pub fn query(self) -> String {
        format!("on:{:02}-{:02}", self.month, self.day)
    }

    /// "14 July", for the row's hint.
    pub fn label(self) -> String {
        let month = MONTHS
            .get(self.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or_default();
        format!("{} {month}", self.day)
    }
}

impl Fixed {
    /// What a click on the row asks for, or nothing for the row that is not a button.
    pub fn step(self, today: Today) -> Option<Step> {
        Some(match self {
            Fixed::All => Step::View(GridView::All),
            Fixed::Starred => Step::View(GridView::Starred),
            Fixed::Recent => Step::View(GridView::Recent),
            Fixed::OnThisDay => Step::Search(today.query()),
            Fixed::Videos => Step::View(GridView::Videos),
            Fixed::Duplicates => Step::View(GridView::Duplicates),
            Fixed::Hidden => Step::View(GridView::Hidden),
            Fixed::CopiesOf => return None,
        })
    }
}

/// The rows, top to bottom, for a library holding `counts` and a user at `at`.
///
/// Videos, Duplicates and Hidden are there only while they hold something or are what is
/// shown: most libraries have no duplicates, and a permanent "0" row is noise.
pub fn fixed_rows(counts: &Counts, at: &Place, today: Today) -> Vec<Row> {
    let view = at.view;
    let row = |what: Fixed, label: &str, count: Option<usize>, active: bool, hint: &str| Row {
        what,
        label: label.to_owned(),
        count,
        active,
        hint: hint.to_owned(),
    };
    let mut rows = vec![
        row(
            Fixed::All,
            "All photos",
            None,
            view == GridView::All,
            "Every photo, back where you left the gallery",
        ),
        row(
            Fixed::Starred,
            "Starred",
            Some(counts.starred),
            view == GridView::Starred,
            "Starred photos",
        ),
        row(
            Fixed::Recent,
            "Recent",
            None,
            view == GridView::Recent,
            "The newest photos by capture date",
        ),
        // A search, not a view, and no count, as a saved search has none: it would mean
        // running the search on every change to the library.
        row(
            Fixed::OnThisDay,
            "On this day",
            None,
            *at == Place::search(&today.query()),
            &format!("Photos taken on {}, in any year", today.label()),
        ),
    ];
    if counts.videos > 0 || view == GridView::Videos {
        rows.push(row(
            Fixed::Videos,
            "Videos",
            Some(counts.videos),
            view == GridView::Videos,
            "Every video in the library",
        ));
    }
    if counts.duplicates > 0 || matches!(view, GridView::Duplicates | GridView::Copies) {
        rows.push(row(
            Fixed::Duplicates,
            "Duplicates",
            Some(counts.duplicates),
            view == GridView::Duplicates,
            "Photos with a byte-identical copy elsewhere in the library",
        ));
        if view == GridView::Copies {
            let name = counts.copies_of.as_deref().filter(|name| !name.is_empty());
            rows.push(row(
                Fixed::CopiesOf,
                &format!("Copies of {}", name.unwrap_or("a photo")),
                None,
                true,
                name.unwrap_or_default(),
            ));
        }
    }
    if counts.hidden > 0 || view == GridView::Hidden {
        rows.push(row(
            Fixed::Hidden,
            "Hidden",
            Some(counts.hidden),
            view == GridView::Hidden,
            "Photos you have hidden. They stay on disk; unhide them from here",
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Today = Today { month: 7, day: 4 };

    fn kinds(rows: &[Row]) -> Vec<Fixed> {
        rows.iter().map(|row| row.what).collect()
    }

    #[test]
    fn a_library_of_photos_alone_has_the_four_rows_every_library_has() {
        let rows = fixed_rows(&Counts::default(), &Place::of(GridView::All), TODAY);
        assert_eq!(
            kinds(&rows),
            [Fixed::All, Fixed::Starred, Fixed::Recent, Fixed::OnThisDay]
        );
        // Starred is counted even at none; All photos and Recent are not counted at all.
        assert_eq!(rows[1].count, Some(0));
        assert_eq!(rows[0].count, None);
        assert_eq!(rows[2].count, None);
        let active: Vec<bool> = rows.iter().map(|row| row.active).collect();
        assert_eq!(active, [true, false, false, false]);
    }

    #[test]
    fn videos_duplicates_and_hidden_are_there_while_they_hold_something() {
        let counts = Counts {
            starred: 12,
            duplicates: 3,
            hidden: 240,
            videos: 7,
            copies_of: None,
        };
        let rows = fixed_rows(&counts, &Place::of(GridView::Starred), TODAY);
        assert_eq!(
            kinds(&rows),
            [
                Fixed::All,
                Fixed::Starred,
                Fixed::Recent,
                Fixed::OnThisDay,
                Fixed::Videos,
                Fixed::Duplicates,
                Fixed::Hidden
            ]
        );
        let counted: Vec<Option<usize>> = rows.iter().map(|row| row.count).collect();
        assert_eq!(
            counted,
            [None, Some(12), None, None, Some(7), Some(3), Some(240)]
        );
        assert!(rows[1].active);
        assert_eq!(rows.iter().filter(|row| row.active).count(), 1);
    }

    // The last hidden photo unhidden while Hidden is shown: the row the user is on must
    // not vanish from under them.
    #[test]
    fn an_empty_one_stays_while_it_is_what_is_shown() {
        for (view, what) in [
            (GridView::Videos, Fixed::Videos),
            (GridView::Duplicates, Fixed::Duplicates),
            (GridView::Hidden, Fixed::Hidden),
        ] {
            let rows = fixed_rows(&Counts::default(), &Place::of(view), TODAY);
            let row = rows.iter().find(|row| row.what == what).unwrap();
            assert!(row.active, "{what:?}");
            assert_eq!(row.count, Some(0));
        }
    }

    #[test]
    fn the_copies_of_a_photo_are_a_row_under_duplicates_that_is_not_a_button() {
        let counts = Counts {
            copies_of: Some("IMG_0042.jpg".to_owned()),
            ..Counts::default()
        };
        let at = Place {
            view: GridView::Copies,
            arg: "42".to_owned(),
        };
        let rows = fixed_rows(&counts, &at, TODAY);
        let position = rows
            .iter()
            .position(|row| row.what == Fixed::CopiesOf)
            .unwrap();
        assert_eq!(rows[position - 1].what, Fixed::Duplicates);
        assert!(
            !rows[position - 1].active,
            "Duplicates is not what is shown"
        );
        assert_eq!(rows[position].label, "Copies of IMG_0042.jpg");
        assert!(rows[position].active);
        assert_eq!(Fixed::CopiesOf.step(TODAY), None);
        // A photo that has gone has no name left to read.
        let rows = fixed_rows(&Counts::default(), &at, TODAY);
        assert!(rows.iter().any(|row| row.label == "Copies of a photo"));
    }

    #[test]
    fn on_this_day_is_a_search_for_todays_date_in_any_year() {
        assert_eq!(TODAY.query(), "on:07-04");
        assert_eq!(TODAY.label(), "4 July");
        assert_eq!(Today { month: 12, day: 25 }.query(), "on:12-25");
        assert_eq!(
            Fixed::OnThisDay.step(TODAY),
            Some(Step::Search("on:07-04".to_owned()))
        );
        let rows = fixed_rows(&Counts::default(), &Place::search("on:07-04"), TODAY);
        let row = rows
            .iter()
            .find(|row| row.what == Fixed::OnThisDay)
            .unwrap();
        assert!(row.active);
        assert_eq!(row.hint, "Photos taken on 4 July, in any year");
        // Another search is not this row's, and yesterday's is not today's.
        let rows = fixed_rows(&Counts::default(), &Place::search("on:07-03"), TODAY);
        assert!(rows.iter().all(|row| !row.active));
    }

    #[test]
    fn each_row_asks_for_its_own_view() {
        assert_eq!(Fixed::All.step(TODAY), Some(Step::View(GridView::All)));
        assert_eq!(
            Fixed::Starred.step(TODAY),
            Some(Step::View(GridView::Starred))
        );
        assert_eq!(
            Fixed::Recent.step(TODAY),
            Some(Step::View(GridView::Recent))
        );
        assert_eq!(
            Fixed::Videos.step(TODAY),
            Some(Step::View(GridView::Videos))
        );
        assert_eq!(
            Fixed::Duplicates.step(TODAY),
            Some(Step::View(GridView::Duplicates))
        );
        assert_eq!(
            Fixed::Hidden.step(TODAY),
            Some(Step::View(GridView::Hidden))
        );
    }
}
````

Create `crates/photon-ui/src/empty.rs`:

````rust
//! What is said where there are no photos: nothing before the grid is known, why when the
//! library could not be read, and each view's own line when it is simply empty. The Svelte
//! UI's `grid-state.ts`, and the notices `Grid.svelte` wrote beside it.
//!
//! No egui here.

use crate::grid::labels::photo_count as counted;
use crate::nav::Place;
use photon_core::grid::GridView;
use photon_engine::engine::NOT_BUILT;

/// The grid on hand, as far as these rules need it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridState {
    pub version: u64,
    pub len: usize,
    /// Why the grid is empty, when it is only because the library could not be read at
    /// launch (`Engine::build_first_grid`).
    pub build_error: Option<String>,
}

/// Whether the engine has built a grid at all. `Engine::open` leaves the first build to its
/// startup thread and holds an empty index at `NOT_BUILT` until it lands.
pub fn grid_built(grid: &GridState) -> bool {
    grid.version > NOT_BUILT
}

/// What is said in place of photos when the library could not be read at launch.
pub fn build_failure(grid: &GridState) -> Option<String> {
    grid.build_error
        .as_ref()
        .map(|why| format!("photon could not read the library: {why}"))
}

/// Whether the grid should say it is empty. Not before the first build: the index is empty
/// then because nothing has been read, and on a large library the line stood on screen for
/// as long as the read took. Nor when that build failed: that grid is empty because
/// nothing *could* be read.
pub fn show_empty_notice(grid: &GridState) -> bool {
    grid_built(grid) && grid.len == 0 && grid.build_error.is_none()
}

/// The status bar's count, or nothing before the first build and after a failed one, when
/// "0 photos" would be the same misreading.
pub fn photo_count(grid: &GridState) -> Option<String> {
    (grid_built(grid) && grid.build_error.is_none()).then(|| counted(grid.len))
}

/// The line an empty view shows in place of its photos. All photos and Recent have none
/// here: an empty library says why in a panel of its own. Nor have the views that are
/// reached through a list the sidebar does not hold yet.
pub fn view_notice(grid: &GridState, place: &Place) -> Option<String> {
    if let Some(failure) = build_failure(grid) {
        return Some(failure);
    }
    if !show_empty_notice(grid) {
        return None;
    }
    match place.view {
        GridView::Starred => {
            Some("No starred photos. Star one in the viewer, or in Picasa.".to_owned())
        }
        GridView::Search => Some(format!("No photos match “{}”", place.arg)),
        GridView::Duplicates => {
            Some("No duplicates. Every photo in the library is the only copy of itself.".to_owned())
        }
        GridView::Videos => Some(
            "No videos. photon finds MP4, M4V, MOV and WebM files in your watched folders."
                .to_owned(),
        ),
        GridView::Hidden => Some(
            "No hidden photos. Right-click a photo and choose Hide to put it away here.".to_owned(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(version: u64, len: usize) -> GridState {
        GridState {
            version,
            len,
            build_error: None,
        }
    }

    #[test]
    fn nothing_is_said_of_a_grid_that_has_not_been_built() {
        let unbuilt = grid(NOT_BUILT, 0);
        assert!(!grid_built(&unbuilt));
        assert!(!show_empty_notice(&unbuilt));
        assert_eq!(photo_count(&unbuilt), None);
        assert_eq!(view_notice(&unbuilt, &Place::of(GridView::Starred)), None);
    }

    #[test]
    fn a_built_grid_is_counted_and_an_empty_one_says_so() {
        assert_eq!(photo_count(&grid(3, 1234)).as_deref(), Some("1,234 photos"));
        assert_eq!(photo_count(&grid(3, 0)).as_deref(), Some("0 photos"));
        assert!(show_empty_notice(&grid(3, 0)));
        assert!(!show_empty_notice(&grid(3, 1)));
    }

    // That grid is empty because nothing could be read: "No starred photos" would be a
    // statement about the library, and "0 photos" the same.
    #[test]
    fn a_library_that_could_not_be_read_says_that_and_nothing_else() {
        let failed = GridState {
            version: 1,
            len: 0,
            build_error: Some("disk I/O error".to_owned()),
        };
        assert!(!show_empty_notice(&failed));
        assert_eq!(photo_count(&failed), None);
        for view in [GridView::All, GridView::Starred] {
            assert_eq!(
                view_notice(&failed, &Place::of(view)).as_deref(),
                Some("photon could not read the library: disk I/O error")
            );
        }
    }

    #[test]
    fn an_empty_view_says_what_it_is_empty_of() {
        let empty = grid(2, 0);
        let said = |place: &Place| view_notice(&empty, place).unwrap_or_default();
        assert!(said(&Place::of(GridView::Starred)).starts_with("No starred photos."));
        assert_eq!(
            said(&Place::search("lake 2031")),
            "No photos match “lake 2031”"
        );
        assert!(said(&Place::of(GridView::Duplicates)).starts_with("No duplicates."));
        assert!(said(&Place::of(GridView::Videos)).starts_with("No videos."));
        assert!(said(&Place::of(GridView::Hidden)).starts_with("No hidden photos."));
        // An empty library has a panel of its own for these two.
        assert_eq!(view_notice(&empty, &Place::of(GridView::All)), None);
        assert_eq!(view_notice(&empty, &Place::of(GridView::Recent)), None);
        // And a view with photos has no line.
        assert_eq!(
            view_notice(&grid(2, 5), &Place::of(GridView::Starred)),
            None
        );
    }
}
````

Create `crates/photon-ui/src/toasts.rs`:

````rust
//! Messages that go away by themselves: a step the engine refused, and later what an
//! action did. The Svelte UI's `library.toasts`.
//!
//! No egui here: time is a number, and `shell.rs` draws the list.

/// How long a failure stays. It is read, so it stays longer than a report.
pub const ERROR_MS: f64 = 6000.0;
/// How long a report of something that worked stays: read at a glance or not at all.
pub const DONE_MS: f64 = 4000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Error,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: Kind,
    until_ms: f64,
}

#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Toasts {
    /// Says that something failed.
    pub fn error(&mut self, message: impl Into<String>, now_ms: f64) {
        self.push(message.into(), Kind::Error, now_ms + ERROR_MS);
    }

    /// Says what an action did.
    pub fn done(&mut self, message: impl Into<String>, now_ms: f64) {
        self.push(message.into(), Kind::Done, now_ms + DONE_MS);
    }

    fn push(&mut self, message: String, kind: Kind, until_ms: f64) {
        self.next += 1;
        self.list.push(Toast {
            id: self.next,
            message,
            kind,
            until_ms,
        });
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|toast| toast.id != id);
    }

    /// Lets go of those whose time is over, and answers when the next one's is: a still
    /// window draws no frame by itself, and a message nobody removed would stay.
    pub fn tick(&mut self, now_ms: f64) -> Option<f64> {
        self.list.retain(|toast| toast.until_ms > now_ms);
        self.list
            .iter()
            .map(|toast| toast.until_ms)
            .min_by(f64::total_cmp)
    }

    /// The messages showing, oldest first.
    pub fn showing(&self) -> &[Toast] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_goes_away_by_itself_and_a_failure_stays_longer() {
        let mut toasts = Toasts::default();
        toasts.done("3 photos starred", 1_000.0);
        toasts.error("could not read the library", 1_000.0);
        assert_eq!(toasts.showing().len(), 2);
        // The frame to be drawn next is the one in which the first is over.
        assert_eq!(toasts.tick(1_000.0), Some(5_000.0));
        assert_eq!(toasts.tick(4_999.0), Some(5_000.0));

        assert_eq!(toasts.tick(5_000.0), Some(7_000.0));
        let left: Vec<&str> = toasts
            .showing()
            .iter()
            .map(|t| t.message.as_str())
            .collect();
        assert_eq!(left, ["could not read the library"]);
        assert_eq!(toasts.showing()[0].kind, Kind::Error);

        assert_eq!(toasts.tick(7_000.0), None);
        assert!(toasts.showing().is_empty());
    }

    #[test]
    fn a_message_dismissed_goes_at_once_and_takes_no_other_with_it() {
        let mut toasts = Toasts::default();
        toasts.error("one", 0.0);
        toasts.error("two", 0.0);
        toasts.error("three", 0.0);
        let showing = |toasts: &Toasts| -> Vec<String> {
            toasts
                .showing()
                .iter()
                .map(|toast| toast.message.clone())
                .collect()
        };
        // The one in the middle: neither the older nor the newer goes with it.
        let second = toasts.showing()[1].id;
        toasts.dismiss(second);
        assert_eq!(showing(&toasts), ["one", "three"]);
        // An id is never given twice, so a late dismissal of one long gone hits nothing.
        toasts.error("four", 0.0);
        toasts.dismiss(second);
        assert_eq!(showing(&toasts), ["one", "three", "four"]);
    }
}
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib sidebar::rows:: && cargo test -p photon-ui --lib empty:: && cargo test -p photon-ui --lib toasts::`
Expected: rows: 6 pass. empty: 4 pass. toasts: 2 pass. And `state_modules_name_no_egui_type` passes with seventeen modules.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/toasts.rs` | `self.list.retain(\|toast\| toast.until_ms > now_ms);` | `self.list.retain(\|toast\| toast.until_ms >= now_ms);` | `a_message_goes_away_by_itself_and_a_failure_stays_longer` (`cargo test -p photon-ui --lib`) |
| 2 | `src/toasts.rs` | `self.push(message.into(), Kind::Error, now_ms + ERROR_MS);` | `self.push(message.into(), Kind::Error, now_ms + DONE_MS);` | `a_message_goes_away_by_itself_and_a_failure_stays_longer` (`cargo test -p photon-ui --lib`) |
| 3 | `src/toasts.rs` | `self.list.retain(\|toast\| toast.id != id);` | `self.list.retain(\|toast\| toast.id > id);` | `a_message_dismissed_goes_at_once_and_takes_no_other_with_it` (`cargo test -p photon-ui --lib`) |
| 4 | `src/empty.rs` | `grid.version > NOT_BUILT` | `grid.version >= NOT_BUILT` | `nothing_is_said_of_a_grid_that_has_not_been_built` (`cargo test -p photon-ui --lib`) |
| 5 | `src/empty.rs` | `grid_built(grid) && grid.len == 0 && grid.build_error.is_none()` | `grid_built(grid) && grid.len == 0` | `a_library_that_could_not_be_read_says_that_and_nothing_else` (`cargo test -p photon-ui --lib`) |
| 6 | `src/empty.rs` | `(grid_built(grid) && grid.build_error.is_none()).then(\|\| counted(grid.len))` | `grid_built(grid).then(\|\| counted(grid.len))` | `a_library_that_could_not_be_read_says_that_and_nothing_else` (`cargo test -p photon-ui --lib`) |
| 7 | `src/empty.rs` | `if let Some(failure) = build_failure(grid) { ⏎         return Some(failure); ⏎     }` | *(nothing: the lines are taken out)* | `a_library_that_could_not_be_read_says_that_and_nothing_else` (`cargo test -p photon-ui --lib`) |
| 8 | `src/empty.rs` | `if !show_empty_notice(grid) { ⏎         return None; ⏎     }` | *(nothing: the lines are taken out)* | `an_empty_view_says_what_it_is_empty_of` (`cargo test -p photon-ui --lib`) |
| 9 | `src/sidebar/rows.rs` | `if counts.videos > 0 \|\| view == GridView::Videos {` | `if counts.videos > 0 {` | `an_empty_one_stays_while_it_is_what_is_shown` (`cargo test -p photon-ui --lib`) |
| 10 | `src/sidebar/rows.rs` | `if counts.hidden > 0 \|\| view == GridView::Hidden {` | `if true {` | `a_library_of_photos_alone_has_the_four_rows_every_library_has` (`cargo test -p photon-ui --lib`) |
| 11 | `src/sidebar/rows.rs` | `if counts.duplicates > 0 \|\| matches!(view, GridView::Duplicates \| GridView::Copies) {` | `if counts.duplicates > 0 \|\| view == GridView::Duplicates {` | `the_copies_of_a_photo_are_a_row_under_duplicates_that_is_not_a_button` (`cargo test -p photon-ui --lib`) |
| 12 | `src/sidebar/rows.rs` | `*at == Place::search(&today.query()),` | `view == GridView::Search,` | `on_this_day_is_a_search_for_todays_date_in_any_year` (`cargo test -p photon-ui --lib`) |
| 13 | `src/sidebar/rows.rs` | `format!("on:{:02}-{:02}", self.month, self.day)` | `format!("on:{}-{}", self.month, self.day)` | `on_this_day_is_a_search_for_todays_date_in_any_year` (`cargo test -p photon-ui --lib`) |
| 14 | `src/sidebar/rows.rs` | `Fixed::CopiesOf => return None,` | `Fixed::CopiesOf => Step::View(GridView::Copies),` | `the_copies_of_a_photo_are_a_row_under_duplicates_that_is_not_a_button` (`cargo test -p photon-ui --lib`) |

Each of the 14 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/lib.rs crates/photon-ui/src/sidebar/rows.rs crates/photon-ui/src/empty.rs crates/photon-ui/src/toasts.rs
git commit -m "feat(ui): the sidebar's first rows, the line of an empty view, and toasts

The rows every library has, with Videos, Duplicates and Hidden only while they
hold something or are what is shown. Nothing is said of a grid that has not been
built, and a library that could not be read says that and nothing else. A
message goes away by itself, a failure later than a report.

14 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The shell, drawn

**Files:**
- Modify: `crates/photon-ui/src/lib.rs`
- Modify: `crates/photon-ui/src/icons.rs`
- Modify: `crates/photon-ui/src/theme/tokens.rs`
- Create: `crates/photon-ui/src/sidebar/view.rs`
- Create: `crates/photon-ui/src/shell.rs`

**Interfaces:**
- Consumes: Task 3's `areas`, `Layout`, `clamp_sidebar_width`, `SIDEBAR_STEP`; Task 4's `Row`, `Fixed`, `Toast`, `Kind`; `text::paint_line`; the theme.
- Produces:
  - `photon_ui::icons::Icon::{Calendar, Clock, EyeOff, LayoutGrid, PanelLeft, Settings, X}` beside the four there are;
  - `photon_ui::theme::tokens::{Shadow, SHADOW_MENU}`;
  - `photon_ui::sidebar::view::show(ui, rect, rows: &[Row]) -> Option<Fixed>`;
  - `photon_ui::shell`: `ShellData { layout, rows, count: Option<&str>, notice: Option<&str>, toasts }`; `Action::{ToggleSidebar, Go(Fixed), Width { width: f32, store: bool }, Dismiss(u64)}`; `Shell` (`Default`) with `resizing()` and `show(&mut self, ui: &mut egui::Ui, data: &ShellData, content: impl FnOnce(&mut egui::Ui)) -> Vec<Action>`.

The icons' path data is copied from `ui/src/lib/icons.ts` exactly; `every_icon_is_the_svelte_uis` fails on any that differs.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/lib.rs`:

````diff
--- a/crates/photon-ui/src/lib.rs
+++ b/crates/photon-ui/src/lib.rs
@@ -24,8 +24,10 @@
 pub mod icons;
 pub mod nav;
 pub mod probe;
+pub mod shell;
 pub mod sidebar {
     pub mod rows;
+    pub mod view;
 }
 pub mod tasks;
 pub mod text;
````

Change `crates/photon-ui/src/icons.rs`:

````diff
--- a/crates/photon-ui/src/icons.rs
+++ b/crates/photon-ui/src/icons.rs
@@ -9,18 +9,44 @@
 
 #[derive(Clone, Copy, Debug, PartialEq, Eq)]
 pub enum Icon {
+    Calendar,
+    Clock,
     Copy,
+    EyeOff,
+    LayoutGrid,
+    PanelLeft,
     Play,
+    Settings,
     Star,
     TriangleAlert,
+    X,
 }
 
 impl Icon {
-    pub const ALL: [Icon; 4] = [Icon::Copy, Icon::Play, Icon::Star, Icon::TriangleAlert];
+    pub const ALL: [Icon; 11] = [
+        Icon::Calendar,
+        Icon::Clock,
+        Icon::Copy,
+        Icon::EyeOff,
+        Icon::LayoutGrid,
+        Icon::PanelLeft,
+        Icon::Play,
+        Icon::Settings,
+        Icon::Star,
+        Icon::TriangleAlert,
+        Icon::X,
+    ];
 
     fn name(self) -> &'static str {
         match self {
+            Icon::Calendar => "calendar",
+            Icon::Clock => "clock",
             Icon::Copy => "copy",
+            Icon::EyeOff => "eye-off",
+            Icon::LayoutGrid => "layout-grid",
+            Icon::PanelLeft => "panel-left",
+            Icon::Settings => "settings",
+            Icon::X => "x",
             Icon::Play => "play",
             Icon::Star => "star",
             Icon::TriangleAlert => "triangle-alert",
@@ -30,6 +56,23 @@
     /// The inside of the icon's `<svg>`.
     fn inner(self) -> &'static str {
         match self {
+            Icon::Calendar => {
+                r#"<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>"#
+            }
+            Icon::Clock => r#"<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>"#,
+            Icon::EyeOff => {
+                r#"<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>"#
+            }
+            Icon::LayoutGrid => {
+                r#"<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>"#
+            }
+            Icon::PanelLeft => {
+                r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#
+            }
+            Icon::Settings => {
+                r#"<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>"#
+            }
+            Icon::X => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
             Icon::Copy => {
                 r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
             }
````

Change `crates/photon-ui/src/theme/tokens.rs`:

````diff
--- a/crates/photon-ui/src/theme/tokens.rs
+++ b/crates/photon-ui/src/theme/tokens.rs
@@ -114,6 +114,24 @@
     ("--scrim", SCRIM),
 ];
 
+/// A shadow: how far it is offset, how far it is blurred, and its ink.
+#[derive(Clone, Copy, Debug, PartialEq)]
+pub struct Shadow {
+    pub x: f32,
+    pub y: f32,
+    pub blur: f32,
+    pub ink: Rgba,
+}
+
+/// `--shadow-menu`: what lies under a message, a menu, a list. Dark ink in both themes: a
+/// shadow reads dark against a light surface too.
+pub const SHADOW_MENU: Shadow = Shadow {
+    x: 0.0,
+    y: 6.0,
+    blur: 24.0,
+    ink: rgba(0x00000040),
+};
+
 /// Corner radii, `--r-1` to `--r-4`.
 pub const R: [f32; 4] = [4.0, 6.0, 8.0, 12.0];
 /// Spacing, `--s-1` to `--s-6`.
@@ -198,6 +216,18 @@
                 .parse::<f32>()
                 .unwrap()
         };
+        // A shadow is "x y blur colour".
+        let shadow: Vec<&str> = root["--shadow-menu"].split_whitespace().collect();
+        let length = |text: &str| text.trim_end_matches("px").parse::<f32>().unwrap();
+        assert_eq!(
+            Shadow {
+                x: length(shadow[0]),
+                y: length(shadow[1]),
+                blur: length(shadow[2]),
+                ink: parse(shadow[3]),
+            },
+            SHADOW_MENU
+        );
         for (i, radius) in R.iter().enumerate() {
             assert_eq!(px(&format!("--r-{}", i + 1)), *radius);
         }
````

Create `crates/photon-ui/src/sidebar/view.rs`:

````rust
//! The sidebar, drawn: the rows of `rows.rs`, top to bottom, and which of them was clicked.

use super::rows::{Fixed, ROW, Row};
use crate::{
    grid::labels::grouped,
    icons::Icon,
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{R, S, T},
    },
};
use eframe::egui::{self, Align2, Rect, Sense, WidgetInfo, WidgetType, pos2};

/// What a row is inset by from the panel's edges.
const INSET: f32 = 6.0;
const ICON: f32 = 14.0;

fn icon(what: Fixed) -> Option<Icon> {
    Some(match what {
        Fixed::All => Icon::LayoutGrid,
        Fixed::Starred => Icon::Star,
        Fixed::Recent => Icon::Clock,
        Fixed::OnThisDay => Icon::Calendar,
        Fixed::Videos => Icon::Play,
        Fixed::Duplicates => Icon::Copy,
        Fixed::Hidden => Icon::EyeOff,
        Fixed::CopiesOf => return None,
    })
}

/// Draws the sidebar in `rect` and answers the row that was clicked.
pub fn show(ui: &mut egui::Ui, rect: Rect, rows: &[Row]) -> Option<Fixed> {
    let palette = palette(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, color(palette.chrome));

    let mut clicked = None;
    let mut top = rect.top() + S[1];
    for row in rows {
        let place = Rect::from_min_max(
            pos2(rect.left() + INSET, top),
            pos2(rect.right() - INSET, top + ROW),
        );
        top += ROW;
        if place.top() >= rect.bottom() {
            break;
        }
        // The row that names the photo whose copies are shown does nothing on a click -
        // the view is already open - so it must not offer one.
        let button = row.what != Fixed::CopiesOf;
        let sense = if button {
            Sense::click()
        } else {
            Sense::hover()
        };
        let mut response = ui.interact(place.intersect(rect), ui.id().with(row.what), sense);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, button, &row.label));
        if !row.hint.is_empty() {
            response = response.on_hover_text(&row.hint);
        }
        if button && response.clicked() {
            clicked = Some(row.what);
        }

        if row.active {
            painter.rect_filled(place, R[2], color(palette.accent_soft));
        } else if button && response.hovered() {
            painter.rect_filled(place, R[2], color(palette.hover));
        }
        let middle = place.center().y;
        let text = color(palette.text);
        let mut left = place.left() + S[1];
        match icon(row.what) {
            Some(icon) => {
                let at =
                    Rect::from_min_size(pos2(left, middle - ICON / 2.0), egui::Vec2::splat(ICON));
                icon.paint(ui, at, ICON, false, text);
                left += ICON + S[1];
            }
            // No icon: the name starts where a row under a group starts.
            None => left = place.left() + 28.0,
        }
        let mut right = place.right() - S[1];
        if let Some(count) = row.count {
            // Dimmed, except over the fill of the row that is shown, where the dimmed
            // colour does not have the contrast.
            let tint = if row.active {
                text
            } else {
                color(palette.text_dim)
            };
            let drawn = painter.text(
                pos2(right, middle),
                Align2::RIGHT_CENTER,
                grouped(count),
                fonts::regular(T[0]),
                tint,
            );
            right = drawn.left() - S[1];
        }
        paint_line(
            ui,
            &painter,
            (left, middle),
            right - left,
            &row.label,
            fonts::regular(T[2]),
            text,
        );
    }
    clicked
}
````

Create `crates/photon-ui/src/shell.rs`:

````rust
//! The window around the grid: the top bar, the sidebar and its splitter, the status bar,
//! and the messages over them. `App.svelte`'s frame, drawn in the four areas of
//! `window_layout.rs`.
//!
//! A view: it draws what it is given and answers what the user did. What that changes is
//! the application's business.

use crate::{
    icons::Icon,
    sidebar::{
        rows::{Fixed, Row},
        view as sidebar,
    },
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{Palette, R, S, SHADOW_MENU, T},
    },
    toasts::{Kind, Toast},
    window_layout::{Area, Layout, SIDEBAR_STEP, areas, clamp_sidebar_width},
};
use eframe::egui::{
    self, CursorIcon, Event, EventFilter, Id, Key, Order, Rect, Sense, Stroke, UiBuilder,
    WidgetInfo, WidgetType, epaint::Shadow, pos2, vec2,
};

/// A button of the top bar: thirty points square, its icon eighteen.
const BUTTON: f32 = 30.0;
const BUTTON_ICON: f32 = 18.0;
/// The widest a message is drawn, its edge and its button included.
const TOAST_WIDTH: f32 = 420.0;
const TOAST_BUTTON: f32 = 24.0;
/// The coloured edge of a message, on its left.
const TOAST_EDGE: f32 = 3.0;

pub struct ShellData<'a> {
    pub layout: &'a Layout,
    pub rows: &'a [Row],
    /// The status bar's photo count, when there is one to give.
    pub count: Option<&'a str>,
    /// The line shown in the middle of the content in place of photos.
    pub notice: Option<&'a str>,
    pub toasts: &'a [Toast],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The toggle in the top bar, or its key.
    ToggleSidebar,
    /// A row of the sidebar.
    Go(Fixed),
    /// The splitter moved to `width`. `store` when the user has let go of it or moved it
    /// by a key: a width is stored when it was chosen, not at every point on the way.
    Width {
        width: f32,
        store: bool,
    },
    Dismiss(u64),
}

#[derive(Default)]
pub struct Shell {
    /// How far right of the sidebar's edge the pointer took hold of the splitter, while it
    /// is held.
    grab: Option<f32>,
}

fn rect_of(area: Area, window: Rect) -> Rect {
    Rect::from_min_max(
        window.min + vec2(area.left, area.top),
        window.min + vec2(area.right, area.bottom),
    )
}

impl Shell {
    /// Whether the splitter is being dragged.
    pub fn resizing(&self) -> bool {
        self.grab.is_some()
    }

    /// Draws the shell in the whole of `ui`, and `content` - the grid - in the area left
    /// for it. Answers what the user did, in the order they did it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &ShellData<'_>,
        content: impl FnOnce(&mut egui::Ui),
    ) -> Vec<Action> {
        let window = ui.max_rect();
        let found = areas(window.width(), window.height(), data.layout);
        let palette = palette(ui.ctx());
        let mut actions = Vec::new();

        // Ctrl+B, or Command+B. Not on a key held down, which would flap the sidebar, and
        // not while the splitter is held, which would take away what is being dragged.
        let pressed = ui.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key { key: Key::B, pressed: true, repeat: false, modifiers, .. }
                        if modifiers.command
                )
            })
        });
        if pressed && !self.resizing() {
            actions.push(Action::ToggleSidebar);
        }

        self.top_bar(
            ui,
            rect_of(found.top_bar, window),
            data,
            palette,
            &mut actions,
        );
        if let Some(area) = found.sidebar
            && let Some(row) = sidebar::show(ui, rect_of(area, window), data.rows)
        {
            actions.push(Action::Go(row));
        }
        if let Some(area) = found.splitter {
            self.splitter(
                ui,
                rect_of(area, window),
                window,
                data.layout,
                palette,
                &mut actions,
            );
        }
        status_bar(ui, rect_of(found.status_bar, window), data.count, palette);

        let place = rect_of(found.content, window);
        ui.painter_at(place)
            .rect_filled(place, 0.0, color(palette.surface));
        ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
            ui.set_clip_rect(place.intersect(ui.clip_rect()));
            content(ui);
        });
        if let Some(notice) = data.notice {
            self::notice(ui, place, notice, palette);
        }
        if let Some(id) = toasts(ui, window, data.toasts, palette) {
            actions.push(Action::Dismiss(id));
        }
        actions
    }

    fn top_bar(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        data: &ShellData<'_>,
        palette: &Palette,
        actions: &mut Vec<Action>,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color(palette.chrome));
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
        let middle = rect.center().y - 0.5;
        let at = |left: f32| {
            Rect::from_min_size(pos2(left, middle - BUTTON / 2.0), vec2(BUTTON, BUTTON))
        };

        let label = if data.layout.sidebar_hidden {
            "Show sidebar"
        } else {
            "Hide sidebar"
        };
        if bar_button(
            ui,
            at(rect.left() + S[1]),
            Icon::PanelLeft,
            label,
            true,
            palette,
        ) {
            actions.push(Action::ToggleSidebar);
        }
        // Settings come with a later part of the native interface: the gear is drawn where
        // it will be, so nothing moves when it starts to work, and takes no press yet.
        bar_button(
            ui,
            at(rect.right() - S[1] - BUTTON),
            Icon::Settings,
            "Settings",
            false,
            palette,
        );
    }

    fn splitter(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        window: Rect,
        layout: &Layout,
        palette: &Palette,
        actions: &mut Vec<Action>,
    ) {
        let id = ui.id().with("splitter");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Resize sidebar"));
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let shown = layout.shown_width(window.width());
        let pointer = ui.input(|input| input.pointer.interact_pos());
        // Where the button went down, not where the pointer is when the press has become
        // a drag: by then it has moved, and the edge would be left that far behind it.
        if response.drag_started()
            && let Some(pressed) = ui.input(|input| input.pointer.press_origin())
        {
            self.grab = Some(pressed.x - window.left() - shown);
        }
        if let (Some(grab), Some(pointer)) = (self.grab, pointer)
            && response.dragged()
        {
            let width = clamp_sidebar_width(pointer.x - window.left() - grab, window.width());
            if width != shown {
                actions.push(Action::Width {
                    width,
                    store: false,
                });
            }
        }
        // Every way a drag ends - the button let go, the pointer gone - comes here: a
        // splitter left held would follow the pointer for ever.
        if self.grab.is_some() && !response.dragged() {
            self.grab = None;
            actions.push(Action::Width {
                width: shown,
                store: true,
            });
        }
        if response.has_focus() {
            // The arrows are the splitter's while it has the focus, not egui's for moving
            // the focus on.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    EventFilter {
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let step = ui.input(|input| {
                f32::from(i8::from(input.key_pressed(Key::ArrowRight)))
                    - f32::from(i8::from(input.key_pressed(Key::ArrowLeft)))
            });
            if step != 0.0 {
                let width = clamp_sidebar_width(shown + step * SIDEBAR_STEP, window.width());
                actions.push(Action::Width { width, store: true });
            }
        }

        let lit = response.hovered() || response.dragged() || response.has_focus();
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let painter = ui.painter_at(rect);
        // Its own focus treatment rather than a ring: a bar five points wide cannot hold one.
        let fill = if lit { palette.accent } else { palette.chrome };
        painter.rect_filled(rect, 0.0, color(fill));
        if !lit {
            painter.vline(
                rect.left() + 0.5,
                rect.y_range(),
                (1.0, color(palette.line)),
            );
        }
    }
}

/// One button of the top bar. Answers whether it was pressed, which one that is not
/// `enabled` never is.
fn bar_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    enabled: bool,
    palette: &Palette,
) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, ui.id().with(("bar", label)), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let hovered = enabled && response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, R[2], color(palette.hover));
    }
    let tint = if hovered {
        color(palette.text)
    } else if enabled {
        color(palette.text_dim)
    } else {
        // What cannot be pressed yet is there, and fainter than what can.
        color(palette.text_dim).gamma_multiply(0.5)
    };
    icon.paint(ui, rect, BUTTON_ICON, false, tint);
    enabled && response.clicked()
}

fn status_bar(ui: &egui::Ui, rect: Rect, count: Option<&str>, palette: &Palette) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, color(palette.chrome));
    painter.hline(rect.x_range(), rect.top() + 0.5, (1.0, color(palette.line)));
    if let Some(count) = count {
        painter.text(
            pos2(rect.right() - S[2], rect.center().y + 0.5),
            egui::Align2::RIGHT_CENTER,
            count,
            fonts::regular(T[1]),
            color(palette.text_dim),
        );
    }
}

/// The line an empty view shows, in the middle of where its photos would be.
fn notice(ui: &egui::Ui, place: Rect, text: &str, palette: &Palette) {
    let painter = ui.painter_at(place);
    let font = fonts::regular(T[2]);
    let dim = color(palette.text_dim);
    // Measured as egui lays it out, drawn as `text.rs` orders it: a search for a name in
    // another script is quoted in this line.
    let width = painter
        .layout_no_wrap(text.to_owned(), font.clone(), dim)
        .size()
        .x
        .min(place.width() - 2.0 * S[3]);
    let left = place.center().x - width / 2.0;
    paint_line(
        ui,
        &painter,
        (left, place.center().y),
        place.width() - 2.0 * S[3],
        text,
        font,
        dim,
    );
}

/// The messages, the newest lowest, over the bottom right of the window. Answers the one
/// whose button was pressed.
fn toasts(ui: &egui::Ui, window: Rect, list: &[Toast], palette: &Palette) -> Option<u64> {
    let mut dismissed = None;
    let mut bottom = window.bottom() - 40.0;
    let room = TOAST_WIDTH - TOAST_EDGE - 3.0 * S[2] - TOAST_BUTTON;
    for toast in list.iter().rev() {
        let galley = ui.painter().layout(
            toast.message.clone(),
            fonts::regular(T[2]),
            color(palette.text),
            room,
        );
        let size = vec2(
            TOAST_EDGE + S[2] + galley.size().x + S[2] + TOAST_BUTTON + S[2],
            galley.size().y.max(TOAST_BUTTON) + 20.0,
        );
        let place =
            Rect::from_min_size(pos2(window.right() - S[3] - size.x, bottom - size.y), size);
        bottom = place.top() - S[1];
        // Over everything, and taking its own presses: a message lies on the grid.
        egui::Area::new(Id::new(("toast", toast.id)))
            .order(Order::Foreground)
            .fixed_pos(place.min)
            .show(ui.ctx(), |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter();
                painter.add(shadow().as_shape(rect, R[1]));
                // The edge is the whole shape in its colour, with the message's own
                // surface laid over all of it but the left three points.
                let edge = match toast.kind {
                    Kind::Error => palette.danger,
                    Kind::Done => palette.accent,
                };
                painter.rect_filled(rect, R[1], color(edge));
                let face = Rect::from_min_max(pos2(rect.left() + TOAST_EDGE, rect.top()), rect.max);
                painter.rect_filled(
                    face,
                    egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: R[1] as u8,
                        se: R[1] as u8,
                    },
                    color(palette.raised),
                );
                painter.rect_stroke(
                    rect,
                    R[1],
                    Stroke::new(1.0, color(palette.line)),
                    egui::StrokeKind::Outside,
                );
                painter.galley(
                    pos2(face.left() + S[2], rect.center().y - galley.size().y / 2.0),
                    galley.clone(),
                    color(palette.text),
                );
                let button = Rect::from_center_size(
                    pos2(rect.right() - S[2] - TOAST_BUTTON / 2.0, rect.center().y),
                    vec2(TOAST_BUTTON, TOAST_BUTTON),
                );
                let response = ui.interact(button, ui.id().with("dismiss"), Sense::click());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Dismiss"));
                let tint = if response.hovered() {
                    ui.painter().rect_filled(button, R[1], color(palette.hover));
                    palette.text
                } else {
                    palette.text_dim
                };
                Icon::X.paint(ui, button, 14.0, false, color(tint));
                if response.clicked() {
                    dismissed = Some(toast.id);
                }
            });
    }
    dismissed
}

/// `--shadow-menu`, as egui draws a shadow.
fn shadow() -> Shadow {
    Shadow {
        offset: [SHADOW_MENU.x as i8, SHADOW_MENU.y as i8],
        blur: SHADOW_MENU.blur as u8,
        spread: 0,
        color: color(SHADOW_MENU.ink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nav::Place,
        sidebar::rows::{Counts, Today, fixed_rows},
        toasts::Toasts,
        window_layout::{SIDEBAR_DEFAULT, SIDEBAR_MIN, SPLITTER, STATUS_BAR, TOP_BAR},
    };
    use eframe::egui::{Modifiers, PointerButton, Pos2, RawInput};
    use photon_core::grid::GridView;

    const TODAY: Today = Today { month: 7, day: 4 };

    /// Every shape in `shape`.
    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            egui::Shape::Noop => {}
            other => visit(other),
        }
    }

    /// The shell in a window of its own, a frame at a time.
    struct Fixture {
        ctx: egui::Context,
        shell: Shell,
        layout: Layout,
        rows: Vec<Row>,
        toasts: Toasts,
        notice: Option<String>,
        size: egui::Vec2,
        time: f64,
        /// Every text the last frame drew, with where.
        texts: Vec<(String, Rect)>,
        /// The area the last frame gave the content.
        content: Rect,
    }

    impl Fixture {
        fn new() -> Self {
            let counts = Counts {
                starred: 12,
                hidden: 3,
                ..Counts::default()
            };
            Self {
                ctx: egui::Context::default(),
                shell: Shell::default(),
                layout: Layout::default(),
                rows: fixed_rows(&counts, &Place::of(GridView::All), TODAY),
                toasts: Toasts::default(),
                notice: None,
                size: vec2(1280.0, 800.0),
                time: 0.0,
                texts: Vec::new(),
                content: Rect::NOTHING,
            }
        }

        /// One frame with `events`. What the user did is applied as the application
        /// applies it, so that a drag is followed from frame to frame.
        fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let data = ShellData {
                layout: &self.layout,
                rows: &self.rows,
                count: Some("1,234 photos"),
                notice: self.notice.as_deref(),
                toasts: self.toasts.showing(),
            };
            let (shell, mut actions, mut content) = (&mut self.shell, Vec::new(), Rect::NOTHING);
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        actions = shell.show(ui, &data, |ui| content = ui.max_rect());
                    });
            });
            // The font atlas and the icons: nothing here draws to a screen.
            full.textures_delta.clear();
            self.content = content;
            self.texts.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        self.texts
                            .push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                    }
                });
            }
            for action in &actions {
                match action {
                    Action::ToggleSidebar => {
                        self.layout.sidebar_hidden = !self.layout.sidebar_hidden
                    }
                    Action::Width { width, .. } => self.layout.sidebar_width = *width,
                    Action::Dismiss(id) => self.toasts.dismiss(*id),
                    Action::Go(_) => {}
                }
            }
            actions
        }

        fn pointer(&mut self, at: Pos2) -> Vec<Action> {
            self.frame(vec![Event::PointerMoved(at)])
        }

        fn button(&mut self, at: Pos2, pressed: bool) -> Vec<Action> {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])
        }

        /// A press and a release at `at`, and everything it did.
        fn click(&mut self, at: Pos2) -> Vec<Action> {
            let mut actions = self.pointer(at);
            actions.extend(self.button(at, true));
            actions.extend(self.button(at, false));
            actions
        }

        /// `key` going down. egui works out for itself whether that is a repeat: it is
        /// one when the key has not come up since it last went down.
        fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Action> {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        }

        fn key_up(&mut self, key: Key, modifiers: Modifiers) -> Vec<Action> {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            }])
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            self.texts
                .iter()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }

        /// The middle of the sidebar row `index` from the top.
        fn row(&self, index: usize) -> Pos2 {
            pos2(
                100.0,
                TOP_BAR + S[1] + (index as f32 + 0.5) * crate::sidebar::rows::ROW,
            )
        }
    }

    #[test]
    fn the_content_is_given_what_the_bars_and_the_sidebar_leave() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(
            f.content,
            Rect::from_min_max(
                pos2(SIDEBAR_DEFAULT + SPLITTER, TOP_BAR),
                pos2(1280.0, 800.0 - STATUS_BAR)
            )
        );
        // The rows are in the sidebar, the count at the right of the status bar.
        let starred = f.drew("Starred").expect("the row is drawn");
        assert!(starred.right() < SIDEBAR_DEFAULT && starred.top() > TOP_BAR);
        let twelve = f.drew("12").expect("its count is drawn");
        assert!(twelve.right() <= SIDEBAR_DEFAULT - 6.0 && twelve.left() > starred.right());
        let count = f.drew("1,234 photos").expect("the count is drawn");
        assert!(count.top() > 800.0 - STATUS_BAR && count.right() > 1200.0);
    }

    #[test]
    fn a_click_on_a_row_asks_for_it() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.click(f.row(1)), [Action::Go(Fixed::Starred)]);
        assert_eq!(f.click(f.row(0)), [Action::Go(Fixed::All)]);
        // The fifth row of this library is Hidden: Videos and Duplicates hold nothing.
        assert_eq!(f.click(f.row(4)), [Action::Go(Fixed::Hidden)]);
        // Below the last row there is nothing to click.
        assert_eq!(f.click(f.row(7)), []);
    }

    // It names the photo whose copies are shown and does nothing: the view is open.
    #[test]
    fn the_row_that_is_not_a_button_takes_no_click() {
        let mut f = Fixture::new();
        let at = Place {
            view: GridView::Copies,
            arg: "42".to_owned(),
        };
        f.rows = fixed_rows(&Counts::default(), &at, TODAY);
        let copies = f
            .rows
            .iter()
            .position(|row| row.what == Fixed::CopiesOf)
            .unwrap();
        f.frame(Vec::new());
        assert!(f.drew("Copies of a photo").is_some());
        assert_eq!(f.click(f.row(copies)), []);
        assert_eq!(f.click(f.row(copies - 1)), [Action::Go(Fixed::Duplicates)]);
    }

    #[test]
    fn the_toggle_and_its_key_hide_the_sidebar_and_bring_it_back() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let toggle = pos2(S[1] + BUTTON / 2.0, TOP_BAR / 2.0);
        assert_eq!(f.click(toggle), [Action::ToggleSidebar]);
        // Hidden: the content begins at the window's edge, and no row is drawn.
        f.frame(Vec::new());
        assert_eq!(f.content.left(), 0.0);
        assert_eq!(f.drew("Starred"), None);
        assert_eq!(
            f.click(f.row(1)),
            [],
            "a row that is not there takes no click"
        );

        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        f.frame(Vec::new());
        assert_eq!(f.content.left(), SIDEBAR_DEFAULT + SPLITTER);
        assert!(f.drew("Starred").is_some());
    }

    #[test]
    fn the_key_is_not_answered_held_down_or_without_its_modifier() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        // Held down, the key goes down again and again: answered each time, the sidebar
        // would flap.
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        f.key_up(Key::B, Modifiers::COMMAND);
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), [Action::ToggleSidebar]);
        f.key_up(Key::B, Modifiers::COMMAND);
        assert_eq!(f.key(Key::B, Modifiers::NONE), []);
        assert_eq!(f.key(Key::N, Modifiers::COMMAND), []);
    }

    // The gear is where it will be and takes no press: Settings are a later part.
    #[test]
    fn the_gear_is_drawn_and_does_nothing_yet() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let gear = pos2(1280.0 - S[1] - BUTTON / 2.0, TOP_BAR / 2.0);
        assert_eq!(f.click(gear), []);
    }

    #[test]
    fn dragging_the_splitter_moves_the_sidebar_and_stores_the_width_when_it_is_let_go() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        assert_eq!(f.button(hold, true), []);
        // Followed while it is held, and not stored at every point on the way.
        let moved = f.pointer(pos2(hold.x + 60.0, 400.0));
        assert_eq!(
            moved,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 60.0,
                store: false
            }]
        );
        assert!(f.shell.resizing());
        let moved = f.pointer(pos2(hold.x + 100.0, 380.0));
        assert_eq!(
            moved,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 100.0,
                store: false
            }]
        );
        // The sidebar's key is not answered while its edge is held.
        assert_eq!(f.key(Key::B, Modifiers::COMMAND), []);
        let let_go = f.button(pos2(hold.x + 100.0, 380.0), false);
        assert_eq!(
            let_go,
            [Action::Width {
                width: SIDEBAR_DEFAULT + 100.0,
                store: true
            }]
        );
        assert!(!f.shell.resizing());
        f.frame(Vec::new());
        assert_eq!(f.content.left(), SIDEBAR_DEFAULT + 100.0 + SPLITTER);
        // And the next frames, with nothing held, say nothing more.
        assert_eq!(f.pointer(pos2(700.0, 400.0)), []);
    }

    #[test]
    fn a_drag_stops_at_the_minimum_and_at_half_the_window() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let hold = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        f.pointer(hold);
        f.button(hold, true);
        let far_left = f.pointer(pos2(20.0, 400.0));
        assert_eq!(
            far_left,
            [Action::Width {
                width: SIDEBAR_MIN,
                store: false
            }]
        );
        let far_right = f.pointer(pos2(1200.0, 400.0));
        assert_eq!(
            far_right,
            [Action::Width {
                width: 640.0,
                store: false
            }]
        );
    }

    #[test]
    fn the_focused_splitter_is_moved_by_the_arrows_and_each_move_is_stored() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        // A click gives it the focus, and moves nothing.
        let on = pos2(SIDEBAR_DEFAULT + 2.0, 400.0);
        let clicked = f.click(on);
        assert!(
            clicked.iter().all(|action| matches!(
                action,
                Action::Width { width, .. } if *width == SIDEBAR_DEFAULT
            )),
            "{clicked:?}"
        );
        f.frame(Vec::new());
        assert_eq!(
            f.key(Key::ArrowRight, Modifiers::NONE),
            [Action::Width {
                width: SIDEBAR_DEFAULT + 16.0,
                store: true
            }]
        );
        assert_eq!(
            f.key(Key::ArrowLeft, Modifiers::NONE),
            [Action::Width {
                width: SIDEBAR_DEFAULT,
                store: true
            }]
        );
    }

    // Without the focus the arrows are the grid's, or nobody's: not the splitter's.
    #[test]
    fn the_arrows_move_nothing_while_the_splitter_has_no_focus() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::ArrowRight, Modifiers::NONE), []);
    }

    #[test]
    fn the_line_of_an_empty_view_is_in_the_middle_of_the_content() {
        let mut f = Fixture::new();
        f.notice = Some("No starred photos. Star one in the viewer, or in Picasa.".to_owned());
        f.frame(Vec::new());
        let line = f
            .drew("No starred photos. Star one in the viewer, or in Picasa.")
            .expect("the line is drawn");
        assert!(
            (line.center().x - f.content.center().x).abs() < 2.0,
            "{line:?}"
        );
        assert!(
            (line.center().y - f.content.center().y).abs() < 8.0,
            "{line:?}"
        );
    }

    #[test]
    fn a_message_is_drawn_over_the_corner_and_its_button_dismisses_it() {
        let mut f = Fixture::new();
        f.toasts.error("could not read the library", 0.0);
        f.toasts.done("3 photos starred", 0.0);
        // egui measures a new area in a frame it does not show, and shows it in the next.
        f.frame(Vec::new());
        f.frame(Vec::new());
        let error = f.drew("could not read the library").expect("drawn");
        let done = f.drew("3 photos starred").expect("drawn");
        // The newest lowest, both in the bottom right, clear of the status bar.
        assert!(done.top() > error.bottom());
        assert!(done.bottom() < 800.0 - STATUS_BAR && error.right() < 1280.0 - S[3]);
        assert!(error.left() > 800.0);

        // The button is at the message's right end, level with its text.
        let first = f.toasts.showing()[0].id;
        let button = pos2(1280.0 - S[3] - S[2] - TOAST_BUTTON / 2.0, error.center().y);
        assert_eq!(f.click(button), [Action::Dismiss(first)]);
        f.frame(Vec::new());
        assert_eq!(f.drew("could not read the library"), None);
        assert!(f.drew("3 photos starred").is_some());
    }
}
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib shell:: && cargo test -p photon-ui --lib icons:: && cargo test -p photon-ui --lib theme::`
Expected: shell: 12 pass. icons and theme: all pass, with eleven icons and the shadow held to the stylesheet.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/sidebar/view.rs` | `if button && response.clicked() {` | `if button && response.double_clicked() {` | `a_click_on_a_row_asks_for_it` (`cargo test -p photon-ui --lib`) |
| 2 | `src/sidebar/view.rs` | `let button = row.what != Fixed::CopiesOf;` | `let button = true;` | `the_row_that_is_not_a_button_takes_no_click` (`cargo test -p photon-ui --lib`) |
| 3 | `src/shell.rs` | `Event::Key { key: Key::B, pressed: true, repeat: false, modifiers, .. }` | `Event::Key { key: Key::B, pressed: true, modifiers, .. }` | `the_key_is_not_answered_held_down_or_without_its_modifier` (`cargo test -p photon-ui --lib`) |
| 4 | `src/shell.rs` | `if modifiers.command` | `if modifiers.command \|\| !modifiers.any()` | `the_key_is_not_answered_held_down_or_without_its_modifier` (`cargo test -p photon-ui --lib`) |
| 5 | `src/shell.rs` | `if pressed && !self.resizing() {` | `if pressed {` | `dragging_the_splitter_moves_the_sidebar_and_stores_the_width_when_it_is_let_go` (`cargo test -p photon-ui --lib`) |
| 6 | `src/shell.rs` | `self.grab = Some(pressed.x - window.left() - shown);` | `self.grab = Some(0.0);` | `dragging_the_splitter_moves_the_sidebar_and_stores_the_width_when_it_is_let_go` (`cargo test -p photon-ui --lib`) |
| 7 | `src/shell.rs` | `self.grab = None; ⏎             actions.push(Action::Width { ⏎                 width: shown, ⏎                 store: true, ⏎             });` | `self.grab = None;` | `dragging_the_splitter_moves_the_sidebar_and_stores_the_width_when_it_is_let_go` (`cargo test -p photon-ui --lib`) |
| 8 | `src/shell.rs` | `let width = clamp_sidebar_width(pointer.x - window.left() - grab, window.width());` | `let width = (pointer.x - window.left() - grab).round();` | `a_drag_stops_at_the_minimum_and_at_half_the_window` (`cargo test -p photon-ui --lib`) |
| 9 | `src/shell.rs` | `if response.has_focus() { ⏎             // The arrows are` | `if true { ⏎             // The arrows are` | `the_arrows_move_nothing_while_the_splitter_has_no_focus` (`cargo test -p photon-ui --lib`) |
| 10 | `src/shell.rs` | `let width = clamp_sidebar_width(shown + step * SIDEBAR_STEP, window.width());` | `let width = clamp_sidebar_width(shown + step, window.width());` | `the_focused_splitter_is_moved_by_the_arrows_and_each_move_is_stored` (`cargo test -p photon-ui --lib`) |
| 11 | `src/shell.rs` | `let left = place.center().x - width / 2.0;` | `let left = place.left() + S[3];` | `the_line_of_an_empty_view_is_in_the_middle_of_the_content` (`cargo test -p photon-ui --lib`) |
| 12 | `src/shell.rs` | `for toast in list.iter().rev() {` | `for toast in list.iter() {` | `a_message_is_drawn_over_the_corner_and_its_button_dismisses_it` (`cargo test -p photon-ui --lib`) |
| 13 | `src/shell.rs` | `dismissed = Some(toast.id);` | `dismissed = None;` | `a_message_is_drawn_over_the_corner_and_its_button_dismisses_it` (`cargo test -p photon-ui --lib`) |
| 14 | `src/shell.rs` | `ui.scope_builder(UiBuilder::new().max_rect(place), \|ui\| {` | `ui.scope_builder(UiBuilder::new().max_rect(window), \|ui\| {` | `the_content_is_given_what_the_bars_and_the_sidebar_leave` (`cargo test -p photon-ui --lib`) |
| 15 | `src/theme/tokens.rs` | `y: 6.0, ⏎     blur: 24.0,` | `y: 5.0, ⏎     blur: 24.0,` | `the_tokens_are_the_stylesheets` (`cargo test -p photon-ui --lib`) |

Each of the 15 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/lib.rs crates/photon-ui/src/icons.rs crates/photon-ui/src/theme/tokens.rs crates/photon-ui/src/sidebar/view.rs crates/photon-ui/src/shell.rs
git commit -m "feat(ui): the top bar, the sidebar, the splitter and the status bar, drawn

A view: it draws what it is given and answers what the user did. Ctrl+B and the
toggle hide the sidebar, not on a key held down and not while the splitter is
held. The splitter is taken hold of where the button went down, follows the
pointer between the minimum and half the window, and says when it was let go;
focused, the arrows move it. The gear is drawn where it will be and takes no
press. Seven more icons, held to the Svelte UI's.

15 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The grid goes back to its top, and leaves the keys to a text field

**Files:**
- Modify: `crates/photon-ui/src/grid/view.rs`

**Interfaces:**
- Consumes: nothing of this plan's.
- Produces: `GridView::to_top(&mut self)` (the widget in `grid/view.rs`, not `photon_core`'s enum of the same name). And `take_input` answers Home, End and the page keys only while no text field has the focus.

There is no text field in the window until the search box, in the third pull request; the rule and its test are here because the shell is what makes the grid one thing among several.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/grid/view.rs`:

````diff
--- a/crates/photon-ui/src/grid/view.rs
+++ b/crates/photon-ui/src/grid/view.rs
@@ -96,6 +96,14 @@
     /// sweep as if it had always been standing there.
     pub fn move_to(&mut self, position: f64) {
         self.asked = Some(position);
+    }
+
+    /// Puts the grid at its top, which is no scroll: the photos shown are another list -
+    /// another view, another search, another order - and a place in the old one names
+    /// nothing in this one.
+    pub fn to_top(&mut self) {
+        self.asked = None;
+        self.scroll.set(0.0);
     }
 
     pub fn scroll_by(&mut self, delta: f64) {
@@ -214,6 +222,11 @@
             }
         }
 
+        // The keys are the grid's unless a text field has them: with the search box
+        // focused, Home and End move its caret and not the library.
+        if ui.ctx().text_edit_focused() {
+            return;
+        }
         let row = self.laid_out.map_or(0.0, |laid| tile_row(laid.tile));
         let page = (viewport - row).max(ARROW_STEP);
         let scroll = &mut self.scroll;
@@ -472,6 +485,8 @@
         repaint_delay: Duration,
         /// Room left above the grid, as a bar over it would take.
         space_above: f32,
+        /// A text field above the grid that has the focus, and what is typed in it.
+        typing: Option<String>,
         /// What the last frame drew: each text with where it is and what it is clipped
         /// to, and the top of every clip rectangle anything was drawn in.
         texts: Vec<(String, Rect, Rect)>,
@@ -534,6 +549,7 @@
             time: 0.0,
             repaint_delay: Duration::MAX,
             space_above: 0.0,
+            typing: None,
             texts: Vec::new(),
             clip_tops: Vec::new(),
         }
@@ -557,6 +573,7 @@
                 zone: &TimeZone::UTC,
             };
             let (view, thumbs, space) = (&mut self.view, &mut self.thumbs, self.space_above);
+            let typing = &mut self.typing;
             let mut output = None;
             let mut full = self.ctx.run_ui(input, |ui| {
                 egui::CentralPanel::default()
@@ -564,6 +581,9 @@
                     .show(ui, |ui| {
                         ui.spacing_mut().item_spacing = Vec2::ZERO;
                         ui.add_space(space);
+                        if let Some(typed) = typing {
+                            ui.add(egui::TextEdit::singleline(typed)).request_focus();
+                        }
                         output = Some(view.show(ui, &data, thumbs));
                     });
             });
@@ -968,4 +988,40 @@
         assert!(f.view.speed.settles_at().is_some());
         assert!(f.repaint_delay < Duration::from_secs(1));
     }
+
+    // Another view, another search, another order: the photos are another list, and the
+    // place held in the old one names nothing in this one.
+    #[test]
+    fn other_results_are_shown_from_their_top() {
+        let mut f = fixture(&[4000]);
+        f.frame(Vec::new());
+        f.view.move_to(5000.0);
+        assert_eq!(f.frame(Vec::new()).position, 5000.0);
+        // With a move still asked for and not yet taken: that was for the old list too.
+        f.view.move_to(9000.0);
+        f.view.to_top();
+        assert_eq!(f.frame(Vec::new()).position, 0.0);
+        assert_eq!(f.frame(Vec::new()).position, 0.0);
+    }
+
+    // With the search box focused, Home and End move its caret, and Page Down is not the
+    // library's either: the grid has the keys only while no text field does.
+    #[test]
+    fn the_keys_are_a_text_fields_while_it_has_the_focus() {
+        let mut f = fixture(&[4000]);
+        f.frame(Vec::new());
+        f.typing = Some(String::new());
+        // The field asks for the focus in one frame and has it in the next.
+        f.frame(Vec::new());
+        f.frame(Vec::new());
+        assert_eq!(f.key(Key::End).position, 0.0);
+        assert_eq!(f.key(Key::PageDown).position, 0.0);
+
+        // The field gone, the keys are the grid's again.
+        f.typing = None;
+        f.frame(Vec::new());
+        f.frame(Vec::new());
+        assert!(f.key(Key::PageDown).position > 0.0);
+        assert_eq!(f.key(Key::End).position, f.view.max_position());
+    }
 }
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui --lib grid::view`
Expected: 20 pass.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/grid/view.rs` | `self.asked = None; ⏎         self.scroll.set(0.0);` | `self.scroll.set(0.0);` | `other_results_are_shown_from_their_top` (`cargo test -p photon-ui --lib`) |
| 2 | `src/grid/view.rs` | `if ui.ctx().text_edit_focused() { ⏎             return; ⏎         }` | *(nothing: the lines are taken out)* | `the_keys_are_a_text_fields_while_it_has_the_focus` (`cargo test -p photon-ui --lib`) |

Each of the 2 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/grid/view.rs
git commit -m "feat(ui): the grid starts other results at their top, and leaves the keys to a text field

A position in one list of photos names nothing in another: a new view, search or
order is shown from its top, and a move still asked for is forgotten with it.
Home, End and the page keys are a text field's while it has the focus.

2 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: The application

**Files:**
- Modify: `crates/photon-ui/src/app.rs`
- Create: `crates/photon-ui/tests/common/mod.rs`
- Create: `crates/photon-ui/tests/shell.rs`

**Interfaces:**
- Consumes: everything above; `commands::{set_grid_view, set_search_query, grid_info, set_items_hidden}`; `Engine::{published, view_and_arg, sort}`.
- Produces, on `App`: `engine()`, `place() -> Place`, `settled() -> &Place`, `counts() -> &Counts`, `layout() -> &Layout`, `toasts() -> &[Toast]`, `version() -> u64`, `photo_count() -> Option<String>`, `notice() -> Option<String>`. In `tests/common`: `Driver` (`new`, `frame`, `act`, `until`, `settle`, `frames_in`), `sources`, `library(dir, photos, starred, hidden)`, `launch`.

`tests/probe.rs` keeps its own loop: it puts two frames of its own into the idle step, and is left as it is.

- [ ] **Step 1: The code and its tests**

The tests are in the files they test, but for Task 7's.

Change `crates/photon-ui/src/app.rs`:

````diff
--- a/crates/photon-ui/src/app.rs
+++ b/crates/photon-ui/src/app.rs
@@ -1,20 +1,23 @@
-//! The application: the engine, the grid, and the order of a frame.
+//! The application: the engine, the shell around the grid, and the order of a frame.
 
 use crate::{
     dirs::Dirs,
+    empty::{self, GridState},
     events::{Event, UiEvents},
     grid::{
         view::{GridData, GridOutput, GridView},
         visible::VisibleReport,
     },
     icons,
+    nav::{Nav, Place, Step},
     probe::{Facts, Move, Outside, Probe, Report},
-    tasks::Latest,
-    theme::{
-        self,
-        apply::{color, palette},
-    },
+    shell::{Action, Shell, ShellData},
+    sidebar::rows::{Counts, Today, fixed_rows},
+    tasks::{Latest, Queue},
+    theme,
     thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
+    toasts::{Toast, Toasts},
+    window_layout::Layout,
 };
 use eframe::egui;
 use jiff::tz::TimeZone;
@@ -44,8 +47,24 @@
     /// never per frame: `published` takes the engine's lock.
     index: Arc<GridIndex>,
     layout_gen: u64,
+    /// The published grid's version and length, and why it is empty when it could not be
+    /// read: what the rules about saying nothing yet are asked of.
+    grid: GridState,
     folders: HashMap<i64, Folder>,
     folder_list: Latest<(), Option<Vec<Folder>>>,
+    /// Where the user is and is going, and the queue the steps there are made on.
+    nav: Nav,
+    steps: Queue<Step, Result<(), String>>,
+    /// What the library holds of each kind, for the sidebar. Asked for by the layout
+    /// generation held, so that the answer need not carry the layout.
+    counts: Counts,
+    counting: Latest<u64, Counts>,
+    today: Today,
+    shell: Shell,
+    layout: Layout,
+    /// Writes the layout to its file, off this thread, the latest alone.
+    layout_store: Latest<Layout, ()>,
+    toasts: Toasts,
     size: GridTile,
     zone: TimeZone,
     visible: VisibleReport,
@@ -93,6 +112,11 @@
         theme::fonts::install(&ctx);
 
         let (events, receiver) = UiEvents::new(ctx.clone());
+        // How the window was left, here and not by a task, for the reason the theme is
+        // read here: read after the first frame, the sidebar would open at the default
+        // width and jump.
+        let layout = Layout::load(&dirs.layout_path);
+        let layout_path = dirs.layout_path;
         let config = EngineConfig {
             db_path: dirs.db_path,
             cache_dir: dirs.cache_dir,
@@ -134,18 +158,80 @@
         );
         folder_list.ask(());
 
-        let (_, index, _, layout_gen) = engine.published();
+        // A step that moves the engine's view rebuilds the grid where it is made: here,
+        // one at a time and in the order asked.
+        let steps = Queue::spawn(
+            "views",
+            {
+                let engine = engine.clone();
+                move |step: Step| {
+                    match step {
+                        Step::View(view) => commands::set_grid_view(&engine, view),
+                        Step::Search(query) => commands::set_search_query(&engine, &query),
+                    }
+                    .map(|_| ())
+                    .map_err(|err| err.message)
+                }
+            },
+            repaint(&ctx),
+        );
+        let mut counting = Latest::spawn(
+            "counts",
+            {
+                let engine = engine.clone();
+                move |layout_gen: u64| {
+                    let info = commands::grid_info(&engine, Some(layout_gen));
+                    Counts {
+                        starred: info.starred_count,
+                        duplicates: info.duplicate_count,
+                        hidden: info.hidden_count,
+                        videos: info.video_count,
+                        copies_of: info.copies_of.map(|photo| photo.file_name),
+                    }
+                }
+            },
+            repaint(&ctx),
+        );
+        // Nothing is drawn from this, so nothing is asked to be drawn for it.
+        let layout_store = Latest::spawn(
+            "layout",
+            move |layout: Layout| {
+                if let Err(err) = layout.save(&layout_path) {
+                    tracing::warn!(%err, path = %layout_path.display(), "the window's layout was not stored");
+                }
+            },
+            || {},
+        );
+
+        let (version, index, build_error, layout_gen) = engine.published();
+        counting.ask(layout_gen);
+        let (view, arg) = engine.view_and_arg();
+        let zone = TimeZone::system();
         Ok(Self {
-            engine,
+            engine: engine.clone(),
             events: receiver,
             view: GridView::default(),
             thumbs: Thumbs::new(loader),
+            grid: GridState {
+                version,
+                len: index.len(),
+                build_error,
+            },
             index,
             layout_gen,
             folders: HashMap::new(),
             folder_list,
+            nav: Nav::new(Place { view, arg }, engine.sort()),
+            steps,
+            counts: Counts::default(),
+            counting,
+            today: today(&zone),
+            shell: Shell::default(),
+            layout,
+            layout_store,
+            toasts: Toasts::default(),
             size,
-            zone: TimeZone::system(),
+            zone,
             visible: VisibleReport::default(),
             last: None,
             probe: None,
@@ -180,6 +266,11 @@
         self
     }
 
+    /// The engine under the interface.
+    pub fn engine(&self) -> &Arc<Engine> {
+        &self.engine
+    }
+
     /// The GPU the window is drawn with, as wgpu describes it.
     pub fn adapter(&self) -> Option<&str> {
         self.adapter.as_deref()
@@ -203,6 +294,79 @@
     /// The folders the headers are named from.
     pub fn folders(&self) -> &HashMap<i64, Folder> {
         &self.folders
+    }
+
+    /// Where the user is: the place the last step asked leads to, or the one shown.
+    pub fn place(&self) -> Place {
+        self.nav.target()
+    }
+
+    /// The place the published grid shows.
+    pub fn settled(&self) -> &Place {
+        self.nav.settled()
+    }
+
+    /// What the library holds of each kind, as the sidebar last learnt it.
+    pub fn counts(&self) -> &Counts {
+        &self.counts
+    }
+
+    /// How the window is laid out.
+    pub fn layout(&self) -> &Layout {
+        &self.layout
+    }
+
+    /// The messages showing.
+    pub fn toasts(&self) -> &[Toast] {
+        self.toasts.showing()
+    }
+
+    /// The version of the grid on screen. Every grid the engine publishes has another.
+    pub fn version(&self) -> u64 {
+        self.grid.version
+    }
+
+    /// The status bar's count of the photos shown, when there is one to give.
+    pub fn photo_count(&self) -> Option<String> {
+        empty::photo_count(&self.grid)
+    }
+
+    /// The line shown in place of photos. It is about the grid that is on screen, not
+    /// about the view being gone to.
+    pub fn notice(&self) -> Option<String> {
+        empty::view_notice(&self.grid, self.nav.settled())
+    }
+
+    /// Asks for `step`, unless it leads to where the user already is.
+    fn go(&mut self, step: Step) {
+        if !self.nav.wants(&step) {
+            return;
+        }
+        let number = self.steps.push(step.clone());
+        self.nav.asked(number, step);
+    }
+
+    fn act(&mut self, action: Action) {
+        match action {
+            Action::ToggleSidebar => {
+                self.layout.sidebar_hidden = !self.layout.sidebar_hidden;
+                self.layout_store.ask(self.layout);
+            }
+            Action::Go(row) => {
+                // The clock itself, not the day the row was drawn with: a click is never a
+                // day behind.
+                if let Some(step) = row.step(today(&self.zone)) {
+                    self.go(step);
+                }
+            }
+            Action::Width { width, store } => {
+                self.layout.sidebar_width = width;
+                if store {
+                    self.layout_store.ask(self.layout);
+                }
+            }
+            Action::Dismiss(id) => self.toasts.dismiss(id),
+        }
     }
 
     /// Takes what the engine has reported since the last frame, and says whether it had
@@ -220,9 +384,24 @@
             }
         }
         if changed {
-            let (_, index, _, layout_gen) = self.engine.published();
+            let (version, index, build_error, layout_gen) = self.engine.published();
+            self.grid = GridState {
+                version,
+                len: index.len(),
+                build_error,
+            };
             self.index = index;
             self.layout_gen = layout_gen;
+            // Other results are another list: a place in the old one names nothing here.
+            let (view, arg) = self.engine.view_and_arg();
+            if self.nav.settle(Place { view, arg }, self.engine.sort()) {
+                self.view.to_top();
+            }
+            // The counts read SQLite when they are not cached, so they are asked for.
+            self.counting.ask(layout_gen);
+            // And today is read again, so that a window left open past midnight catches
+            // up the next time anything in the library moves.
+            self.today = today(&self.zone);
         }
         // A folder renamed, added or given an alias: the headers are named from the list.
         if data_changed {
@@ -231,12 +410,26 @@
         reported
     }
 
-    fn take_answers(&mut self) {
+    fn take_answers(&mut self, now_ms: f64) {
         if let Some(Ok(Some(folders))) = self.folder_list.answer() {
             self.folders = folders
                 .into_iter()
                 .map(|folder| (folder.id, folder))
                 .collect();
+        }
+        if let Some(Ok(counts)) = self.counting.answer() {
+            self.counts = counts;
+        }
+        let _ = self.layout_store.answer();
+        for (number, answer) in self.steps.answers() {
+            let refused = match answer {
+                Ok(Ok(())) => None,
+                Ok(Err(why)) => Some(why),
+                Err(_) => Some("photon could not change the view.".to_owned()),
+            };
+            if let Some(said) = self.nav.answered(number, refused) {
+                self.toasts.error(said, now_ms);
+            }
         }
     }
 
@@ -304,7 +497,8 @@
         // reported, or by the person, whose mouse or keys are in the frame's input.
         let touched = ui.input(|input| !input.events.is_empty());
         let reported = self.take_events() || touched;
-        self.take_answers();
+        let now = ui.input(|input| input.time) * 1000.0;
+        self.take_answers(now);
 
         let data = GridData {
             layout_gen: self.layout_gen,
@@ -313,14 +507,32 @@
             size: self.size,
             zone: &self.zone,
         };
-        let surface = color(palette(ui.ctx()).surface);
+        // The row marked is where the user is going; the line an empty view shows is
+        // about the grid that is on screen.
+        let rows = fixed_rows(&self.counts, &self.nav.target(), self.today);
+        let count = self.photo_count();
+        let notice = self.notice();
+        let shell = ShellData {
+            layout: &self.layout,
+            rows: &rows,
+            count: count.as_deref(),
+            notice: notice.as_deref(),
+            toasts: self.toasts.showing(),
+        };
         let (view, thumbs) = (&mut self.view, &mut self.thumbs);
         let mut output = None;
-        egui::CentralPanel::default()
-            .frame(egui::Frame::NONE.fill(surface))
-            .show(ui, |ui| output = Some(view.show(ui, &data, thumbs)));
-
-        let now = ui.input(|input| input.time) * 1000.0;
+        let actions = self.shell.show(ui, &shell, |ui| {
+            output = Some(view.show(ui, &data, thumbs));
+        });
+        for action in actions {
+            self.act(action);
+        }
+        // A still window draws no frame by itself, and a message would stay.
+        if let Some(at) = self.toasts.tick(now) {
+            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
+            ui.ctx().request_repaint_after(wait);
+        }
+
         if let Some(output) = &output {
             if let Some(ids) = self.visible.update(&output.on_screen, now) {
                 commands::set_visible(&self.engine, ids);
@@ -343,6 +555,15 @@
 
     fn on_exit(&mut self) {
         self.close();
+    }
+}
+
+/// Today in `zone`, as "On this day" means it.
+fn today(zone: &TimeZone) -> Today {
+    let now = jiff::Timestamp::now().to_zoned(zone.clone());
+    Today {
+        month: now.month().unsigned_abs().into(),
+        day: now.day().unsigned_abs().into(),
     }
 }
 
````

Create `crates/photon-ui/tests/common/mod.rs`:

````rust
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
````

Create `crates/photon-ui/tests/shell.rs`:

````rust
//! The shell and the views, through the whole application: a library on disk, the engine,
//! and the frames a window would draw (`common::Driver`).

mod common;

use common::{Driver, Harness, launch, library};
use eframe::egui::{self, Key, Modifiers, pos2, vec2};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;
use photon_ui::{
    nav::Place,
    sidebar::rows::Counts,
    window_layout::{Layout, SIDEBAR_DEFAULT},
};
use std::time::{Duration, Instant};

/// Clicks the sidebar row called `label`, and draws the frames the click is in.
fn click(driver: &mut Driver, harness: &mut Harness<'_>, label: &str) {
    harness.get_by_label(label).click();
    driver.act(harness);
}

/// Launches over `library` and waits until its grid and its counts are there.
fn opened<'a>(library: &photon_ui::fixture::Fixture, photos: usize) -> (Harness<'a>, Driver) {
    let mut harness = launch(library);
    let mut driver = Driver::new(&harness);
    driver.until(&mut harness, "the library shown", |app| {
        app.photos() == photos && app.last_frame().is_some_and(|frame| frame.settled)
    });
    (harness, driver)
}

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
    // Where the user is going, before the rebuild has landed.
    assert_eq!(harness.state().place(), Place::of(GridView::Starred));
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

// Two clicks before the first has landed. Each is a rebuild on the engine's side that
// cannot be taken back once begun, so they are made in the order asked: answered by the
// latest alone, the first could land last, under a row marking the second.
#[test]
fn clicks_land_in_the_order_they_were_made() {
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
````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p photon-ui`
Expected: everything passes: `tests/shell.rs` has 10, `tests/probe.rs` 4 and 1 ignored, `tests/app.rs` 2.

- [ ] **Step 3: Probe each rule**

| # | In | Replace | With | Then this test fails |
|---|---|---|---|---|
| 1 | `src/app.rs` | `if !self.nav.wants(&step) { ⏎             return; ⏎         }` | *(nothing: the lines are taken out)* | `a_click_on_the_row_that_is_shown_asks_for_nothing` (`cargo test -p photon-ui --test shell`) |
| 2 | `src/app.rs` | `if self.nav.settle(Place { view, arg }, self.engine.sort()) { ⏎                 self.view.to_top(); ⏎             }` | `self.nav.settle(Place { view, arg }, self.engine.sort());` | `another_view_is_shown_from_its_top` (`cargo test -p photon-ui --test shell`) |
| 3 | `src/app.rs` | `// The counts read SQLite when they are not cached, so they are asked for. ⏎             self.counting.ask(layout_gen);` | *(nothing: the lines are taken out)* | `the_counts_follow_the_library` (`cargo test -p photon-ui --test shell`) |
| 4 | `src/app.rs` | `self.counts = counts;` | `let _ = counts;` | `a_click_on_a_view_shows_it_and_the_row_follows_at_once` (`cargo test -p photon-ui --test shell`) |
| 5 | `src/app.rs` | `empty::view_notice(&self.grid, self.nav.settled())` | `None` | `an_empty_view_says_what_it_is_empty_of` (`cargo test -p photon-ui --test shell`) |
| 6 | `src/app.rs` | `self.grid = GridState { ⏎                 version, ⏎                 len: index.len(), ⏎                 build_error, ⏎             }; ⏎             self.index = index;` | `let _ = (version, build_error); ⏎             self.index = index;` | `a_click_on_a_view_shows_it_and_the_row_follows_at_once` (`cargo test -p photon-ui --test shell`) |
| 7 | `src/app.rs` | `self.layout.sidebar_hidden = !self.layout.sidebar_hidden; ⏎                 self.layout_store.ask(self.layout);` | `self.layout.sidebar_hidden = !self.layout.sidebar_hidden;` | `a_sidebar_hidden_is_hidden_at_the_next_launch` (`cargo test -p photon-ui --test shell`) |
| 8 | `src/app.rs` | `if store { ⏎                     self.layout_store.ask(self.layout); ⏎                 }` | `let _ = store;` | `the_sidebar_is_as_wide_at_the_next_launch_as_it_was_dragged` (`cargo test -p photon-ui --test shell`) |
| 9 | `src/app.rs` | `self.layout.sidebar_width = width;` | *(nothing: the lines are taken out)* | `the_sidebar_is_as_wide_at_the_next_launch_as_it_was_dragged` (`cargo test -p photon-ui --test shell`) |
| 10 | `src/app.rs` | `let layout = Layout::load(&dirs.layout_path);` | `let layout = Layout::default();` | `a_sidebar_hidden_is_hidden_at_the_next_launch` (`cargo test -p photon-ui --test shell`) |
| 11 | `src/app.rs` | `let actions = self.shell.show(ui, &shell, \|ui\| {` | `ui.ctx().request_repaint(); ⏎         let actions = self.shell.show(ui, &shell, \|ui\| {` | `a_still_window_draws_nothing` (`cargo test -p photon-ui --test shell`) |
| 12 | `src/app.rs` | `Step::Search(query) => commands::set_search_query(&engine, &query),` | `Step::Search(_) => commands::set_grid_view(&engine, photon_core::grid::GridView::All),` | `on_this_day_is_a_search_for_todays_date` (`cargo test -p photon-ui --test shell`) |

Each of the 12 must fail its test. Restore and `touch` after each.

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/src/app.rs crates/photon-ui/tests/common/mod.rs crates/photon-ui/tests/shell.rs
git commit -m "feat(ui): the shell around the grid, and the views behind the sidebar's rows

A click on a row is a step on the queue; the row follows at once and the grid
when its rebuild lands. The counts are read off the UI thread, at launch and at
every change. Other results are shown from their top. The window's layout is
read before the first frame and written off the thread when the user changes it.

The whole-application tests drive it as an event loop does (tests/common's
Driver): frames only when asked for, on a clock that jumps, the other threads
waited for before it does. A still window draws nothing in five seconds.

Not held by a test: that the row drawn is the one gone to (the tests read the
same call, not the row), and that a step the engine refuses becomes a toast
(nothing in a test can make it refuse).

12 probes, each an exact replacement, each failing its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Pictures, and what is here

**Files:**
- Modify: `crates/photon-ui/tests/screenshots.rs`, `crates/xtask/src/native_shot.rs`, `CLAUDE.md`

**Interfaces:**
- Consumes: Task 7's `App::settled` and `App::layout`; the rows' names.
- Produces: `cargo run -p xtask -- native-shot` writes `target/screenshots/native-main-light.png`, `native-main-dark.png`, `native-starred-empty-light.png` and `native-sidebar-hidden-dark.png`.

- [ ] **Step 1: The pictures**

Change `crates/photon-ui/tests/screenshots.rs`:

````diff
--- a/crates/photon-ui/tests/screenshots.rs
+++ b/crates/photon-ui/tests/screenshots.rs
@@ -1,5 +1,5 @@
-//! The grid as it is drawn, written to PNG without a window: the application itself, over
-//! a library made of the CC0 photos the Svelte screenshots use (credited in their
+//! The window as it is drawn, written to PNG without a window: the application itself,
+//! over a library made of the CC0 photos the Svelte screenshots use (credited in their
 //! `CREDITS.md`), rendered off screen through wgpu.
 //!
 //! Ignored by default - it needs a GPU adapter, which a CI runner need not have - and run
@@ -9,8 +9,12 @@
 //! Three of the folders are named in other scripts on purpose. What their headers show is
 //! what the pull request reports about text (`src/text.rs`).
 
-use eframe::egui::{self, vec2};
-use photon_core::library::{Library, ThemeChoice};
+use eframe::egui::{self, Key, Modifiers, vec2};
+use egui_kittest::kittest::Queryable;
+use photon_core::{
+    grid::GridView,
+    library::{Library, ThemeChoice},
+};
 use photon_ui::{app::App, dirs};
 use std::{
     path::{Path, PathBuf},
@@ -48,7 +52,23 @@
     }
 }
 
-fn shot(theme: ThemeChoice, name: &str) {
+type Harness<'a> = egui_kittest::Harness<'a, App>;
+
+/// Steps the application until `shown`, giving its other threads the time they take.
+fn until(harness: &mut Harness<'_>, what: &str, shown: impl Fn(&App) -> bool) {
+    let deadline = Instant::now() + Duration::from_secs(120);
+    loop {
+        harness.step();
+        if shown(harness.state()) {
+            break;
+        }
+        assert!(Instant::now() < deadline, "never: {what}");
+        std::thread::sleep(Duration::from_millis(10));
+    }
+}
+
+/// The library opened in `theme`, then whatever `then` does to it, as `name`.
+fn shot(theme: ThemeChoice, name: &str, then: impl Fn(&mut Harness<'_>)) {
     let dir = tempfile::tempdir().unwrap();
     let pictures = dir.path().join("Pictures");
     library(&pictures);
@@ -74,17 +94,11 @@
         .build_eframe(|cc| App::new(cc, dirs, Some(pictures)).unwrap());
 
     let total: usize = FOLDERS.iter().map(|(_, count)| count).sum();
-    let deadline = Instant::now() + Duration::from_secs(120);
-    loop {
-        harness.step();
-        let app = harness.state();
+    until(&mut harness, "the library shown", |app| {
         let shown = app.last_frame().is_some_and(|frame| frame.settled);
-        if app.photos() == total && app.folders().len() >= FOLDERS.len() && shown {
-            break;
-        }
-        assert!(Instant::now() < deadline, "the library never came up");
-        std::thread::sleep(Duration::from_millis(10));
-    }
+        app.photos() == total && app.folders().len() >= FOLDERS.len() && shown
+    });
+    then(&mut harness);
     // The icons are rasterised a frame after they are first asked for.
     for _ in 0..5 {
         harness.step();
@@ -99,12 +113,43 @@
 
 #[test]
 #[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
-fn native_grid_light() {
-    shot(ThemeChoice::Light, "native-grid-light.png");
+fn native_main_light() {
+    shot(ThemeChoice::Light, "native-main-light.png", |_| {});
 }
 
 #[test]
 #[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
-fn native_grid_dark() {
-    shot(ThemeChoice::Dark, "native-grid-dark.png");
+fn native_main_dark() {
+    shot(ThemeChoice::Dark, "native-main-dark.png", |_| {});
 }
+
+// A view with nothing in it: the row marked, the line in the middle, the count at none.
+#[test]
+#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
+fn native_starred_empty_light() {
+    shot(
+        ThemeChoice::Light,
+        "native-starred-empty-light.png",
+        |harness| {
+            harness.get_by_label("Starred").click();
+            until(harness, "the starred view shown", |app| {
+                app.settled().view == GridView::Starred
+            });
+        },
+    );
+}
+
+#[test]
+#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
+fn native_sidebar_hidden_dark() {
+    shot(
+        ThemeChoice::Dark,
+        "native-sidebar-hidden-dark.png",
+        |harness| {
+            harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
+            until(harness, "the sidebar hidden", |app| {
+                app.layout().sidebar_hidden
+            });
+        },
+    );
+}
````

Change `crates/xtask/src/native_shot.rs`:

````diff
--- a/crates/xtask/src/native_shot.rs
+++ b/crates/xtask/src/native_shot.rs
@@ -1,5 +1,5 @@
-//! `cargo run -p xtask -- native-shot`: the native grid as two PNGs, light and dark, in
-//! `target/screenshots/`.
+//! `cargo run -p xtask -- native-shot`: the native window as PNGs, in
+//! `target/screenshots/native-*.png`.
 //!
 //! The rendering is a test of photon-ui (`tests/screenshots.rs`), ignored by default and
 //! run from here, so that xtask does not itself depend on a GPU stack. Like `screenshots`
````

- [ ] **Step 2: Take them, and look**

```bash
rm -f target/screenshots/native-*.png
cargo run -p xtask -- native-shot
```

Expected: four tests pass and four PNGs are written. Read each one. What to see: the top bar with the toggle at its left and a fainter gear at its right; the sidebar on the chrome colour with All photos filled; a line between the sidebar and the grid; the count at the right of the status bar; in the starred picture, Starred filled, its count 0, and the line "No starred photos. Star one in the viewer, or in Picasa." in the middle of the content; in the hidden picture, the grid from the window's left edge. The names in Arabic, Hebrew and Japanese in the headers are as they were.

Say in the pull request what was seen, and attach nothing: the pictures are built by whoever wants them.

- [ ] **Step 3: CLAUDE.md**

Change `CLAUDE.md`:

````diff
--- a/CLAUDE.md
+++ b/CLAUDE.md
@@ -1289,7 +1289,7 @@
 `crates/photon-ui` is the interface being rebuilt in Rust on egui and wgpu, in place of
 `ui/` and the Tauri shell (spec `2026-10-09-photon-native-ui-design.md`). Its binary is
 `photon-native` until the switch-over. **It is not launched to verify a change either**:
-`cargo run -p xtask -- native-shot` writes the grid to `target/screenshots/native-grid-*.png`
+`cargo run -p xtask -- native-shot` writes the window to `target/screenshots/native-*.png`
 off screen, and those are read. It needs a GPU adapter (no display), so it is an ignored
 test that xtask runs, not part of the gate.
 
@@ -1350,6 +1350,39 @@
 at every launch. A screenshot sets its theme twice, stored in the library and given to the
 `egui_kittest` harness, because the harness sets one of its own (dark) after the
 application has been made.
+
+**The window is four areas around the grid** (`window_layout.rs`: the top bar, the sidebar
+with its splitter, the status bar, the content), drawn by `shell.rs`, a view that answers
+what the user did as a list of `Action`s; `app.rs` is the one place an action changes
+anything. What needs a later sub-project is drawn in place and takes no press (the gear):
+a button that will be there is there, so nothing moves when it starts to work
+(spec `2026-10-10-photon-native-sidebar-views-search-design.md`).
+
+**A step that moves the engine's view goes through `tasks::Queue`, never `Latest`.** The
+view setters rebuild the grid on the thread that calls them, and a step begun cannot be
+taken back, so they are made in the order asked; answered by the latest alone, a search
+already sent lands after the click that emptied its box. `nav.rs` holds where the user is:
+`settled` is what the published grid shows, read from the engine when it says the library
+changed, and `target` is where the last step asked leads - which is what the sidebar
+marks, at the click and not when a rebuild of the whole library has landed. A step to
+where the user is already going is not asked (`Nav::wants`). `grid_info` reads SQLite on a
+cache miss although it returns no `CmdResult`: the sidebar's counts are a `Latest`, asked
+again at every change.
+
+**How the window is laid out is in `layout.json` beside `library.db`** - the sidebar's
+width, whether it is hidden, which groups are open - and not in the settings table, whose
+accessors are photon-core's and which the Tauri photon reads too. It is read before the
+first frame and written off the UI thread when the user changes one of the three; the
+width is stored as it was dragged and clamped where it is shown.
+
+**Tests of the whole application drive it through `tests/common`'s `Driver`**: `until`
+draws the frames that are asked for, `settle` waits in real time for the other threads,
+`frames_in` counts what a stretch of the clock draws. A row is clicked by its name
+(`harness.get_by_label("Starred").click()`), which is why every row and button tells
+AccessKit what it is (`widget_info`) although AccessKit itself is switched on only in
+sub-project 7. Two things about egui's input that a frame test meets: egui works out for
+itself whether a key going down is a repeat, whatever the event says, and it shows a new
+`Area` - a toast - only in the frame after the one it measures it in.
 
 **`cargo run -p xtask -- grid-gate` is the one command that launches the application**, and
 it does nothing unless told which: `--go --refresh-hz <hz>` opens fullscreen windows that
````

- [ ] **Step 4: The gate, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

```bash
git add crates/photon-ui/tests/screenshots.rs crates/xtask/src/native_shot.rs CLAUDE.md
git commit -m "docs: the native shell in pictures, and its rules in CLAUDE.md

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

## After the last task

One independent review of the whole branch, then the pull request into `native-ui`. The reviewer is pointed at the Review Focus above, at `App::ui` and `take_events` (what the shell arms in the application's old code: the gate's probe still runs through them, and `tests/probe.rs` must still pass), and at whether a view is ever asked for on the UI thread.

Then the second pull request of the spec: the sidebar's lists.
