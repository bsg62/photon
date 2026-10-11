# The native selection and the keyboard lead: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the first of sub-project 3's six pull requests: photos in the native grid can be
selected - by a click, a Ctrl/Cmd+click, a Shift+click, the arrows, Home, End and the page
keys, plain and with Shift, and Ctrl/Cmd+A - and the selection stays on its photos when
the grid under it is rebuilt.

**Architecture:** `selection.rs` is a state module holding the selected ids, the lead and
the anchor, every call given the index on screen. `grid/layout.rs` gains what the Svelte
`layout.ts` and `nav.ts` have for it: which photo a point is on, where an arrow key and a
page key lead. `grid/view.rs` turns the mouse and the keys into calls on the selection,
draws the ring, and keeps the lead in sight. `app.rs` holds the selection, has it follow
each new grid (`follow`) or cleared when a step's grid arrives, and gives the status bar
its count.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"The selection", "What of the Svelte selection is not ported", "The mouse", "The keys",
"The status bar".

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the plans of sub-project 2: no egui type in a state module, each added to
`STATE_MODULES`; nothing reads or writes SQLite on the UI thread after `App::new`; every
new test shown to fail with its rule broken, by an exact replacement, and a probe that
passes is a finding; the Rust gate at every commit; the application never launched;
whole-application tests wait for a state and never for a length of time.

This pull request writes nothing to the library.

## Not ported, and why

The spec's table, for this pull request: `fetchIds` and its chunks, `extendCall`,
`reaching` and `keyboardLead`, `picks`, `adoptLead`, `isSelectedTile`'s fallback,
`selectItem`, `anchorWrites`. Each is a consequence of the webview paging through the
grid, which the native window holds whole. Their tests in `library.test.ts` (the groups
"where the keyboard goes on from", "a lead put on a page that has not loaded", "a range
reaching a page that has not loaded", "a range still on its way", and the cases about
chunks, a newer index and a superseded call) have no successor: the race each is about
cannot happen in one frame of one thread. The hide's landing, and its many cases, are the
third pull request's, where the first write is; `follow`'s landing is built and tested
here, on pairs of indexes, since a scan can take the lead's photo away today.

## Rulings made in the plan

- **The selection is cleared when a grid of other results is put on screen**
  (`Shown::other_results`), not at every step: a step that changes nothing shown - the
  grouping under a sort that ignores it - leaves the photos and their order as they
  were, and the selection with them. The spec's "a view, a search, a sort" is this.
- **The Escape that leaves the search box is the box's**, taken out of the frame's
  input: the grid is drawn after the box in the same frame and would clear the selection
  with it. (In the Svelte UI that key never reaches the grid; only its repeat does, which
  the grid ignores, as it does here.) Enter is left as it is until the sub-project that
  gives Enter a meaning in the grid.
- **An arrow with nothing selected starts from the first photo**, as `nav.ts`'s `move`
  does: ArrowRight then selects the second photo. Ported as it is.
- The view is given the selection to change, as it is given its scroll position; nothing
  here needs the application to decide. What will (a menu, a write) comes back in
  `GridOutput` from the third pull request on.

## Review focus

1. **An offset used across two indexes.** The lead and the anchor are an offset and an
   id; everything that reads the offset after a new grid must have gone through `follow`.
2. **A key answered twice in a frame, or by the wrong thing**: Escape in the search box,
   an arrow in an open dropdown, Home with the caret in the box, Ctrl+A in the box.
3. **A click that is not on a photo**: the gap between tiles, the room beside a short
   last row, a header, the pinned header lying over a row, the scrollbar, the year strip.
4. **The lead out of sight**: a key moves the lead and the grid must follow, under the
   pinned header's height; a grid that arrives must not move a user who was elsewhere.
5. **A still window**: a selection draws nothing more by itself.

## Task 1: `grid/layout.rs` - the photo under a point, and where the keys lead

