# The native UI: sidebar, views and search

Date: 2026-10-10. Sub-project 2 of `2026-10-09-photon-native-ui-design.md`. It is built on
`native-ui`, after sub-project 1 (`2026-10-09-photon-native-grid-slice-design.md`) and its
gate, which was run on 2026-10-10 and passed every line (the table is on pull request #173).

**Changed by the review of the first pull request** (2026-10-10): the settled place is
not read from the engine when a grid arrives. The engine's view moves when a step begins,
a whole rebuild before its grid is published, so with two steps on their way the first's
grid arrived under the second's view. What a grid shows is taken from the answer of the
step that built it, and a grid published while a step is on its way waits for that answer
before it is put on screen.

**Changed while the second pull request was built** (2026-10-10), each from what a probe
or a test showed:

- The albums, saved searches, people and tags are one read, not five tasks, and are not
  asked for at launch: the engine's first grid is a data change, and asking as well read
  them twice.
- The last folder is read once, in `App::new`, and kept in memory from then on. The
  application is its only writer, so the way back from an excursion reads nothing.
- "No folders yet." and its button are the fifth pull request's, with the folder list's
  refetches: read at launch the list may predate the Pictures folder the engine adds.
- A frame is asked for to bring the folder's mark up to the grid. It was taken out once,
  as doing nothing - egui draws two frames for every immediate request - and put back by
  the review, which found the frame eframe draws for a resize, which nothing asked for.
- The list is held by an entry when its entries change (the row under the pointer, the
  marked folder's, the first in view): the review found the marked row pushed out of
  sight at launch by the albums arriving after the list had followed the grid.
- A run of the gate neither goes back to a folder nor remembers one.
- A row tells AccessKit whether it is where the user is, ahead of sub-project 7: it is
  how a test of the whole application sees which row is filled.

**Changed while the third pull request was built** (2026-10-10):

- Saving a search is a write, made on a queue of writes off the UI thread; the lists are
  read again when it is done, the engine announcing no data change for it.
- The help closes on Escape, on a press outside and on a pick, as written; not when the
  focus walks out of the field by Tab, as the Svelte box also does.
- An Enter or Escape that ends an input method's composition is not told apart from the
  box's own: egui's field gives no sign of a composition. On the smoke checklist.

## What this is for

After sub-project 1 `photon-native` is a window with one thing in it: the grid of whatever
view and sort the library was last left in, which nothing native can change. This
sub-project puts the rest of the main window around it - the top bar, the sidebar, the
status bar - and makes the library navigable: every view, the search, the sort and the
grouping, a jump to a folder.

It changes nothing in the library but two things the Svelte UI also writes while
navigating: the folder last looked at, and a saved search. Everything that edits - menus,
rename fields, stars, albums - is sub-project 3.

## Decided with the user (2026-10-10)

- **One spec, five pull requests**, each with its own plan and its own independent review,
  each leaving `native-ui` working ("The five pull requests", below).
- **What needs a later sub-project is drawn in place and inactive**: "Add folder…" (a
  system folder picker, sub-project 7), and the gear, "Watched folders…", "Add a folder in
  Settings…" and the People label (Settings and the People page, sub-project 6). A button
  that will be there is there, so nothing moves when it starts working. `photon-native`
  opens the library the Tauri photon opens, so a folder can be added there meanwhile.
  Saving a search with the bookmark button is in: it is one command and needs nothing else.
- **English throughout.** Numbers and dates are written in English form, as the words are,
  where the Svelte UI writes them in the system's locale. No locale library is taken. It is
  a visible change on a system that is not English, to be looked at again before the
  switch-over.

## What was read first

The Svelte surfaces, the commands they call and the native crate as it stands, in an
inventory taken from the code on 2026-10-10. Five things in it shape this design:

1. **A view setter rebuilds the grid on the thread that calls it.** `set_grid_view`,
   `set_search_query`, `set_sort` and the album, person and tag setters all go through
   `Engine::rebuild_or_restore`: the query, `GridIndex::build`, and a rollback when it
   fails. They return the grid version that shows the new state.
2. **They must be applied in the order they were asked.** The Svelte store chains them
   (`viewChain`): a search already sent cannot be cancelled, so a click on Starred right
   after it has to wait for it, or the search lands last under an empty box. The task layer
   sub-project 1 built (`tasks::Latest`) answers only the latest question and would drop a
   sort that a view switch was asked behind.
3. **`grid_info` reads SQLite**, although it returns no `CmdResult`: the four counts on a
   cache miss, and an item in the Copies view. The umbrella's list of commands that may be
   called on the UI thread does not have it, and it stays off that list.
4. **The native grid already draws every layout.** A section with a period gets a header
   as one with a folder does, and a flat section gets none (`grid/layout.rs`,
   `grid/header.rs`). A library left on "By month" in the Svelte photon already opens that
   way. What is missing is the control, not the drawing.
5. **The folder list is refetched too rarely today.** The native app asks for it when the
   library reports a data change and ignores `folder_status` and `scan_progress`; the
   Svelte store refetches on a folder's status, on a scan's end and on the first event of a
   folder it does not list.

## What is in it

### Where the user is: `nav.rs` and the ordered queue

**`tasks::Queue`**, beside `Latest`: one worker thread that runs every job it is given, in
order, and hands each answer back with the job's number. Two more things, both the Svelte
chain's: a job that has not started and is still the last one may be replaced (a search by
a later search, and only then - with a view switch or a sort queued behind it, replacing
it would reorder the two); and a job that panics answers `Panicked` and the next one still
runs, since a queue that died on one job would leave the grid dead for the session.

**`nav.rs`**, a state module, holds what the Svelte store holds about the view:

- *the settled place* - the view, its argument and the sort the published grid was built
  with, read from the engine when a rebuild lands (`Engine::view_and_arg`, `sort`: both in
  memory);
- *what has been asked and not yet landed*, so that the sidebar marks the view being gone
  to and the sort control shows the sort in flight, as `library.sort` does;
- *the steps* it hands the queue: a view, a search, a sort, a folder.

Its rules, each the Svelte store's and each with the test that holds it there:

- A switch empties the search box **when it is issued, not when it lands**: what is typed
  after the click is meant for after the switch, and the queue sends it after. Only a
  refused switch puts the box's text back, and only while no later switch has been issued
  (`switchesIssued`).
- The box never follows the engine's query. A refused search hands the box the query the
  engine still holds only if the user has not typed since.
- **A jump to a folder is a step like the others**: it cancels a search not yet sent, and,
  once every step before it has landed, leaves the view for All unless the view is All or
  Hidden (which is disjoint from All, so a folder in it is jumped to where it is). The
  offset is read from the published grid on the UI thread when the step's rebuild has been
  taken (`grid_offset_of_folder`: in memory).
- **Back to All photos** does nothing when All is already showing, and otherwise reads the
  last folder before the switch, and only while the grid is laid out by folder.
- A refused step is reported in a toast and changes nothing else.

A change of results puts the grid back at its top (`view_key`, `results_changed`): the
view, its argument, the sort key and direction, and the grouping only under a date sort, so
that a grouping changed under Name moves nothing.

### What the library holds: counts and lists

Held by the application and read off the UI thread, each by a `Latest` (the newest answer
is the only one wanted):

| What | Command | Asked for |
|---|---|---|
| the four counts, the Copies view's anchor | `grid_info` | when the engine reports a change |
| albums, saved searches, people, groups to name, tags | `list_albums`, `list_saved_searches`, `list_people`, `people_to_name`, `list_tags` | at launch and when a change is a data change |
| watched folders and folders | `list_folders` | at launch; on a `folder_status`; on a scan's end; on the first event of a folder not listed |
| a folder's size when its scan begins | `watched_folder_stats` | on a scan's first event |

The tag counts alone are about 220 ms at 300,000 photos: nothing here is read per frame,
and nothing is read because a view changed (a view setter's rebuild is not a data change).

### The shell

The window becomes four areas around the grid: a top bar, the sidebar with its splitter, a
status bar, and the content, where the grid is.

- **Top bar**, left to right: the sidebar's toggle, the search box, the sort, grouping and
  size controls, the gear (inactive).
- **The splitter** is five points wide and takes the keyboard: the arrows move it by 16.
  The width is held to between 160 and half the window, the minimum winning, and is stored
  when a drag ends or a key moves it - never when a narrower window clamps it.
- **A hidden sidebar** (the toggle, Ctrl+B) gives its columns to the grid. In the Svelte UI
  it has to stay laid out, unseen, to keep its scroll position; here the list's position
  is a number the application holds, and nothing is laid out that is not drawn. Ctrl+B is
  not answered on a key repeat or while the splitter is held.
- **Toasts**: a message that goes away by itself, over the bottom of the content. This
  sub-project needs one for a refused step, and every later one needs them.
- **Who has the keys.** The grid answers its keys only while no text field has them. With
  the search box focused, Home and End move its caret and not the library.

**The window's layout is kept per machine**, in `layout.json` beside `library.db`: the
sidebar's width, whether it is hidden, and which groups are open. Not in the settings
table, whose accessors are photon-core's and which the Tauri photon reads too. It is read
before the first frame, written when the user changes one of the three, and a file that is
missing or cannot be read is the defaults (260 wide, shown, Albums and Searches open).
Sub-project 7 puts the window's own state in the same file.

### The sidebar

One list, drawn row by row at 28 points a row, **only the rows in view**: a library has
thousands of folders, and the People and Tags lists are as long as the user has made them.
The Svelte list leans on the browser for that (`content-visibility`); here the list is a
flat sequence of rows built in a state module, and the view draws a window of it, as the
grid does.

Top to bottom, as today:

1. All photos; Starred (count); Recent; On this day; Videos, Duplicates and Hidden, each
   only above zero or while it is the view shown; under Duplicates, while a photo's copies
   are shown, "Copies of *name*", which is not a button.
2. **Albums**, with each album's count and the mark of one that is Picasa's.
3. **Searches**, only when there is one.
4. **People**: the fold and, beside it, the label (inactive: the People page). Its count is
   "N to name" when there are groups to name, else the number of people.
5. **Tags**, those with a photo in them.
6. **The folders**, under their years. A row is a folder the grid holds photos of - from
   the published index's tallies, not from the folder list, which also holds the empty
   folders between - named from the list when the list has it. Under a sort that is not
   by date there are no years and the order is the sort's.

- The row of the view shown is filled. A saved search is the view shown when the search
  is exactly its query.
- **The folder at the top of the grid is marked** (a bar, not the fill, which is the view),
  and the list scrolls to keep it in sight - except while the pointer is over the list.
  Not "while the list has the focus": a clicked row keeps it, and the list would stop
  following the very scroll the click began. The grid says which folder is at its top
  (`top_folder_id`, ported), and says none under a layout that is not by folder.
- A group is folded and unfolded where the user clicks, and stored then.
- Names are ordered by `photon_core::sort::natural_cmp`, which is the order the grid's own
  name sort uses. The Svelte list uses the browser's collation, which differs from it
  beyond ASCII; here the two agree.
- A folder's year is read in the viewer's time zone, as today.
- Every name goes through `text::paint_line`.

### Going to a folder, and coming back to one

The grid gains what the Svelte grid calls `scrollToOffset`: put a photo's row at the top,
below the pinned header, or just into view.

**The last folder** is the folder at the top of the grid in All photos. It is written when
that changes - only in All, only once the launch's own restore is over, and never under a
layout that names no folder at the top, which is the whole guard - and read once at
launch, when the grid has photos and a width, to go back to it.

### The search

- **The box** is egui's text field, with the placeholder the Svelte box has, the help
  button, and the bookmark button while there is a query.
- **Typing searches**, 150 ms after the last key (`search_box.rs`, on a clock that is a
  number). Enter and Escape are the Svelte box's: Enter leaves the box, the search having
  run as it was typed; Escape closes the help if it is open, else clears the box when it
  has text or the view is a search, else leaves it. **An empty box outside a search clears
  nothing**: clearing sends the empty query, which goes to All photos, and would throw the
  user out of Starred.
- Ctrl+F and `/` put the focus in the box and select what is in it; a held `/` does not
  type a second one.
- **The help** is a panel under the box listing the grammar in six groups
  (`search_help.rs`); an entry that can be inserted is a button, and inserting closes an
  open quote first. It closes on Escape, on a press outside the box, and on a pick. The
  list keeps the test the Svelte one has: it reads `Query::terms` out of
  `photon_core`'s `search.rs` and fails on a prefix the list lacks and on one it offers
  that the parser would drop.
- **On this day** is the search `on:MM-DD` for today, where today is read again whenever
  the library changes, so a window left open past midnight catches up.
- **The bookmark** saves the search under its own text as the name, and is filled when the
  query is a saved search's.

### Sort, grouping, size

`select.rs` ports `createSelect` (open and closed, the keys, type-ahead, `disabled`), and
one view draws it.

- **Sort**: date taken, date modified, name, size, and the toggle that reverses it.
- **Grouping**: by folder, day, month, year, none. Under a sort that is not by date it is
  dimmed, not removed, so the top bar does not shift, and says why.
- **Size**: small, medium, large.

A control changes the `Sort` it read by replacing one field of it. Built from two fields,
the Svelte control once dropped the grouping.

### The year strip

Beside the grid's scrollbar, 44 points wide, when the grid is laid out by date and has
somewhere to scroll: the years where they begin, a line where the grid is, the year under
the pointer, and a drag that moves the grid. `timeline.rs` ports `timeline.ts`. Whether it
is shown is asked of the grid **as it would be beside the strip** (`shows_timeline`): asked
of the grid as it stands, the strip narrows the tiles, the grid then fits, the strip goes,
the tiles widen, and it comes back, every frame. A year can appear twice in a search.

### The status bar

- Left: that live updates are limited, when a watched folder is degraded; a line and a bar
  for each running scan, in the watched folders' order; the export's line; the face pass's.
- Right: the photo count, and nothing before the first grid is built or when it failed.

`scans.rs` holds what the Svelte store holds about scans, with its rules: a scan the UI was
*told* is running (it asks `scanning_folders` once, after it listens) is a note and not an
event, so the scan's first real event still counts as its first and its folder's size is
still read; a scan that reports done on an empty grid counts as running until the grid has
been read again; a folder that is no longer watched is not degraded. The wording is
`status.ts`'s.

The export's line is drawn although nothing native can start an export until sub-project
3: the event exists, and drawing it now is the line it will need.

### An empty library, and an empty view

`empty.rs` ports `emptyLibrary`, `noPhotosLine` and the per-view notices.

- In All photos and Recent, when there is nothing to show, a panel beside the grid says
  why, in the order hidden photos, a scan running, not known yet, nothing watched or
  nothing found: "Every photo is hidden" with its button; "No photos yet"; "Looking for
  photos…"; or that photon has found no photos in the folder, never worded as a finished
  search. The scanning and the nothing-found states are one block with one sentence
  changing, so that the buttons under it are not replaced as a folder is rescanned.
  "Add folder…" and "Watched folders…" are drawn and inactive.
- In every other view, its own line: no starred photos, nothing found for the search, and
  so on; and, when the first grid could not be built, that the library could not be read.
- Nothing is said before the grid has been built once and the folders have been read once.

### Icons, tokens, keys

- Seventeen more Lucide icons, held to `ui/src/lib/icons.ts` by the test that holds the
  four there are.
- The shadow under a menu, for the dropdown and the help panel, from `tokens.css`.
- `keys.rs` begins the list that `lib/shortcuts.ts` is today, with the keys this
  sub-project answers. The sheet that draws it, and the test that holds every handler to
  it, come with sub-project 6.

## The five pull requests

Each is a branch off `native-ui` with its own plan, the Rust gate at every commit, one
independent review of the whole branch, and new pictures from `native-shot`.

1. **The shell and the views.** `tasks::Queue`; `nav.rs`; the counts; the four areas, the
   splitter, the hidden sidebar and `layout.json`; the sidebar's fixed rows with their
   counts, each switching the view; the photo count in the status bar; the grid back at its
   top on new results; the line an empty view shows; toasts; who has the keys.
2. **The sidebar's lists.** Albums, searches, people and tags, each switching the view; the
   folders under their years, virtualised; the mark of the folder at the top and the list
   following it; the jump to a folder; the last folder; the folds.
3. **The search.** The box, the debounce, Enter and Escape, Ctrl+F and `/`; the help; On
   this day; the bookmark.
4. **Sort, grouping, size and the year strip.**
5. **The status bar's lines and the empty library.** `scans.rs`, the folder list's
   refetches, the panel.

After the first, every view the library was left in can be reached and left; after the
second and third, everything can be found; the last two are the surfaces that say what is
going on.

## What is not in it

- Every menu on a sidebar row, every rename field, "New album…", deleting anything, the
  folder dropped on the window: sub-project 3.
- Settings, the People page, the `?` sheet: sub-project 6.
- The folder picker, fullscreen and F11, the window's own state, AccessKit: sub-project 7.
- The selection, and with it "N selected" in the status bar: sub-project 3.
- A new screenshot harness: the pictures here are more of `native-shot`'s.

## Tests

- **State modules are ported with their cases.** The small Svelte modules behind these
  surfaces have about 150 test cases between them, and `library.test.ts` has 165 more, of
  which the view, sort, search and scan cases are this sub-project's. A ported test is a
  new test: each is shown to fail with its rule broken.
- **Views are tested in whole frames**: a click on a row, a key in the box, a drag on the
  splitter, read back from what was drawn and from the calls made.
- **The application is tested as an event loop runs it** (`tests/probe.rs`'s loop: a frame
  only when one was asked for, on a clock that jumps, waiting for the other threads before
  it first does). A view switch, a search typed and a folder jumped to are each a test
  through the real engine; and a still window, sidebar and all, draws no frame.
- **Every tripwire is kept or replaced**: the grammar list against the parser; the icons
  against the Svelte paths; `STATE_MODULES`, to which each new state module is added.
- **Pictures**, read by whoever builds them: the shell in both themes; by month; a search
  with its help open; the sort's list open; a first run; the sidebar hidden; a scan in the
  status bar.

## Risks particular to this sub-project

1. **The text field.** egui's field does not reorder text that mixes writing directions,
   which is the umbrella's second risk arriving, and its input-method support cannot be
   tried without a window. A search for a name in Hebrew will find the photos and show the
   query oddly. Both go on the smoke checklist; neither is fixed here.
2. **A long rebuild holds the queue.** A search over 300,000 photos takes what it takes,
   and a click on Starred waits for it, as in the Svelte UI. The sidebar marks the view
   being gone to at once, so the click is seen.
3. **The focus.** The Svelte UI's rules about where the focus goes when a field closes or
   a list goes inert have no direct counterpart; "the grid has the keys unless a text
   field does" is the rule here, and the smoke checklist is where it is tried.
4. **English numbers and dates** on a system that is not English: the user's decision,
   recorded above.

## Not decided here

- The wording of anything that is new to the native UI: there is none planned; every
  string is the Svelte UI's.
- Whether a view switch should show progress when its rebuild is slow. The Svelte UI shows
  none.
