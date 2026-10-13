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
- **The viewer's menu is of the photo on screen** (`MenuOf::Viewer(on)`), as the tile's
  is of the selection: its items are built from what the library last said of that
  photo, its picks act on it, and with nothing said of the photo - a step just made, a
  photo that is gone, the viewer shut - it has no items, and closes. (As first written
  it carried the photo's id: see "Changed in the making".)
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

## Changed in the making

- **The menu carries no photo.** The first version was `MenuOf::Viewer { photo, on }`,
  with the id checked against the viewer's and against the item's as the items were
  built, and the menu not opened unless the library had said what the photo was. No
  test could tell either check was there: no step is made under an open menu (the keys
  are the menu's, and the viewer is deaf to the wheel and the buttons), and after a
  step in the very frame the menu is asked for nothing is known of the photo, so the
  menu has no items whatever it carries. The checks went, and with them the id: the
  picks act on the photo the items were just built from.
- **The keyword picker and the person dialog are one drawing** (`Picker` in
  `dialog_view.rs`), where the plan had a second function modelled on the first: the
  two differ in their words, in the line under the field and in what stands in the
  list's place, and in nothing a press or a key does.
- **The menu is dark in both themes** (`MenuView::show_in`): a picture showed it light
  in a light window, over a viewer that is dark in both. The Svelte menu is inside the
  viewer's dark subtree; the dialog is not, and is in the window's theme in both.
- **egui's walk of the keyboard is not called off in the viewer under a dialog.** The
  viewer calls it off for itself, after the dialog is drawn. Tab egui has settled by
  then, inside the dialog (read in `Focus::interested_in_focus`); what was called off
  was an arrow's walk from one of the dialog's buttons to the next, which egui makes at
  the end of the frame: over the viewer the arrows did nothing among the people's rows.
  (Written first as "Shift+Tab wraps round the dialog's end", which was wrong, and a
  probe said so.)
- **A dialog's title is told to AccessKit** (both pickers): it is what says how many
  photos a dialog writes to, and a test of the whole application could not read it.
- **A face's plate says its name to AccessKit**, so two things on screen are called
  "Anna" while the panel is open: a test of the info panel's link that asked for the
  one thing of that name now asks for the link.
- **`Controls::over` beside `hold`**: the other button's press asks "is this on
  something that lies over the photo", which an outline is not - the menu on a face is
  the point - while the first button's asks "is this a control's", which a pressable
  outline is.
- **A face pressed in the frame that closes the viewer opens no dialog**, and a menu
  asked for in the frame an outline's click comes up in is asked for alone: neither was
  in the plan, both came of asking what two things in one frame do.
- **What the wheel had turned is dropped under a pause**, and a press held is over:
  the plan said only that the viewer is deaf.
- `paused`'s first version also forgot the first click of a double click. Nothing
  could tell: a click on the photo, a menu or a dialog opened and closed, and a second
  click on the same spot do not fit into a double click's time. The line went.

## Probes

Sixty-four, each rule broken in turn with an exact replacement, and the test named that
was to fail. Four passed the first time.

- A face that offers nothing (`On::Other`) offered nothing in the tests only because
  nobody else on their photos was named: with the photo's own "Not …" items put in its
  place every test passed. The test has a named person beside the face now.
- "The viewer is deaf under a dialog" and "what the viewer answers under a dialog is
  dropped" each passed with the other in place: the wheel's step was dropped with the
  answers, and a deaf viewer read no click. They are told apart by the wheel with Ctrl,
  which zooms and answers nothing, and by a click on the star within the dialog's first
  frame, which egui hands the star whatever the viewer reads. Each has its test.
- The keyboard's walk under a dialog: the test pressed Shift+Tab, which the rule does
  not touch. It walks with an arrow now, from one person's row to the next.

## Not tested, and why

- That the application draws the viewer's menu in the dark palette: the frame test
  holds `show_in`, and the application's choice is seen in the picture alone
  (`native-viewer-menu-light.png`) - a test of the whole application reads AccessKit,
  which has no colours.
- A Mac's Control with the first button on an outline, which asks for the menu and
  whose click is the outline's too: the two rules that are to cover it (a menu asked
  for in the frame of an outline's click is asked for alone; under the open menu the
  viewer names no face) are each tested, but not on a Mac.
- A name written reloading no picture: `picture_changed` leaves the faces out by its
  type, and the fixture's photos have no full picture to count.
- The people's read failing - *wrong, and put right by the review*: a second connection
  renames the table out from under the read.

## What the review found

An independent read of the whole branch (a fresh reviewer, its own worktree and build),
with a failing test for each finding and thirty-one probes of its own. Nothing critical:
no write that lands on the wrong photo or person through the menu, no key, wheel or
button that reaches the viewer under the dialog or the menu. Two things to fix before
merging, nine smaller, seven rules no test held. All fixed in the commit after the
feature's unless said otherwise.

**To fix before merging**

- **A face pressed in the frame of a step opened "Who is this?" over the next photo**,
  for the face of the one that was left - with a face of the next one's own in the same
  place, so that the name typed went to a photo nobody was looking at. An arrow key held
  repeats thirty times a second. The same frame with Escape opened no dialog, by this
  branch's own rule. Neither the dialog nor the menu is opened now in a frame that
  stepped (`show_viewer`'s end: only while the library has said what the photo on screen
  is). This is also what the plan's "Changed in the making" got wrong about the menu:
  whether a menu asked for in such a frame closed was a race between the library's
  answer and the next frame, and with the answer first it became the next photo's menu.
- **A click on the bar's glass, its caption or a dimmed button named the face under
  it.** egui gives a click to the last widget made under the pointer that takes one,
  whatever lies over it that takes none. "Next photo" on the last photo, with a face at
  the right edge, was the likely way in. Which outline a click is on is the view's to
  say now (`faces`' `which`): on no control, the smallest that holds the point.

**Smaller, fixed**

- The person dialog jumped when its people were read, and at every letter after (the
  keyword picker had done the second since it was built): a dialog that is a field over
  a list stands where it would at its tallest, and grows downwards.
- The line under the field spoke before the people were read ("New person “anna”", in
  a library that has her), and a read that failed said "No one is named yet.". Neither
  now: until the people are read, and when they cannot be, nothing is said of them.
- The dialog outlived a viewer that other results shut from under it; it goes with it.
- A double click on a person's row named one face and asked about another, the one
  the row had covered. An outline's click within a double click's time of a dialog
  closing asks nothing - and, by the same reasoning though the review did not ask it,
  of a menu's item being picked.
- The other button's press was on a face wherever its rectangle was, in the black
  beside the panel too; only in the photo's area now, as the first button's.
- Of two outlines one inside the other a click named the outer and the menu was the
  inner one's; both are the smaller one's.
- Two outlines of one detection (Picasa's, each given the detection beneath it) were
  one widget; an outline's widget is named by its number among the faces.
- The other button asked for the menu while the first held the slider; not while the
  first button is down at all - but for one egui still calls down after the window was
  left, which a first fix would have made into a menu that could not be asked for.
- A press in the frame after the panel opened was asked of where the faces were about
  to be drawn, not where they were: a press is asked of the last frame's faces.

**Rules no test held, each with a test now**: an outline is where the library says the
face is, the named ones first (`viewer_faces::drawn` had no test at all); "Unhide photo"
is picked, not only read; a letter and Enter in one frame through the application; an
outline is no stop of the Tab key; a face a zoom has taken out of the photo's area is
not told to whoever reads the window (the two `is_positive` checks).

**Lines that did nothing, taken out**: a second test of "is this face unnamed" where the
buttons are made; the guard that the people's answer is for a dialog still open; "and in
the photo's area" where a click is already inside a button cut to it; the test of which
kind of dialog stands over a viewer being shut (only one kind can).

**Statements that were wrong, put right**: the menu "has no items and closes" after a
step (a race, above); "so the bar and the panel are pressed where they lie over an
outline" (true of two buttons); "nothing is said in the list's place until they are
read" (the line under the field spoke); "as the shell's is" of what the viewer answers
under a dialog (the shell went by the frame's beginning, the viewer by its middle - now
both by `under_dialog`, and the frames a dialog opens and closes in have a test); the
picture's test waited 200 ms for the people instead of for them.

**Probes for the fixes**: twenty-eight, each fix broken in turn against the test meant
to hold it. One passed: the Tab stop's test looked for the outline's widget under the
name it had before the fix that renamed it, and so for nothing. It asks for the right
one now, and fails with the outline made a stop.

And the first sixty-four were run again over the fixed code, seventeen of them written
anew where the code under them had moved. One passed: "a menu asked for in the frame an
outline's click comes up in is asked for alone" pressed the other button beside the
outline, where the fixed view already names no face; it presses it on the outline now,
which is the Mac's press the rule is for.

**Found on the way, not of this branch**: `a_photo_that_cannot_be_shown_says_so_and_
holds_no_picture`, from the viewer's first pull request, failed once on Windows' CI. It
waited a second for a render that takes its turn behind every other test of its file
that holds the engine's one render lock; it waits for the answer now.

**Left**: a Mac's Control-click on an outline, not run on a Mac; the engine's side of
"two outlines of one detection" (`viewer_item` gives each Picasa outline the first
detection beneath it, not one each), read and not changed - the viewer copes with what
it is given; a very long list of people is laid out whole in every frame, as the
keyword picker's is; a screen reader's press goes through `paused`, since the viewer's
egui widgets still answer under the menu (AccessKit is not switched on yet).
