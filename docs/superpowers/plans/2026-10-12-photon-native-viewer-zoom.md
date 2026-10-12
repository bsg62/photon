# The native viewer, 2 of 8: zoom and pan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** the photo in the viewer is zoomed, to four times its fitted size, and panned.

**Architecture:** the arithmetic is `nav.ts`'s zoom half in a state module of its own
(`viewer_zoom.rs`); the zoom and the pan are the viewer's state (`Viewer::zoom`), reset
where the photo or its picture changes; the view turns keys, the wheel with Ctrl, a pinch,
a double click, a drag and the slider into calls on it and draws the picture where it
says.

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("The keys and
the mouse": zoom, pan; "The view": the zoom's slider; "The picture": the level drawn).

## Global constraints

- Nothing here writes anything: no file, no row of the library.
- State modules name no egui type, and are added to `STATE_MODULES`.
- The gate at every commit; every new test shown to fail with its rule broken.
- A test of the whole application waits for a state, never for a length of time.
- No control of the viewer is a stop of the Tab key.

## What was read first

- `ui/src/lib/nav.ts` from `clampZoom` on, and `nav.test.ts`'s cases for it.
- `Viewer.svelte`: `zoomTo`, `onwheel`, `ondblclick`, `onzoom`, the three pointer
  handlers, `fromCentre`, `viewport`, the slider's markup, and where the zoom is put back
  to fit (a step, another picture).
- egui 0.36.2: `egui-winit` sends a wheel as `Event::MouseWheel` with its modifiers and a
  pinch (macOS' `PinchGesture`) as `Event::Zoom(factor)`; egui's own `zoom_delta` scales a
  wheel with Ctrl by its scroll speed, so it is not read. `Key::Plus`, `Equals`, `Minus`,
  `Num0` are named by what they type, the number pad's among them.

## Rulings made in the plan

- **The zoom and the pan live in `Viewer`**, as the spec has them: `land` and a changed
  picture (`took`) put them back to fit there, where the state tests see it, and nothing
  in the application has to remember to.
- **The pan is bounded by the photo's area, not by the photo**, as `clampPan` bounds it:
  a photo that does not fill its area at fit can be moved until its letterbox is at the
  window's edge. Ported as it is.
- **The slider sets the zoom and leaves the pan where it is**, pulled in where it has to
  be (`onzoom`); the keys zoom about the middle; the wheel, a pinch and a double click
  about the pointer.
- **A notch of the wheel with Ctrl is a step of a fifth** (`wheelZoomFactor` of a hundred,
  capped at twenty): a line is taken for a hundred of the web view's pixels, a trackpad's
  points as they come.
- **A double click is read from two clicks of the first button on the photo**, each a
  press that came up without moving, within egui's double-click time and a click's
  distance of each other: the press is read from the button's own events for the pan, and
  one reader decides what a press was. The double click that opened the viewer is not the
  first of one here: the view is made anew at the opening.
- **At the photo's own pixels the picture is drawn on whole pixels**, from its full
  level: that zoom is what a photo is checked for sharpness at, and half a pixel off it
  is a linear filter's blur over the whole photo.
- **No zoom for a video, nor where something is said in the picture's place**; the slider
  is not drawn for a video and takes no press where there is no picture.
- **The fitted copy is drawn only at fit.** Zoomed, the level that is at least as large
  as what is drawn.
- **The pointer over a zoomed photo is a hand**, open and closed (`grab`, `grabbing`).

## Review focus

1. A pan that outlives its gesture: the button let go of over another window, a touch
   cancelled, the viewer stepped or closed under a held button, a control pressed.
2. The point a zoom holds still, in a window of any size and at any scale; a zoom during
   a drag; the pan after the window is made smaller.
3. Which texture is drawn at which zoom, and where: the fitted copy off fit, the level
   at 100%, the preview zoomed before the full picture is there.
4. Keys and wheel answered twice or wrongly: `+` with a chord, the wheel with Ctrl
   stepping as well, a double click on a control, the click that opened the viewer.
5. A still window while zoomed; frames asked for by a hand that does not move.

## Tasks

### 1. `viewer_zoom.rs` (state)

`MIN_ZOOM`, `MAX_ZOOM`, `ZOOM_KEY_STEP`, `WHEEL_ZOOM_CAP`, `DOUBLE_CLICK_ZOOM`;
`clamp_zoom`, `wheel_zoom_factor`, `actual_size_zoom`, `zoom_at`, `clamp_pan`; and
`Zoom { zoom, pan }` with `to(next, point, area)`, `by(factor, point, area)`,
`set(value, area)`, `pan_to(x, y, area)`, `held_to(area)`, `is_fit()`.

Tests: `nav.test.ts`'s cases for each, and `Zoom`'s calls.

### 2. `Viewer::zoom`

`zoom()`, `zoom_mut()`; fit again at `land` and at a changed picture, kept at any other
answer and at `follow`.

### 3. The view

Where the picture is drawn (`placed`: the fitted rectangle scaled about the middle of
the area and moved by the pan), which texture, the snap at the photo's own pixels; the
keys; the wheel with Ctrl and the pinch, at the pointer; the press on the photo - a click,
a double click, a pan - with the band's endings; the slider and its percentage; the hand.

Frame tests: each of the above, and each ending.

### 4. The application

`ViewerData` is given the zoom; nothing else. Tests through the whole application: zoomed
by the keys and put back by `0`; fit again after a step and after a turn made elsewhere,
kept across a star; the double click that opened the viewer zooms nothing; a zoomed window
is still.

### 5. Pictures, the checklist, CLAUDE.md, the pull request

`native-viewer-zoomed-{light,dark}.png`.

## Changed in the making

- **"Its own pixels" are the full picture's, not the library's number.** The plan had
  the view given the photo's width as the library says it, turned by its orientation. It
  takes the width of the largest level there is to draw: before the full picture is
  made, or where it cannot be (a drive that is not there), a double click goes to twice,
  since there are no more pixels to show than the preview's; and a panorama whose full
  level the device cannot hold is checked at the level it can. One wiring fewer, and no
  fixture needed for a photo stored on its side.
- **The fitted copy needs no "only at fit"**: it is drawn where what is drawn is its own
  size within a pixel and a half, which a zoom is not. The condition was there and did
  nothing.
- **The pan begins at once**, not once the pointer has gone further than a click may:
  the photo follows the hand from the first point. What the distance decides is only
  whether the press was a click when it comes up.
- **A press held across a step remembers no pan**: the next photo is fitted, and the
  drag goes on from its middle.
- **Two things found on the way, in commits of their own.** A test of the menu clicked
  "Hidden" before its row was drawn and failed on Windows' CI once (`native-ui` at
  a1d9abc): the tests' click waits for the row. And `native-shot` crashed in the Vulkan
  loader about once in four runs when all its pictures made their GPU devices at once -
  seen when two pictures more were added, read from the core dump: one device is made
  at a time.

**Probes**: ninety-two, each rule broken in turn with the test that was to fail. Seven
passed the first time. Two were lines that did nothing and went: the slider's own clamp
of where it was pressed (the zoom stops at its ends by itself) and a second way of
asking whether a press was on the photo (whether its surface "contains the pointer",
beside the layer egui finds on top - which is the one judge, and stays). Four were tests
that could not see their rule and were made to: the keys zooming about the middle with
the pointer elsewhere; the pan beginning at the first point; a press held across a step
that began on a photo already moved; and "nothing zooms a video", whose double click put
back what its key had done, so the whole list passed with the rule gone. One had no
test: a press just beside a button is the photo's.
