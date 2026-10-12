# Native folder drop and export: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the last of sub-project 3's six pull requests: a folder dropped on the window
is watched, and "Export…" in the tile's menu writes copies of the selected photos into a
folder whose path is typed.

**Architecture:** `folder_drop.rs` is what a drop comes to and what is said of it,
`createFolderDrop` in Rust. `export_dialog.rs` is the export dialog's state,
`createExportDialog`; it is a third `Dialog`, drawn by the dialog layer through
`export_view.rs`. The folders of a drop are watched on the queue of writes, one after
another; an export has a thread of its own. The application reads what egui says is held
over the window and dropped on it, once a frame.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"Folder drop" and "Export", the two rows of "The writes", and the first two of "Decided
while writing".

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the first pull request's plan, and four of this one's own:

- **An export writes new files, where the user said, and never inside a watched folder**:
  `Engine::export_items` refuses that destination (`check_export_dest`) and creates every
  file with `create_new`. Nothing here adds a write of its own: the native window calls
  the engine's command and no file API.
- **No watched photo is opened for writing.** A dropped folder is *read*: watching it
  writes `library.db` and photon's cache.
- **A test writes only into directories it made**: the library, the photos and the
  export's destination are all under one `tempfile::tempdir()`.
- **No new dependency but `raw-window-handle`**, which `eframe` already builds: it is
  named here only to ask what kind of window this is. No `rfd`, no `arboard`.

## What was read first

- `egui` 0.36.2, `data/input/raw_input.rs` and `dropped_file.rs`; `egui-winit` 0.36.2,
  `lib.rs`: a file held over the window is in `RawInput::hovered_files` in every frame
  until the drag leaves or ends; a dropped one is in `dropped_files` for the one frame,
  as an `Arc<dyn DroppedFile>` whose `path()` is absolute. Both are fed from winit's
  `HoveredFile`, `HoveredFileCancelled` and `DroppedFile`, one event a file.
