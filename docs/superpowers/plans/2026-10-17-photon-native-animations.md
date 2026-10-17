# Animations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sidebar and the viewer's info panel slide, and the grid's photos glide to their
new places when the columns change or when the user's own write changes the grid - with a
switch in Settings that turns all of it off.

**Architecture:** Two egui-free state modules hold the arithmetic: `animate.rs` (a `Slide`
from one value to another over time, eased out) and `grid/glide.rs` (where each photo and
header is drawn while the grid moves from one layout to the next). The views - the shell, the
viewer, the grid - read a drawn value from them each frame; the stored state (`sidebar_hidden`,
`open.info`, the rows) flips at once as now and every rule keeps reading it. The application
decides which rebuilt grid is the user's by the writes it has on its queue.

**Tech Stack:** Rust, egui/eframe 0.36.2, egui_kittest; photon-core's settings table.

**Spec:** `docs/superpowers/specs/2026-10-17-photon-native-animations-design.md`

## Global Constraints

- Duration: `animate::DURATION_MS = 200.0`, eased out (`1 - (1 - t)^3`), the same for all four.
- Progress is time (`ui.input(|i| i.time)` in ms), never a count of frames.
- A still window draws nothing: an animation asks for a frame only while it runs
  (`request_repaint` while `running`), none once it has ended.
- Hit tests, the selection, the band, the pin and the menus go by the *new* places from the
  first frame; a glide or a slide is drawing only.
- The stored state flips at the press: `layout.sidebar_hidden`, `open.info`, the rows.
- With the switch off every change is one frame, as today, and a late thumbnail does not fade
  in. The slideshow's crossfade is not covered.
- Setting: key `animations` in the settings table, `"1"`/`"0"`, `true` when never set. No
  schema change.
- No glide for: a scan's or any other rebuild that is not the user's, other results (another
  view, search, sort, grouping), any scroll.
- Only the toggle (the button, Ctrl+B) slides the sidebar; "Rename in photon…" shows it at once.
  Only the info button and `I` slide the panel; every other close is instant.
- While the sidebar or the panel slides *closed* it is drawn and takes no press and no keyboard;
  a press on the panel's drawn part is not the photo's.
- New state modules go into `STATE_MODULES` in `lib.rs` and name no egui type.
- The whole-application tests launch with animations off (`App::without_animations`) unless the
  test is about animations; `native-shot` draws with them off.
- CLAUDE.md's gate before every commit: `cargo fmt --all`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`,
  `cargo bench -p photon-core --bench grid --no-run`.
- Every new test is shown to fail with its change reverted (CLAUDE.md, Conventions).

## Review Focus

1. **Ctrl+B pressed twice inside 200 ms** - the slide turns round from where it is, never
   jumps, and the stored state is the second press's. (Task 2's reversal test, Task 3's shell test.)
2. **The window resized, or the splitter dragged, during a sidebar slide** - the pin keeps the
   top photo, and a glide re-targets from where each photo is drawn. (Task 6:
   `a_second_column_change_mid_glide_starts_from_where_the_photo_is_drawn`.)
3. **A rubber band held, or a photo pressed, during a glide** - the press lands on the photo
   at its new place and the band selects by the new rows. (Task 6:
   `a_press_during_a_glide_is_on_the_photo_at_its_new_place`.)
4. **The switch turned off in the middle of a slide or a glide** - it ends at once, no frame is
   asked for after. (Task 2: `off_ends_a_running_slide`; Task 6 the same for a glide.)
5. **The viewer opened while the sidebar slides, or the grid left mid-glide** - nothing asks
   for frames for a grid that is not drawn; coming back finds it settled. (Task 3:
   `a_slide_under_the_viewer_asks_for_no_frames_once_its_time_is_over`.)

---

