# The native sidebar's menus and the rename field: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the fourth of sub-project 3's six pull requests: a right-click on an album, a
saved search or a folder - its row in the sidebar, its header in the grid - opens its
menu; a name is typed in the row itself; "New album…" makes an album; a folder is given
a name in photon, hidden, brought back and scanned again.

**Architecture:** the third pull request's menu serves here unchanged: `side_menu.rs`
says which items each of the three menus has, and the application holds *whose* menu is
open beside where it is. `rename.rs` is the one field the sidebar has room for, keyed by
the entry it stands in for; `sidebar/view.rs` draws it in that entry's place. Each name
stored and each folder hidden is a write on the queue of writes, as a star is.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"The sidebar's menus and the rename field", and the rows of "The writes" for albums,
saved searches and folders.

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the first pull request's plan, and four of this one's own:

- **Nothing here writes outside `library.db`.** A folder's name in photon is the
  library's `alias`; the directory keeps its name, and no file or folder is renamed,
  moved or deleted.
- **"Delete…" is drawn and takes no press** on an album and on a saved search, until the
  fifth pull request has the confirmation it asks first. "Reveal in file manager" waits
  for sub-project 7.
- **What a menu acts on is read at the pick**, from the lists as they are then: a menu
  whose album, search or folder is no longer listed is closed.
- **A test writes only into a library it made**, in a temporary directory.

## Not ported, and why

- **Three editors** (`editor`, `searchEditor`, `folderEditor` in `FolderTree.svelte`):
  they are three because the lists key their rows by ids that collide. One field keyed
  by what it stands in for (`rename::Of`) cannot open in two rows, and a rename begun
  while another is half typed takes its place by being the only one.
- **`backToRow` and the listener that keeps a held Enter from pressing a row twice**:
  a field closed from the keyboard gives the keys to the grid, not to its row, and the
  Enter that stored the name is nobody's until it comes up (`Rename::enter_held`).
- **`focusLost`, `tick`**: there is no DOM to catch up with. The field has the keyboard
  because it is asked for in the frame the field is first drawn.
- **`closeMenus` when the sidebar is hidden**: an open menu has the keys, Ctrl+B among
  them, and a press on the toggle is a press outside the menu, which closes it.
- **The confirmation** (`ask`) and the two "Delete…" items' doing: the fifth pull
  request.

## Rulings made in the plan

- **One menu, of one thing**: `App::menu` becomes where it is and whose it is
  (`MenuOf::{Selection, Album, Search, Folder}`). Opening any closes any other.
- **A menu is asked for where the other button goes down**, on a row as on a photo.
- **The field is egui's `TextEdit`**, as the search box's is, with that box's lessons:
  Escape is claimed for it, a press elsewhere is the view's own rule and not egui's
  click, and its text is written from this side alone.
- **While a name's write runs the field keeps the keyboard and takes no typing**: what
  is typed then is dropped, not kept for after.
- **A second "Rename…" while a write runs takes the field**; the first write's answer
  finds another field and leaves it alone (`Rename::landed` goes by the number the
  field was sent under).
- **"New album…" is an entry of the list** (`What::NewAlbum`), under the albums while
  their group is open, as the Svelte row is.
- **A folder's menu on a header in the grid** is asked for on the real header and on the
  pinned copy alike, from the header's own rectangle; a period's header answers nothing.
- **"Rename in photon…" from the grid** shows the sidebar first when it is hidden and
  brings the folder's row into sight. In an order that lists no folders it does not
  work.

## Review focus

