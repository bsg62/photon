# The native viewer, 5 of 8: faces and the person dialog

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** the faces on the photo in the native viewer are outlined and named while the
info panel is open, the viewer has its menu (the other button), a face is named and a
photo taken off a person from there, and the grid's "Add … to a person…" works - all
through one new dialog, the fourth of the dialog layer.

**Architecture:** `viewer_faces.rs` (state) is `faces.ts`: where a face is drawn, which
one a point is on, what the menu offers for it. `person_picker.rs` (state) is
`person-picker.svelte.ts` and the lines of `people.ts` it says. `viewer_menu.rs` (state)
is what the viewer's menu holds. `viewer_view.rs` draws the outlines and answers a press
on one and the other button's press; `dialog_view.rs` draws the dialog; `app.rs` opens
both and queues the writes, which are the engine's own commands (`name_faces`,
`name_items`, `remove_from_person`).

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("Faces").

## Global constraints

- Nothing here writes outside `library.db`: a name is never written to an INI or a photo,
  and none of the three commands changes Picasa's faces.
- No SQLite on the UI thread: the people the dialog lists are read by a task as it
  opens, the writes go through the queue of writes.
- State modules name no egui type and are listed in `STATE_MODULES`.
- The gate at every commit; every new test shown to fail with its rule broken.

## What was read first

- `ui/src/lib/faces.ts` and its tests; `person-picker.svelte.ts` and its tests;
  `people.ts` (`nameChoice`, `namedFaceMessage`, `namedItemsMessage`) and theirs;
  `PersonPicker.svelte`; `Viewer.svelte`'s `drawnFaces`, `.face`, `.face-name`,
  `oncontextmenu`, `faceActions`, `notPerson`, and its menu's items; `App.svelte`'s
  `locate` (a hidden photo is looked for in Hidden).
- `commands::named_people`, `name_faces`, `name_items`, `remove_from_person`;
  `ItemFace`, `UnnamedFace`, `NamedItems`, `Skipped`.
- The native dialog layer (`dialogs.rs`, `dialog_view.rs`, `tag_picker.rs`), the menu
  (`menu.rs`, `menu_view.rs`, `App::menu_items`, `open_menu`), `Write::PersonRemove`.

## Rulings made in the plan

- **A menu is of a face by what the face is, not by its number in the list**
  (`viewer_faces::On`: nowhere, an unnamed face by its id, a person by theirs, a face
  that offers nothing). The Svelte menu works its face items out once, as it opens; a
  native menu's items are built again in every frame, and the list of faces can be
  another by then (a detection landing, the photo read again).
- **The viewer's menu is of one photo** (`MenuOf::Viewer { photo, on }`), and has no
  items - and closes - once the viewer is not on that photo with the library's word for
  it: a step, a photo that is gone, the viewer shut. Its picks act on that photo's id.
- **The faces are placed in the picture as it is drawn** (the rectangle `picture`
  answers, zoom and pan in it), not in a box worked out from the photo's size: an
  outline is on what is on screen. No picture, no outline and no face under a press.
- **A face that can be pressed is a control**: a press on it is not the photo's (no pan
  begins there, it is no half of a double click), as in the Svelte viewer.
- **The viewer is deaf under a dialog and under its menu** (`ViewerData::paused`): it
  reads the wheel and the buttons from the frame's events, which a dialog does not take
  out. Under a menu the wheel steps nothing; a press outside the menu closes it before
  the viewer is drawn, and is then the viewer's.
- **The dialog has no "Cancel" button at its foot**: the keyword picker has none either
  (its cross, Escape, the scrim).
- **The list is empty until it is read** - no "No one is named yet." for the moment the
  read takes: nothing is said before the library has said it.
- **"This is Anna." says the name the library has**: the write's worker reads the
  person's name after it, where the Svelte picker goes by the list it read as it opened
  and by what was typed for a new person.
- **"Locate in photon" looks a hidden photo up in Hidden**, as the Svelte viewer does.
- "Reveal in file manager" and "Open in default app" are in their places and take no
  press (sub-project 6).

## Review focus

1. A menu or a dialog that outlives what it is of: a step under the open menu (the
   wheel), a photo purged, the viewer closed by a grid of other results, a face that is
   gone or named by the time the item is picked.
2. Keys and presses that reach the viewer behind the dialog or the menu, in the frame
   one opens and after.
3. The outlines under a zoom and a pan, beside the panel, on a turned photo; a press on
   an outline against a press on the photo; an outline under the bar or the panel.
4. What "Not Anna" and "Add to a person…" say afterwards, and that they write what
   their words said.
5. The keyboard after the dialog closes, over the viewer and over the grid.

## Tasks

### 1. `viewer_faces.rs`

`Drawn`, `drawn(item)`, `face_box`, `face_at`, `FaceAction`, `person_of`, `On`,
`on(faces, hit)`, `actions(faces, on)`, `face_actions_at`. Tests: `faces.test.ts`'s cases;
a menu's face found again, or not, in another list.

### 2. `person_picker.rs`

`Target`, `PersonPicker` (`title`, `suggestions`, `hint`, `submit`), `name_choice`,
`named_face_message`, `named_items_message`. Tests: the two Svelte files' cases.

### 3. `viewer_menu.rs`

`ViewerPick`, `viewer_menu(faces, hidden)`. Tests: the items and their order, with and
without face actions; what takes no press.

### 4. The dialog

`Dialog::Person`, drawn by `dialog_view.rs` (`people`): title, field with its button, the
line that says what Enter will do, the people. Tests in whole frames: the caret in the
field, the hint, a row, Enter with a letter in the same frame, a blank name, the list
before it is read and empty.

### 5. The viewer's view

The outlines and plates; a press on an unnamed outline; the other button's press; deaf
when paused. Tests in whole frames.

### 6. The application

`MenuOf::Viewer`, `Picked::Viewer`, `Write::Name`, `Wrote::NamedFace`, `FaceGone`,
`NamedItems`; `TileAction::AddToPerson`; `locate` with Hidden. Tests through the whole
application (`tests/viewer.rs`, `tests/menu.rs`).

### 7. The picture, the checklist, CLAUDE.md, the pull request