### Task 1: The switch

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs` (beside `theme`/`set_theme`)
- Modify: `crates/photon-engine/src/commands.rs` (beside `theme`/`set_theme`, line ~738)
- Modify: `crates/photon-ui/src/settings.rs` (`Seen`, `Asked`, copy constants)
- Modify: `crates/photon-ui/src/settings_view.rs` (`Section::Appearance`, line ~771)
- Modify: `crates/photon-ui/src/app/settings_dialog.rs` (`SettingWrite`, `Asked` handling, `Seen` fill)
- Modify: `crates/photon-ui/src/app.rs` (`App::new` read beside the theme, line ~743; builder)
- Modify: `crates/photon-ui/src/thumbs/shown.rs` (`Thumbs::set_fades`)
- Modify: `crates/photon-ui/tests/common/mod.rs` (`launch_with`, `over_with`) and every other
  `App::new` builder in `tests/` except `probe.rs` and `app.rs`'s still-window tests

**Interfaces:**
- Produces: `Library::animations(&self) -> Result<bool>`, `Library::set_animations(&self, on: bool) -> Result<()>`;
  `commands::animations(&Engine) -> CmdResult<bool>`, `commands::set_animations(&Engine, bool) -> CmdResult<()>`;
  `App.animations: bool`; `App::without_animations(self) -> Self` (for the session, not stored);
  `Seen.animations: bool`; `Asked::Animations(bool)`; `SettingWrite::Animations(bool)`;
  `Thumbs::set_fades(&mut self, fades: bool)`;
  `settings::ANIMATIONS: &str = "Animations: move the sidebar, the info panel and the photos smoothly"`.

- [ ] **Step 1: Failing tests.** In `settings.rs` (core): `animations_are_on_until_switched_off`
  - a new library answers `true`; after `set_animations(false)` it answers `false`; after
  `set_animations(true)` `true`. In `settings_view.rs`'s tests:
  `the_animations_box_asks_for_the_other_value` - `settings(Section::Appearance, true)` with
  `seen.animations = true`, `click(&mut f, ANIMATIONS)` answers `[Asked::Animations(false)]`.
- [ ] **Step 2:** Run `cargo nextest run -p photon-core animations_are_on` and
  `cargo nextest run -p photon-ui --lib the_animations_box`. Expected: FAIL (not defined).
- [ ] **Step 3: Implement.** The core pair stores `"1"`/`"0"` under `ANIMATIONS: &str = "animations"`,
  as `set_export_apply_edits` does, `true` when unset. The engine pair wraps them as `theme` does.
  The Appearance section draws a `checkbox` (the one `SHUFFLE` uses) under the theme's segmented
  control, before Photo size. `App::new` reads `commands::animations(&engine).unwrap_or(true)`
  beside the theme and calls `thumbs.set_fades(animations)` on the grid's store; `Asked::Animations`
  sets `App.animations`, `set_fades`, and stores through `SettingWrite::Animations` on
  `push_setting` (a refused store keeps the value for the session, as the theme does).
  `without_animations` sets `animations = false` and `set_fades(false)`.
- [ ] **Step 4:** Add `.without_animations()` to `common::launch_with`, `common::over_with` and the
  direct `App::new` builders in `tests/` (clipboard, opener, menu, shell, pickers, people, side,
  drop, status, screenshots), not `probe.rs`, whose still-window loop must hold with them on.
- [ ] **Step 5:** Run the two tests: PASS. Then the gate. Commit:
  `feat(ui): a switch in Settings for animations, on by default`.

### Task 2: `animate.rs`, a slide over time

**Files:**
- Create: `crates/photon-ui/src/animate.rs` (state module; add to `STATE_MODULES` and `lib.rs`)

**Interfaces:**
- Produces:
  ```rust
  pub const DURATION_MS: f64 = 200.0;
  pub fn ease_out(t: f64) -> f64;                 // t clamped to 0..=1
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Slide { /* from, to, started_ms */ }
  impl Slide {
      pub fn at(value: f64) -> Self;              // settled at value
      pub fn to(&mut self, target: f64, now_ms: f64, animate: bool); // from the drawn value now; off: jump
      pub fn value(&self, now_ms: f64) -> f64;
      pub fn target(&self) -> f64;
      pub fn running(&self, now_ms: f64) -> bool;
  }
  ```
  `to` with the same target as now is a no-op (it must not restart a running slide).

- [ ] **Step 1: Failing tests** in `animate.rs`:
  - `a_slide_eases_out_over_its_time`: `Slide::at(0.0)`, `to(1.0, 1000.0, true)`; `value(1000.0) == 0.0`,
    `value(1100.0) == ease_out(0.5) == 0.875`, `value(1200.0) == 1.0`, `running(1199.0)`, `!running(1200.0)`.
  - `a_slide_turned_round_starts_from_where_it_was`: `to(1.0, 0.0, true)`, then at 100 ms `to(0.0, 100.0, true)`;
    `value(100.0) == 0.875` (no jump), `value(300.0) == 0.0`.
  - `asking_for_the_target_again_does_not_restart_it`: `to(1.0, 0.0)`, `to(1.0, 150.0)`; `!running(200.0)`.
  - `off_ends_a_running_slide`: running to 1.0, then `to(1.0, 50.0, false)`; `value(50.0) == 1.0`, `!running(50.0)`.
  - `progress_is_time_not_frames`: values sampled every 8.33 ms and every 16.67 ms agree at 100 ms.
- [ ] **Step 2:** `cargo nextest run -p photon-ui --lib animate`. Expected: FAIL.
- [ ] **Step 3:** Implement. `to` with `animate == false`, or with the target already reached and
  not running, sets the slide settled at `target`.
- [ ] **Step 4:** PASS, gate, commit `feat(ui): a slide from one value to another over time`.

### Task 3: The sidebar slides

**Files:**
- Modify: `crates/photon-ui/src/window_layout.rs` (`areas`, `Areas`)
- Modify: `crates/photon-ui/src/shell.rs` (`Shell`, `ShellData`, `show`, line ~170)
- Modify: `crates/photon-ui/src/app.rs` (`ShellData` fill: `animations`; `Action::ToggleSidebar` unchanged)
- Modify: `crates/photon-ui/src/sidebar/view.rs` only if `show` cannot already take a rect wider than its clip
- Test: `crates/photon-ui/tests/shell.rs`

**Interfaces:**
- Consumes: `animate::Slide`, `DURATION_MS` (Task 2); `App.animations` (Task 1).
- Produces: `window_layout::areas(width: f32, height: f32, layout: &Layout, shown: f32) -> Areas`
  where `shown` is 0..=1 of the sidebar's width; `Areas.sidebar_full: Option<Area>` - the
  sidebar's whole width, at the left edge, which the list is laid out in and clipped to
  `sidebar`; `sidebar`/`splitter` are `Some` while `shown > 0`, the splitter at the moving edge,
  the content from `shown * (side + SPLITTER)`. `ShellData.animations: bool`.
  `Shell::sliding(&self, now_ms) -> bool` for the application's tests.
  `Shell::sidebar_at_once(&mut self)` - the next frame settles the slide on the stored state
  instead of sliding; called where "Rename in photon…" shows a hidden sidebar (`app.rs` ~1997).

- [ ] **Step 1: Failing tests.** `window_layout.rs`:
  `a_sidebar_part_way_gives_the_content_the_rest` - `areas(1280, 800, &Layout::default(), 0.5)`:
  sidebar right `== SIDEBAR_DEFAULT * 0.5` (rounded as the code rounds), content left
  `== 0.5 * (SIDEBAR_DEFAULT + SPLITTER)`, `sidebar_full` right `== SIDEBAR_DEFAULT`;
  `shown == 0` gives the hidden layout of `a_hidden_sidebar_gives_its_room_to_the_content`.
  `tests/shell.rs`, launched *with* animations:
  - `the_sidebar_slides_open_and_then_asks_for_nothing`: hidden, Ctrl+B, at +100 ms of the clock
    the content's left is strictly between 0 and its open value; at +200 ms it is the open value;
    `frames_in` over the next second draws nothing.
  - `a_closing_sidebar_takes_no_press`: open, Ctrl+B, at +50 ms click a sidebar row's last drawn
    place: no view is asked for (`App::here` unchanged).
  - `a_slide_under_the_viewer_asks_for_no_frames_once_its_time_is_over`: Ctrl+B, Enter on a
    photo in the same frame span; after 200 ms `frames_in` draws nothing.
  - `the_top_photo_stays_at_the_top_through_a_slide`: scrolled to a folder, Ctrl+B; at
    +80 ms and +200 ms `App::top_folder` is the same folder.
  - `rename_in_photon_shows_the_sidebar_at_once`: hidden, "Rename in photon…" on a header: in
    the next frame the content's left is the open value and the name field has the keyboard.
- [ ] **Step 2:** Run them; Expected: FAIL.
- [ ] **Step 3:** Implement. `Shell` keeps `sidebar: Slide` (starts `at(1.0)`/`at(0.0)` from the
  stored layout on its first frame). Each frame: `slide.to(if hidden {0} else {1}, now, data.animations)`,
  `areas(.., slide.value(now))`, and `request_repaint` while `running`. While the stored state is
  hidden and `shown > 0`, draw the list in `sidebar_full` clipped to `sidebar` inside a scope
  that takes no input (a `UiBuilder::disabled()` child with `visuals.disabled_alpha = 1.0`, so it
  does not dim), and still call `not_drawn()` - the rules for a sidebar that is not drawn
  apply from the press. The splitter is painted at the edge and is only a widget while the stored
  state is shown. The grid needs nothing: its pin already keeps the place through a resize.
- [ ] **Step 4:** PASS, gate, commit `feat(ui): the sidebar slides in and out`.

### Task 4: The info panel slides

**Files:**
- Modify: `crates/photon-ui/src/viewer_layout.rs` (`info_room`, `photo_area`, `info_place`)
- Modify: `crates/photon-ui/src/viewer_view.rs` (`ViewerData.info_shown`, lines ~397-415, ~503, ~521; `Controls::panel`)
- Modify: `crates/photon-ui/src/app.rs` (the viewer's state beside `info`, line ~5133; `ViewerAction::Info` at ~3134; the copy box at ~2726; the instant closes at ~3177 and ~3352)
- Test: `crates/photon-ui/tests/viewer.rs`

**Interfaces:**
- Consumes: `animate::Slide` (Task 2), `App.animations` (Task 1).
- Produces: `info_room(shown: f32) -> f32` (`shown * (INFO_WIDTH + INFO_RIGHT + INFO_GAP)`),
  `photo_area(window: Frame, shown: f32) -> Frame`, `info_place(window: Frame, shown: f32) -> Frame`
  (the panel's left moves from the window's right edge to its open place, its width constant);
  `ViewerData.info_shown: f32`, beside `info_open: bool`, which stays the stored state.

- [ ] **Step 1: Failing tests.** `viewer_layout.rs`: `a_panel_part_way_takes_part_of_its_room` -
  `photo_area(window, 0.5).width == window.width - 0.5 * info_room(1.0)`, `info_place(window, 0.0).left
  == window.left + window.width`, `info_place(window, 1.0)` is today's place. `viewer_view.rs`
  frame tests: `a_closing_panel_takes_no_press_and_is_not_the_photos` - `info_open = false`,
  `info_shown = 0.5`, a zoomed photo, a press and drag on the panel's drawn part: no pan
  (`zoom.pan` unchanged), no `ViewerAction`. `tests/viewer.rs` with animations:
  `the_panel_slides_and_then_asks_for_nothing` - `I`; at +100 ms the photo's area is between its
  two widths; at +200 ms the open width; `frames_in` over a second draws nothing.
  `the_crop_tool_shuts_the_panel_at_once` - panel open, `C`: in the next frame the panel is not drawn.
- [ ] **Step 2:** Run; Expected: FAIL.
- [ ] **Step 3:** Implement. The viewer's state keeps `info_slide: Slide` (new at each opening,
  `at(0.0)`); `ViewerAction::Info` flips `info` and calls `to(.., animations)`; the other closes
  call `to(0.0, now, false)`. Everything measured in the photo's area uses `info_shown`; the panel
  is drawn while `info_shown > 0`, its widgets only while `info_open`; `Controls::panel` is the drawn
  panel's rect whatever the state, so a press there is not the photo's. Faces' outlines stay on
  `info_open`. `request_repaint` while the slide runs.
- [ ] **Step 4:** PASS, gate, commit `feat(ui): the info panel slides beside the photo`.

### Task 5: `grid/glide.rs`, where a photo is drawn while the grid moves

**Files:**
- Create: `crates/photon-ui/src/grid/glide.rs` (state module; `STATE_MODULES`, `grid/mod` or `lib.rs`)

**Interfaces:**
- Consumes: `animate::{ease_out, DURATION_MS}`.
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum Key { Item(i64), Header { folder: Option<i64>, period: Option<Period> } }
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Place { pub x: f64, pub y: f64, pub width: f64, pub height: f64 } // grid coords, y relative to the position
  pub struct Leaving { pub entry: GridEntry, pub place: Place }
  #[derive(Default)]
  pub struct Glides { /* from: HashMap<Key, Place>, leaving: Vec<Leaving>, started_ms, position_at_start */ }
  impl Glides {
      /// Starts a glide: `before` is every key's place in the old rows, relative to the old
      /// position, as far as the snapshot reaches; a key already gliding starts from where
      /// it is drawn at `now_ms` instead.
      pub fn begin(&mut self, before: impl IntoIterator<Item = (Key, Place)>, leaving: Vec<Leaving>, position: f64, now_ms: f64);
      /// Where `key` is drawn at `now_ms` given its new place (relative to `position`):
      /// the old place moved by the scroll since the start, eased towards `target`.
      /// A key with no old place is drawn at `target`.
      pub fn place(&self, key: Key, target: Place, position: f64, now_ms: f64) -> Place;
      /// The keys whose old place, moved by the scroll, is within 0..viewport: drawn during
      /// the glide though their new place is out of view.
      pub fn visible_before(&self, position: f64, viewport: f64) -> Vec<Key>;
      /// The photos that left, each with its opacity at `now_ms` (1 - eased progress).
      pub fn leaving(&self, position: f64, now_ms: f64) -> Vec<(Leaving, f32)>;
      pub fn running(&self, now_ms: f64) -> bool;
      pub fn stop(&mut self);
  }
  ```

