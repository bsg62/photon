# The native sidebar's lists: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the second of sub-project 2's five pull requests: albums, searches, people and
tags in the sidebar, each switching the view; the folders under their years, drawn a window
at a time; the mark of the folder at the top of the grid, and the list following it; the
jump to a folder; the last folder, restored at launch and come back to from All photos; the
folds.

**Architecture:** the sidebar becomes one list. A state module (`sidebar/list.rs`) builds
its entries from what the application holds and stacks them by height; the view
(`sidebar/view.rs`) draws the entries in view at a position it keeps, as the grid does. The
application rebuilds the list only when one of its sources changed, by a key it compares
every frame. A folder jump is a place in the grid asked for (`GridView::go_to`) once the grid
that holds the folder is on screen, which `nav.rs` knows.

**Tech stack:** as the first pull request: eframe/egui 0.36.2, `photon-engine`'s commands,
`tasks::Latest` and `tasks::Queue`.

**Spec:** `docs/superpowers/specs/2026-10-10-photon-native-sidebar-views-search-design.md`,
"The sidebar", "Going to a folder, and coming back to one", "What the library holds".

This plan was written with the user away and the go-ahead to carry on unattended. Unlike
the first pull request's it names every interface and every test and does not carry the
code: nobody was waiting to read the code before it was written.

## Global constraints

- A state module names no egui type and is added to `STATE_MODULES` in `lib.rs`.
- Nothing that reads SQLite or the filesystem runs on the UI thread after `App::new`.
- Every new test is shown to fail with its rule broken, by an exact replacement.
- The Rust gate at every commit: `cargo fmt --all`, `--check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core
  --bench grid --no-run`.
- Every string the user reads is the Svelte UI's. Numbers and dates are English.
- Every name goes through `text::paint_line`.
- The application is never launched to verify a change.
- A new caller of a view setter goes through the queue (`nav.rs`).

## What is left out, and to which pull request

- "No folders yet." and its button: the fifth, with the folder list's refetches. Read at
  launch the list may predate the Pictures folder `startup` adds, and until the refetch
  rules are there the line would be said of a library that is being scanned.
- Cancelling a typed search before a click (`searchBox.cancel`): the third, with the box.
- The photo a jump selects (`library.selected`): sub-project 3, with the selection.
- Every menu and rename field, "New album…": sub-project 3.
- The People label does nothing: the page is sub-project 6.

## Review focus

1. **A jump against the wrong index.** A folder's offset means something only in the grid
   it was looked up in. A click on a folder from Starred must land in All's grid, not in
   Starred's a frame before All arrives, and not at all when the step to All is refused.
2. **The last folder overwritten at launch.** The grid of a launch is at its top until the
   restore has run; a write before it stores the library's first folder over the place the
   user left.
3. **The list rebuilt per frame, or not rebuilt.** Five thousand folders are five thousand
   allocations; and a source that changes without the key moving leaves the list stale.
4. **The mark one frame behind on a still window.** The sidebar is drawn before the grid
   has taken its input: the frame in which a scroll ends must be followed by one that
   marks the folder it ended in.
5. **The list moving under the pointer.** It follows the grid except while the pointer is
   over it; a row must not move away from a click on its way.

## Task 1: `nav.rs` - the collections' steps, and a jump that waits for its grid

**Files:** `crates/photon-ui/src/nav.rs`.

**Interfaces (produces):**

```rust
pub enum Step { View(GridView), Search(String), Album(i64), Person(String), Tag(String) }
pub struct Shown { pub other_results: bool, pub jump: Option<i64> }
impl Nav {
    pub fn adopt(&mut self, version: u64) -> Option<Shown>;
    /// The step that brings the folders' own view on screen, when the user is not in it.
    pub fn home_of_folders(&self) -> Option<Step>;
    /// Goes to `folder`: answers it when the grid on screen is the one to look it up in,
    /// and otherwise keeps it for the grid of the last step asked.
    pub fn jump(&mut self, folder: i64) -> Option<i64>;
}
pub fn laid_out_by_folder(sort: Sort) -> bool;
```

- [ ] Tests first, each failing for want of the rule: a step to an album, a person and a
  tag leads to the place the engine makes of it; `home_of_folders` is All from every view
  but All and Hidden; a jump with nothing on its way is answered at once; a jump behind a
  step is answered by the `adopt` that shows that step's grid and by no earlier one; a
  refused step takes its jump with it; a later step shown at once drops the jump; a step
  put in the place of the one waited for drops it; `laid_out_by_folder` is date and folder
  only.
