# The native UI: grid interaction

Date: 2026-10-11. Sub-project 3 of `2026-10-09-photon-native-ui-design.md`. It is built on
`native-ui`, after sub-project 2 (`2026-10-10-photon-native-sidebar-views-search-design.md`),
whose five pull requests (#175 to #180, with the wheel's fix between) are merged.

## What this is for

After sub-project 2 `photon-native` shows the whole library and goes anywhere in it, and
changes nothing: no photo can be picked, and the only writes are the folder last looked at,
a saved search and the tile size. This sub-project makes the grid something to work in.
Photos are selected, by the mouse, the keyboard and a rubber band; a right-click offers
what can be done with them; a star, a hide, an album and a keyword are written; an album, a
saved search and a folder are renamed where they stand in the sidebar; a folder dropped on
the window is watched; a selection is exported.

It is the first sub-project in which the native window writes what the user would miss:
`library.db`, and the one line photon writes outside it, a `star=` in a folder's
`.picasa.ini`. Every write goes through a command the engine already has. No new kind of
write is made, and no photo file is opened for writing.

## Decided with the user (2026-10-11)

- **Writes are switched on as they land.** A star writes `star=` into the folder's
  `.picasa.ini` through `set_stars`, as the Tauri photon does; hide, keywords, albums and
  folder names write `library.db`. Each is on from the pull request that builds it.
- **What needs the platform is drawn and inactive until sub-project 7**: "Reveal in file
  manager" (a photo's and a folder's), "Open in default app", "Copy photo", and every
  system folder picker. No `arboard` and no `rfd` is taken now.
- **One spec, six pull requests** ("The six pull requests", below): the selection and the
  keyboard lead; the rubber band; the tile's menu with star, hide and albums; the
  sidebar's menus and the rename field; the dialog layer with the keyword picker; folder
  drop and export.

## Decided while writing

Five things the three answers did not settle, each a choice made here so that the spec is
whole. They were put to the user with the spec on 2026-10-11, the second as a correction
of what had been said when the platform was asked about, and the answer was to go ahead.
Any of them can still be turned round before the plan of the pull request it is in.

1. **An export's destination is a path typed or pasted into the dialog** until
   sub-project 7 brings the folder picker, whose button is drawn beside the field and
   inactive. The alternative is to leave "Export…" inactive until then and make the sixth
   pull request the folder drop alone. A field is recommended: the export, its report and
   its line in the status bar are then run through the real engine now, and the picker is
   later one button switched on beside a field that stays.
2. **Folder drop does not work on a Wayland session**, and nothing here can make it:
   `winit` 0.30.13, which `eframe` 0.36.2 is built on, delivers dropped files on X11,
   Windows and macOS and has no code for it on Wayland (read in its source, 2026-10-11).
   When the question about the platform was asked the answer given was that egui delivers
   dropped paths; on Wayland it does not. The drop is built for the three systems that have
   it. On Wayland the window offers none - the line "Or drop a folder onto this window"
   stays out there - and a folder is added in the Tauri photon, as today, until the picker
   of sub-project 7 or a `winit` that has the drop.
3. **A confirmation is photon's own dialog.** The Svelte UI asks through the system's
   message box (Tauri's `ask`) before an album or a saved search is deleted. Natively that
   is `rfd`, which is not taken now, so the question is drawn by the dialog layer with two
   buttons, "Delete" and "Cancel". Because that layer is the fifth pull request and the
   sidebar's menus are the fourth, **"Delete…" is drawn inactive for one pull request**.
4. **Three items of the tile's menu wait for the sub-project that owns what they open**,
   drawn and inactive: "Compare" and its key `C`, and "Add … to a person…" (the viewer's
   sub-project, 4, which has compare and the dialog that names a face); and Enter and a
   double click, which open the viewer, do nothing yet. "Show N duplicates" is not drawn
   at all until sub-project 6, which has duplicates and copies: it is a line that is only
   sometimes there, so no place has to be held for it.
5. **Captions are not in this sub-project**, although the umbrella lists them under it.
   A caption is read from the photo and shown by the viewer; nothing in photon writes one.

## What was read first

The Svelte store's selection (`library.svelte.ts`, some 600 lines of it), `Grid.svelte`'s
mouse, keys, band and menu, `layout.ts`, `nav.ts`, `Menu.svelte` and its two helpers,
`FolderMenu.svelte`, `FolderTree.svelte`'s menus and rename fields, the editors and
dialogs (`album-editor`, `tag-picker`, `export-dialog`, `folder-drop`, `star-key`), the
commands they call, and the native crate as it stands. Seven things in it shape this
design.

1. **Most of the Svelte selection is there because the webview does not hold the grid.**
   It pages through rows a thousand at a time, so a range's ids, the photo a hide lands
   on, the lead's photo after a rebuild and the photos under a band are each a round trip,
   and each round trip can be overtaken - by a rebuild, by a later call, by a click. The
   native window holds the whole `GridIndex` it draws from, with `position_of` answering
   in constant time. Every one of those questions is a read of memory in the frame that
   asks it.
2. **When a new grid is put on screen the window has both**, the index it was drawing and
   the one it is about to. "Where did this photo go" and "what stays near it" are answered
   between the two, there and then, whatever caused the rebuild.
3. **A write command rebuilds the grid on the thread that calls it**
   (`refresh_after_write`), as a view setter does. A star also writes a file. None of it
   may run on the UI thread, and the order of two writes matters (star, then unstar).
4. **Several writes announce nothing the lists follow.** An album made, renamed, deleted
   or added to, and a saved search renamed or deleted, change no grid unless that album is
   on screen; a folder's name and its hidden flag rebuild the grid but the folder list is
   not among what a data change re-reads. The Svelte store re-reads after each itself.
5. **Four things the Svelte UI does are not the engine's**: reveal, open in the default
   app, copy a photo and every system dialog are Tauri plugins, called in `ipc.rs`.
6. **The native grid has no widget for a tile.** It paints rows from the layout. What is
   under the pointer is worked out from the same layout, which is also what the rubber
   band's hit test does.
7. **egui 0.36.2 has a `Modal`**, and `winit` has no file drop on Wayland (above).

## What is in it

### The selection: `selection.rs`

A state module. It holds what the Svelte store holds and no more: the selected photos'
**ids**; the **lead**, the photo an arrow key moves on from and Enter will open, as an
offset and the id there; the **anchor**, where a Shift+click ranges from, the same way;
and, while a band is drawn, what the band adds to and what it started from. Every call is
given the index on screen.

Ids, not offsets, for the Svelte store's reason: an offset means a photo against one
version of the index only, and a scan that indexes a photo into an earlier folder shifts
every later one.

The rules, each the Svelte store's and each ported with the test that holds it:

- **A plain click** selects that photo alone; lead and anchor go to it.
- **Ctrl/Cmd+click** puts the photo in or takes it out. Lead and anchor go to it either
  way, so the next Shift+click ranges from where the user last clicked. Taking the last
  photo out leaves no lead.
- **Shift+click** replaces the selection with everything from the anchor to the photo
  (from the lead where there is no anchor, from the first photo where there is neither).
  The lead goes to the photo; the anchor stays, so dragging the far end back and forth
  ranges from the same start. Shift wins over Ctrl when both are held.
- **Select all** (Ctrl/Cmd+A) is the view outside All photos - an album, a search, the
  starred photos are sets the user asked for. In All photos it is what stands under the
  lead's header: its folder, or under a date grouping its day, month or year; and where
  the grid has no headers, the folder of the lead's photo, whose photos lie scattered
  (`GridIndex::folder_ids_at`). The lead stays where it is, so Enter still opens the photo
  the user was on; the anchor goes to the start of what was selected. A section "has a
  header" by `has_header`, not by having a folder: a month has none.
- **Escape** clears the selection, the lead and the anchor.
- **A right-click on a photo outside the selection selects it first**, alone, so that
  what a menu acts on is always what is ringed. Inside the selection, all of it stands.

**When a new grid is put on screen** (`follow`, given the old index and the new):

- every id the new index does not hold leaves the selection. The Svelte store drops only
  the lead's, having no cheap way to ask about the rest, and over-counts until the next
  click; here "N selected" is exact, and no action is handed a photo no tile shows. The
  two sets a band holds lose them the same way;
- the lead and the anchor are found again by id;
- **a lead whose photo has left lands on the nearest photo that stays** - the first after
  it in the old index that the new one holds, else the last before it - as a file manager
  does after a delete. If nothing is left selected, that photo is selected alone, and is
  the anchor. So `H` pressed again and again works through Duplicates, and the star key
  through Starred, from where the user was;
- if the photo that left was in sight, the one landed on is brought into sight by the
  least movement. If it was not - a scan purged it while the user was elsewhere - nothing
  moves;
- an anchor whose photo has left is no anchor.

This one rule is the Svelte store's `rebindSelection`, `setHidden`'s landing and
`adoptLead` together. It does not ask what caused the rebuild, so it needs no account of a
click made while a write was out: the click's photos are still there and stay selected.

**When a step's grid is put on screen** - a view, a search, a sort - the selection is
cleared, with its lead and anchor: under a new order the anchor's offset names another
photo, and in another view the photos are another set. A refused step clears nothing.

### What of the Svelte selection is not ported

Listed so that it is left out on purpose, as the grid slice's spec lists `placeIn` and
the pages. Each is a consequence of the first two things read.

| Svelte | Why it has no successor |
|---|---|
| `fetchIds`, `fetchIdsOf`, the thousand-row chunks | a range's ids are read from the index |
| `extendCall` (the last call wins) | a range lands in the frame that asked for it |
| `reaching`, `keyboardLead` | the lead has moved before the next key repeat |
| `picks` (a click made while ids were on their way) | nothing is on its way |
| `runs`, `landingOf`, `landingIn`, `findLanding`, `lastPhoto` | the landing is found between two indexes in hand |
| `bandUnresolved`, `bandsResolving`, the band answered twice | a band's photos have ids as it is drawn |
| `adoptLead`, `isSelectedTile`'s fallback to the offset, `selectItem` | every offset has its id |
| `anchorWrites` | the anchor is found again by its own id |

Their tests are read one by one for the plan. A test of a race that cannot happen here is
named there with the reason, not dropped in silence; a test of a *rule* is ported, whatever
race it was written around.

### The mouse

One sensing area over the photos. Which photo a point is on is `tile_at`, in
`grid/layout.rs`: the row from the position, the column from the tile's drawn width, and
no photo in the gap between two tiles or beside a short last row.

- A click is a click and a drag is a band by egui's own rule for telling them apart; the
  Svelte grid's four-pixel threshold is not ported.
- A click on the room between photos does nothing. A band drawn over nothing is how a
  selection is cleared with the mouse.
- A press on the pinned header, on the scrollbar or on the year strip is theirs, and
  starts no band.
- The primary button only: the secondary one opens the menu, at the press's place.
- A selected tile has a ring, drawn inside the tile, because a row lies flush to the
  grid's top edge and anything outside the tile's own box is cut there. There is no second
  mark for the lead.

### The keys

Answered by the grid while nothing else has the keyboard - a text field, a dropdown, the
splitter, a button the Tab key reached - and nothing lies open over the photos: no menu,
no dialog, no list, the search box's help. And, while a band is held, Escape alone.
(Changed by the review of the first pull request, which found the grid answering keys
that something else had just answered.)

- **The arrows, Home and End** move the lead and select the photo it lands on, alone
  (`nav.ts`'s `move`: vertical moves follow the rows on screen, which start again at every
  section; with nothing selected a key starts from the first photo, so End still reaches
  the last). Today these keys scroll the native grid; from this sub-project they move the
  lead, and the grid follows it by the least movement, below the pinned header.
- **Page Up and Page Down** move the lead a screenful in its column (`pageMove`: always
  at least a row; headers and the gaps between sections are part of the distance). With
  nothing selected they scroll, as now: a move would start from the first photo.
- **Shift with any of them** selects from the anchor to there, as a Shift+click does. With
  nothing selected the key is the plain one.
- **Ctrl/Cmd+A**, **Escape**: above. Escape is not answered on a key's repeat - Escape in
  an empty search box hands the keys to the grid, and the selection must not go with it.
- **`.`** stars the selection, or takes its stars off; **`H`** hides it, or in Hidden
  brings it back (the third pull request). Neither with a modifier held.
- Enter, `C`, Ctrl/Cmd+C and Ctrl/Cmd+Shift+R do nothing until the sub-project that has
  what they open.

### The rubber band

A press on the photos that becomes a drag draws a rectangle, and what it touches is
selected. `items_in_rect` and `edge_scroll_speed` are ported from `layout.ts` with their
cases.

- The band is kept **in the layout's coordinates**, so it keeps its grip on the photos it
  was started over while the grid scrolls under it. Its far corner is worked out again
  from the pointer's place in the window in every frame of a scroll.
- A tile is touched over its own square, not the gap under it; a band inside a gap
  selects nothing. Rows that join up are one range, and a band does not run off the end
  of a short last row into the next section's photos.
- **A band replaces the selection; with Ctrl, Cmd or Shift held as it starts, it adds.**
  Every frame is "what it started with, plus what the rectangle covers now", so a
  rectangle dragged smaller takes photos back off.
- **On release** the lead and the anchor go to the band's first photo. A band over
  nothing leaves what an adding band started with, and otherwise nothing, with no lead.
- **Escape abandons it** and puts back the selection it started from. The lead is left
  alone: a band never moves it until it is released.
- **Held near the top or bottom edge, the grid scrolls** (a margin of 48 points, at most a
  third of the grid's height, so there is always a middle that holds still; up to 1,400
  points a second, by how far into the margin the pointer is). The step is the speed times
  the frame's own duration, capped at 50 ms, so it is as fast at 120 Hz as at 60. A frame
  is asked for only while the pointer rests in a margin; a band held still elsewhere draws
  nothing.
- **A drag has four endings and one teardown**: the release, Escape, the pointer lost to
  the window (the button let go where the window does not hear it, a touch the system
  took), and a step's grid arriving under it - which clears the selection the band began
  from, so that band is dropped and nothing is put back. While a band is held nothing
  opens over it: no menu, no dialog, and every key but Escape is ignored.
- A grid that arrives under a band for any other reason - a scan - does not end it. The
  rectangle covers what it covers in the grid now on screen, and that is what is ringed.

### The writes

All of them go through the queue of writes sub-project 2 began (`App::writes`): one
thread, in the order asked, each answer handed back to `app.rs`. A refused write is a
toast with the engine's words and changes nothing else.

| What | Command | Then |
|---|---|---|
| star, unstar | `set_stars` | a toast when fewer landed than were asked |
| hide, unhide | `set_items_hidden` | - |
| add to an album, take out of one | `add_to_album`, `remove_from_album` | the lists are read again |
| take from a person | `remove_from_person` | a toast saying what came of it; the lists |
| a new album; an album renamed, deleted | `create_album`, `rename_album`, `delete_album` | the lists; All photos, when the album on screen was deleted |
| a saved search renamed, deleted | `rename_saved_search`, `delete_saved_search` | the lists |
| a folder's name in photon | `set_folder_alias` | the folder list |
| a folder hidden or brought back | `set_folder_hidden` | the folder list |
| a folder scanned again | `rescan_folder` | - |
| a keyword added or removed | `add_items_tag`, `remove_items_tag` | a toast with the count |
| a folder watched | `add_folder` | the folder list; a toast |
| the export's checkbox | `set_export_apply_edits` | - |

The grid is not told what a write did. The write's own rebuild arrives like any other, and
`follow` does the rest: hidden photos leave the selection because they left the view, and
the lead lands beside where it was.

**The export is not on that queue**: it runs for minutes, and a star pressed meanwhile
must not wait for it. It has a thread of its own, one export at a time.

### The menu: `menu.rs` and `menu_view.rs`

One menu for the whole window, held by the application: opening one closes any other, so
a right-click elsewhere while one is open moves it. `menu.rs` is what a menu is - items
with a label, a key's hint, whether they work, the look (an item, an indented one, a
heading, a note, a separator, a dangerous one) and what picking it answers - with
`menu_step` and `place_menu`, both ported with their cases.

- **It opens down and to the right of the pointer** and turns to the other side on an
  axis where it would cross the window's edge; pushed against the margin only when
  neither side fits; pinned to the top with its items scrolling when it is taller than
  the window. It is placed again in every frame from the size it was measured at, so one
  that changes or a window that is resized needs nothing more.
- **While it is open the keys are its own.** The arrows move through the items that work
  and wrap; Home and End; Enter and Space pick; Escape closes. No other plain key reaches
  the grid behind it - an `h` must not hide the photos the menu is about. A chord and a
  function key are let through.
- **It closes** at a pick, at Escape, at a press outside it (the press, not the click: the
  dropdown's lesson), when a step's grid arrives, and when the sidebar is hidden under
  one of its menus. Closed, the keys are the grid's again.
- An item that does not work is drawn fainter, is skipped by the arrows and takes no
  press.
- What an item acts on is read when it is picked, not when the menu opened: a grid that
  arrived meanwhile may have taken photos out of the selection.

### The tile's menu

In the Svelte menu's order and words. "Photo" is "photo" for one and "12 photos" for
several.

| Item | Offered | Works from |
|---|---|---|
| Reveal in file manager · Ctrl/Cmd+Shift+R | one photo | sub-project 7 |
| Open in default app | one photo | sub-project 7 |
| Copy photo · Ctrl/Cmd+C | one photo, not a video | sub-project 7 |
| Star photo, or Unstar photo · `.` | one photo | pull request 3 |
| Star *N photos* and Unstar *N photos* | several: both, the key's hint beside the one it would do | pull request 3 |
| Compare *N photos* · C | two to four | sub-project 4 |
| Hide *photo* · H; in Hidden, Unhide | always | pull request 3 |
| Add keyword to *photo*… / Remove keyword from *photo*… | always | pull request 5 |
| Add *photo* to a person… | while photon finds faces | sub-project 4 |
| Remove *photo* from “*name*” | in the view of a person photon named | pull request 3 |
| Remove *photo* from “*album*” | in the view of one of photon's own albums | pull request 3 |
| Export *photo*… | always | pull request 6 |
| Add to album: each of photon's own albums | always; "No albums of your own yet — create one in the sidebar." with none | pull request 3 |

**Which way the star key goes** is decided by one photo: the lead when it is selected,
else the selected photo that comes first in the grid. Starred, the selection is unstarred;
not, it is starred. (The Svelte rule takes the first selected photo *on screen*, for want
of the rest; with none on screen it stars.) A star that could not be written - a folder
whose INI is read-only - costs a toast, "3 photos of 12 could not be starred", and not
the other nine.

Adding to an album is not filtered by what the photos are already in: adding twice is
harmless, and the grid's rows do not know their albums. A Picasa album offers neither
"Add to" nor "Remove from": it is changed in Picasa.

### The sidebar's menus and the rename field

- **An album of photon's own**: "Rename…" and "Delete…". A Picasa album has no menu.
- **A saved search**: "Rename…" and "Delete…". Deleting one leaves the grid alone: the
  photos on screen are still the answer to what was typed.
- **A folder**, on its row and on its header in the grid, the real one and the pinned
  copy - one list, so the two cannot come to offer different things: "Rescan" (inactive
  while that watched folder is being scanned), "Reveal in file manager" (sub-project 7),
  "Rename in photon…", "Use folder name" (only for a folder that has a name in photon),
  and "Hide folder" or "Unhide folder". A period's header names no folder and has no menu.
- **"New album…"**, a row under the albums, opens the field in its own place.

`rename.rs` ports `createAlbumEditor`: one field for the whole sidebar, keyed by the
entry it stands in for, so a rename begun while another is half typed takes its place.

- The field opens with the name in it, all selected. Enter stores it. Escape closes it. A
  press elsewhere stores it, as leaving the field does in the Svelte list.
- **An empty name is a cancel**, as Escape is - except a folder's, where it is how the
  user asks for the directory's own name back.
- **A name the library refuses leaves the field open**, with a toast, so the name can be
  put right.
- **While its write runs the field takes no typing and keeps the keyboard.** A field that
  gave the keyboard up would send the next name typed to the grid, where `h` hides a
  photo.
- A write that comes back closes the field only if it is still the field that was sent:
  a folder's write waits for a rebuild, long enough for another row's rename to have
  begun.
- The Enter that stores a name presses nothing else, and neither does its repeat.
- Closed, the keys are the grid's.
- The list does not follow the grid while a name is being typed, and its field's row is
  the entry the list is held by (`Kept`).
- **"Rename in photon…" on a header in the grid** shows the sidebar first when it is
  hidden, and opens the field in the folder's row, brought into sight. A folder the list
  does not hold has no row to open it in, and the item does not work.

### The dialog layer

One dialog at a time, over everything, held by the application: egui's own `Modal` where
the pinned version's does what follows (its source is read for the plan), and an area
over a scrim where it does not.

- Under a dialog nothing takes a press or a key: not the grid, not the sidebar, not the
  top bar. Tab stays inside it.
- Escape closes it, and so does a press on the scrim.
- It opens with the keyboard in its first field. Closed, the keys are the grid's.
- Nothing opens over a dialog, and none opens while a band is held.
- What a dialog acts on is **taken when it opens**: the selection behind it can change
  while it is up (a scan), and what the user was told it would act on is what it acts on.

Three dialogs use it here; Settings, the dialog that names a person and the list of keys
are later sub-projects' and stand on the same layer.

**The confirmation.** A title, the question, "Delete" and "Cancel", with the keyboard on
"Cancel". The two questions are the Svelte UI's: "Delete the album “*name*”? The photos
stay in your library." and "Delete the saved search “*name*”? Your photos are not
affected."

**The keyword picker** (`tag_picker.rs`, ported from `createTagPicker`). Its title says
which way it writes and to how many photos. A field, and under it the library's keywords
narrowed by what is typed: the exact name first, then those that begin with it, then
those that contain it, each group in the library's own order - Enter writes what is
typed, so the first row must be what Enter would do. Matching is without case, in Rust.
It closes *before* its write, which is the user's last word on it; a second Enter in
that frame writes nothing. The toast is the count the library answers with, not the one
asked for: "Added “lisbon” to 10 of 12 photos" when two had gone meanwhile.

### Folder drop

`folder_drop.rs` ports `createFolderDrop`, fed from the files egui says are hovering over
the window and dropped on it.

- While something with a path is held over the window, a card over the content says what
  letting go will do: "Drop folders to add them to photon".
- Every dropped path is offered to the engine, **one at a time**: two folders of one drop
  can overlap each other, and the engine can refuse the second only once it has finished
  the first. The engine is what knows a folder from a file, and a folder already watched
  (answered as itself, not as an error).
- **The drop is reported once**: "Watching “Holiday”" or "Watching 3 folders", and the
  first refusal with a count of the rest - forty photos dropped are not forty toasts.
- A folder added has its scan counted as running at once, unless that scan has already
  been heard from (`Scans`), and the folder list is read again.
- The empty library's panel gets its line back, "Or drop a folder onto this window",
  where a drop can arrive.
- While the export's dialog is open a dropped folder is its destination, and is not
  watched.

### Export

`export_dialog.rs` ports `createExportDialog`.

- The title says how many photos. The sizes are the Svelte dialog's - the photo's own,
  3840, 2560, 1920 and 1280 pixels on the longest edge - and are **not remembered**: a
  size left on from last time would quietly shrink the next export, and that one might be
  the archive.
- "Apply edits" is remembered, and stored as it is ticked and not when the export runs;
  put back when the library will not store it. It is read when the dialog opens, off the
  UI thread; a read that fails falls back to applying them.
- **The destination** is a field for a folder's path, with "Choose…" beside it, inactive.
  A path is checked as soon as it is given (`check_export_dest`), and a folder photon will
  not write into - one inside a watched folder - is said against the field while the
  dialog is still open, not after it has gone.
- "Export" works once a destination has passed. The dialog closes *before* the export
  runs: it can take minutes, and a dialog over the progress it started would hide it.
- The line in the status bar is sub-project 2's. At the end a toast: "Exported 12 photos
  to *folder*", or "10 of 12 photos" with the reason; and when nothing at all could be
  written, the reason alone, as an error.
- "Export…" does not work while an export is running.

### The status bar

"*N* selected", before the photo count, when more than one photo is selected.

## The six pull requests

Each is a branch off `native-ui` with its own plan, the Rust gate at every commit, one
independent review of the whole branch, and new pictures from `native-shot`.

1. **The selection and the keyboard lead.** `selection.rs`; `tile_at`; the click, the
   Ctrl+click and the Shift+click; the arrows, Home, End and the page keys, plain and with
   Shift; select all and Escape; the ring; `follow` and the clearing at a step; the grid
   following the lead; "N selected".
2. **The rubber band.** `items_in_rect`, `edge_scroll_speed`; the band's state, its
   drawing, the scroll at the edges, its four endings.
3. **The tile's menu, with star, hide and albums.** `menu.rs` and `menu_view.rs`; the
   writes' answers and toasts; `.` and `H`; "Add to album", the two "Remove from"; the
   lead landing in sight after a hide. Every item of a later pull request or sub-project
   is in its place, inactive.
4. **The sidebar's menus and the rename field.** The album's, the saved search's and the
   folder's menus, on the rows and on the grid's headers; "New album…"; `rename.rs`;
   "Use folder name", "Hide folder", "Rescan". "Delete…" is drawn and inactive.
5. **The dialog layer, with the keyword picker.** The layer; the confirmation, and with
   it "Delete…"; `tag_picker.rs`, and with it the two keyword items.
6. **Folder drop and export.** `folder_drop.rs`; `export_dialog.rs`, its thread and its
   toasts; "Export…".

After the first two, anything can be picked; after the third, the day's work of a library
- star, hide, file into albums - is done natively; the last three are the sidebar's
housekeeping, the keywords, and the two ways photos come in and go out.

## What is not in it

- The viewer and everything that opens it; compare; the dialog that names a person:
  sub-project 4.
- "Show N duplicates" and the Copies view's way in; Settings (watched folders, the
  keywords' rename and removal); the People page; the list of keys and the test that
  holds every handler to it: sub-project 6.
- Reveal, open in the default app, copy a photo, the folder pickers, a system message
  box: sub-project 7.
- Captions: read-only, the viewer's.
- A menu on a person's or a keyword's row: the Svelte sidebar has none.
- Dragging photos onto an album. The Svelte UI has no such thing.

## Tests

- **State modules are ported with their cases.** `library.test.ts` has some seventy-six
  cases of selection, hiding and the band; `layout.test.ts` has the band's hit test, the
  page move and the edge's speed; `nav.test.ts` has the arrows; and the small modules have their own (`menu-keys`,
  `menu-place`, `star-key`, `album-editor`, `tag-picker`, `folder-drop`,
  `export-dialog`). A ported test is a new test: each is shown to fail with its rule
  broken, and a probe that passes is a finding.
- **`follow` is tested on pairs of indexes**: a photo arriving above the selection; the
  lead's photo leaving with another after it, with none after it, with its neighbour
  leaving too (Duplicates); the whole selection leaving; a click made before the grid
  arrived; a photo leaving that was out of sight.
- **Views are tested in whole frames**: a click and a Shift+click on tiles found through
  the layout, a band dragged across two rows and into the margin, a menu opened near each
  edge, its keys, a name typed into a row.
- **Every write is a test through the real engine**, in a library made for the test: a
  star is read back from the folder's `.picasa.ini`, a hide from the Hidden view, an
  album from its count in the sidebar, a folder's name from its row and its header, a
  keyword from the Tags list, an export from the files in a temporary destination, a drop
  from the watched folders. They wait for a state and never for a length of time.
- **A still window draws nothing** (the event loop's driver): with photos selected; with
  a band held where the grid does not scroll; with a menu open.
- **Tripwires**: `STATE_MODULES` gains each new state module; the menu's and the dialogs'
  words are held to the Svelte files they are copied from where a test can read them.
- **Pictures**, read by whoever builds them: a selection with its count; a band; the
  tile's menu, for one photo and for several; a folder's menu; a name being typed; the
  keyword picker; the confirmation; the export's dialog; the card of a drop.

## Risks particular to this sub-project

1. **The first native writes.** A wrong id handed to `set_stars` writes a line into a
   folder the user did not mean. Everything that acts takes its ids from one place, the
   selection, read at the pick; the tests write only into libraries they made; and the
   smoke checklist has a star and an unstar read back in Picasa's own file.
2. **Click or drag is egui's call.** A press held still long enough is a drag to egui, so
   a long press on a photo is a band over that one photo - which selects it, as the click
   would have. A double click is two clicks until the viewer's sub-project gives the
   second one a meaning.
3. **Three more text fields** - a name in the sidebar, a keyword, a path. egui's field
   does not reorder text that mixes writing directions and its input-method support
   cannot be tried without a window: sub-project 2's first risk, three more times.
4. **A long write holds the queue.** Starring four thousand photos across three hundred
   folders is three hundred files written and one rebuild, and a hide asked for behind it
   waits, as in the Svelte UI. The export has its own thread for that reason.
5. **A very large selection is walked at every new grid**: `follow` asks the new index
   about each selected id. Select all in a search that matches the whole library, during a
   scan that publishes a grid a second, is 300,000 lookups a grid. It is measured in the
   first plan, and bounded there if it shows.
6. **Wayland has no drop** (above), which is the user's own desktop.
7. **Two photons, one library.** Both can be open, and the native one now writes what the
   other shows. A star set in one reaches the other through the watcher, the INI having
   changed; a hide or an album reaches it at its next rebuild, whenever that is. Nothing is
   lost and nothing is fixed here: there is one photon after the switch-over.

## Not decided here

- Whether sub-project 7 puts the system's message box in the confirmation's place.
- Whether a sort should keep the selection. It clears it, as in the Svelte UI.
- The words that are new to the native UI: the confirmation's two buttons, the path
  field's label and what is said of a path that is no folder. Every other string is the
  Svelte UI's.