- [ ] **Step 1: Failing tests** in `glide.rs`:
  - `a_photo_glides_from_its_old_place_to_its_new`: begin with `Item(1)` at (8,0,100,100), position 0,
    at 1000; `place(Item(1), (8,108,100,100), 0, 1100)` is `ease_out(0.5)` of the way; at 1200 the target.
  - `a_scroll_during_a_glide_moves_both_ends`: same, then at 1100 with position 50 the drawn y is
    the 1100 answer minus 50 for both ends.
  - `a_key_already_gliding_starts_again_from_where_it_is_drawn`: begin, at 1100 begin again with
    another old place for `Item(1)`; `place(.., 1100)` equals the drawn place before the second begin.
  - `a_key_with_no_old_place_is_at_its_target`.
  - `a_leaving_photo_fades_out_and_is_gone_at_the_end`: opacity 1 at start, `1 - ease_out(0.5)` at
    half, list empty once `!running`.
  - `visible_before_lists_what_was_in_view`: two keys, one at y 50 and one at y 900, viewport 700:
    only the first.
- [ ] **Step 2:** `cargo nextest run -p photon-ui --lib glide`; FAIL.
- [ ] **Step 3:** Implement. `GridEntry` and `Period` are photon-core types; the module names no egui.
- [ ] **Step 4:** PASS, gate, commit `feat(ui): the arithmetic of a grid gliding to new places`.

