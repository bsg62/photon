# The native dialog layer, with the confirmation and the keyword picker: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the fifth of sub-project 3's six pull requests: one dialog at a time over the
whole window; with it "Delete…" on an album and a saved search, which asks first, and
the tile menu's two keyword items, which open the keyword picker.

**Architecture:** `dialogs.rs` is what a dialog is - the two kinds there are here, their
words, and what they act on, taken when they open. `tag_picker.rs` is the picker's own
state, `createTagPicker` in Rust. `dialog_view.rs` draws whichever is open in egui's
`Modal` and answers what was done. The application holds the open dialog, shows it
before anything else in the frame, and makes what it answers a write on the queue.

**Spec:** `docs/superpowers/specs/2026-10-11-photon-native-grid-interaction-design.md`,
"The dialog layer", and the rows of "The writes" for an album and a saved search deleted
and a keyword added or removed.

Written with the user away: interfaces and tests, not the code.

## Global constraints

As the first pull request's plan, and three of this one's own:

- **Nothing here writes outside `library.db`.** A keyword added or removed is the
  library's `tag_rules` and `item_tags`, never the photo: keywords are read from the
  photo and not written to it. An album deleted is photon's own; its photos stay.
- **What a dialog acts on is taken when it opens**: the photos of the keyword picker,
  the album or search of the confirmation. The selection behind a dialog can change
  while it is up (a scan), and what the user was told it would act on is what it acts
  on. This is the opposite of a menu's rule, on purpose: a menu's items are rebuilt in
  every frame and say what they would act on *now*; a dialog's title was read once.
- **A test writes only into a library it made**, in a temporary directory.

## What egui's `Modal` does, read from the pinned source

`containers/modal.rs`, `memory/mod.rs`. An area in the foreground order with a backdrop
over the whole window that senses clicks and drags. `Memory::set_modal_layer` each frame
it is shown, and from the frame after: `allows_interaction(layer)` is false for every
layer under it - no widget there is clicked, hovered or given the keyboard, so Tab stays
inside - and `layer_id_at(pos)` answers the modal's layer wherever it is asked, which is
the question the grid and the sidebar ask of a press since the third pull request's
review. What it does not do, and this plan does: the keys. The grid, the search box and
the shell read keys from the frame's events and not through a widget.

## Not ported, and why

- **`onclosed` and the hand-back of the focus** (`TagPicker.svelte`, `App.svelte`): a
  dialog closed leaves nothing with the keyboard, and the keys are the grid's.
- **`covered` and `inert`**: `Modal` and the stripped input do that.
- **The native `ask`** of the Svelte UI: photon's own confirmation stands in for it
  until sub-project 7 decides whether the system's message box does.
- **`busy`** of the picker: it closes before its write, and a closed dialog reads no
  second Enter. (The Svelte one is on screen for a render more.)

## Rulings made in the plan

- **The dialog is shown first in a frame, and takes every key press and everything
  typed out of the frame's input after it has read them**: nothing behind it answers a
  key. A menu open when a dialog opens is closed.
- **Escape and a press on the scrim close it**, the press and not the click, as the
  menus close.
- **The confirmation opens with the keyboard on "Cancel"**; Enter and Space press the
  button the keyboard is on, and Tab moves between the two.
- **The new words** (the spec leaves them to the plan): the confirmation's buttons are
  "Delete" and "Cancel"; its titles are the Svelte `ask`'s, "Delete album" and "Delete
  saved search".
- **The keywords listed are the library's**, as the sidebar holds them
  (`Collections::tags`), in its order. Those carried by hidden photos alone are among
  them (`total`), as in the Svelte picker.
- **Deleting the album on screen goes to All photos**, as the Svelte list does.

## Review focus

1. **A write on other photos than the title said**: the ids taken at opening, a scan
   under the dialog, a view switched under it; a keyword's name as stored against the
   one typed; a confirmation for one album deleting another after the lists were read
   again.
2. **Keys and presses that reach what lies behind a dialog**: `h`, `.`, the arrows,
   Ctrl+F, Ctrl+B, Ctrl+A, Escape clearing the selection, Tab leaving it, a right-click
   through the scrim, the wheel.
