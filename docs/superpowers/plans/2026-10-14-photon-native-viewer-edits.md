# The native viewer, 6 of 8: turns and the crop

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** a photo is turned a quarter left or right from the native viewer, cropped with
a rectangle drawn on it, and put back to its original - each a write of the edit the
engine already stores, and none a write to the photo.

**Architecture:** `crop.rs` (state) is `crop.ts`: what a drag and a ratio make of the
rectangle, and how it is written. `crop_tool.rs` (state) is `createCropTool`: the
rectangle, the ratio it is held to, the drag in progress. `crop_view.rs` draws the tool
over the picture and reads its drags. `viewer_view.rs` has the bar's three tools and
"Original", the keys, and the tool's own bar; `app.rs` holds the tool while it is open,
asks for the picture turned and not cropped, and queues the writes
(`commands::rotate_item`, `set_item_edit`).

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("Turns and
the crop"; "The keys and the mouse").

## Global constraints

- **No photo is written, moved or deleted.** A turn and a crop are `items.edit_turns`
  and `edit_crop` in `library.db`, written by the engine under `Engine.edit_write`; the
  picture that follows is the engine's render.
- **Nothing here draws a turn or a crop.** The write rebuilds the grid, the photo's key
  changes, `picture_changed` says so, and the viewer asks for the picture again.
- No SQLite and no decode on the UI thread; state modules name no egui type and are
  listed in `STATE_MODULES`.
- The gate at every commit; every new test shown to fail with its rule broken.

## What was read first

- `ui/src/lib/crop.ts` and its tests; `crop-tool.svelte.ts` and its tests;
  `Viewer.svelte`'s `rotate`, `resetEdit`, `editable`, `startCrop`, `applyCrop`,
  `cropBox`, the crop tool's markup and styles (`.crop-area`, `.crop-rect`,
  `.crop-handle`), its bar, the keys while it is open, and what is not drawn while it is
  (the round buttons, the zoom's slider, the photo's caption, the faces).
- `commands::rotate_item`, `set_item_edit`, `viewer_picture`'s `uncropped`;
  `photon_core::edit` (`Edit::new`, `Crop`, `CROP_UNIT`, the least crop the library
  takes); `ViewerItem::edit`, `uncropped_width`, `uncropped_height`.
- The native viewer as it is: `Viewing`, `PictureAsk`, `FullPicture`, `show_viewer`'s
  asking, `Viewer::took`; `select.rs` and `select_view.rs`, whose list opens downwards
  only.

## Rulings made in the plan

- **A turn's key is answered at its press and not at its repeat**, like every key that
  writes: a held `R` would spin the photo for as long as it is held, a write and a
  render each time.
- **A photo can be edited when the library has said what it is and it can be shown**:
  not a video, not one that is gone or cannot be shown, and not one that has left the
  view (`orphaned`) - the Svelte viewer's `editable`. Where it cannot, the three tools
  are in their places, fainter, and take no press; `R`, Shift+R and `C` do nothing.
- **The crop tool is the viewer's while it is open** (`Viewing::crop`), and it closes
  when the photo under it is another picture: a step cannot be made under it, but a
  rebuild can say the photo has been turned or rewritten elsewhere, and the rectangle
  was drawn on the picture before. So does a photo that leaves the view or the library.
- **The tool is drawn over the picture turned and not cropped**, which is another
  picture than the one on screen only where the photo has a crop already. Where it has
  none the picture on hand is that picture, and none is asked for. Where it has one,
  the uncropped picture is asked for as the tool opens (`PictureAsk::uncropped`), and
  until it lands **the picture on hand is drawn where its crop lies in the uncropped
  one** - the part that will be kept, in its place, with black round it - where the
  Svelte tool shows nothing until its image has loaded.
- **The rectangle's box is worked out from the size the library gives**
  (`uncropped_width`, `uncropped_height`), fitted into the window, and not read off a
  texture: it is the same box before and after the uncropped picture lands.
