# The native viewer, 8 of 8: compare

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** two to four photos side by side in the native window, at one zoom over one
point, each saying what distinguishes it from the others - from `C` in the grid and the
tile's "Compare N photos".

**Architecture:** `compare.rs` (state) is `createCompare` and `differingFacts`: the panes,
the pane with the keys, and the one zoom - the viewer's own `Zoom`, measured in a pane's
size. `compare_view.rs` draws the panes in two columns and answers the keys, the wheel and
a drag as `CompareAction`s. `app.rs` asks what each photo is off the UI thread before
anything is shown (`App::comparing`), draws compare over the whole window as it draws the
viewer, gives the panes their previews, and once the zoom has left the fit asks for each
pane's full picture in turn (`App::compare_pictures`).

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("Compare").

## Global constraints

- No SQLite and no decode on the UI thread. The one write is a star, the folder's
  `.picasa.ini`, through the engine's own command.
- State modules name no egui type and are listed in `STATE_MODULES`; the gate at every
  commit; every new test shown to fail with its rule broken.

## Rulings made in the plan

- **Every pane's full picture once zoomed**, the spec's rule, where the Svelte compare asks
  only for the focused pane's: the native window holds the pictures as textures and makes
  them one after another, the pane with the keys first. Fitted, a pane is drawn from its
  preview, twice or three times a pane at an ordinary window size; no copy fitted to a box
  is made for a pane.
- **The wheel zooms by itself**, as in the Svelte compare (no Ctrl): there is nothing to
  step to. A notch is a factor of 1.15; a trackpad's points add up, a hundred to a notch.
- **A star is the library's**: pressed, it is written, and the pane says so once the write
  has answered - where the Svelte compare flips it at once and puts it back on failure.
- **Enter opens the pane with the keys in the viewer** and closes compare, when the grid on
  screen holds the photo; otherwise it does nothing (the review's ruling, below).
- **Compare opens when the library has said what every photo is**: half a comparison is
  worse than none, and a refusal is said and opens nothing.
- **Escape closes; while a drag is held, Escape lets go of the drag and does nothing
  else**, and no other key is answered - the band's rule.
- What the Svelte compare says over a pane whose preview could not be loaded is not said:
  the pane stays blank.

## Probes

Twenty-six, each rule broken by an exact replacement and its test run: twenty-two failed as
stated. The four that passed, each settled: a press at fit was not seen to hold nothing (it
would have held the keys for the drag), and a press on the cross over a pane not seen to
give the pane no keys - both tested now; the count asked of before the library is read had
no test (`C` with the whole library selected would read every photo of it) - counted now,
`App::compares_asked`; and a box of nought made into no box did nothing, `fitted_copy`
making no copy for one already - taken out.

## What the review found

An independent review (opus, its own worktree) wrote a failing test for each finding; they
are `tests/compare_edges.rs`, with the fix pass's beside them.

- **Important: a pane's star followed the grid on screen alone.** In Starred a photo
  unstarred from compare left the grid, its pane said "starred" for good, and `S` unstarred
  it again. The star now follows what was written as well (`Wrote::Stars` carries its ids).
- **Important: `C` was answered under a held press** (Enter is not): compare covered the
  window, the button came up there, and after Escape a band was drawn with no button down.
- **Important: compare read the keys of the frame it opened in** - a `.` meant for the grid
  starred pane 1 alone - **and opened over whatever had come meanwhile** (a viewer opened by
  Enter in the same frame as `C`). It goes up at the end of the frame now, over the grid
  alone, and its answer is dropped otherwise.
- A pane's key was fixed at the opening (a photo turned elsewhere never had its full picture
  made); a pinch did not zoom; a press on a message over a pane gave the pane the keys;
  AccessKit was not told a pane is starred; the full pictures came in pane order, not the
  ringed pane first; `Pane::video` was never read (taken out). Each fixed, each with a test.
- Ruled, not as the review proposed: **Enter on a pane the grid no longer holds does
  nothing**, where the review wanted the viewer opened on it - the viewer goes through the
  grid's photos, and opening it elsewhere would be a view switched under compare.
- Seventeen of its probes passed; each rule has a test now, or is gone: the search sent
  while compare is open (sent when it closes), and the view made fresh at the opening as
  well as at the closing.
- Statements corrected: "a pane's preview is about as large as a pane" (it is two or three
  times a pane at an ordinary window size), "the panes' stars follow the grid on screen",
  "through the grid's key gate", the view's "no control here is a stop of the Tab key", and
  the plan's "fill in one by one rather than waiting behind each other".

## Not tested

- Compare's answer taken in the very frame Enter opens the viewer: the end of the frame
  asks whether a viewer is open before putting compare up, and no seam holds an answer
  back for one frame - a test that waited for the answer found it taken a frame later,
  where the viewer's own branch drops it, and passed with the check gone.
