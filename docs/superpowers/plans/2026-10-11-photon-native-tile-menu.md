# The native tile menu, with star, hide and albums: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the third of sub-project 3's six pull requests, and the first in which the
native window writes: a right-click on a photo opens the menu of what can be done with
the selection; a star, a hide, an album and a removal are written; `.` and `H` do the
first two from the keyboard.

**Architecture:** `menu.rs` is what a menu is - its items, the step of a key through
them, where it goes on screen - and `menu_view.rs` draws one for the whole window.
`tile_menu.rs` says which items the tile's menu has, from what is selected and where the
user is. The application holds the open menu, makes each pick a write on the queue of
writes, and says what came of it in a toast. The grid is told nothing of a write: its
rebuild arrives like any other and `Selection::follow` does the rest.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"The writes", "The menu", "The tile's menu", and the lines of "The keys" about `.` and `H`.

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the first pull request's plan, and three of this one's own:

- **The only write outside `library.db` is `commands::set_stars`**, the engine's, which
  sets or clears `star=` lines through `picasa::set_star`. Nothing here opens a file.
- **Every id acted on is read from the selection at the pick**, in the grid's order.
- **A test writes only into a library it made**, in a temporary directory, with real
  photo files where a star is written: the INI is read back from the folder.

## Not ported, and why

- **`setHidden`'s landing** (`landingOf`, `landingIn`, `findLanding`, `lastPhoto`,
  `runs`) and its cases in `library.test.ts` from "hiding the selection moves it" to "a
  hide that fails keeps the selection": `follow` lands the lead between the old index and
  the new, whatever took the photos out. The cases that are rules and not races are
  tested through the application: the next photo is selected after a hide, the one
  before when none follows, `H` again goes on, a click made while the write is out is
  kept, a refused hide keeps the selection.
- **The copies line** ("Show N duplicates"): sub-project 6.
- **The menu handing the focus back** to what had it, and re-reading `at` so that a
  second right-click moves the focus too: the menu has the keys because it is open, by
  the application's word (`GridData::keys`, and the same word to the sidebar and the
  search box), and they are the grid's again when it closes.

## Rulings made in the plan

- **The star key goes by one photo**: the lead when it is selected, else the selected
  photo that comes first in the grid (the spec's rule; the Svelte one takes the first on
  screen, for want of the rest).
- **Items that wait for a later pull request or sub-project are in their place and take
  no press**: Reveal, Open in default app, Copy photo (7), Compare (4), the two keyword
  items (the fifth pull request), Add to a person (4), Export (the sixth).
- **A menu is closed by a grid of other results**, and by nothing else that arrives: a
  scan's grid leaves it open, and what an item acts on is read at the pick.
- **`H` and `.` are the grid's keys through the gate the first review made**
  (`GridView::elsewhere`, `GridData::keys`), and not while a band is held.

## Review focus

1. **A write on the wrong photos.** The ids at the pick, not at the right-click; a
   right-click outside the selection selects first; a selection that changed under an
   open menu (a scan); `H` held down while its write is out.
2. **A key that reaches the grid from under a menu, or the menu from under a dialog**:
   `h`, `.`, the arrows, Escape, Ctrl+A with the menu open.
3. **The menu off the window**: near each edge, taller than the window, the window
   resized under it, a second right-click while it is open.
4. **What a write's answer is paired with**: a toast for fewer stars than asked; the
   lists read again after an album changes; an answer that arrives after a view switch.
5. **A still window** with a menu open; and no frame asked for while a write is out
   but the one its answer asks for.

## Task 1: `menu.rs`

```rust
pub enum Look { Plain, Sub, Danger }
pub enum Item<A> {
    Pick { label: String, hint: Option<&'static str>, works: bool, look: Look, does: A },
    Separator,
    Heading(&'static str),
    Note(&'static str),
}
pub enum MenuKey { Down, Up, Home, End }
/// `menu-keys.ts`'s `menuStep`, over the items that work.
pub fn menu_step(key: MenuKey, current: Option<usize>, count: usize) -> Option<usize>;
/// `menu-place.ts`'s `placeMenu`.
pub fn place_menu(at: (f32, f32), size: (f32, f32), window: (f32, f32), margin: f32) -> (f32, f32);
pub const MENU_MARGIN: f32 = 4.0;
```

- [ ] Tests first: `menu-keys.test.ts` and `menu-place.test.ts` whole.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 2: `tile_menu.rs`

```rust
pub enum TileAction { Reveal, OpenInApp, CopyPhoto, Star(bool), Compare, Hide(bool),
    AddKeyword, RemoveKeyword, AddToPerson, RemoveFromPerson(i64), RemoveFromAlbum(i64),
    Export, AddToAlbum(i64) }
pub struct Facts<'a> { pub count: usize, pub sole: Option<Sole>, pub decider_starred: Option<bool>,
    pub place: &'a Place, pub albums: &'a [AlbumSummary], pub people: &'a [Person], pub finds_faces: bool }
pub struct Sole { pub video: bool, pub starred: bool }
pub fn tile_menu(facts: &Facts<'_>) -> Vec<Item<TileAction>>;
pub fn works(action: &TileAction) -> bool;
/// "photo", or "12 photos".
pub fn subject(count: usize) -> String;
```

- [ ] Tests first: the items for one photo, for one video, for several part starred
  (both star items, the hint beside the one the key would do), in Hidden (Unhide), in an
  own album and in a Picasa album, in a named person's view and a Picasa contact's,
  with and without face finding, with no albums of one's own (the note); every string
  held to `Grid.svelte`'s; which items work.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 3: `menu_view.rs`, and the grid's right-click and keys

- The menu: an area over everything, placed by `place_menu` from its measured size,
  every frame; rows drawn by `text::paint_line`; the arrows, Home, End, Enter, Space,
  Escape; a press outside closes it at the press; a row under the pointer is the active
  one when the pointer moves.
- The grid: a secondary click on a photo selects it first when it is not selected, and
  answers `GridOutput::menu`; `.` and `H` answer `GridOutput::asked`.
- `Selection::deciding(index)`: the photo the star key goes by.

- [ ] Tests first, in whole frames: the menu near each corner; taller than the window;
  the keys step over items that do not work and wrap; a pick; Escape; a press outside;
  a letter typed in it reaches nothing behind it; a right-click on an unselected photo,
  on a selected one, on no photo, on the pinned header; `.` and `H` with nothing
  selected, with something else holding the keyboard, during a band.
- [ ] Implement, gate, commit.

## Task 4: the writes

`Write::{Stars, Hidden, AlbumAdd, AlbumRemove, PersonRemove}` on the queue of writes;
their answers; the toasts (`counted`, `removedMessage`), string for string.

- [ ] Tests first, through the application and the real engine, in a library of real
  files: a star set from the menu and from the key is in the folder's `.picasa.ini` and
  on the tile, and taken off again; several photos part starred go the way of the
  deciding photo; a folder whose INI cannot be written costs a toast and not the other
  photos; a hide takes the photos out and selects the next, `H` again hides that one,
  the last photo hidden selects the one before; in Hidden `H` brings back; "Add to
  album" shows in the album's count, "Remove from" takes out and lands; a refused write
  is a toast and the selection stands; a view switched while a write is out.
- [ ] Implement, gate, commit.

## Task 5: pictures, the checklist, CLAUDE.md, the pull request

- [ ] Pictures: the menu for one photo and for several, light and dark.
- [ ] The smoke checklist (a star read back in Picasa's own file); CLAUDE.md; push; one
  fresh reviewer; one fix pass; merge.
