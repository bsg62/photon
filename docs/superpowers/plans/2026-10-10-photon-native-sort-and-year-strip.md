# Native sort, grouping, size and the year strip: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the fourth of sub-project 2's five pull requests: the three controls at the right
of the top bar - sort with its reverse toggle, grouping, tile size - and the year strip
beside the grid.

**Architecture:** `select.rs` is the Svelte `createSelect` (the select-only combobox: open,
active, the keys, type-ahead, disabled) on a clock that is a number, and `select_view.rs`
draws it: the closed control and its list in an area of its own. A change of sort or
grouping is a `Step::Sort` on the views' queue, built by replacing one field of the sort
the user is going to. The tile size is a setting, written off the UI thread, and the grid
keeps its place across it as it does across a resize. `timeline.rs` is `timeline.ts`; the
strip is drawn by the grid's view, which asks `shows_timeline` of the grid as it would be
beside the strip.

**Spec:** `docs/superpowers/specs/2026-10-10-photon-native-sidebar-views-search-design.md`,
"Sort, grouping, size" and "The year strip".

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the earlier plans of this sub-project: no egui type in a state module, each added to
`STATE_MODULES`; nothing reads or writes SQLite on the UI thread after `App::new`; every
new test shown to fail with its rule broken; the Rust gate at every commit; every string
the Svelte UI's; the application never launched; the engine's view and sort are moved
through the queue alone.

## Left out

- The keys of the controls beyond the select's own: there are none.
- The year strip under the keyboard: it is not in the tab order in the Svelte UI either;
  the sidebar's years are the keyboard's way to a year.

## Review focus

1. **A sort lost behind a view click, or a view click lost behind a sort.** Both are steps
   on one queue; a typed search may take the place of a waiting search and of nothing
   else.
2. **The grouping dropped by a change of sort.** A control changes the sort it read by
   replacing one field; built from two fields, the Svelte control once dropped the third.
   And a second change made before the first has landed builds on the first.
3. **The strip coming and going on every frame.** It narrows the tiles, which shortens the
   grid, which may then fit: whether it is shown is asked of the grid as it would be
   beside it.
4. **A place lost across a change of size or of strip.** The grid is on the photo it was
   on, as across a resize.
5. **The list of a select left open** over a view switch, a press elsewhere, Escape; and
   its keys reaching the grid while it is open.

## Task 1: `select.rs`

```rust
pub const TYPEAHEAD_RESET_MS: f64 = 500.0;
pub enum SelectKey { ArrowDown, ArrowUp { alt: bool }, Home, End, PageDown, PageUp, Escape, Enter, Tab, Char(char) }
pub struct Select { /* open, active, typed, typed_at */ }
pub struct Options<'a> { pub labels: &'a [&'a str], pub selected: Option<usize>, pub disabled: bool }
impl Select {
    pub fn open(&self) -> bool; pub fn active(&self) -> usize;
    pub fn show(&mut self, options: &Options<'_>); pub fn close(&mut self);
    pub fn toggle(&mut self, options: &Options<'_>);
    pub fn hover(&mut self, index: usize, options: &Options<'_>);
    /// Answers whether the key was used, and the option chosen, if one other than the
    /// one held was.
    pub fn key(&mut self, key: SelectKey, now_ms: f64, options: &Options<'_>) -> (bool, Option<usize>);
    pub fn commit(&mut self, index: usize, options: &Options<'_>) -> Option<usize>;
}
```

- [ ] Tests first: `select.svelte.test.ts`'s cases, on a numeric clock.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 2: `timeline.rs` and `shows_timeline`

