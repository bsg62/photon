# photon: the viewer zooms out of its photo, menus and dialogs grow in

2026-11-03. Status: design, approved for planning once read.

## Why

The second round of the window's motion. The first (spec
`2026-10-17-photon-native-animations-design.md`, PR #227) made the sidebar and the info
panel slide and the grid's photos glide; the user checked it by hand ("looks very good")
and left the viewer, menus, dialogs and the People page for later. Now the viewer and
the menus "just pop in", and those are what the user opens most. They want them to move
as the rest of the window does now. As before, the aim is a window that feels a bit
nicer, not a showpiece.

The user saw both motions as moving mockups made from photon's own screenshots
(`.superpowers/brainstorm/`, not committed) and picked the numbers below in them.

## Goals

1. **The viewer zooms out of the photo's tile, as Apple Photos does.** Opening a photo
   from the grid (Enter, a double click) grows it from its tile to the place the viewer
   fits it in, over 200 ms. As it grows, the tile's square crop opens out to the whole
   photo, so the photo never stretches. The grid behind fades to the viewer's black
   over the same time. The viewer's controls (the close button, the arrows, the bar,
   the zoom slider, a caption) fade in once the photo has landed and do not ride
   along with it.
2. **Closing runs it backwards** (Escape, Backspace, the mouse's back button, the close
   button): the photo shrinks into its tile in the grid while the black fades back to
   the grid, over 200 ms. If the user stepped to another photo in the viewer, it shrinks
   into *that* photo's tile: the grid is brought to the photo first, as closing does now
   (`close_viewer` hands it back selected and in sight), and the shrink ends where the
   tile is then drawn.
