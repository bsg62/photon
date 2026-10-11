# The native rubber band: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the second of sub-project 3's six pull requests: a press on the photos that
becomes a drag draws a rectangle, what it touches is selected, and held near the top or
bottom edge the grid scrolls under it.

**Architecture:** `grid/layout.rs` gains the band's hit test and the edge's speed, both
`layout.ts`'s with their cases. `selection.rs` gains the band's four calls - begin, to,
end, cancel - over two sets it holds while a band is drawn. `grid/view.rs` holds the
band itself, a rectangle in the layout's coordinates, turns a drag on the photos into
those calls, scrolls, draws it, and tears it down by one path whatever ends it.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"The rubber band", and the band's lines of "What of the Svelte selection is not ported".

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the first pull request's plan. This one writes nothing to the library either.

## Not ported, and why

- **The band answered twice** (`bandTo` from the loaded pages, `endBand` from the
  backend), and with it `bandUnresolved`, `bandsResolving` and every test of a count
  kept right while ids were on their way: a band's photos have ids as it is drawn, so
  the frame's answer is the answer.
- **A band abandoned because the grid was rebuilt under it.** The rectangle is in the
  layout's coordinates and its photos are read from the index on screen in every frame:
  after a scan's grid it covers what it covers there, and that is what is ringed.
  `follow` takes photos that left out of the band's two sets as out of the selection.
- **The four-pixel threshold**, pointer capture, the swallowed click after a release,
  the second finger: egui decides what is a drag (six points from the press, or held
  past its click limit), a drag is the widget's until the button comes up wherever the
  pointer goes, and a press that became a drag is no click.

## Rulings made in the plan

- **A press held still past egui's click limit is a band over the one point**, which
  selects the photo under it as the click would have, or with Ctrl, Cmd or Shift adds it;
  on no photo it clears, as any band over nothing does.
- **A band begun with Shift adds**, as Ctrl's does - the Svelte grid's rule - though a
  Shift+click ranges.
- **The step's grid that arrives under a band drops the band** and puts nothing back:
  `Selection::clear` forgets the band's sets, and the view forgets its rectangle
  (`GridView::drop_band`, called where the selection is cleared).

## Review focus

1. **A band that is never torn down**: the button let go of outside the window, the
   window left, Escape then the button still down and moving, a step's grid arriving,
   the pointer gone. After every one of them a new press must start a new band, and
   nothing may scroll.
2. **Time**: the scroll is speed times the frame's own duration, capped; no frame is
   asked for unless the pointer rests in a margin; a band held still elsewhere draws
   nothing.
3. **Coordinates**: the origin is in the layout, the pointer in the window; the far
   corner is worked out again in every frame that scrolls; the wheel turned during a
   drag; the window resized during a drag (the rows move under the origin).
4. **Who else sees the drag**: the scrollbar, the strip, the pinned header, the
   splitter beside the photos; a drag begun on them is theirs, and one begun on the
   photos is not theirs when it crosses them.
5. **The keys under a band**: nothing but Escape, by the gate the first pull request's
   review made.

## Task 1: `grid/layout.rs` - what a rectangle touches, and how fast an edge scrolls

```rust
pub struct Span { pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64 } // any corner order
/// The grid offsets the rectangle touches, as inclusive ranges, rows that join up merged.
pub fn items_in_rect(rows: &[Row], rect: Span, tile: f64) -> Vec<(usize, usize)>;
/// Points a second: negative up, positive down, none well inside.
pub fn edge_scroll_speed(y: f64, top: f64, bottom: f64, margin: f64, max: f64) -> f64;
```

- [ ] Tests first: `layout.test.ts`'s "itemsInRect" whole, at the three sizes, with its
  deep-grid block; "edgeScrollSpeed" whole. `tile_at` and `items_in_rect` agree on every
  point of a grid (a rectangle of no size is the photo `tile_at` names, or none).
- [ ] Implement, gate, commit.

## Task 2: `selection.rs` - the band's four calls

```rust
impl Selection {
    pub fn band_begin(&mut self, adding: bool);
    pub fn band_to(&mut self, index: &GridIndex, ranges: &[(usize, usize)]);
    pub fn band_end(&mut self, index: &GridIndex, ranges: &[(usize, usize)]);
    pub fn band_cancel(&mut self);
    pub fn banding(&self) -> bool;
}
```

- [ ] Tests first: a band replaces the selection; an adding band adds to what it began
  with and shrinks back again; the lead does not move while it is drawn; on release the
  lead and the anchor are on the band's first photo; a band over nothing clears, and an
  adding one leaves what it began with, lead and all; cancel puts back what it began
  from and leaves the lead alone; `follow` under a band takes a photo that left out of
  both sets, so neither the next frame nor a cancel brings it back; `clear` under a
  band forgets it; a frame, a release and a cancel with no band begun do nothing.
- [ ] Implement, gate, commit.

## Task 3: the view

- The photos' area senses a drag as well as a click. A drag by the primary button that
  starts on the photos - not on the pinned header - begins a band at the press's origin,
  in the layout's coordinates.
- Every frame of it: the far corner from the pointer's place in the window and the
  grid's position; `band_to`; the rectangle drawn, cut at the grid's edges.
- In a margin (`BAND_EDGE` 48, `BAND_SCROLL_MAX` 1400, `BAND_FRAME_MAX_MS` 50) the grid
  scrolls by the speed times the time since the last frame, the part of a point it
  could not move carried over, and the next frame is asked for.
- Release ends it; Escape cancels it and the rest of that press is nobody's; the button
  no longer down without a release tears it down as a cancel.
- `GridOutput::banding`, for the application: nothing opens over a band.
- The keys: while a band is held, Escape alone.

- [ ] Tests first, in whole frames: a drag across two rows rings what it crosses and
  the release leaves it selected with the lead on its first photo; dragged back, photos
  come off; with Ctrl it adds; over the gutter alone it clears; Escape puts the
  selection back and the same press moving on selects nothing; a press on the pinned
  header, the scrollbar and the strip starts none; a click is still a click; held in
  the bottom margin the grid scrolls at the edge's speed whatever the frame rate (two
  runs, sixty and a hundred and twenty frames a second, the same distance in the same
  time) and the band grows; a frame of half a second moves no further than fifty
  milliseconds' worth; held in the middle no frame is asked for; the wheel turned
  during a drag keeps the origin on its photos; an arrow, Ctrl+A and Page Down during
  a drag do nothing; the button let go of where the window does not hear it leaves no
  band and no scroll, and the next press starts one.
- [ ] Implement, gate, commit.

## Task 4: the application, pictures, the checklist, CLAUDE.md, the pull request

- A grid of other results drops the band with the selection.

- [ ] Tests first, through the application: a band dragged over photos selects them and
  the status bar counts them; a switch of view under a held band leaves nothing
  selected and no band; a window with a band held still draws nothing.
- [ ] Pictures: a band half drawn, light and dark.
- [ ] The smoke checklist; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.

**Changed in the making.** The spec's "the pointer lost to the window" is no ending egui
has: a pointer that leaves the window stays a drag, and egui ends one only at the button
coming up and at Escape. What is left to guard against is the button let go of over
another window and never heard of, and the band is abandoned when the window loses the
focus. The part of a point the Svelte loop carries over from frame to frame is not
ported: the native grid's position is not a whole number of points. No `drop_band` is
called by the application: the view finds the selection's band gone - cleared with the
selection - and drops its rectangle. Four probes passed at first, each a test that did
not tell: a band given by its other corners, one wholly left of the photos, and the
frame's lag between the grid's scroll and what the band covers.
