# The native viewer, 1 of 8: open, look, step, close

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** a photo is opened from the grid, shown at full size, stepped from and closed.

**Architecture:** `App::viewer` is the viewer's state while it is open, and the shell is
not drawn then. The picture is two layers: the cached preview, from a second `Thumbs` over
`ThumbSize::Preview`, and the full picture, rendered by a new engine command on a
`tasks::Latest` and halved into levels of photon's own.

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("The viewer's
state", "The picture" without colour, "The view", "The keys and the mouse" without zoom and
pan, "Opening it").

## Global constraints

- photon never writes to a photo file. This pull request writes a star (`set_stars`) and a
  hide (`set_items_hidden`), through sub-project 3's queue of writes, and nothing else.
- Nothing that reads SQLite or a file runs on the UI thread.
- State modules name no egui type, and are added to `STATE_MODULES`.
- The gate at every commit; every new test shown to fail with its rule broken.
- A test of the whole application waits for a state, and for what is said, never for a
  length of time or a number of messages.

## What was read first (beyond the spec)

- `commands::copy_picture`: the pattern for a full-size render behind `RENDERING`, and its
  message for a file that is gone or offline.
- `thumbs/{source,shown,textures}.rs`: the grid's thumbnails are `ThumbSize::Grid` by
  three literals; `TexKey` already carries a size.
- `App::ui`: the dialog and the menu are shown first, then the shell with the grid in it;
  `self.last` is the grid's last frame and is read by the probe, the mark and the tests.

## Rulings made in the plan

- **The viewer is opened on a photo's id**, and its offset is looked up in the index on
  screen. Enter opens the lead while it is selected; a double click the photo under it.
- **While the viewer is open the grid's last frame stands** (`App::last` is not replaced
  by nothing): the place is kept, and what reads it reads the grid as it was left.
- **A menu or a band cannot be open under the viewer**: opening closes the menu, and a
  band has ended by the time a double click or Enter is read.
- **The full picture is asked for once the preview is on screen or cannot be had**, after
  the rest; the photo the viewer opens on does not wait.
- **The caption's date is English, medium, with the hour by twelve**: "Jul 14, 2026,
  12:30 PM", in the camera's own clock.
- **Levels**: halves by the mean of four pixels, until the longer side is no more than
  `LEVEL_FLOOR` (2048); a level whose side exceeds the device's limit is dropped, so the
  largest kept is the first that fits. The level drawn is the smallest whose width in
  pixels is at least the width drawn in pixels.
- **The star and the hide go by the photo on screen**, not by the selection behind it.
- **A video** shows its preview and "The native window does not play videos yet."

## Review focus

1. A picture drawn for another photo: an answer of the loader arriving after a step, after
   a close, after an edit made elsewhere (the key), after the viewer was opened again.
2. The photo on screen under a new grid: found again, orphaned, gone; a step from an
   orphaned photo; the caption's position; close handing back a photo the grid lacks.
3. Keys answered twice or by the wrong surface: the grid's under the viewer, the viewer's
   under a dialog, the Enter that opens and is still down, the double click's two clicks.
4. A still window with a photo shown; frames asked for while a picture is on its way.
5. Memory: textures of a photo left, of a viewer closed; levels beyond the device's limit.

## Tasks

### 1. The engine: `viewer_picture`

`commands::viewer_picture(engine, id, uncropped) -> CmdResult<ViewerPicture>`;
`ViewerPicture { thumb_key: u64, width: u32, height: u32, rgba: Vec<u8> }`.

Tests (photon-engine): an unedited photo at its own size, upright by its orientation; a
turned one turned; a cropped one cropped, and whole with `uncropped`; a video refused; a
file gone, in `copy_picture`'s words; `RENDERING` held holds it.

### 2. State: `viewer.rs`, `viewer_levels.rs`, `viewer_layout.rs`, `viewer_caption.rs`

- `Viewer::open(id, &index)`, `at()`, `id()`, `step(dir, &index) -> Option<i64>`,
  `first`/`last`, `follow(&old, &new) -> Followed`, `took(item)`, `lost(exists)`,
  `picture_changed(&old, &new)`, `rested(now)`/`rest_due()`, `hand_back()`.
- `halve`, `levels(full, max_side) -> Vec<Pixels>`, `level_for(widths, drawn_px)`.
- `photo_area`, `contained`.
- `caption(item, position)`, `format_taken`, `format_size`, `caption_line`.

Tests: the Svelte modules' cases, and each rule of "Rulings" above.

### 3. Previews

`Thumbs::new(loader, size, limit)`, `EngineThumbs { engine, size }`; the grid's are as
they were. Test: a preview is asked for and held under `ThumbSize::Preview`.

### 4. The view: `viewer_view.rs`

`ViewerView::show(ui, &ViewerData) -> Vec<ViewerAction>`: the ground, the picture (the
level, else the preview, else nothing), the message, the bar (star, close; the others of
later pull requests are not drawn yet), previous and next, the caption.

Frame tests: where the photo stands for a landscape, a portrait and a window resized; the
level drawn; each control's answer; the keys; the wheel's notch; a still frame.

### 5. The application

`App::viewer`, `open_viewer`, `close_viewer`, the two `Latest`s, the previews, `follow` at
`adopt`, the grid's `open` in `GridOutput`.

Tests (`tests/viewer.rs`, real photos): opened by Enter and by a double click; the preview
then the full picture; stepped by keys and wheel; Home and End; closed by Escape,
Backspace and the back button, onto the photo; a star; a hide and the step after it; a
photo purged under the viewer; a video's line; a still window.

### 6. Pictures, the checklist, CLAUDE.md, the pull request

`native-viewer-{light,dark}.png`; a picture of a detailed photo at fit for the spec's
first risk; the upload's cost measured and written here.

## Changed in the making

(Filled in as it is built.)