```rust
// timeline.rs
pub const TIMELINE_WIDTH: f64 = 44.0;
pub struct YearMark { pub year: i16, pub top: f64 }
pub fn year_marks(sections: &[Section], rows: &[Row], zone: &TimeZone) -> Vec<YearMark>;
pub fn year_at(marks: &[YearMark], y: f64) -> Option<i16>;
pub fn strip_y(top: f64, total: f64, strip: f64) -> f64;
pub fn labelled_marks(marks: &[YearMark], total: f64, strip: f64, min_gap: f64) -> Vec<YearMark>;
pub fn scroll_top_for(y: f64, strip: f64, total: f64, viewport: f64) -> f64;
// grid/layout.rs
pub fn layout_height(sections: &[Section], columns: usize, tile: f64) -> f64;
pub fn shows_timeline(sections: &[Section], years: usize, outer: f64, gutter: f64, viewport: f64, nominal: f64) -> bool;
```

- [ ] Tests first: `timeline.test.ts` whole, and `layout.test.ts`'s `showsTimeline` and
  `layoutHeight` cases.
- [ ] Implement, gate, commit.

## Task 3: `nav.rs` - a step that changes the sort

```rust
pub enum Step { /* ... */ Sort(Sort) }
impl Nav { pub fn sort_target(&self) -> Sort; }   // the sort the last step asked leads to
```

`leads_to` is the place the user is going to already (a sort does not move the view), so
`wants` compares the sort as well. A sort is not a search: it gives way to nothing.

- [ ] Tests first: a sort asked is the sort the controls show before it lands; a second
  change builds on the first; a sort that is the one in force asks for nothing; a view
  asked behind a sort keeps the sort; a refused sort leaves the controls on the sort in
  force.
- [ ] Implement; the worker calls `commands::set_sort`. Gate, commit.

## Task 4: the views - the select, the controls, the strip

- `select_view.rs`: the closed control (its value, a chevron), the list under it in an area
  of its own, the keys while it has the focus, a press outside closing it.
- `shell.rs`: sort, reverse, grouping (dimmed, with its reason, under a sort that is not by
  date) and size at the right of the top bar, left of the gear.
- `grid/view.rs`: the strip at the grid's right, left of its scrollbar: years, the line
  where the grid is, the year under the pointer, a press or a drag moving the grid.

- [ ] Tests first, in whole frames: for the select, each key and the pointer; for the
  controls, what each answers; for the strip, when it is shown and that it does not
  flicker in the band of heights where it would, where a year is drawn, what a press and
  a drag do, and that the grid keeps its photo when the strip comes or goes.
- [ ] Implement, gate, commit.

## Task 5: the application, pictures, the checklist, CLAUDE.md, the pull request

- The size is read in `App::new` already; a press writes it through the queue of writes.
- [ ] Tests first, through the application: a sort changed is shown from its top and the
  sidebar's folders follow it; the grouping is dimmed under Name and kept; a change made
  before the last has landed builds on it; the size is the one chosen at the next launch;
  a still window draws nothing with a list closed.
- [ ] Pictures: by month with the strip; the sort's list open. The checklist. CLAUDE.md.
- [ ] One fresh reviewer; one fix pass; merge.

**Changed in the making.** The strip stands right of the scrollbar, as in the Svelte
grid; this plan's "left of its scrollbar" was written without looking. The sidebar's
folders follow the sort of the grid on screen, not the one being gone to. A dimmed
control is no stop of the Tab key. Tab in an open list closed it without choosing, which
a probe's planning found and no test had asked.

**The review** (one fresh reader, 2026-10-10) found nothing critical and three things a
user would have met. All photos clicked behind a sort still on its way asked the sort on
screen whether its remembered folder was a place, and opened by name at a photo of that
folder. A sort the engine made and could not store was taken as not made, and the photos
were in one order while the controls said the other. And a sort chosen with the mouse
left the keyboard in the control, so End, scrolled away from, opened the list and Enter
sorted by size. All three are fixed, with: a letter typed into a list taken out of the
frame's input like an arrow; the sideways arrows claimed while a list is open; a row lit
by the pointer's step, not its smoothed speed. Its test with a folder of a newer year
showed the pin across a new index carries an offset and not a photo, which the comment,
CLAUDE.md and the checklist now say instead of claiming more. Left for later: a focus
mark on the toggle and the size segments, `/` and Alt with a letter in an open list, the
strip's hold when the pointer leaves the window, `shows_timeline` summed every frame.