```rust
pub enum NavKey { Left, Right, Up, Down, Home, End }
/// The photo at (`x`, `y`) in the layout's coordinates, `x` from the photos' left edge.
pub fn tile_at(rows: &[Row], x: f64, y: f64, tile: f64) -> Option<usize>;
/// `nav.ts`'s `move`: `None` is nothing selected, which starts from the first photo.
pub fn move_from(offset: Option<usize>, key: NavKey, sections: &[Section], columns: usize) -> usize;
/// `layout.ts`'s `pageMove`.
pub fn page_move(rows: &[Row], offset: usize, down: bool, viewport: f64) -> usize;
```

- [ ] Tests first: `nav.test.ts`'s "move" and "move from no selection" whole;
  `layout.test.ts`'s "a page key in the grid" whole; for `tile_at`: a point on each tile
  of a row, in the gap between two, in the gap under a row, beside a short last row, on a
  header, above the first row and below the last, and at each tile's own edges.
- [ ] Implement, gate, commit.

## Task 2: `selection.rs`

```rust
pub struct At { pub offset: usize, pub id: i64 }
/// What following a new grid came to, when the lead's photo had left: where it was in
/// the old grid, and where the lead is in the new one.
pub struct Landed { pub left: i64, pub at: usize }
#[derive(Default)] pub struct Selection { /* ids, lead, anchor */ }
impl Selection {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn is_selected(&self, id: i64) -> bool;
    pub fn ids(&self, index: &GridIndex) -> Vec<i64>;       // in grid order
    pub fn lead(&self) -> Option<At>;
    pub fn anchor(&self) -> Option<At>;
    pub fn select(&mut self, index: &GridIndex, offset: usize);
    pub fn toggle(&mut self, index: &GridIndex, offset: usize);
    pub fn extend(&mut self, index: &GridIndex, offset: usize);
    pub fn select_all(&mut self, index: &GridIndex, library: bool);
    pub fn clear(&mut self);
    pub fn follow(&mut self, old: &GridIndex, new: &GridIndex) -> Option<Landed>;
}
```

- [ ] Tests first, each a rule of the spec:
  - a plain selection replaces the whole set, and lead and anchor go to it;
  - Ctrl+click toggles a photo in and out, moving the lead and the anchor each time; the
    last photo taken out leaves no lead;
  - Shift+click selects from the anchor, replaces what was selected, extends backwards
    too, and twice re-ranges from the same anchor; with no anchor it ranges from the
    lead, with neither from the first photo; an offset past the end is held to the end;
  - select all: the lead's folder in the library, not the library; the lead's month under
    a date grouping; the lead photo's folder under a flat sort, wherever its photos are;
    the first folder with nothing selected; the whole view outside the library; the lead
    stays and the anchor is the start of what was selected; nothing in an empty grid;
  - `follow`: the selection stays on its photos when a photo arrives above them, lead and
    anchor carried by id, and the anchor of a range stays the range's start; a selected
    photo that left is no longer selected; the lead's photo leaving lands on the next
    that stays, on the one before when none follows, and nowhere in a grid left empty;
    the landing is selected alone only when nothing else is left selected; a lead that
    was not selected stays unselected where it lands; an anchor whose photo left is no
    anchor; the neighbour leaving with the lead's photo (Duplicates) is passed over.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 3: the grid's mouse, keys and ring

`GridData` gains `library: bool` (All photos is shown: what Ctrl/Cmd+A is asked of).
`GridView::show` is given `&mut Selection`. `tile::paint` is told whether its photo is
selected. `GridView::rect_of(offset)` answers where a photo was last drawn, for a test of
the whole application to press it.