1. **A name stored for the wrong thing**: the field of one row committed under another's
   id; an answer closing a newer field; a rename begun on a row that is rebuilt away
   under the field (the albums read again, a scan's folder list); the same id in two
   lists (album 3 and search 3 and folder 3).
2. **Keys that go astray around the field**: a letter typed into it reaching the grid
   (`h`, `.`); Enter, held, pressing something after the field closes; Escape clearing
   the selection behind it; Tab; the keys while its write runs; the keyboard after it
   closes, by key and by a press elsewhere.
3. **The field and a list that moves**: the list following the grid under it; rows
   arriving above it; the sidebar hidden with a name half typed; a group folded over it.
4. **A folder hidden**: its photos leave the grid like any others (`follow`), its row
   leaves the list; in Hidden, the menu brings it back; a hidden sidebar and a header's
   menu.
5. **A still window** with the field open and nothing typed - but for the caret, which
   blinks as in any application - and with a row's menu open.

## Task 1: `side_menu.rs`

```rust
pub enum SideAction {
    RenameAlbum(i64), DeleteAlbum(i64),
    RenameSearch(i64), DeleteSearch(i64),
    Rescan { watched: i64 }, RevealFolder(i64), RenameFolder(i64),
    UseFolderName(i64), HideFolder { folder: i64, hidden: bool },
}
impl SideAction { pub fn works(&self) -> bool; }
pub struct FolderFacts { pub id: i64, pub watched: i64, pub named: bool, pub hidden: bool,
    pub scanning: bool, pub listed: bool }
pub fn album_menu(id: i64) -> Vec<Item<SideAction>>;
pub fn search_menu(id: i64) -> Vec<Item<SideAction>>;
pub fn folder_menu(facts: &FolderFacts) -> Vec<Item<SideAction>>;
```

- [ ] Tests first: each menu's lines; "Use folder name" only for a folder with a name
  in photon; "Hide folder" or "Unhide folder"; Rescan off while its watched folder is
  scanned; "Rename in photon…" off for a folder the list does not hold; the two
  "Delete…" off and dangerous; every string held to `FolderTree.svelte` and
  `FolderMenu.svelte`.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 2: `rename.rs`

```rust
pub enum Of { NewAlbum, Album(i64), Search(i64), Folder(i64) }
/// What to write for a field committed.
pub struct Commit { pub of: Of, pub name: String, pub sent: u64 }
pub struct Rename { .. }
impl Rename {
    pub fn field(&self) -> Option<(&Of, &str)>;
    pub fn editing(&self, of: &Of) -> bool;
    pub fn busy(&self) -> bool;
    pub fn start(&mut self, of: Of, name: &str);
    pub fn type_in(&mut self, text: String);          // dropped while a write runs
    pub fn cancel(&mut self);
    /// Enter, or a press elsewhere. A blank name is a cancel - but a folder's, which is
    /// how its own name is asked back (an empty `name`).
    pub fn commit(&mut self) -> Option<Commit>;
    pub fn landed(&mut self, sent: u64);              // closes only the field sent
    pub fn refused(&mut self, sent: u64);             // leaves it open, to be put right
    pub fn enter(&mut self, down: bool) -> bool;      // whether this Enter is nobody's
}
```

- [ ] Tests first: `album-editor.svelte.test.ts`'s eight cases, and: a commit while a
  write runs sends nothing; typing while a write runs changes nothing; an answer for a
  field that was replaced meanwhile leaves the new one open and not busy; a refusal for
  a field replaced meanwhile leaves it alone; the same id in two lists is two fields;
  the Enter that stored a name is nobody's until it comes up, and its repeats.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 3: the rows, the headers and the field

- `sidebar/list.rs`: `What::NewAlbum`, under the albums of an open group; an entry says
  whether it has a menu (`What::menu`: an album of one's own, a saved search, a folder).
- `sidebar/view.rs`: the other button going down on such a row answers it with the
  place; the entry the field stands in for draws the field (`SidebarData::rename`), and
  answers what was typed, a commit and a cancel; the list is held by that entry and does
  not follow the grid while the field is open.
- `shell.rs`: `Action::RowMenu { what, at }`, `Action::Rename(RenameAction)`.
- `grid/view.rs`: the other button going down on a folder's header, real or pinned,
  answers `GridOutput::header_menu`.

- [ ] Tests first, in whole frames: a right-press on an album's row, a search's, a
  folder's; none on Picasa's album, a person, a keyword, a heading, a view; on a
  folder's header and on the pinned one, none on a period's header nor on a photo's
  place under the pinned one; the field in a row, with the name in it and all of it
  selected; typing; Enter; Escape; a press elsewhere; nothing typed while busy and the
  keyboard kept; the list not following the grid under an open field; rows arriving
  above an open field leave it where it is.
- [ ] Implement, gate, commit.

## Task 4: the application

`MenuOf`; the items of whichever menu is open, built in every frame; `Write::{AlbumNew,
AlbumRename, SearchRename, FolderAlias, FolderHidden, Rescan}` and their answers.

- [ ] Tests first, through the application and the real engine: an album renamed from
  its row's menu reads its new name in the sidebar; "New album…" makes one, with no
  photos, and a blank name makes none; a name the library refuses (an album's name
  taken) leaves the field open under a message; a saved search renamed; a folder named
  in photon reads so on its row and on its header in the grid, "Use folder name" and a
  blank name bring its own back; a folder hidden leaves the grid and the list, and its
  photos the selection; in Hidden its menu brings it back; Rescan starts a scan and is
  off while one runs (the library's write lock held, as `tests/status.rs` holds it);
  "Rename in photon…" on a header with the sidebar hidden shows it, with the field in
  the folder's row; a letter typed in the field hides nothing and Enter held presses
  nothing; a menu whose album was deleted from elsewhere closes; a still window.
- [ ] Implement, gate, commit.

## Task 5: pictures, the checklist, CLAUDE.md, the pull request

- [ ] Pictures: a folder's menu on its header; a name being typed in an album's row;
  light and dark.
- [ ] The smoke checklist; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.

## Changed in the making

- **No held Enter is kept track of.** The spec has "the Enter that stores a name presses
  nothing else, and neither does its repeat", and Task 2 gave `Rename` the state for it.
  Nothing can be pressed by that Enter: the field closes into nothing that has the
  keyboard, where the Svelte field hands the focus to its row. When the grid is given
  Enter (the viewer's sub-project), it must not answer a repeat, as `.` and `H` do not.
- **Escape closes a field whose name is being stored**, as the Svelte editor's `cancel`
  does: the write is on its way and its answer finds no field. Task 2 had it stay.
- **A field whose row is not drawn is closed**: stored when it is scrolled out of sight,
  cancelled when its entry has gone from the list. The Svelte input stays in the DOM
  wherever the list is scrolled to; here a row out of sight is not drawn, egui takes
  the keyboard off a widget that is gone, and the next name typed would be the grid's.
- **The arrow keys in the search box**, found while the second text field was built
  (CLAUDE.md, "egui's text field, twice now"): fixed here, with a test through the whole
  application.
- **On a Mac, Control with a click on a row opens its menu and does not go to the row**
  (`menu_click`): the menu was opened at the press, and the same click is not the row's
  as well. Read from source and not run: it is one line no test here reaches.
- The folder's menu on a header asks `header_at`, a function of the layout with its own
  cases, and not the view's "is this the pinned header's height" used for photos.

**Probes**: a hundred rules broken in turn, each with the test that was to fail. Eight
passed at first. Four were lines that did nothing and went: a read of the folder list
after a folder's write (the engine's rebuild already asks for one); a check that a point
in a header's row is within the row (it is, the rows of photos beginning where a header
ends); a "not while it is stored" on what was typed (nothing is typed then: the input
is stripped); and an empty folder name turned into none (the library takes it for none).
Four were tests that could not see their rule and were sharpened: a menu whose album is
gone (the list was empty either way), the name a folder's field opens with (it had no
name in photon when asked), and Rescan twice over - a file added for the scan to find
was found by the watcher as well, so the test now adds none and holds the scan at the
one write every scan ends with.