- [ ] Implement. `Nav` numbers what has landed; `adopt` answers `Shown`.
- [ ] `app.rs` follows the new `adopt` (no behaviour yet). Gate, commit.

## Task 2: `sidebar/folders.rs` and `sidebar/list.rs` - the list as entries

**Files:** create `sidebar/folders.rs`, `sidebar/list.rs`; modify `sidebar/mod.rs`,
`lib.rs` (`STATE_MODULES`).

**Interfaces (produces):**

```rust
// folders.rs: `folders.ts`'s list, case for case
pub struct FolderRow { pub folder_id: i64, pub name: String, pub count: usize, pub year: i16,
    pub taken_at_min: i64, pub bytes: i64, pub modified_ms: i64 }
pub struct YearGroup { pub year: Option<i16>, pub rows: Vec<FolderRow> }
pub fn year_of(taken_at_min: i64, zone: &TimeZone) -> i16;
pub fn folder_rows(tallies: &[FolderTally], folders: &HashMap<i64, Folder>, zone: &TimeZone) -> Vec<FolderRow>;
pub fn group_by_year(rows: Vec<FolderRow>, reverse: bool) -> Vec<YearGroup>;
pub fn arrange_folders(rows: Vec<FolderRow>, sort: Sort) -> Vec<YearGroup>;

// list.rs
pub enum Group { Albums, Searches, People, Tags }
pub enum What { Fixed(Fixed), Group(Group), Album(i64), Search { id: i64, query: String },
    Person(String), Tag(String), Year(i16), Folder(i64), Note(Group) }
pub enum Count { Of(usize), ToName(usize) }
pub enum Detail { None, Open(bool), Picasa }
pub struct Entry { pub what: What, pub label: String, pub count: Option<Count>,
    pub active: bool, pub hint: String, pub detail: Detail }
pub struct Collections { pub albums: Vec<AlbumSummary>, pub searches: Vec<SavedSearch>,
    pub people: Vec<Person>, pub to_name: usize, pub tags: Vec<TagCount> }
pub struct Sources<'a> { /* counts, at, today, collections, open, tallies, folders, sort, zone */ }
pub struct Key { /* what the list was built from: compared, never read */ }
pub struct List { pub entries: Vec<Entry>, pub generation: u64, /* folder -> entry */ }
impl List { pub fn build(sources: &Sources<'_>, generation: u64) -> Self;
    pub fn folder(&self, folder_id: i64) -> Option<usize>; }
impl What { pub fn step(&self, today: Today) -> Option<Step>; }
pub struct Stack { /* tops */ }
impl Stack { pub fn new(heights: impl IntoIterator<Item = f64>, above: f64, below: f64) -> Self;
    pub fn total(&self) -> f64; pub fn top(&self, index: usize) -> f64;
    pub fn bottom(&self, index: usize) -> f64;
    pub fn range(&self, from: f64, to: f64) -> std::ops::Range<usize>; }
pub fn into_view(top: f64, bottom: f64, position: f64, viewport: f64) -> Option<f64>;
pub fn height(what: &What) -> Option<f64>;   // none for a note: the view measures it
```

- [ ] `folders.rs`, tests first: `folders.test.ts`'s `folderRows`, `groupByYear` and
  `arrangeFolders` cases, plus the year read in the viewer's zone at the year's edge.
  Names by `natural_cmp` over lower case, ties in the order given.
- [ ] `list.rs`, tests first: the order top to bottom; a group's count and fold; Searches
  only when there is one; "N to name" before the number of people; tags with no visible
  photo left out; the two notes of an empty People and Tags list, only when open; the
  Picasa mark; the row of the album, person, tag and saved search shown is the active one,
  a saved search only when the query is exactly its own; years newest first and none under
  a sort that is not by date; a folder not in the list yet has a blank name; `What::step`;
  `Stack` (tops, the room above and below, `range` at both ends and past them);
  `into_view`.
- [ ] Gate, commit.

## Task 3: the grid says where it is, and goes where it is told

**Files:** `grid/layout.rs`, `grid/view.rs`.

**Interfaces (produces):**