- `winit` 0.30.13 has those three on X11, Windows and macOS and not on Wayland (the
  spec's second decision).
- `eframe` 0.36.2, `epi.rs`: `CreationContext` is a `HasWindowHandle`, and under
  `egui_kittest` there is no window (`HandleError::NotSupported`).
- `Engine::add_folder`: validates, watches, tells the watcher, starts the first scan,
  and answers a folder already watched as itself. A file, a folder that overlaps a
  watched one and a path that is not there are errors with words of their own.
- `Engine::export_items`, `check_export_dest`: the destination must exist (it is
  canonicalized) and must not be inside a watched folder. A destination that is a file
  passes the check and then fails every photo.
- `photon-engine`'s `RENDERING` is `pub`: a test that holds it holds an export at its
  first photo that has to be rendered.

## Not ported, and why

- **`refresh` awaited before the toast** (`createFolderDrop`): the folder list is asked
  for and the toast said in the same frame; the list arrives when it arrives.
- **`busy`** of the export dialog: closed in the frame it answers, it reads no second
  Enter. "One export at a time" is the application's (`App::exporting`).
- **`pick`**: there is no picker. The destination is typed, or dropped.
- **The export dialog's dropdown for the size**: five buttons in a row (below).

## Rulings made in the plan

- **Whether a drop can arrive is asked of the window, once** (`App::new`): a Wayland
  window has none, every other kind has, and no window at all - a test - has. Where none
  can arrive nothing is offered: no card, no line in the empty library's panel.
- **A drop is the window's, whatever is open**: under a menu, the confirmation or the
  keyword picker it is watched as ever. It is not a key or a press, and the Svelte window
  does the same. Under the export's dialog the first folder dropped is its destination,
  and none is watched (the spec).
- **The card is drawn while a path is held over the window**, over everything, the
  export's dialog included, where it says "Drop a folder to export into it".
- **The folders of one drop are one write** (`Write::Watch`), made in its turn on the
  queue of writes: in order, each after the one before has been answered, as the Svelte
  loop awaits each. One answer, one report.
- **The destination is a typed path, checked off the UI thread as it changes**
  (`tasks::Latest`, which answers the latest question only). It passes when it is an
  absolute path of a folder that is there and the engine will write into. A leading `~`
  is the home folder. Nothing is said of an empty field; of any other path that does not
  pass, one line under the field:
  - not absolute: "Type the whole path of a folder"
  - not there, or not a folder: "There is no folder at this path"
  - refused by the engine: the engine's own words.
- **The new words** (the spec leaves them to the plan): the field's label "Folder to
  export to", its placeholder "Path of a folder", "Choose…" beside it (inactive), the
  three lines above, and the card's two sentences. Every other string is the Svelte
  dialog's.
- **The sizes are five buttons in a row**, "Original", "3840", "2560", "1920", "1280",
  under the words "Longest edge", where the Svelte dialog has a dropdown opening upwards:
  a list over a dialog is a second thing open at once, and five choices fit a row.
- **Enter in the field exports**, once the destination has passed; before that it does
  nothing.
- **"Apply edits to the copies" is read when the dialog opens** (`Latest`); until the
  answer it is ticked, which is also what a failed read leaves. A tick is stored at once
  on the queue of writes and put back if the library will not store it.
- **The window does not wait for an export when it closes.** An export takes minutes; the
  Tauri photon does not wait either. A copy being written at that moment is left short.
- **"Export…" takes no press while an export runs**, in the menu as in the Svelte one's
  dialog.

## Review focus

1. **A folder watched that nobody dropped, or a file written where nobody said**: a drop
   under the export's dialog; a destination checked for one text and used for another;
   the check's answer arriving after the text changed, after the dialog closed, in the
   next dialog; `~`; a path with spaces at its ends; a relative path; a destination that
   is a file, a symlink into a watched folder, a watched folder itself, a folder that
   holds one.
2. **The photos of an export**: taken when the dialog opens; a scan under it; an export
   asked for while one runs; two dialogs in a row.
3. **What a drop arms in old code**: `Scans` (a folder counted as scanned for ever, an
   empty library that says "no photos" over a scan); the empty library's panel; the
   folder list; a toast for each of forty files.
4. **Keys and presses under the export's dialog**, as under the other two; Tab through
   its field, buttons and checkbox; Escape with a check on its way.
5. **A still window**: with the card up; with the export's dialog open; while an export
   runs (the status bar's line moves when the engine reports, and not between).
6. **Platforms**: what a Windows path, a UNC share and a path with a trailing separator
   come to in `leaf` and in the check; `~` on Windows.

## Task 1: `folder_drop.rs`

```rust
pub const CARD: &str = "Drop folders to add them to photon";
pub const CARD_EXPORT: &str = "Drop a folder to export into it";
pub const ASIDE: &str = "Or drop a folder onto this window.";

/// What egui says of the drag, a frame at a time.
pub struct FolderDrop { .. }
impl FolderDrop {
    /// Whether something with a path is held over the window.
    pub fn hovering(&self) -> bool;
    /// A frame's news: how many paths are held over the window, and those dropped on it.
    /// Answers the paths to act on, which is none for a drop that carried none.
    pub fn frame(&mut self, held: usize, dropped: Vec<String>) -> Vec<String>;
}

/// What came of offering a drop's paths to the engine, in the order they were offered.
pub struct Watched { pub added: Vec<String>, pub refused: Vec<String> }
impl Watched {
    /// "Watching “Holiday”", "Watching 3 folders"; none when nothing was.
    pub fn said(&self) -> Option<String>;
    /// The first refusal, with a count of the rest; none when nothing was refused.
    pub fn refusal(&self) -> Option<String>;
}
/// The last component of a path, whichever way its separators lean.
pub fn leaf(path: &str) -> &str;
```

- [ ] Tests first: `folder-drop.svelte.test.ts`'s eight cases, as far as they are this
  module's (the hovering, the two messages, the Windows leaf, the refusal's count, the
  drop that carried nothing); a path with a trailing separator; the words held to
  `App.svelte`, `Grid.svelte` and `folder-drop.svelte.ts`.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 2: `export_dialog.rs`

```rust
pub const SIZES: [(Option<u32>, &str); 5];
pub enum Dest { Empty, Checking, Passed(String), Refused(String) }
pub struct Job { pub ids: Vec<i64>, pub dest: String, pub apply_edits: bool, pub max_edge: Option<u32> }
pub struct ExportDialog { .. }
impl ExportDialog {
    pub fn new(ids: Vec<i64>) -> Self;                 // ticked, the photo's own size
    pub fn count(&self) -> usize;
    pub fn title(&self) -> String;                     // "Export 12 photos"
    pub fn path(&self) -> &str;                        // what the field holds
    pub fn dest(&self) -> &Dest;
    pub fn type_in(&mut self, text: String) -> Option<String>;   // the path to check
    pub fn checked(&mut self, asked: &str, answer: Result<String, String>);
    pub fn apply_edits(&self) -> bool;
    pub fn remembered(&mut self, apply: bool);         // the stored choice, read
    pub fn tick(&mut self, apply: bool);
    pub fn max_edge(&self) -> Option<u32>;
    pub fn size(&mut self, max_edge: Option<u32>);
    pub fn job(&self) -> Option<Job>;                  // none until a destination passed
}
/// What the field's text is as a path to ask the engine about, or why it is none.
pub fn typed_path(text: &str, home: Option<&Path>) -> Result<PathBuf, &'static str>;
/// "Exported 12 photos to /x", "Exported 10 of 12 photos to /x - why"; an error's words
/// when nothing could be written.
pub fn report(written: usize, failed: usize, reason: Option<&str>, asked: usize, dest: &str)
    -> Result<String, String>;
```

- [ ] Tests first: `export-dialog.svelte.test.ts`'s cases that are this module's; an
  answer for a text the field no longer holds changes nothing; a tick read after the
  user has ticked does not undo the tick; `typed_path` for `~`, `~/x`, a relative path,
  spaces at the ends, an empty text; the words held to `ExportDialog.svelte` and
  `export-dialog.svelte.ts`.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 3: the views

`export_view.rs` (the body of the export's dialog, called by `dialog_view.rs`), the card
(`shell.rs`), the panel's line (`empty_panel.rs`).

```rust
// dialog_view.rs
pub enum Done { .., Path(String), Tick(bool), Size(Option<u32>), Export }
```

- [ ] Tests first, in whole frames: the title, the field with the keyboard, its
  placeholder, what is typed, the line under the field in each of `Dest`'s states, the
  inactive "Choose…", the checkbox ticked and unticked by a click, the five sizes with
  the chosen one marked, the note, "Export" taking no press until a destination has
  passed and Enter doing nothing till then, both exporting after; Escape, the scrim and
  "Cancel" close; nothing behind it reads a key; no frame asked for while it stands. The
  card: drawn over the content while something is held, its words in both cases, not a
  frame asked for while it stands. The panel's line: there when a drop can arrive, not
  otherwise.
- [ ] Implement, gate, commit.

## Task 4: the application

`App::drops` (whether one can arrive), `App::drop: FolderDrop`, `Write::Watch`,
`Wrote::Watched`, `Scans::added`; `Dialog::Export`, `App::checking: Latest`,
`App::remembering: Latest`, `Write::ExportApplyEdits`, `App::exports: Queue`,
`App::exporting`; `TileAction::Export` works.

- [ ] Tests first, through the application and the real engine, in a library of real
  files (`tests/drop.rs`, `tests/export.rs`): a folder dropped is watched and scanned,
  its photos arrive, the message names it; a file dropped is refused in the engine's
  words; two folders of which the second is inside the first: one watched, one refusal;
  a folder already watched says so once and nothing changes; an empty library says it is
  looking from the frame of the drop's answer; under the export's dialog a dropped folder
  is its destination and is not watched; the card while something is held. An export of
  three photos writes three files into a temporary folder and says so; a destination
  inside the watched folder is refused under the field and "Export" takes no press; a
  path that is not there; the photos are those selected when the dialog opened; the tick
  is stored when it is made and read when the dialog next opens; "Export…" takes no
  press while an export is held at `RENDERING`, and the status bar counts; a still
  window.
- [ ] Implement, gate, commit.

## Task 5: pictures, the checklist, CLAUDE.md, the pull request

- [ ] Pictures: the export's dialog with a path that passed; the card; light and dark.
- [ ] The smoke checklist (a drop on X11, Windows and macOS; none on Wayland); CLAUDE.md;
  push; one fresh reviewer; one fix pass; merge.

## Changed in the making

- **`can_arrive` is `folder_drop.rs`'s**, not the application's: a state module can name
  the window's handle, and a test can then make a Wayland one.
- **The card tells AccessKit its words**, which the plan had not: a card that is only
  painted is invisible to a test of the whole application.
- **`Write::Watch` does not ask for the folder list.** The list is read again when the
  folder's scan ends, as after any scan, and the folder is put into the list on hand at
  once; asked for at the answer as well, the read changed nothing any test could see.
- **`TextEdit`'s own event filter** is left as it is in the path field, which wants the
  arrows and has no key of its own to claim.
- **A field's name is its label** (`WidgetInfo::label`), after the dialogs' review.
- **What is said of a refused path stands until the next answer.** As planned it went
  at every letter and came back at every answer, and the dialog, which is centred,
  moved with it. Found in a picture: the click that was to choose a size missed a button
  that had moved a frame before.
- **What the review of the fifth pull request found was asked of this one before its
  own review**: the double click on "Export…" is the dialog layer's to keep off, the
  held Enter too; the photos are taken as the dialog opens, with the risk recorded
  there.

**Probes**: a hundred and twenty-five rules broken in turn, each with the test that was
to fail. Two passed at first, and both were tests that could not see their rule: the
card's words were found inside the Svelte file's longer sentence when cut short (the
words are now held with what stands on either side of them), and a folder watched twice
was listed once again by the time the test looked (it now looks in the frame of the
answer). One call has no test through the application and is said so in CLAUDE.md:
`Scans::added` at a drop's answer, a race with no seam, whose rule is tested in
`scans.rs`. The red line round a refused field is seen in the picture and by no test.

