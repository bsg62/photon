# The viewer's photo kept clear of the bar - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A setting in Settings → Appearance that fits the viewer's photo above the bar (and above the photo's own caption) instead of under them.

**Architecture:** One stored flag (library setting, like `animations`) read at launch into `App`. Each frame the application works out the room kept at the window's foot from a pure function in `viewer_layout.rs`, remembers it across a step until the caption is known, and hands it to the view in `ViewerData::foot`. Every place that measures the photo's area (`photo_area`) takes that room, so the fit, the fitted copy's box, zoom, pan, faces and the opening passage follow without further change. The crop tool fits its box the same way.

**Tech Stack:** Rust, egui/eframe (pinned), egui_kittest for whole-app tests, SQLite settings table.

**Spec:** `docs/superpowers/specs/2026-11-25-photon-viewer-photo-clear-of-bar-design.md`

## Global Constraints

- Checkbox label, exact: `Keep photos clear of the viewer's bar`. Off by default.
- Library settings key: `viewer_clear`, stored `"1"`/`"0"`, absent = off.
- Room with no caption: `EDGE + BAR + EDGE` (62 points). With a caption drawn: `CAPTION_BOTTOM + caption height + EDGE`.
- No room is kept (0) when the setting is off, for a video, and while a slideshow runs.
- Until the library has said what the photo is (the bar's line `caption` is empty), the room is the last room kept in this viewer; at the viewer's opening, the no-caption room.
- No SQLite read on the UI thread except the launch read beside `animations` (`app.rs` ~801), which is already allowed there.
- The gate in `CLAUDE.md` (fmt, clippy `-D warnings`, nextest, bench `--no-run`) passes before each commit. Every new test is shown to fail with its change reverted (exact replacement, not `sed`).
- Never launch the GUI; `cargo run -p xtask -- native-shot` for pictures.

## Review Focus

1. **A step between a captioned and an uncaptioned photo** - the photo resizes once when the library answers, not twice, and not at all between two uncaptioned photos. (Task 2's `the_room_is_kept_across_a_step_until_the_caption_is_known`; Task 4's whole-app step test.)
2. **The fitted copy's box matches what is drawn** - `within` (app) and the view's area must come from the same `foot`, or the Lanczos copy is made for the wrong size and the levels are drawn for good. (Task 4: `viewer_box` equals the drawn area.)
3. **The setting flipped while the viewer is open** - the photo refits at once, no restart. (Task 4 test.)
4. **A slideshow** - the photo takes the whole window while a show runs, with the setting on. (Task 4 test.)
5. **A low window** - room larger than the window gives an area of no height, not a negative one, and nothing panics. (Task 2's `a_window_lower_than_the_room_has_an_area_of_nothing`.)

---

### Task 1: The stored setting and its checkbox

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs` (key const beside `ANIMATIONS`; getter/setter beside `animations`; unit test beside `animations_are_on_until_switched_off`)
- Modify: `crates/photon-engine/src/commands.rs` (beside `animations`/`set_animations`)
- Modify: `crates/photon-ui/src/settings.rs` (const `PHOTO_CLEAR`, `Seen::photo_clear`, `Asked::PhotoClear(bool)`)
- Modify: `crates/photon-ui/src/settings_view.rs` (checkbox under the animations one, Appearance section ~784)
- Modify: `crates/photon-ui/src/app/settings_dialog.rs` (`SettingWrite::PhotoClear`, `store`, `Seen` build, `Asked` handling)
- Modify: `crates/photon-ui/src/app.rs` (field `photo_clear: bool` beside `animations`, read at launch ~801)
- Test: `crates/photon-ui/tests/settings.rs`

**Interfaces:**
- Produces: `Library::viewer_clear(&self) -> Result<bool>`, `Library::set_viewer_clear(&self, on: bool) -> Result<()>`; `commands::viewer_clear(&Engine) -> CmdResult<bool>`, `commands::set_viewer_clear(&Engine, bool) -> CmdResult<()>`; `photon_ui::settings::PHOTO_CLEAR: &str`; `App::photo_clear: bool` (private field, read by Task 4).

- [ ] **Step 1: Write the failing tests**

`settings.rs` (core): `the_viewer_is_not_kept_clear_until_switched_on` - fresh library `!lib.viewer_clear()`, set true → true, set false → false.

`settings_view.rs`: `the_clear_box_asks_for_the_other_value` - as `the_animations_box_asks_for_the_other_value`, with `seen.photo_clear = false` and `click(&mut f, PHOTO_CLEAR) == [Asked::PhotoClear(true)]`.

`tests/settings.rs`: `the_clear_box_ticked_in_settings_is_stored` - as `animations_ticked_off_in_settings_are_off_at_once` up to the click (role `CheckBox`, label `PHOTO_CLEAR`), then `assert!(stored(&library).viewer_clear().unwrap())`.

- [ ] **Step 2: Run them; they fail to compile / fail**

Run: `cargo nextest run -p photon-core viewer_clear && cargo nextest run -p photon-ui clear_box`
Expected: FAIL.

- [ ] **Step 3: Implement**

Copy the animations path at each layer, with off as the default (`is_some_and(|s| s == "1")`). `Asked::PhotoClear(on)` sets `self.photo_clear = on` at once and pushes `SettingWrite::PhotoClear(on)` (kept for the session if refused, as `Animations`). The checkbox goes straight under the animations checkbox, same call shape.

- [ ] **Step 4: Run them; they pass.** Same command. Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git commit -am "feat(settings): store whether the viewer keeps photos clear of its bar"
```

---

### Task 2: The room at the foot, as arithmetic

**Files:**
- Modify: `crates/photon-ui/src/viewer_layout.rs`
- Modify: `crates/photon-ui/src/viewer_view.rs` (remove its `CAPTION_BOTTOM`, import it from `viewer_layout`; pass `0.0` to `photo_area` for now)
- Modify: `crates/photon-ui/src/app.rs` (~2948) and `crates/photon-ui/src/app/motion.rs` (~215): pass `0.0` for now

**Interfaces:**
- Produces, in `viewer_layout`:
  - `pub const CAPTION_BOTTOM: f32 = 64.0;` (moved, doc comment kept)
  - `pub const BAR_ROOM: f32 = EDGE + BAR + EDGE;`
  - `pub enum Caption { NotKnown, Without, Lines(f32) }` (`Clone, Copy, Debug, PartialEq`) - `Lines` holds the caption's drawn height in points.
  - `pub fn foot_room(clear: bool, caption: Caption, was: Option<f32>) -> f32` - `!clear` → `0.0`; `NotKnown` → `was.unwrap_or(BAR_ROOM)`; `Without` → `BAR_ROOM`; `Lines(h)` → `CAPTION_BOTTOM + h + EDGE`.
  - `pub fn photo_area(window: Frame, shown: f32, foot: f32) -> Frame` - height `(window.height - foot).max(0.0)`; nothing else changes.

- [ ] **Step 1: Write the failing tests** (in `viewer_layout.rs`'s `tests`)

```rust
#[test]
fn the_room_at_the_foot_is_the_bars_or_up_to_the_caption() {
    assert_eq!(foot_room(false, Caption::Lines(20.0), Some(99.0)), 0.0);
    assert_eq!(foot_room(true, Caption::Without, Some(99.0)), 62.0);
    assert_eq!(foot_room(true, Caption::Lines(20.0), None), 64.0 + 20.0 + 12.0);
}

#[test]
fn the_room_is_kept_across_a_step_until_the_caption_is_known() {
    assert_eq!(foot_room(true, Caption::NotKnown, Some(96.0)), 96.0);
    assert_eq!(foot_room(true, Caption::NotKnown, None), BAR_ROOM);
}

#[test]
fn the_photos_area_stops_the_room_above_the_foot() {
    let window = Frame { left: 10.0, top: 20.0, width: 1280.0, height: 800.0 };
    assert_eq!(photo_area(window, 1.0, 62.0),
        Frame { width: 1280.0 - 348.0, height: 800.0 - 62.0, ..window });
}

#[test]
fn a_window_lower_than_the_room_has_an_area_of_nothing() {
    let low = Frame { left: 0.0, top: 0.0, width: 500.0, height: 40.0 };
    assert_eq!(photo_area(low, 0.0, BAR_ROOM).height, 0.0);
}
```

Update the existing `photo_area` calls in this file's tests to pass `0.0`.

- [ ] **Step 2: Run; fails to compile.** `cargo nextest run -p photon-ui --lib viewer_layout`

- [ ] **Step 3: Implement** the interfaces above; every existing caller passes `0.0` (no behaviour change yet).

- [ ] **Step 4: Run; passes.** Same command, then `cargo nextest run -p photon-ui --lib viewer_view` (nothing else moved).

- [ ] **Step 5: Gate and commit** - `feat(viewer): the photo's area can keep room at the window's foot`

---

### Task 3: The view draws in the room it is given

**Files:**
- Modify: `crates/photon-ui/src/viewer_view.rs` (`ViewerData`, `show` ~486, `cropping` ~770, `caption_place` ~2275, the test `Fixture`)
- Modify: `crates/photon-ui/src/app.rs` (the `ViewerData` literal ~3196: `foot: 0.0` for now)

**Interfaces:**
- Consumes: `viewer_layout::{photo_area, CAPTION_BOTTOM}` from Task 2.
- Produces:
  - `ViewerData::foot: f32` - the room kept at the window's foot, worked out by the application; doc comment says the view measures the photo's area and the crop box with it and never works it out itself.
  - `pub fn caption_height(painter: &egui::Painter, window: Rect, line: &str) -> f32` in `viewer_view` - the height `caption_place` gives its rectangle for `line` (galley plus padding). `caption_place` is rebuilt on the same layout so the two cannot disagree.

- [ ] **Step 1: Write the failing tests** (view `tests`, `Fixture` gains `foot: f32`, default `0.0`, passed into `ViewerData`)

- `the_picture_stops_above_the_room_at_the_foot`: `f.foot = 62.0`, a level of a tall photo (taller than wide, as the existing fit tests make one), one frame; the image rect from `f.images` has `bottom() <= 700.0 - 62.0 + 0.5` and its centre's y within 1 point of `(700.0 - 62.0) / 2.0`.
- `the_crop_box_stops_above_the_room_at_the_foot`: the existing crop fixture (`cropping()`), `f.foot = 62.0`; the drawn picture's rect has `bottom() <= 700.0 - 62.0 + 0.5`.
- `caption_height_is_what_the_caption_is_drawn_at`: `f.photo_caption = Some("Lisbon, at dusk")`, one frame; `caption_height(&painter, window, "Lisbon, at dusk")` equals the caption's drawn rectangle's height (the existing caption tests show how to find it; use a painter from `f.ctx`).

- [ ] **Step 2: Run; fails.** `cargo nextest run -p photon-ui --lib viewer_view::tests::the_picture_stops viewer_view::tests::the_crop_box viewer_view::tests::caption_height`

- [ ] **Step 3: Implement**

`show`: `photo_area(whole, data.info_shown, data.foot)`. `cropping`: `whole.height` less `data.foot`, `max(0.0)`. A video's `fit_in` is left alone (the application gives a video `0.0`).

- [ ] **Step 4: Run; passes.** Same command, then the whole `viewer_view` module.

- [ ] **Step 5: Gate and commit** - `feat(viewer): the photo and the crop box are fitted above the room the frame keeps`

---

### Task 4: The application works the room out, and keeps it across a step

**Files:**
- Modify: `crates/photon-ui/src/app.rs` (`Viewing` gains `foot: Option<f32>`, initialised `None` at opening ~2436; `show_viewer` ~2948 and the `ViewerData` literal ~3196)
- Modify: `crates/photon-ui/src/app/motion.rs` (~215, a fade's landing)
- Modify: `docs/architecture/ui-viewer.md` (a paragraph after the info panel's, see Step 6)
- Test: `crates/photon-ui/tests/viewer.rs`

**Interfaces:**
- Consumes: `App::photo_clear` (Task 1); `foot_room`, `Caption`, `BAR_ROOM`, `photo_area` (Task 2); `ViewerData::foot`, `viewer_view::caption_height` (Task 3).
- Produces: nothing for later tasks.

- [ ] **Step 1: Write the failing whole-app tests** (`tests/viewer.rs`; launch `without_animations` as its other tests do, `set_viewer_clear(true)` written through `stored(&library)` before the launch unless the test says otherwise)

- `a_photo_kept_clear_stands_above_the_bar`: open a photo, wait for its facts; `viewer_photo()`'s rect has `bottom() <= window bottom - BAR_ROOM + 0.5`, and `viewer_box()` is the content rect's size less `BAR_ROOM` in height, times `pixels_per_point`, rounded.
- `a_photo_with_a_caption_stands_above_it`: one fixture photo given a caption the way `tests/viewer.rs` writes columns before launch (`doctored`); `viewer_photo()`'s bottom is above the caption's top (found by its label through AccessKit).
- `stepping_between_photos_without_captions_does_not_resize_the_photo`: two uncaptioned photos of one shape; record `viewer_box()` after the first's facts, press Right, and assert `viewer_box()` in **every** frame until the second's facts arrive equals the recorded box (step with `frames_in` / `until`, checking each frame).
- `the_clear_box_ticked_with_the_viewer_open_refits_the_photo`: setting off at launch, viewer open, `viewer_box()` height is the full window's; call `harness.state_mut().set_photo_clear(true)` (Settings cannot be opened over the viewer; add `pub fn set_photo_clear(&mut self, on: bool)` beside `viewer_box`, which only sets the field - the same thing `Asked::PhotoClear` does) - then the next frame's `viewer_box()` height is less by `BAR_ROOM`.
- `a_slideshow_takes_the_whole_window_even_kept_clear`: start a show (`S`), wait for it to play; `viewer_box()` height is the whole window's.

- [ ] **Step 2: Run; they fail.** `cargo nextest run -p photon-ui --test all viewer::`

- [ ] **Step 3: Implement**

In `show_viewer`, before the area at ~2948 (move the `photo_caption` read up from ~3115 so it is known there):
- `clear = self.photo_clear && !open.viewer.is_video() && !open.show.active()`
- `caption`: `NotKnown` while the bar's line is empty (no `open.item`); `Without` when there is no photo caption, the info panel is open (`open.info`), or the crop tool is open; else `Lines(caption_height(..))` with a painter on the viewer's layer.
- `foot = foot_room(clear, caption, open.foot)`; when `clear`, `open.foot = Some(foot)`.
- Pass `foot` to `photo_area` there and to `ViewerData::foot`.

`motion.rs`: a fade's landing uses `photo_area(window, 0.0, foot_room(self.photo_clear && !entry_is_video, Caption::NotKnown, None))` - the panel is shut and nothing is known at an opening.

- [ ] **Step 4: Run; they pass.** Same command, then `cargo nextest run -p photon-ui`.

- [ ] **Step 5: Revert-proof each new test** - revert its piece of the change with an exact replacement, see it fail, restore, `touch` the file (see the stale-build trap in memory).

- [ ] **Step 6: Note and picture**

`ui-viewer.md`, after the info panel's paragraph: **The photo can be kept clear of the bar** (Settings → Appearance; `viewer_layout::foot_room`, `ViewerData::foot`). What the room is, that the application works it out once a frame and hands it to the view (so `within` and the drawing cannot disagree), why it is kept across a step until the caption is known (the `bar_place` `was` rule met again), and why a show and a video keep none. Add an entry to `docs/smoke-checklist.md`: with the setting on, a portrait photo's foot and a captioned photo's foot are visible above the bar, and a step between captioned and uncaptioned photos resizes once.

Run `cargo run -p xtask -- native-shot` and look at the viewer pictures (setting off: unchanged).

- [ ] **Step 7: Gate and commit** - `feat(viewer): keep the photo clear of the bar and its caption when asked`