```rust
// layout.rs
pub fn top_folder_id(rows: &[Row], sections: &[Section], top: f64) -> Option<i64>;
pub fn scroll_to_start(row: &Row, inset: f64) -> f64;
pub fn scroll_into_grid(row: &Row, top: f64, viewport: f64, inset: f64) -> Option<f64>;
// view.rs
pub enum Align { Start, Nearest }
impl GridView { pub fn go_to(&mut self, offset: usize, align: Align); }
pub struct GridOutput { /* ... */ pub top_folder: Option<i64> }
```

- [ ] Tests first: `layout.test.ts`'s cases for the three functions; in whole frames, a
  place asked for is where the next frame is drawn, under the pinned header's room where
  the section has a header; it is not a scroll (the grid does not count as moving); `to_top`
  after it wins, and it after `to_top`; an offset the grid does not hold moves nothing;
  `top_folder` is the folder of the row at the top and none in a flat layout.
- [ ] Implement, gate, commit.

## Task 4: the sidebar drawn as a list

**Files:** `sidebar/view.rs`, `shell.rs`, `icons.rs`.

**Interfaces (produces):**

```rust
pub struct SidebarData<'a> { pub list: &'a List, pub here: Option<i64> }
#[derive(Default)] pub struct SidebarView { /* position, the stack it was drawn with */ }
impl SidebarView { pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, data: &SidebarData<'_>) -> Option<What>;
    pub fn position(&self) -> f64; }
// shell.rs
pub struct ShellData<'a> { pub layout, pub list: &'a List, pub here: Option<i64>, pub count, pub notice, pub toasts }
pub enum Action { ToggleSidebar, Row(What), Width { .. }, Dismiss(u64) }
```

Seven icons more, held to `icons.ts` by the test that holds the others: chevron-down,
chevron-right, folder, bookmark, user, tag, images.

- [ ] Tests first, in whole frames: every kind of entry is drawn where its height puts it;
  a click on each kind answers it, and the People label, a year and a note answer nothing;
  the People fold answers its group; only the entries in view are drawn in a list of five
  thousand; the wheel over the list moves it and not past its end; the mark is drawn beside
  the folder the grid is in; the list follows the mark by the least movement, and not while
  the pointer is over it; a note is wrapped to the list's width; the thumb is drawn only
  when there is somewhere to scroll and a drag on it moves the list.
- [ ] Implement, `Shell` holding the view. `shell.rs`'s own tests follow the new data.
- [ ] Gate, commit.

## Task 5: the application - collections, the jump, the last folder, the folds

**Files:** `app.rs`, `empty.rs`, `tests/common/mod.rs`, `tests/lists.rs`, `tests/shell.rs`.

- The collections are one `Latest`, asked at launch and at every data change; a part that
  could not be read keeps what was there.
- The list is rebuilt when its `Key` differs from the one it was built with.
- `act`: a row's step; All photos reads the folder remembered before it asks for the step
  (`laid_out_by_folder`), and jumps there when All is shown; a folder asks for
  `home_of_folders` and jumps; a group folds and is stored.
- The last folder: read in `App::new`; restored once, when the first grid with photos is on
  screen; written, by a `Latest`, when the folder at the top changes in All after the
  restore.
- A frame is asked for when the folder at the top is not the one the sidebar was drawn
  with.
- `empty::view_notice` gains the album's, the person's and the tag's lines.
- `App::go` is public: a test asks for a step as a row does.

- [ ] Tests first, through the application: the lists are read and drawn; a click on an
  album shows it and its row is the active one; steps to a person and a tag reach the
  engine as those views, and an empty one says its line; a folder clicked in All is at the
  top of the grid; clicked from Starred it is at the top of All; clicked in Hidden the view
  stays; All photos from an excursion comes back to the folder left; the folder is there
  again after a relaunch, and the launch does not overwrite it; a fold is stored and its
  rows are gone; a still window draws no frame with the lists in it.
- [ ] Implement, gate, commit.

## Task 6: pictures, the checklist, CLAUDE.md

- [ ] `tests/screenshots.rs`: the library gains an album, a saved search and keywords; a
  picture with People and Tags unfolded. Read every picture.
- [ ] `docs/smoke-checklist.md`: the native section's items for the lists.
- [ ] CLAUDE.md, "The native UI": the list and its key, the jump, the last folder.
- [ ] Gate, commit, push, pull request.

## After the plan

One fresh reviewer over the whole branch, the Review focus given verbatim; one fix pass,
each fix test-first; the pull request merged into `native-ui` when CI is green.
