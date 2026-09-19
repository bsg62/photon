# photon — Grid Multi-Select Design

**Date:** 2026-09-19
**Status:** Approved design
**Builds on:** v0.17.0

## 1. What changes

The grid selects one photo. `LibraryStore` holds a single `selectedOffset` (with a
remembered `selectedId` so the selection survives an index rebuild), `nav.move()` answers
with one offset, `Tile` takes a `selected` boolean, and the tile's context menu acts on
exactly one entry.

This design lets the user **select several photos in the grid** and act on all of them at
once. Two actions, both of which already exist for one photo:

- **star and unstar** — new to the grid entirely; starring was viewer-only until now;
- **add to an album** and **remove from the current album** — already array-shaped in the
  IPC (`add_to_album`, `remove_from_album`).

Two gestures select: **Ctrl/Cmd+click** toggles a photo, **Shift+click** extends a range
from the anchor. There is no marquee drag, no `Ctrl+A`, and no keyboard range extension;
arrow keys keep collapsing the selection to one photo, as they do today.

### The promise is unchanged

photon still writes exactly one kind of byte inside a watched folder: a `star=` line in
Picasa's own `.picasa.ini`, through the writer in `picasa.rs`. Bulk starring changes *how
often* that file is rewritten, not what goes into it — see §3.

## 2. The selection is a set of photo ids

A grid offset is only meaningful against one version of the index. A scan that indexes a
photo into a folder that sorts earlier shifts every later offset, which is why the single
selection is re-found by id through `grid_offset_of_item` after every rebuild.

A multi-selection cannot take that route: re-finding *n* photos is *n* round trips, and the
selection must survive a scan (the user's decision — only a view switch, a plain click or an
arrow key collapses it). So the set holds **item ids**, which no rebuild can invalidate, and
needs no rebinding at all.

`LibraryStore` gains:

| Field | Type | Meaning |
|---|---|---|
| `selection` | `Set<number>` (`$state`, replaced wholesale) | Photo ids currently selected. |
| `anchor` | `number \| null` (plain, not `$state`) | Grid offset a Shift+click extends from. |

and the existing `selectedOffset`/`selectedId` stay exactly as they are: the **lead** is what
`Enter` opens, what the viewer opens on, and what `rebindSelection` keeps in place.

### The five entry points

- **`set selected(offset)`** — every caller it has today (arrow keys, a plain click,
  right-clicking a tile outside the selection). Now also replaces `selection` with that one
  id and moves the anchor to the offset. This is what makes "a plain click or an arrow key
  collapses" fall out of the existing code rather than needing new rules at each call site.
- **`selectItem(offset, id)`** — the viewer closing, and "Locate in photon". Replaces the
  selection **unless `id` is already the lead's id**. Closing the viewer on the photo it was
  opened with therefore keeps the multi-selection; navigating to another photo inside the
  viewer and closing there collapses to that photo, which is the one the user is looking at.
- **`toggleSelected(offset)`** — Ctrl/Cmd+click. Toggles the entry's id; lead and anchor move
  to that offset whether it was added or removed, so the next Shift+click extends from where
  the user last clicked. A selection emptied this way leaves the lead `null`.
- **`extendSelection(offset)`** — Shift+click. The range runs from the anchor (or the lead,
  or 0) to `offset` inclusive, and **replaces** the selection rather than adding to it. The
  anchor does not move, so dragging the far end back and forth re-ranges from the same start.
- **`clearSelection()`** — a view switch. Empties the set and the lead.

### Fetching a range's ids

The pages the grid has loaded do not cover an arbitrary range — Shift+click across a
thousand photos names ids the UI has never seen. `extendSelection` asks the backend directly
with `api.gridRows(start, count)`, **chunked at `MAX_ROWS` (1000)**: `clamp_count` silently
truncates a larger ask, so a single un-chunked call would select the first thousand photos of
the range and quietly drop the rest.

The call is `async`. A second Shift+click landing while the first is in flight is answered by
the later one: each records the grid version it started from and abandons its result if a
refresh has landed, the same rule `refresh` and `rebindSelection` already apply.

### The one stale case, recorded

If a scan purges a selected photo, its id stays in the set. Nothing is drawn wrong — there is
no tile left to ring — but the status bar's count over-reports until the next plain click.
Pruning it properly needs a new "which of these ids are still live" command; that is not
worth a fifth IPC surface for a count that is one click from correct. The comment on
`selection` says so, so a future reader does not "fix" it by reintroducing offsets.

## 3. Bulk starring

`Engine::set_star` writes one photo: it takes the `ini_write` lock, rewrites that folder's
`.picasa.ini` through `picasa::set_star`, writes the rating, and refreshes the grid. Starring
200 selected photos by calling it 200 times means 200 whole-file rewrites (each a temp file
and a rename) of what is often the *same* INI, 200 rating writes, and 200 grid rebuilds.

### `picasa::set_stars`

A folder-level writer: `set_stars(dir, &[(file_name, starred)])` applies every change in one
pass and one rewrite. It **shares the line classifier** with the reader and with
`set_star` — a writer with its own header and key logic drifts from the reader, which is why
`picasa.rs` has one classifier today. `set_star` becomes the one-element case of it, keeping
its own error semantics (an unknown or missing id is still `NotFound`, and nothing is
written).

Every other byte of the file is still preserved, and no key but `star=` is touched. This is
the same write the 2026-09-16 spec narrowed the promise to, done once per folder instead of
once per photo.

### `Engine::set_stars(ids, starred) -> usize`