- **"Apply" closes the tool when its write has succeeded**, as the Svelte tool does: a
  rectangle the library refuses leaves the user where they can put it right. Until the
  answer the tool stands and takes no drag (`CropTool::busy`), and the answer counts
  only for the tool that sent it (a number, as a name typed in the sidebar has).
- **While the tool is open the keyboard is its own**: Enter applies, Escape cancels,
  and nothing else is answered - no step, no star, no hide, no panel. The wheel steps
  to no photo and zooms nothing; the other button asks for no menu.
- **The info panel is closed as the tool opens**, and the zoom put back to the fit, as
  in the Svelte viewer: the rectangle is drawn on the whole picture, fitted to the
  whole window.
- **The ratio is chosen from photon's own dropdown, opening upwards** (the spec):
  `select_view.rs` learns to open above its control.
- "Original" is offered only while the photo has an edit, between the crop's tool and
  the info panel's, so the bar is wider by one button on an edited photo - as in the
  Svelte bar.

## Review focus

1. A write that goes to another photo than the one it was pressed on: a turn, "Original"
   or "Apply" in the frame of a step, with the photo read again under it, with the tool
   open across a rebuild.
2. The tool left open over something it is not of: the photo turned elsewhere, gone,
   orphaned; the viewer closed by other results; a second tool opened while the first
   one's write is on its way.
3. A drag's endings: the button up, the window left, a touch cancelled, Escape with the
   button down, the tool closed under a held button; a drag that began on a handle and
   one that began inside the rectangle; a press outside the rectangle.
4. The picture under the tool: before the uncropped picture lands, when it cannot be
   made, at another window size, on a photo stored on its side, with a crop at the
   picture's edge; the picture after "Apply", after "Cancel", after "Whole photo".
5. Keys, the wheel and the buttons that reach the viewer behind the open tool, and the
   ones the dropdown must keep for itself.

## Tasks

### 1. `crop.rs` and `crop_tool.rs`

`Rect`, `FULL`, `Handle`, `HANDLES`, `MIN_SIDE`, `Ratio`, `ASPECTS`, `fraction_ratio`,
`drag_rect`, `fit_aspect`, `to_wire`, `from_wire`; `CropTool` (`begin`, `rect`,
`aspect`, `set_aspect`, `clear`, `start_drag`, `drag_to`, `end_drag`, `wire`). Tests: the
cases of `crop.test.ts` and `crop-tool.svelte.test.ts`; the smallest rectangle a drag
makes is one the library takes.

### 2. The bar's tools and the turns

`ViewerAction::Turn(bool)`, `Original`, `Crop`; the three icons; `ViewerData::editable`
and `edited`; `Write::Turn`, `Write::Edit`. Tests: the keys and the buttons, a repeat,
a photo that cannot be edited; through the application, a photo turned and the picture
that follows, "Original".

### 3. The dropdown opening upwards

`select_view.rs`: a placement. Tests in whole frames.

### 4. The crop tool's view

`crop_view.rs`: the dimming, the line, the eight handles, their cursors, the drags read
from the button's own events. Tests in whole frames.

### 5. The tool in the viewer and the application

`Viewing::crop`; the uncropped picture; the tool's bar; what is not drawn while it is
open; "Apply", "Whole photo", "Cancel"; the tool closed when the picture is another.
Tests through the whole application, over real files.

### 6. The pictures, the checklist, CLAUDE.md, the pull request

## Changed in the making

- **The tool closes by one rule** (`if !editable` in `show_viewer`) and one exception
  (`took`), not by a line in each place a photo can stop being editable - gone, orphaned,
  stepped from, its picture refused. A step made in the frame the tool opened in is that
  rule too: the photo stepped to is not known yet.
- **A turn, "Original" and "Apply" are written in the frame's action loop, by the photo the
  library last described**, like the star: a turn after a step in one frame is of no photo
  (nothing is known of the one stepped to), one before it is of the photo seen.
