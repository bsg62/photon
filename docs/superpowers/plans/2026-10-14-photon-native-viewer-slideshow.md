# The native viewer, 7 of 8: the slideshow, fullscreen and F11

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** a view is played as a slideshow from the native viewer - fullscreen, photo after
photo, crossfaded, in the view's order or shuffled, with the controls out of the way of a
resting pointer - and F11 puts the window in and out of fullscreen in the grid and in the
viewer.

**Architecture:** `slideshow_order.rs` (state) is `slideshow-order.ts`: the next photo that
is not a video, and a shuffled show's stride. `slideshow.rs` (state) is `createSlideshow` on
a clock that is a number: the countdown, the pause, the hold under a dialog, the pointer's
rest, whether the show took the window fullscreen. `viewer_view.rs` has the bar's button,
the keys, the controls hidden at rest and the crossfade drawn; `app.rs` holds the show while
it is open (`Viewing::show`), reads the interval and the shuffle once at its start off the
UI thread, steps it, asks the window for fullscreen (`ViewportCommand::Fullscreen`), and
answers F11 everywhere.

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("The
slideshow"; "Decided while writing" 8: F11 in the grid as well).

## Global constraints

- No SQLite on the UI thread: the interval and the shuffle are read by a task. Nothing is
  written: the Settings that set them are sub-project 6's.
- A slideshow between two photos is a still window: a frame is asked for at the
  countdown's end, while the crossfade runs and when the controls go, and at no other time.
- State modules name no egui type and are listed in `STATE_MODULES`; the gate at every
  commit; every new test shown to fail with its rule broken.

## Rulings made in the plan

- **The countdown begins when the photo's full picture is shown, or when it cannot be**
  (refused, cannot be shown, gone) - not at its preview, which the Svelte viewer does not
  show as "loaded" either. Told twice, it runs on from the first (the Svelte store re-arms
  at each `shown()`, which an effect calls once; here the frame would call it in every
  frame).
- **The full picture of a photo the show steps to is asked for at once**, not after the
  rest a step by hand waits: nothing is being stepped past.
- **The show starts from the photo on screen** and goes on from it; started on a video it
  goes first to the next photo, and with none says "There are no photos here to show."
- **Escape stops the show first and closes the viewer second**; Space pauses and plays; `S`
  starts and stops it; the arrows and the wheel step through the show's order, skipping
  videos and wrapping; the buttons at the sides wrap too.
- **A shuffled show's stride** is drawn by the standard library's own seeded hasher
  (`RandomState`): no new dependency for the one random number a show needs.
- **The crossfade is of the picture as it was drawn**: the texture and its rectangle are
  kept as the show steps and drawn over the arriving photo, fading over 600 ms once the
  arriving photo is shown. Only at fit, as in the Svelte viewer.
- **The controls go after two and a half seconds of a resting pointer**: the bar, the
  slider, the buttons at the sides and the cross, and the pointer itself. Any move of the
  pointer, any press or key brings them back.
- **The window goes fullscreen as the show starts**, unless it is already; it leaves
  fullscreen as the show stops only if the show put it there. Closing the viewer stops the
  show. F11 is the window's own fullscreen, in the grid and in the viewer, at its press.
- **The show is held** - its countdown waits, and starts again whole - while a dialog or the
  viewer's menu is over it.
- The info panel closes as the show starts, as in the Svelte viewer; the crop tool cannot
  start one (the keyboard is the tool's), and `C` stops the show and opens the tool, as
  `startCrop` does.

## Review focus

1. A step of the show to a photo that is no longer there: the view rebuilt under it (a scan,
   a hide), the photo on screen orphaned or gone, a view of one photo, of none.
2. Fullscreen given back wrongly: the user's own fullscreen, F11 during a show, the viewer
   closed by other results, the window closed.
3. A countdown that runs while it should not (a picture still loading, a dialog over it,
   paused) or never runs (a picture that cannot be made).
4. Frames asked for between two photos; the crossfade left drawn.
5. Keys and the wheel during a show, with the panel, a menu or the dialog over it.

## Changed in the making

- **The countdown's end leaves the step to the caller** (`Slideshow::due` does not
  forget the photo): a view of one photo has nowhere to go, and a show that counted the
  same photo again would draw a frame every interval, where the Svelte show stands still.
- **`shown` counts once per photo**: the Svelte store re-arms at every `shown()`, which
  its effect calls once; here it is asked in every frame.
- **F11 is read before anything else in the frame and taken out of it**, so it works under
  a menu, a dialog and a held band, as the Svelte window's listener does.
- **The test of a still paused show runs over a library on a drive that is not there**: a
  watched folder's own background work asks for a burst of frames every few seconds, which
  is not the show's.

- **The picture kept for a fade is let go of when the viewer shuts** (`ViewerView::let_go`):
  `a_viewer_closed_holds_no_texture`, from the first pull request, found it held.

## Not tested

- Fullscreen in a real window manager: the tests' window does what it is asked; whether a
  desktop does it, and comes back to the window it was, is the smoke checklist's.

## Probes

Forty-nine, each rule broken by an exact replacement and its test run: forty-three failed
as stated at the first run. The six that passed, each settled:

- The interval's answer re-armed the countdown only while one ran: arming asks the same
  of itself. Taken out.
- The controls at rest were only *not drawn* in the test; that no glass is drawn at all is
  tested now (and that a press where they were is the photo's, through the whole
  application, after the review: below).
- Three rules of the application had no test: the panel shutting as a show starts, a show
  not waiting for the rest a step by hand waits for (the picture is asked for in the frame
  of the step), and the buttons at the sides going round the ends in a show. Each has one.
- One probe was aimed at a test that could not see its rule; aimed at the right one it
  failed.

## What the review found

An independent review (opus, its own worktree) wrote a failing test for each finding; they
are `tests/slideshow_edges.rs`, with the fix pass's beside them.

- **Important: a show stepped from a lost photo's offset as if the photo stood there.**
  `H` on the photo on screen during a show skipped the next photo, and in a view of two the
  show stood on the hidden photo for good. The walk now begins a place back from a photo
  the view has lost - hidden or gone - and `go_to`'s own refusal replaces the filter that
  stranded it; the plan's "Not tested" line that said it followed the rule was untrue.
- **Important: the show was not held while a keyword was typed in the info panel**, and
  the rest of the word went to the photo stepped to. Held now.
- **Important: an arrow and `S` in one frame counted a picture still being made.** The
  show starts counting only for a photo the frame did not step from.
- A menu asked for in the frame before the countdown's end did not hold the show (the hold
  is asked first now); a press of a resting pointer was judged against the controls it woke
  (the controls wake on the pointer moving alone, as in the Svelte viewer); a held F11
  flickered (its press only); a show stopped and started again at once played windowed
  (fullscreen is what was last asked until the window has done it).
- Seventeen of its twenty-four probes passed. Each rule now has a test, or is gone: the
  fade's frames, its letting go at the stop, a video's start counted from the photo after
  it, a photo that is gone or that the library cannot show counting as shown, the crop
  tool stopping a show, the bar's width kept at rest, a one-photo show making nothing
  again. Taken out as doing nothing: the shuffle answer's `active`, the stride's
  `usize::MAX` sentinel, `start_show` clearing a fade, a video counting as shown (a show
  never stands on one), the filter in `show_step`. The hold under a dialog is the same
  `paused` the viewer is told, held by `tests/faces.rs`.
- Statements corrected: `ViewerView::drawn`'s comment, a test helper's, CLAUDE.md on
  fullscreen and on the frames a show asks for (a step's rest asks one, inside the fade).