- [ ] Tests first, in whole frames (`view.rs`'s fixture):
  - a click selects the photo under it, and its tile alone is ringed; Ctrl+click adds and
    takes out; Shift+click ranges; Shift wins over Ctrl;
  - a click in the gap, on a header and on the pinned header selects nothing and clears
    nothing; a press on the scrollbar or the strip selects nothing;
  - each arrow, Home and End move the lead and select it; with Shift they extend; the
    grid follows the lead into sight, below the pinned header, and does not move when the
    lead is in sight already;
  - Page Down and Page Up move the lead a screenful when there is one, and scroll as
    before when there is none;
  - Ctrl/Cmd+A selects the section; Escape clears, and its repeat does not;
  - none of the keys is the grid's while a text field has the keyboard;
  - nothing in an empty grid.
  The fixture's tests of keys that scroll are rewritten for what the keys now do.
- [ ] The search bar takes its Escape out of the frame; a test in whole frames of the
  shell: Escape in the box leaves the selection, the next clears it.
- [ ] Implement, gate, commit.

## Task 4: the application, pictures, the checklist, CLAUDE.md, the pull request

- `App` holds the `Selection`; `adopt` clears it for other results and otherwise has it
  `follow` the old index into the new, and brings a landing into sight (`Align::Nearest`)
  when the photo that left was on screen in the last frame.
- `ShellData::selected`; the status bar says "*N* selected · " before the count when more
  than one photo is selected, the number in the text's full colour.
- `App::selected()`, `App::lead()`, `App::tile_rect(offset)` for tests.

- [ ] Tests first, through the application and the real engine: a click on a tile
  selects it and a Shift+click two more, and the status bar says "3 selected"; a selected
  photo hidden through the engine from the test's own thread (`commands::set_items_hidden`
  on `app.engine()`, a write this window did not make) leaves the others selected and the
  count right, and hidden while it was the lead and alone, the next photo is selected in
  its place; a switch to Starred clears the selection; Escape in the search box leaves
  it; the arrows move the lead and the grid follows; a window with photos selected draws
  nothing.
- [ ] Pictures (`native-shot`): a selection with its count, light and dark.
- [ ] The smoke checklist; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.

**Changed in the making.** A range with no anchor keeps its start as the anchor from then
on: the Svelte store leaves it without one, and the next Shift+click then ranges from the
lead the last one moved - the walking away the anchor exists to prevent. A key with Ctrl,
Cmd or Alt held is not the grid's, but for Ctrl/Cmd+A (the Svelte handler answers the
arrows whatever is held). The status bar's count is given to AccessKit, so that a test of
the whole application reads it. Eleven probes passed at first. Two were lines that did
nothing and went: `tile_at`'s check that a row is a row of photos (a header holds none,
so no column of it is one) and `extend`'s return in an empty grid. Nine were tests that
did not tell, one of them a rule with no test at all (Escape closing the search help
over a selection). Five more lines went before a probe was written for them, on the same
reasoning: clamps of an offset `follow` already keeps true, and a guard against an empty
grid before calls that do nothing in one.

**The review** (one fresh reader, 2026-10-11, its own worktree and proving tests) found
nothing critical and one thing important: **the grid answered keys that something else
had just answered.** Its only test for "the keys are mine" was "no text field has the
keyboard", which had cost a forty-point scroll while the keys scrolled and now moved or
cleared a selection. Five failing tests: the arrow that moved the splitter moved the
selection; a sideways arrow and Ctrl+A in an open list reached the grid behind it; the
Escape that took the keyboard off a control cleared the selection (the search box's bug,
on another widget); and the arrow that walked the focus on from a button moved the lead.
The grid now has the keys only while nothing else has the keyboard and had none when the
grid was last drawn - egui takes the keyboard away at Escape before anything is drawn -
and not under the search box's open help. The spec's sentence is changed to say so.

Also from it, and fixed: a click took the modifiers held at the end of its frame and not
at its release (a Shift let go of in the frame of the click made it a plain one); the
pinned header's `y` in the click's test did nothing (while it is pushed out, what shows
in those points is the next header's own row) and is gone; a comment on `select_all`
was wrong for one of its three cases; "where until this they scrolled" was not true of
the sideways arrows.

Recorded and left: the area that senses the click takes every press on the photos, so
egui no longer hands a press just beside the splitter or the scrollbar to them (the
splitter's reach is its own five points, as the Svelte one's - a change nobody chose,
now said and pinned by a test); a press held longer than 0.8 seconds selects nothing
(egui's limit, which is also a touch's long press; with the second pull request such a
press is a band over the one photo); Ctrl+A on a Mac is Cmd+A alone, where the Svelte
handler takes either; an arrow with nothing selected starts from the top of the library,
as in the Svelte grid, where until now the up and down arrows scrolled.

The reviewer measured `follow` over 300,000 selected photos: 1.9 ms in a release build
(24 ms in a debug one), so the spec's fifth risk needs no bound.
