# photon: animations for the sidebar, the grid's reflow and the info panel

2026-10-17. Status: design, approved for planning once read.

## Why

Three things in the native window happen in one frame, and the user wants them to move
instead: the sidebar shown and hidden, the info panel in the viewer opened and closed, and
the grid's photos *jumping* whenever the number of columns changes - which is what dragging
the splitter looks like today, since the sidebar's edge already follows the pointer and the
tiles already widen pixel by pixel; it is the row breaks that teleport. The aim is a window
that feels a bit nicer, not a showpiece.

## Goals

1. **The sidebar slides.** The toggle (the top bar's button, Ctrl+B) changes the sidebar's
   drawn width from nothing to its width, or back, over about 200 ms. The grid's room
   changes with it in every frame, so the photos move aside as it comes in.
2. **Photos glide to their new places when the columns change.** A photo that was on screen
   before the change and is on screen after it moves, and resizes, from where it was drawn
   to where it now belongs, instead of appearing there. The photo at the end of a row slides
   down to the start of the next. This covers every change of the grid's *geometry*:
   - the sidebar's slide (goal 1), which passes through several column counts;
   - dragging the splitter, and moving it with the arrow keys;
   - resizing the window;
   - the size buttons in the top bar.
3. **The user's own changes glide.** When a write the user made takes photos out of the
   grid on screen or puts them in, the grid that comes back animates: a photo that left
   fades out where it stood, and those after it glide into the gap. The writes in question
   are the ones that change which photos a view holds: hide and unhide (`H`, the menus,
   "Hide folder"), a star taken off in Starred, a photo taken out of the album, person or
   keyword on screen. Anything else that rebuilds the grid does not glide (below).
4. **The info panel slides.** Its button and `I` in the viewer change the room the panel
   takes from nothing to its width, or back, over about 200 ms; the panel comes in from the
   right edge, and the photo's area shrinks with it in every frame, so the photo is fitted
   smaller (and its zoom, pan, faces and the next-photo button follow) as it goes.
5. **A switch in Settings turns all of it off.** In Appearance, beside the theme. On by
   default. Off, every one of the changes above happens in one frame, as it does today.
   Stored in the library like the theme (a key of the settings table, a photon-core
   accessor and an engine command - no schema change), and read in `App::new` before the
   first frame as the theme is, so a window never animates the first toggle of a session
   in which the user had switched it off.

## Non-goals

- **No other animation in this round.** Not the viewer opening or closing, not menus or
  dialogs, not the People page's faces, not the sidebar's groups folding, not compare. The
  user asked to leave those for later.
- **A scan's rebuild never glides**, nor a thumbnail finishing, a look-alike or face pass, a
  change made from elsewhere, or the year strip coming or going with one of those. During a
  first import the grid is rebuilt every few hundred milliseconds, and the user does not
  want a grid that is always moving.
- **Other results never glide**: another view, another search, another sort or grouping.
  Those are a different set of photos, not the same ones moved, and the selection is
  cleared for them already (`Shown::other_results`).
- **Scrolling never glides** - the wheel, the scrollbar, the keys, the year strip, a folder
  jumped to, the viewer handing a photo back. Those stay exactly as they are.

## Anti-goals

- **A still window must stay still.** Every animation asks for frames only while it runs
  and asks for none once it has ended. The tests that hold "a still window draws nothing"
  (the probe's loop, `frames_in`) must keep passing with animations on.
- **Nothing lags the user's hand.** Between column changes a splitter drag stays exactly
  under the pointer and the tiles widen pixel for pixel, as now; only a *row break* starts a
  glide. A glide in progress follows a target that keeps moving (the next column change,
  the drag going on) from wherever the photo is drawn at that moment, and never jumps.
- **No animation runs at a different speed on a 120 Hz screen**: progress is time, never a
  count of frames (Styling, in CLAUDE.md).
- **What is pressed is what the rules already say.** A glide is drawing only. Hit tests,
  the selection, the band, the pin and the menus go by the photos' *new* places from the
  first frame of a glide; in 200 ms the difference is not worth a second geometry.

## Details decided

- **Timing.** About 200 ms, easing out (fast at first, settling into place). The same curve
  for all four. A toggle pressed again mid-slide turns round from where the slide stands.
- **The splitter** comes and goes with the sidebar, at the sidebar's moving edge.
- **The sidebar's rows keep their full width while it slides** and are uncovered by it (cut
  off at the moving edge), so names do not squash and re-wrap. While it slides *closed* it
  is drawn but takes no press and no keyboard - the toggle already stores a name half typed
  in it at the press (`app.rs`'s `ToggleSidebar`), and its rules for a sidebar that is not
  drawn (`SidebarView::not_drawn`) apply from that press. While it slides open it is the
  sidebar, at the place it is drawn.
- **The stored state flips at the press.** `layout.sidebar_hidden` and `open.info` change
  when the user asks, as now, and are what is stored and what every rule reads; the slide
  is how far the *drawn* width has got towards them.
- **The place in the grid is kept by the pin** through every frame of a slide, as it is
  kept through a splitter drag now (`lay_out`, `pin_at`, `pin_top`): the photo at the top
  stays at the top.
- **Folder and period headers glide with the photos**, by the same rule as a tile (a header
  is identified by its section). The pinned header stays pinned.
- **The screen moves as one, across its edges** (the user's choice). A photo on screen
  before a change whose new place is below the bottom edge (or above the top) glides out
  of view, and a photo whose new place is on screen but which was off it glides in from
  where it stood in the old rows - the sidebar sliding closed pulls photos up from below
  the fold, the sidebar sliding open pushes the ends of the rows down past it. No photo
  blinks out or in at a column change. This means that while a glide runs the grid draws,
  and asks thumbnails for, the photos of both the old and the new rows that are in view at
  either end of their glide, not only the rows in view now; the photos are identified by
  id, and a photo's place in the old rows is known for every photo, not only those drawn
  (the old rows are kept for the glide's length). A photo that left the grid by the
  user's write and was on screen fades out where it stood over the same time.
- **Scrolling during a glide** moves the gliding photos with the scroll, without lag: the
  glide is relative to the grid's position, not to the window.
- **The selection's ring, the lead and the marks** are drawn on the tile, so they glide
  with it.
- **The info panel** is the only thing that slides in the viewer; it is opened and closed
  by its button and `I` alone. Every other way it closes - the crop tool opening, the
  viewer closing or opening, a step that leaves it open - stays instant as now. While it
  slides the photo is drawn from its levels (the Lanczos copy is for a photo fitted at the
  size it was made for, and a new copy is asked for once the size has stood for 200 ms, as
  for a window resize - which is right as it is). While it slides *open* it is the panel,
  pressed where it is drawn. While it slides *closed* it is drawn and takes no press and
  no keyboard, as the sidebar does (my call): its keyword field lets go at the press of
  the toggle, and a press on the part still drawn is nobody's - not the photo's, so it
  starts no pan and is no half of a double click. The photo's area is the drawn one in
  every frame.
- **Only the toggle slides the sidebar** (my call). "Rename in photon…" on a header shows
  a hidden sidebar at once, as now (`SidebarView::reveal`), so its name field takes the
  keyboard in the frame it opens.
- **The switch covers the grid's existing fade too** (my call): with animations off, a
  thumbnail that arrives late is drawn at once instead of fading in over 120 ms, so "off"
  means nothing in the grid moves. The slideshow's crossfade is the slideshow's own and is
  not covered.
- **Which grid is "the user's"**: the engine rebuilds and publishes *inside* the command,
  so the grid a write produced can be adopted before the write's answer is read, or after.
  The rule is therefore: a grid of the same results adopted while one of the writes in
  goal 3 is on the queue of writes, or whose answer arrived since the last grid was
  adopted, glides; any other glides not. A scan's grid that happens to land in that window
  glides too, which is acceptable; one outside it never does.
- **The year strip** changes the grid's width, and so its columns. It glides when it comes
  or goes with a grid that glides by the rule above, and not otherwise.
- **Tests.** The whole-application tests launch with animations off unless they are about
  animations, as they launch with `no_posters`: hundreds of them press rows straight after
  a toggle. The animations' own tests turn them on. `native-shot` draws with animations
  off, or after they have settled.

## Left to the builder

- Where the animation state lives and what it is called. There is a precedent: a state
  module for the arithmetic (easing, progress, interpolating a rectangle, the rule for
  which photos glide), egui-free and listed in `STATE_MODULES`, and the views drawing
  from it. `grid/motion.rs` is the scroll's motion, a different thing.
- How a glide's start is captured - from the old rows by item id, with a photo that is
  mid-glide starting again from where it is drawn - and how a mid-glide photo's start is
  re-taken when the target moves again.
- How the fading photo of goal 3 is drawn (its last texture and rectangle, kept for the
  fade).
- How `window_layout::areas` takes a sidebar that is part-way: a drawn width, or a
  fraction of the stored width.
- The exact easing curve and whether 200 ms becomes 180 or 250 once it is seen.
- The setting's key and wording ("Animations: move the sidebar, the info panel and the
  photos smoothly" or shorter).
- Whether egui's own `animate_value_with_time` / `animate_bool` helps; they key by id and
  ask for repaints themselves, which must still satisfy the "still window" anti-goal.

## What only a person can judge

Whether it feels right: the curve, the duration, how a glide across a row break looks
while dragging the splitter fast. That goes on `docs/smoke-checklist.md`. Each rule above
that can be held by a test (frames asked only while running, progress by time, the
pin kept across a slide, hit tests at the new place, which rebuilds glide, the switch) has
one, demonstrated to fail with its change reverted.