### Task 6: The grid glides when its columns change

**Files:**
- Modify: `crates/photon-ui/src/grid/view.rs` (`GridData`, `lay_out` ~line 568, `show` ~427, `draw` ~1155)
- Modify: `crates/photon-ui/src/app.rs` (`GridData` fill: `animations`)
- Test: `crates/photon-ui/src/grid/view.rs` frame tests

**Interfaces:**
- Consumes: `glide::{Glides, Key, Place, Leaving}` (Task 5), `App.animations` (Task 1).
- Produces: `GridData.animations: bool`, `GridData.glide_from: Option<&'a GridIndex>` (Task 7 fills it;
  `None` here); `GridView::drawn_place(&self, id: i64) -> Option<Rect>` - where the photo was drawn
  in the last frame, gliding or not (for tests); `GridOutput.gliding: bool`.

**When a glide begins** (in `lay_out`, before the rows are rebuilt, with the old rows and the old
position in hand):
- `data.animations`, and either
  - the columns changed, or the nominal tile (`tile_width(data.size)`) changed, *and* `layout_gen`
    did not - a geometry change: the splitter, a slide, a window resize, the size buttons; a
    tile that only widens within the same column count does not begin one; or
  - `data.glide_from` is `Some` (Task 7), whatever changed.
- The snapshot is every item and header of the old rows overlapping
  `[position - viewport, position + 2 * viewport]`, ids read from `glide_from` when given, else
  `data.index`. Photos of the snapshot the new index lacks (`position_of` is `None`) and that were
  in view are the `leaving`.

