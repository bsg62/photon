# The native status bar's lines and the empty library: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the last of sub-project 2's five pull requests: what the status bar says while
photon works - scans, an export, the face pass, live updates limited - and what an empty
library says in place of photos.

**Architecture:** `scans.rs` is what the Svelte store holds about the engine's reports,
with its rules, as a state module the application feeds every event and asks what to do
next. `status.rs` is `status.ts`: the wording and the arithmetic of a line. `empty.rs`
gains `emptyLibrary`, `noPhotosLine` and the panel's content. The shell draws the lines in
the status bar and the panel where the photos would be; the sidebar says "No folders yet."

**Spec:** `docs/superpowers/specs/2026-10-10-photon-native-sidebar-views-search-design.md`,
"The status bar" and "An empty library, and an empty view".

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the earlier plans of this sub-project: no egui type in a state module, each added to
`STATE_MODULES`; nothing reads or writes SQLite on the UI thread after `App::new`; every
new test shown to fail with its rule broken; the Rust gate at every commit; every string
the Svelte UI's; the application never launched.

## Not ported, and why

- **Asking which scans are running** (`scanning_folders`, `noteRunning`, `unheard`). The
  Svelte UI asks because the startup scans begin before a webview exists and an event sent
  to no listener is lost. The native interface's channel is made before the engine is
  opened and is handed to it: no report is sent before someone listens. The other caller of
  the note, a folder just added, comes with the sub-project that can add one. A test holds
  the first frame of a first scan to "Looking for photos…" instead.
- **"Or drop a folder onto this window."** Nothing native takes a drop yet, and a line that
  offers one would not be drawn-and-inactive but false.
- **The count of selected photos** in the status bar: sub-project 3.

## Review focus

1. **An empty library saying the wrong thing for a moment**: "has found no photos" during a
   first scan, or before the hidden count or the folder list has been read; "Every photo is
   hidden" after the last one is unhidden.
2. **A line that never goes**: a scan whose done arrives in the batch of its first report,
   a panicked scan, an export that ends with every photo failed, a face pass switched off.
3. **A still window.** A bar that moves by itself would draw sixty frames a second for as
   long as a scan runs; the bar of a first scan moves when the scan reports.
4. **The folder list read too often or too seldom**: once for a scan in a folder it does
   not hold, not once an event; again when a scan ends and when a folder's status changes.
5. **A status bar too narrow for its lines**: nothing drawn over the photo count.

## Task 1: `status.rs` and `scans.rs`

```rust
// status.rs
pub const LIVE_UPDATES_LIMITED: &str; // the Svelte footer's sentence
pub enum Bar { Share(f64), Unknown { beat: u64 } }
pub struct Line { pub label: String, pub bar: Option<Bar> }
pub fn scan_line(watched: &WatchedFolder, scan: &ScanProgressEvent, expected: Option<u64>) -> Line;
pub fn face_line(progress: Option<&FaceProgress>) -> Option<Line>;
pub fn export_line(progress: &ExportProgress) -> Line;
// scans.rs
pub struct Asks { pub folders: bool, pub expected: bool, pub settle: bool }
#[derive(Default)] pub struct Scans { /* scans, expected, degraded, settling, export, faces */ }
impl Scans {
    pub fn scan(&mut self, event: ScanProgressEvent, listed: bool, photos: usize) -> Asks;
    pub fn expected_read(&mut self, first: &ScanProgressEvent, photo_count: i64);
    pub fn settled(&mut self);
    pub fn folder_status(&mut self, event: FolderStatus);
    pub fn export(&mut self, event: ExportProgress);
    pub fn face(&mut self, event: FaceProgress);
    pub fn scanning(&self) -> bool;
    pub fn lines(&self, watched: &[WatchedFolder]) -> Vec<Line>;
}
```

- [ ] Tests first: `status.test.ts` whole, and the export's wording; `library.test.ts`'s
  cases about scans, the expected count, degraded folders and the export.
- [ ] Implement, add both to `STATE_MODULES`, gate, commit.

## Task 2: `empty.rs` - the library's own panel

```rust
pub enum EmptyLibrary { FirstRun, Scanning, NoPhotos, AllHidden }
pub fn empty_library(known: bool, watched: usize, scanning: bool, hidden: usize) -> Option<EmptyLibrary>;
pub fn no_photos_line(watched: &[WatchedFolder]) -> String;
pub enum PanelButton { ShowHidden, AddFolder, WatchedFolders }
pub struct Panel { pub title: &'static str, pub text: String, pub buttons: &'static [PanelButton] }
pub fn library_panel(grid: &GridState, place: &Place, state: Option<EmptyLibrary>, watched: &[WatchedFolder]) -> Option<Panel>;
```

- [ ] Tests first: `grid-state.test.ts`'s cases for the two functions; the panel only in
  All photos and Recent, only where the empty notice may be said; the scanning and the
  nothing-found states with the same buttons.
- [ ] Implement, gate, commit.

## Task 3: the views

- The status bar: the lines at its left, each with its bar, cut short before the count.
- `empty_panel.rs`: the title, the wrapped sentence, the buttons - "Show hidden photos"
  pressed, the other two drawn and inactive.
- The sidebar: "No folders yet." once the list has been read and holds none.

- [ ] Tests first, in whole frames. Implement, gate, commit.

## Task 4: the application, pictures, the checklist, CLAUDE.md, the pull request

- The folder list is kept whole: the watched folders beside the folders.
- Every event is given to `Scans`; what it asks is done off the UI thread.

- [ ] Tests first, through the application: a first scan says "Looking for photos…" from
  its first frame and has its line in the status bar, and both are gone when the photos
  are there; a library whose photos are all hidden says so and its button shows them; a
  library that watches nothing offers a folder; one whose drive is away says it cannot
  reach it; a still window draws nothing while nothing reports.
- [ ] Pictures; the checklist; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.

**Changed in the making.** The engine gained `startup_scans_started`: it publishes its
first grid before it watches the Pictures folder and starts its scans, and this window is
drawing by then. `settled`, `counted` and the launch's phases are what the native
interface needs in place of the Svelte store's round trips. "No folders yet." has its
button, drawn and inactive, as the spec gives it to this pull request.

**macOS' CI** failed the two tests that waited to see a scan in progress: on that runner
eight hundred files were read before the window had drawn its second frame. They hold
the scan at its first write now, by the library's write lock.

**The review** (one fresh reader, 2026-10-10) found nothing critical and one fault three
times: a number shown beside a thing it was not read with. A first scan was measured
against its own first batches, since the folder was counted when a frame heard of the
scan and not when the scan began ("3,000 of ~500 files" with no frame for 400 ms); the
scan says the number itself now (`ScanProgress::known`), and the count's queue is gone.
Every photo hidden at once said for a round trip that photon had found none, the hidden
count being the grid before's. A drive just back was one photon "cannot reach" for a
frame, its `online` read a frame after its scan's done. All three are fixed, with the
reviewer's tests. It also found seams for two rules this plan called unpinned - the
write lock held past the five seconds a write waits makes the one launch that sends no
event - and that the bar of a first scan, placed by the count of files, hopped backwards
as often as forwards; it steps once a report now. Left for later: a scan that panics
keeps its line and keeps an empty library looking (the engine does not say a scan died);
the panel's buttons are not clipped to a place narrower than they are; a failed read of
the folder list during the launch makes the list from before it the launch's; each of
the watcher's subtree scans reads the folder list and rebuilds the sidebar's list.

