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

## Changed in the making

- **The menu is asked for where the other button goes down, from the button's own
  event** (`GridView::take_pointer`, `Taken::menu`), not at egui's click of it as Task 3
  says. The rubber band's review, which landed while this was being built, is why: egui's
  click of either button gives up after 0.8 seconds, and "clicked" and "dragged" say less
  than they seem to. At the press is also where GTK's and macOS's own menus open (a
  Windows one opens at the release). Not while the first button is down, so a press on
  its way to a click or a band is not interrupted; a test this plan's first draft had -
  the other button ending a band and opening the menu of what it covered - described
  egui's old behaviour and went with it.
- **An open menu takes every key press, chords included**, where `menu-keys.ts`'s
  `menuOwnsKey` lets a chord and a function key through to the window. With the menu
  drawn first in the frame, Ctrl+F put the caret in the search box behind it, where
  nothing typed arrives, and Ctrl+A selected a folder the menu was not about. Nothing
  native answers a function key yet; one that must work whatever is open is read before
  the menu is shown.
- **Tab cannot be taken out of the frame**: egui decides its walk of the keyboard from
  the raw input before anything is drawn. The menu calls the walk off
  (`Memory::move_focus(FocusDirection::None)`), and the application does the same under
  a held band, where the second pull request had missed it: a Tab during a drag left the
  keyboard on the sidebar's toggle, and the grid's keys dead after the band.
- **A menu closes when the photo it was opened on leaves the grid** (`follow` lands the
  lead). The ruling above said "a scan's grid leaves it open, and what an item acts on
  is read at the pick" - which, for a menu of one photo, is the neighbour the selection
  landed on: a photo nobody asked about. A grid that loses another photo, or one of
  several selected, still leaves it open.
- **`Facts` has one deciding photo**, not a sole photo and a deciding photo's star
  beside it: the two were the same photo whenever there was one, and a probe showed the
  application could fill either wrongly unseen.

- **The star's key is the key that types a period**, with Shift where a keyboard needs
  it (a French one) and not where Shift with the period's own key types `>`: egui calls
  both `Key::Period`, and the event's `physical_key` tells them apart. The first version
  refused Shift outright.

**Probes**: 109 rules broken in turn, each with the test that was to fail. Six passed at
first. Two were lines that did nothing and went: a frame asked for when a menu opens
(egui asks for one after any frame with input in it, and a menu is opened by a button),
and a read of the lists after photos are taken off a person (a write of the people is
announced as a change of the data, which reads them). Two were the doubled fact above.
Two were tests that could not see their rule and were sharpened: the keyboard taken from
a text field at the press (egui does it itself at a *click* elsewhere, so the test now
holds the button and lets go of it after moving), and "these photos" for two. Also
removed on the way, each covered by another rule: a guard against a pick with nothing
selected, and closing the menu in `adopt` for other results (the frame's own "nothing is
selected" does both).

**Not done here**: "a view switched while a write is out" is tested for a hide only; two
presses of `.` before the first one's grid has arrived both read the photo as it was;
a long touch opens no menu (left with the question of whether a touch drag is a band).

## The review

One fresh reader (2026-10-11), its own worktree and proving tests. Nothing critical:
every id written comes from the selection on the grid on screen, and no path writes a
file but `commands::set_stars`. Two things important, both on what a press or a key was
*of*:

- **A press where the menu had opened was the menu's and the photo's under it.** egui
  answers "does this widget contain the pointer" for one layer, and lets the layer below
  go on containing it all along the inside of an area's edge. A menu opens with its
  corner at the pointer, which rests there: a click in place left the menu open and
  collapsed the selection under it, and a drag from there drew a band under the menu -
  the very case the menu's "closes at the press" was written against. The grid now asks
  which layer egui finds on top at the press, the menu asks the same for "outside", and
  the menu's area is a point larger all round than the menu, since egui puts an area on
  whole points and the two would otherwise disagree by where on a point the pointer was.
  It was the rubber band's code, armed by this pull request, and held for the search
  help, the dropdown's list and the toasts too.
- **A key that writes acted on what was selected at the end of the frame.** `H` and
  then an arrow in one frame hid the photo the arrow went to. The ask now carries the
  selection it was pressed on, and in a frame with a mouse button in it no such key is
  answered: a click is taken before the frame's keys, whichever came first.

Fixed from the minor ones: the keys of a menu stayed on an item's *number* when the
items became others under them (a grid takes one of two photos, two star items become
one, and Enter picked an album); a menu was not held to the window's width; nothing
held "the view on screen, not the one being gone to" for `H` and for the menu's items
(the test is the reviewer's, with the library's write lock for a seam); a cut, a copy
and a paste are no keys to egui and passed an open menu; Control with a click on a Mac
opened no menu; **writes still waiting were dropped when the window closed**, which this
pull request made matter (`Queue::finish`, ten seconds at most); and four statements
that were wrong - what `albums_changed` announces, "every key press", the spec's two
sentences on chords, and what `follow`'s landing says of the menu's photo. The wheel
over a menu took the scroll out of the frame for nothing under it to read: nothing
could, and the line went.

Rulings in it, unseen by the user: a click exactly where a menu opened is the menu's
and does nothing; a key that writes pressed in the same frame as a click is dropped,
not guessed at; a closing window waits ten seconds for its writes and then closes.

Recorded and left:

- A toast that arrives while a menu is open is drawn over it where they meet (all four
  overlays are in egui's foreground order, the newest on top).
- A menu taller than the window shows no sign that it scrolls.
- `H` or `.` in the very frame a rebuild lands the lead acts on the photo landed on:
  one frame wide, and the key was pressed before that frame was seen.
- The engine's INI pass does not take the lock a star's write takes: a pass that read
  the file before a star and wrote its rows after could undo the star on screen until
  the next pass. It predates this, and is milliseconds wide at the watcher's two seconds.
- An egui widget under the edge of the search help, the dropdown's list or a toast is
  not pressed through it (an area takes the clicks on its own ground); only what reads
  a press from the frame's events had the hole.
- Copy photo, when it comes, is an `Event::Copy` and not a key: it has to be kept from
  under a held band, as the keys are.