**Drawing during a glide:** the rows in view now, plus `visible_before`'s keys whose new place is
out of view (their new row from `row_of_item(position_of(id))`), each at `glides.place(..)`;
then the leaving photos at their opacity (`tile::paint`'s texture tint, as `shown` is). Headers by
`Key::Header`. The gliding keys' thumbnails are added to `wanted`. `request_repaint` while
running. Hit tests, `rect_of`, the band and the pin are not touched. `glides.stop()` when
`animations` is false or other results arrive (`to_top`).

- [ ] **Step 1: Failing tests** (frame tests in `grid/view.rs`, animations on):
  - `a_column_change_glides_the_photos_to_their_new_places`: a width with 6 columns, then one with
    5; the first photo of the second row is drawn (`drawn_place`) at +100 ms strictly between its
    old and new places, at +200 ms at the new one (`rect_of`).
  - `a_tile_that_widens_without_a_column_change_does_not_glide`: two widths with the same column
    count; `drawn_place == rect_of` in the next frame.
  - `a_photo_pushed_below_the_fold_glides_out`: the last photo in view before a 6->5 change is
    drawn at +100 ms though its new row is below the viewport.
  - `a_second_column_change_mid_glide_starts_from_where_the_photo_is_drawn`: 6->5 at 0, 5->4 at
    100 ms; the drawn place at 100 ms before and after the second change is the same.
  - `a_press_during_a_glide_is_on_the_photo_at_its_new_place`: at +50 ms press at `rect_of(n)`'s
    centre; photo `n` is selected.
  - `a_scroll_during_a_glide_moves_the_gliding_photos_with_it`.
  - `the_grid_asks_for_nothing_once_a_glide_is_over`: no repaint is requested after +200 ms.
  - `with_animations_off_nothing_glides`.
  - `a_new_index_with_no_glide_from_does_not_glide`: same columns, `layout_gen` moved, a photo
    removed: the next frame draws every photo at `rect_of`.
- [ ] **Step 2:** Run; FAIL.
- [ ] **Step 3:** Implement as above.
- [ ] **Step 4:** PASS. Also `cargo nextest run -p photon-ui --test all probe::` (the still
  window). Gate, commit `feat(ui): the grid's photos glide when its columns change`.

### Task 7: The user's own writes glide

**Files:**
- Modify: `crates/photon-ui/src/app.rs` (`Write`, the push sites, the answers loop ~4232, `adopt` ~4456)
- Test: `crates/photon-ui/tests/selection.rs` or a new `tests/glide.rs` (a `mod` line in `tests/main.rs`)

**Interfaces:**
- Consumes: `GridData.glide_from` (Task 6).
- Produces: `Write::moves_grid(&self) -> bool` - true for `Stars`, `Hidden`, `AlbumAdd`,
  `AlbumRemove`, `PersonRemove`, `Name`, `FolderHidden`, `Tag`; `App::write(&mut self, write: Write)`,
  the one way a write is pushed (every `self.writes.push(` becomes `self.write(`), which records
  the number of a write that moves the grid in `App.moving: HashSet<u64>`;
  `App.moved_since_adopt: bool`; `App.glide_from: Option<GridIndex>` (the replaced index, moved,
  not cloned), handed to the grid as `GridData.glide_from` and taken after that frame.

**The rule** (the spec's "which grid is the user's"): in `adopt`, for a grid of the same results
(`!shown.other_results`), glide when `animations && (!moving.is_empty() || moved_since_adopt)`;
then `moved_since_adopt = false`. In the answers loop, an answer to a number in `moving` removes it
and sets `moved_since_adopt`. The engine publishes inside the command, so the grid may be adopted
before or after the answer; both are covered.

- [ ] **Step 1: Failing tests** (whole application, animations on, over `common::pictures`-style
  real files so writes succeed):
  - `a_hidden_photo_fades_and_the_rest_glide_into_its_place`: select photo 2, `H`; once the grid
    without it is on screen, photo 3 is drawn (`App::drawn_place`) at +100 ms strictly between
    its old place and photo 2's old place, at +200 ms at photo 2's old place.
  - `a_scans_rebuild_does_not_glide`: a rescan that adds a photo before the ones in view (a file
    written into the folder, `Write::Rescan`): in the frame it lands every photo is at `rect_of`.
  - `another_view_does_not_glide`: All photos -> Starred: drawn places equal `rect_of` at once.
- [ ] **Step 2:** Run; FAIL.
- [ ] **Step 3:** Implement. Add `App::drawn_place(&self, id: i64) -> Option<Rect>` beside
  `App::tile_rect` for the tests.
- [ ] **Step 4:** PASS, gate, commit `feat(ui): the grid glides after the user's own changes`.

### Task 8: Docs and the person's checklist

**Files:**
- Modify: `CLAUDE.md` (a paragraph under "The window", after the grid's pin paragraph)
- Modify: `docs/smoke-checklist.md`

- [ ] **Step 1:** CLAUDE.md: one paragraph - `animate.rs` and `grid/glide.rs`; the stored state flips at
  the press and the slide is drawing only; which geometry changes begin a glide and which rebuilds
  are the user's (`App::write`, `moving`, `moved_since_adopt`), and that a new write that changes a
  view's rows belongs in `Write::moves_grid`; the switch, and that tests launch `without_animations`.
- [ ] **Step 2:** The checklist: the feel of the curve and the 200 ms; a fast splitter drag across
  several column counts; Ctrl+B twice quickly; `I` with a zoomed photo; `H` in a long folder; the
  switch off.
- [ ] **Step 3:** `cargo run -p xtask -- native-shot` and read `target/screenshots/native-*.png`
  (needs a GPU adapter; skip and say so if there is none): nothing drawn mid-slide.
- [ ] **Step 4:** Gate, commit `docs: the window's animations`.