- **A video's bar has no tools for turning or cropping**, as the Svelte bar has none; the
  plan had them fainter. A photo that cannot be edited has them fainter.
- **The dropdown is told its palette** (`SelectData::palette`): in the viewer it is dark in
  a light window, as the viewer's glass is.
- **The tool's bar is drawn before its keys are read**, so that the list's Enter and Escape
  are not the tool's as well.
- **A refusal reopens whatever tool is open**, not only the one that sent it (one number
  fewer): a second "Apply" of another tool would write again, which is harmless.
- **The tool's picture waits for the viewer's rest like any other** - a rule that asked for
  it at once after a step could not be told apart by a test, and was taken out.
- Taken out as doing nothing, found while writing the probes: a filter that kept the
  preview from under the picture without the crop (the picture's levels always win), and a
  `tool.is_some()` the comparison of numbers already made.

## Not tested

- A turn on its way when "Apply" is pressed: the tool read the turns before the turn
  landed, and writes them back. It needs `R`, `C` and Enter inside one round trip, and is
  so in the Svelte viewer.
- The crop tool in a window resized while it is open: its picture is asked for again after
  the size has stood, by the rule every picture follows (tested for the photo's own).

## Probes

Fifty-five, each rule broken by an exact replacement and its test run: fifty failed as
stated. The five that passed were findings, each settled before the pull request:

- The tool took no drag while busy only in `crop_view`'s own tests; that the viewer hands
  it `busy` had none (`under_a_tool_whose_write_is_on_its_way_a_drag_moves_nothing`), nor
  had the application's `sent` (a drag after Enter with the write held, in
  `an_answer_closes_only_the_tool_that_sent_it`).
- The application's own "not while sent" on "Apply" did nothing the view had not done: a
  busy tool answers no Enter and its "Apply" takes no press. Taken out.
- The answer's closing of the tool was covered by the new picture closing it: every test
  changed the crop. The test now writes the crop the photo has first.
- Where the stand-in lies in the tool's box was decided in the application and tested only
  in the view: it is `Cropping::part` now, read by a test (`App::crop_part`) and by where
  the frame drew the picture.

## What the review found

An independent review (opus, its own worktree) wrote a failing test for each finding; its
tests are `tests/edits_edges.rs`, with the fix pass's beside them.

- **Important: every edit made its picture twice.** `took` asked again when the viewer's
  key equalled the new one, which `follow` had already set from the grid: every turn,
  "Original" and "Apply" rendered the photo twice, blank meanwhile. `took` now compares
  with the `Shown` it replaces (a rule older than this pull request, made every edit by it).
- **Important: a picture change under the tool asked for a picture nobody draws.** The tool
  now closes when the grid's key for the photo is not the one it opened under, a round
  trip before the library says so; its picture needs no filter by the photo's state any
  more.
- "Original" left the bar at every step between edited photos, moving the buttons after it
  for the round trip: it stays while nothing is known of the photo stepped to.
- The second click of a double click on the crop button landed on the tool's bar ("Whole
  photo"): a press within a double click's time of the tool's opening presses nothing there.
- `MIN_SIDE`'s comment was wrong: a ratio chosen on the least rectangle shrank a side below
  the library's least crop. `fit_aspect` now grows it back, inside the picture.
- Twelve of its thirty probes passed. Six were held by its tests, now ours; of the rest,
  `!gone` in `editable` and the tool's want filter were taken out as doing nothing, and
  `self.ratio = SelectView::default()` too (the dropdown's own rule shuts a list whose
  control is not drawn); `!is_video` (the engine would be asked, and refuse aloud), the
  toast only for an open tool, `self.press = None` and `self.faces.clear()` under the tool
  each have a test now.
- Statements corrected: CLAUDE.md said the tool's picture is asked for at once (it waits for
  the rest), and described the double asking as one.