3. **Menus fade in and grow from where they were opened**, from 92% of their size to
   their full size over 200 ms, out of the corner nearest the click. A menu that
   `place_menu` flips up or left (near the window's right or bottom edge) therefore grows
   up or left, so it still seems to come from the pointer. This covers every menu the
   one `MenuView` draws (a photo's, a folder's, an album's, a saved search's, a face's,
   the viewer's, a folder header's) and the dropdowns (`select_view.rs`: the sort and the
   grouping), which grow downwards from under their control.
4. **Dialogs fade in and grow from their middle**, 92% to full size over 200 ms, and
   their dimmed backdrop fades in over the same time. Every `Dialog` (the delete
   question, the keyword picker, the export, the person picker, Settings, the shortcut
   sheet).
5. **Menus and dialogs close with a quick fade**: 100 ms (half the opening), with no
   shrinking. A dialog's backdrop fades with it.
6. **The existing switch covers all of it.** The `animations` setting (Appearance in
   Settings), on by default. Off, the viewer, menus and dialogs appear and go in one frame,
   as now.

## Non-goals

- **The People page** is left as it is (the user's choice), and so is a viewer opened
  from it (below).
- **No zoom for a step in the viewer.** The arrows, the wheel and the slideshow change the
  photo as now; the slideshow's crossfade is its own.
- **Nothing else moves this round**: not compare, not the crop tool, not the toasts, not
  the folder drop's card that follows a drag, not the search box's help panel (a panel,
  not a menu - my call), not the sidebar's groups folding. The leftovers of the first
  round (a folder header leaving does not fade, the wheel scrolls a closing sidebar)
  stay leftovers.

## Anti-goals

The first round's hold here unchanged:

- **A still window stays still.** Frames are asked for only while a motion runs, and none
  once it has ended. The "still window draws nothing" tests keep passing with animations
  on.
- **Nothing waits for a clock.** The stored state flips at the press, and the motion only
  draws it. The viewer is open from the press (its keys are its own, and the grid's are
  not answered); a menu or dialog is open from the press; a closed one is closed at the
  press, and what was picked in it happens then.
- **No motion runs faster on a 120 Hz screen**: progress is time, not frames.
- **The zoom never waits for the full picture.** The viewer's picture is read on a
  thread and may arrive after the zoom has ended, or before it began.

## Details decided

**Timing and curve.** 200 ms opening for all three, as for the sidebar (`DURATION_MS`), on
the same `ease_out`; 100 ms for a menu's or a dialog's fade out, on the same curve. A motion
turned round in the middle (the viewer closed while it is still opening) turns from where
it is drawn at that moment, as the sidebar does.

**The viewer.**

- **What is drawn while it zooms.** The tile's own thumbnail texture (the grid draws the
  whole thumbnail and crops it with `cover_uv`, so the UV rectangle goes from the tile's
  crop to the whole picture). Once the viewer's own picture (its preview, or its levels)
  is in hand, the moving rectangle draws that instead, so the photo is sharp when it
  lands; before then, the thumbnail. For a video, its tile's poster, as the tile shows.
- **Where it lands** is where the viewer fits the photo with the info panel closed (it is
  closed at every opening): `viewer_layout::contained` in the photo's area, by the
  photo's aspect as the grid's index has it. If the picture, once read, is fitted
  elsewhere (an aspect the index had wrong), the last part of the zoom follows the
  rectangle the viewer now draws rather than jumping at the end.
- **The grid is drawn under the viewer while the viewer zooms open**, and only then: as a
  picture, taking no press, no hover and no key, as a sliding-closed sidebar is drawn.
  The rule that the shell is not drawn under the viewer (so the grid's keys are not
  answered twice) holds for its input from the press on. It is the grid as it stood at
  the press, at the position it stood at.
- **While the viewer zooms shut, the grid is the grid** (my call, from "nothing waits for
  a clock"): the viewer is closed at the press, as `close_viewer` closes it now, and the
  shrinking photo and the fading black are drawn over the live grid and take no press, no
  hover and no key. An arrow, a click on a tile or the wheel in those 200 ms is the
  grid's. The shrink ends where the tile is drawn in each frame, so a scroll moves its
  target with the grid, as a glide follows the grid's position. Enter or a double click
  during it opens the viewer again and the motion turns round from where it is drawn.
- **The tile stays empty while its photo is out of it.** The tile the photo grew out of
  (or shrinks into) is drawn as an empty place during the zoom, so the photo is not seen
  twice.
- **The black is the viewer's ground** fading from clear to opaque over the grid. The
  viewer is dark in both themes, so the light theme fades from the light grid to black.
- **The controls fade in over 100 ms once the photo has landed** (my call; the mockup the
  user approved faded them in about that time), so an opening is 300 ms in all. They are
  pressed from the frame they begin to fade in. Before that a press where they will stand
  is nobody's (a click on a button not yet drawn is a surprise); the viewer's *keys* are
  its own from the press. In the plain-fade cases below the controls fade with the rest
  of the viewer over the same 200 ms.
- **A tile with nothing to grow** - its thumbnail not built yet, or failed, or a video
  with no poster - opens and closes by the plain fade below (my call).
- **Closing from a zoomed or panned photo** shrinks from the rectangle the photo is drawn in
  at that moment, clipped to the window, into the tile. The controls, the info panel and
  the faces' outlines go at the press, and the photo and the black are what moves.
- **Keys during the opening zoom** are the viewer's. A step (an arrow, the wheel) during
  it ends the zoom at once, and the viewer shows the next photo as now (my call: a zoom
  that lands on another photo than it left is wrong either way).
- **When there is no tile, the viewer fades** in or out over the same 200 ms instead,
  with the photo already in its place. That is:
  - opening from the People page (a face) or from compare - there is no tile under the
    pointer;
  - closing a viewer opened over the People page, which lands back on the page;
  - closing onto a photo the view no longer holds (`hand_back` gives none), a link in the
    info panel that leaves the viewer for another view (`shut_viewer`), and a grid of
    other results closing it (`adopt`);
  - closing a viewer that is in a slideshow: the window leaves fullscreen at that moment
    and changes size, so a tile's place is not known yet (my call).
  - closing onto a tile not drawn on screen: the hand-back brings it into sight, but if
    the grid lays out the tile somewhere the window does not show, it fades.
- **The thumbnail and the viewer's picture are held** for the closing zoom after the viewer
  has closed (`shut_viewer` lets go of the previews), for the 200 ms only.

**Menus.**

- The menu is placed by its full size (`place_menu`, as now) and drawn scaled about the
  corner it grows from: the click's point, clamped into the menu's rectangle. A dropdown's
  list grows from its top edge, at the middle of the control.
- **Hit tests are the menu's full-size rectangle from the first frame**, as the grid's are
  its new places during a glide. In 200 ms a press inside the growing menu but outside its
  drawn edge is not worth a second geometry.
- **A menu fading out is drawn only.** It takes no press, no hover and no key, and does not
  stand in the way of what is under it: the window under it is the window's from the
  press that closed it. It draws the items it had at that press.
- **A menu replaced by another** (a right click while one is open) fades out where it was
  while the new one grows in where it is opened.

**Dialogs.**

- They grow from the middle of the rectangle they are drawn in. A dialog that stands
  where it would stand at its tallest and grows downwards (the pickers, `tallest`) grows
  from the middle of what it draws in its first frame.
- **A dialog fading out is drawn only**, and so is its backdrop. The rules that already
  keep the press that closed a dialog from reaching what is under it
  (`App::overlay_closed`) apply as now.
- egui's own fade of a new area stays off (`fade_in(false)`, as `dialog_view.rs` says
  why); this motion is photon's, timed by its clock.

**Tests.** As in the first round: the whole-application tests launch with animations off,
and the motions' own tests turn them on and say so (`launch_animated`, `opened_animated`,
`over_animated`). `native-shot` draws with them off, or after they settle. Each rule above
that a test can hold has one, shown to fail with its change reverted: frames asked only
while running, progress by time, the stored state flipped at the press (the viewer's keys
from the press, a fading menu taking no press), the zoom's start and end rectangles and
UV, the fade fallbacks, the switch.

## Left to the builder

- Where the motion state lives and what it is called. The precedent is a state module
  for the arithmetic (egui-free, in `STATE_MODULES`) and the views drawing from it;
  `animate::Slide` and `ease_out` are there to reuse.
- How the grid is drawn as a picture under a viewer that is zooming (a disabled child, as
  the closing sidebar is, or the last frame's tiles kept), and how its thumbnails are kept
  meanwhile.
- How the hand-over from the thumbnail to the viewer's picture is drawn mid-zoom: a swap
  or a short crossfade.
- How a menu or dialog is scaled: egui has no transform for an area's contents, so a
  scaled layer (`Context::transform_layer_shapes`) or drawing it at its size and
  interpolating; the menu's text must stay crisp once it has landed.
- How a fading-out menu or dialog keeps its last items and shapes for 100 ms.
- The smoke checklist's new lines.

## What only a person can judge

Whether it feels right: the zoom's speed, the hand-over from thumbnail to picture, how a
growing menu looks near an edge. That goes on `docs/smoke-checklist.md`.