3. **Two things open at once**: a menu and a dialog, the rename field and a dialog, a
   rubber band and a dialog, a dialog opened while a write's answer is on its way.
4. **The picker's list and Enter**: the first row is what Enter would do; a second
   Enter; a blank name; a keyword renamed by a rule; none in the library.
5. **A still window** with a dialog open (but for the caret).

## Task 1: `tag_picker.rs`

```rust
pub enum Mode { Add, Remove }
pub struct Submit { pub mode: Mode, pub tag: String, pub ids: Vec<i64> }
pub struct TagPicker { .. }
impl TagPicker {
    pub fn new(mode: Mode, ids: Vec<i64>) -> Self;
    pub fn mode(&self) -> Mode;
    pub fn count(&self) -> usize;
    pub fn draft(&self) -> &str;
    pub fn type_in(&mut self, text: String);
    /// The known keywords the draft narrows to: the exact name, then those that begin
    /// with it, then those that contain it, each in the library's order.
    pub fn suggestions<'a>(&self, all: &'a [String]) -> Vec<&'a str>;
    /// What to write: `tag`, or the draft. None for a blank name or no photos.
    pub fn submit(&self, tag: Option<&str>) -> Option<Submit>;
}
pub fn title(mode: Mode, count: usize) -> String;
/// "Added “lisbon” to 10 of 12 photos".
pub fn written(mode: Mode, stored: &str, done: usize, asked: usize) -> String;
```

- [ ] Tests first: `tag-picker.svelte.test.ts`'s cases; matching without case beyond
  ASCII (`Ärzte`); the words held to `TagPicker.svelte` and `tag-picker.svelte.ts`.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 2: `dialogs.rs`

```rust
pub enum Confirm { Album { id: i64, name: String }, Search { id: i64, name: String } }
impl Confirm { pub fn title(&self) -> &'static str; pub fn question(&self) -> String; }
pub enum Dialog { Confirm(Confirm), Tags(TagPicker) }
```

- [ ] Tests first: the two questions and titles, held to `FolderTree.svelte`.
- [ ] Implement, add to `STATE_MODULES`, gate, commit.

## Task 3: `dialog_view.rs`

```rust
pub enum Done { Open, Closed, Confirmed, Typed(String), Submit(Option<String>) }
pub struct DialogView { .. }
impl DialogView {
    pub fn reset(&mut self);
    /// Before anything else in the frame.
    pub fn show(&mut self, ui: &egui::Ui, dialog: &Dialog, keywords: &[String]) -> Done;
}
```

- [ ] Tests first, in whole frames, with a button and a text field behind the dialog:
  the scrim over the window and the dialog in its middle; the confirmation's question
  and buttons, the keyboard on "Cancel", Enter cancels, Tab then Enter confirms, a click
  on each; Escape and a press on the scrim close; a press on the dialog does not; the
  picker's title, its field with the keyboard, typing, the list narrowed, Enter submits
  the draft, a click on a row submits that row, a blank Enter submits nothing and stays
  open, the two lines for no keywords; no key, nothing typed, no press and no Tab
  reaches what is behind; no frame asked for while it stands.
- [ ] Implement, gate, commit.

## Task 4: the application

`App::dialog`; `SideAction::DeleteAlbum` and `DeleteSearch`, `TileAction::AddKeyword`
and `RemoveKeyword` work; `Write::{AlbumDelete, SearchDelete, TagAdd, TagRemove}` and
their answers.

- [ ] Tests first, through the application and the real engine: "Delete…" on an album
  asks, "Cancel" and Escape delete nothing, "Delete" deletes it and its photos stay;
  the album on screen deleted goes to All photos; a saved search deleted leaves the
  grid alone; a keyword added to three photos is in the Tags list with a count of
  three and the message says so; removed again; the photos are those selected when the
  dialog opened, whatever is hidden from elsewhere meanwhile, and the message says "2
  of 3"; a keyword a rule renames is stored and reported under its new name; keys and
  presses under an open dialog do nothing; a dialog closes a menu; a still window.
- [ ] Implement, gate, commit.

## Task 5: pictures, the checklist, CLAUDE.md, the pull request

- [ ] Pictures: the confirmation; the keyword picker with a few keywords and a draft;
  light and dark.
- [ ] The smoke checklist; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.