1. Take `ini_write` once, for the whole batch.
2. Load the items; skip ids that are unknown or `missing_since`.
3. Group the survivors by parent directory.
4. Per directory: one `picasa::set_stars`. A directory whose write fails is **skipped**, not
   fatal — a read-only folder in the selection must not cost the user the other eleven photos.
5. One `lib.set_ratings` over every photo whose INI write landed. INI first, database second,
   as `a_failed_ini_write_leaves_the_database_untouched` pins: a star in the database that no
   INI confirms is shown to the user and then silently cleared by the next scan.
6. One `refresh_grid()`.

It returns how many photos were actually starred. The UI compares that with what it asked
for and reports `3 of 12 photos could not be starred` as an ordinary error toast; a batch
where *nothing* landed returns the first folder's error instead, so the user sees the reason
rather than a bare count.

### The IPC surface

Per the three-files rule, plus the two that fail only at runtime:

1. `commands.rs` — `pub fn set_stars(engine: &Engine, ids: &[i64], starred: bool) -> CmdResult<usize>`.
2. `ipc.rs` — the `#[tauri::command(async)]` wrapper that delegates.
3. `app.rs` — the entry in `generate_handler![...]`.
4. `ui/src/lib/api.ts` — the hand-written mirror, `setStars(ids, starred): Promise<number>`.
5. `crates/xtask/screenshots/mock.js` — a canned answer, or the test in `screenshots.rs` that
   reads `api.ts` fails.

No new Tauri plugin permission: this touches nothing in `capabilities/default.json`.

## 4. What the user touches

### Tile

`onselect` carries the `MouseEvent`, so the grid can read `ctrlKey`/`metaKey`/`shiftKey`; the
tile stays dumb about what a modifier means. Its `selected` prop becomes "my id is in the
selection" instead of "my offset is the lead".

There is **one** ring, not a lead-vs-member distinction: the lead is only ever what `Enter`
opens, and a second visual state would have to be invented, themed and contrast-checked to
say something the user does not need. The ring stays drawn *inside* the tile — an outline
clips when the grid scrolls a row flush to the top of the viewport.

### Grid

The click router, in order: Shift extends, else Ctrl/Cmd toggles, else a plain click
collapses to one. `Enter` opens the lead. Nav keys are untouched — they go through
`library.selected`, which collapses.

### The context menu

Right-clicking **inside** the selection keeps it; **outside** it collapses to that tile
first. That preserves today's rule, which the code comment states as "what the menu acts on
is what is outlined".

With *n* selected:

- `Star 12 photos` / `Unstar 12 photos` — both always present. A single toggle would have to
  decide what a mixed selection means and change its own label under the user; two items
  never lie. For *n* = 1 the labels are singular (`Star photo`), so nothing reads worse than
  today.
- `Add 12 photos to “Album”` / `Remove 12 photos from “Album”` — the existing items, with
  `library.selectedItemIds` in place of `[entry.id]`. Adding stays idempotent and the menu
  still does not filter out albums a photo is already in: the grid row does not know its
  memberships, and asking per photo would be a round trip for a menu.
- `Reveal in file manager` — shown **only when exactly one photo is selected**. The file
  manager takes one path. `Ctrl+Shift+R` follows the same rule.

### StatusBar

`12 selected · 4,301 photos`, with the count present only above one selected photo. No new
token: it is text in the existing `--text-dim` footer.

## 5. Testing

There is no component test harness (`vitest` runs in `node`), so the discriminating tests go
where the logic lives, and each is demonstrated failing with its change reverted.

**`ui/src/lib/library.test.ts`** — it already mocks `./api` wholesale:

- Ctrl+click toggling adds, then removes, and leaves the lead on the clicked offset both times.
- `extendSelection` over a range wider than `MAX_ROWS` issues more than one `gridRows` call
  and selects every id in the range. A single un-chunked call passes a narrower assertion,
  so the range in this test is deliberately over 1000.
- **A refresh that shifts every offset leaves the set intact and re-binds the lead.** This is
  the test that discriminates ids from offsets; an offset-based selection passes every other
  test in this list.
- A view switch clears; `selectItem` collapses only when the id differs from the lead.

**Rust (`crates/photon-app/src/engine.rs` tests):**

- Two photos in one folder come back as **one** INI holding both `star=yes` lines. A
  per-photo loop also ends with both photos starred, so the assertion is on the file's whole
  contents, not on the two stars.
- A folder whose INI write fails (an oversized existing INI, as
  `a_failed_ini_write_leaves_the_database_untouched` already arranges) is skipped while a
  second folder's photos land, and the returned count reports it.
- The grid version bumps **once** for the whole batch.
- `set_star` keeps refusing an unknown or missing id, writing nothing — the existing tests
  stay as they are, over the new shared writer.

**Covered by the gates rather than by a test:** the `Tile` prop change and the `api.ts`
mirror (`npm run check`), the mock answer (`screenshots.rs`), and the menu's effect wiring
(the README's smoke checklist, which gains a line for Ctrl+click, Shift+click and a bulk
star across two folders).

## 6. Not in this design

- **Marquee drag**, `Ctrl+A`, and Shift+arrow range extension. Two gestures cover the work;
  a marquee means hit-testing against the virtualised rows and edge auto-scroll.
- **Bulk rotate, bulk tag, bulk reveal.** Rotation would need `Engine.edit_write` per photo
  and a thumbnail GC epoch bump; nothing about this design blocks adding it later.
- **Selecting across a view switch.** The offsets and usually the photos are different.
- **A selection action bar.** The context menu and the status-bar count are the whole
  surface; new chrome above the grid is a reskin-scale decision.
