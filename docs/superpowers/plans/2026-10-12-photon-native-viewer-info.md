# The native viewer, 3 of 8: the info panel

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** the viewer shows what photon knows of the photo on screen in a panel beside it,
and a keyword is added and taken off and an album ticked there.

**Architecture:** what the panel says is plain text made in state modules
(`viewer_info.rs`, `viewer_histogram.rs`: `exif.ts` and `histogram.ts` with their tests'
cases); where it stands is `viewer_layout.rs`'s (`info_room`, `photo_area`, `info_place`);
`viewer_info_view.rs` draws it and answers what was pressed in it; the application acts.
The photo is fitted beside the panel, and everything the zoom measures is measured in
that area.

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("The info
panel"; "The view": the photo's area; "Decided while writing" 5).

## Global constraints

- The writes are sub-project 3's, on the queue of writes: a keyword (`add_item_tag`,
  `remove_item_tag`) and an album's membership (`add_to_album`, `remove_from_album`).
  All of them write `library.db` and nothing else; nothing here touches a photo's file.
- Nothing that reads SQLite or a file runs on the UI thread.
- State modules name no egui type, and are added to `STATE_MODULES`.
- The gate at every commit; every new test shown to fail with its rule broken.
- A test of the whole application waits for a state, never for a length of time.
- No control of the viewer is a stop of the Tab key; the keyword's field takes the
  keyboard at a press, and gives it back.

## What was read first

- `Viewer.svelte`'s `<aside class="info">` and its styles; `toggleInfo`; where the panel
  closes by itself (the slideshow, the crop tool: later pull requests).
- `exif.ts`, `histogram.ts`, `viewer-layout.ts`, `albums.ts`, `tag-editor.svelte.ts`,
  `album-membership.svelte.ts`, `faces.ts`' two labels, `copies.ts`' label, each with its
  tests; `folders.ts`' `locateItem`.
- `commands::viewer_item`: everything the panel says is in its one answer but the albums'
  names (the collections on hand) and whether photon looks for faces
  (`Engine::face_detection`, a flag in memory; `Scans` for a pass that is running).
- `Engine::add_item_tag` and `remove_item_tag` rebuild the grid, which is what has the
  viewer read its photo again; `albums_changed` rebuilds only while an album is on
  screen.

## Rulings made in the plan

- **The panel is the viewer's while it is open, and closed at every opening.** It stays
  across a step.
- **Nothing is shown before the library has said it.** The panel says what the library
  last said of the photo, as the star does: a keyword taken off is gone when the library
  says so, a round trip later, and not before. The Svelte panel shows its own guess at
  once and puts it back on a refusal; here a refusal is a message and nothing to put
  back. A second press of the same thing before the answer is the same write again,
  which the library takes as nothing.
- **A link that leaves the viewer closes it**: a camera or a lens (its search), "Photos
  nearby", a person (their view), a copy (located in All photos, or in Hidden for a
  hidden one, selected and in sight).
- **Drawn and inactive**: "Open in OpenStreetMap" (sub-project 7), "Show N duplicates in
  the grid" (sub-project 6). No text of the panel can be selected or copied until there
  is a clipboard (sub-project 7).
- **The keyword's field has no list of suggestions.** Enter adds what is typed; the
  picker of sub-project 3 is where the library's keywords are listed.
- **The panel scrolls by the wheel**, with a mark at its edge that says where it stands
  and takes no press. A turn of the wheel over the panel is the panel's and steps to no
  photo.
- **While the keyword's field has the keyboard the viewer's keys are the field's**:
  Escape leaves the field and closes nothing.
- **The photo's own caption over the bar is not drawn while the panel is open**: the
  panel has it whole.

## Review focus

1. The photo's area: the fit, the zoom's bounds and the point it holds still, the pan, a
   double click beside the panel, the fitted copy's size, the message, the next button -
   each beside the panel and each back in the window when it closes.
2. Keys typed into the keyword's field that reach the viewer or the grid behind it; the
   field left by Escape, by a press elsewhere, by a step, by the viewer closing.
3. A write answered for a photo the viewer has left; a keyword added twice; an album
   ticked twice before the first answer; a Picasa album.
4. A link pressed while a step is on its way; a copy that has gone; a hidden copy.
5. A still window with the panel open; the panel taller than the window, and scrolled
   when its content gets shorter.

## Tasks

### 1. State: `viewer_info.rs`, `viewer_histogram.rs`, `viewer_layout.rs`

The rows (`camera_rows`, `date_rows`, `copy_groups`, the labels and formats), the
histogram's columns, the panel's room and place. Tests: the Svelte modules' cases.

### 2. The photo's area

`ViewerData::info` (whether the panel is open) moves the photo's area; the view measures
everything in it. `I` and a button in the bar answer `ViewerAction::Info`; the
application keeps `Viewing::info` and asks for the fitted copy by the area.

### 3. The panel: `viewer_info_view.rs`

Drawn from `InfoData` (the photo as the library says it, the albums, whether faces are
looked for). Answers `InfoAction`: `Search`, `Person`, `Locate`, `AddTag`, `RemoveTag`,
`Album { id, on }`. Frame tests: every section present and absent; each link's answer;
the chips and their removal; the field; a tick; the scroll.

### 4. The application

`Write::TagAdd`, `Write::TagRemove`; the album writes with the photo's id; what the
library says read again after each; `Nav`'s jump to a photo (`Target`) for a located
copy; the links through `App::go` with the viewer closed first.

Tests (`tests/viewer.rs`, real photos): the panel opens by its key and its button and
the photo moves over; a keyword added and taken off, in the library; an album ticked; a
camera's link; a copy located; a still window.

### 5. Pictures, the checklist, CLAUDE.md, the pull request

`native-viewer-info-{light,dark}.png`.

## Changed in the making

- **A copy is found in All photos from Hidden too.** The plan had "or in Hidden for a
  hidden one", after `locateItem`. No copy the panel lists is hidden (`copies_of` leaves
  them out, and lists none for a hidden photo), so there is no such case - but there is
  its opposite: a photo unhidden in the viewer stays on screen in Hidden, with its copies
  listed. Looked up as a folder is, in the grid on screen, the press closed the viewer and
  found nothing; the test came first.
- **The jump is `Nav`'s own, with a `Target`**: a folder or a photo. A photo located waits
  for the grid of the step to All photos exactly as a folder clicked from an excursion
  does, and is taken in the frame that draws that grid, by its id.
- **The bar keeps its width while the caption is not known.** At a step the caption is
  empty for the round trip of the library's answer; the bar shrank to its two tools and
  its buttons jumped twice at every step. Seen when a test pressed the panel's button a
  frame after a step and missed it.
- **The field's keyboard is given up in the first frame that finds the viewer shut**, in
  one place, and not where the viewer closes: it is shut in two places, and one of them
  (`adopt`, on a grid of other results) has no context at hand. egui lets go of a field
  that is no longer drawn by itself, a frame late, and the grid is deaf for a frame more.
- **Enter leaves the keyboard in the field**, for the next keyword; Escape leaves it.
- **The wheel over the panel moves it forty points a notch**, whatever the system's
  lines: nothing in the panel is far away. (As first built it read egui's
  `line_scroll_speed`, which this line called "egui's own distance" - and which the
  application sets to the system's notch in every frame. The review: below.)
- **The panel's data is handed to the view whether or not the panel is open**: a filter
  on the way did nothing, the view asking for it only where it draws the panel.
- **What a photo with no face says** needs two things the plan had not named as wiring:
  `Engine::face_detection` and `Scans::looking_for_faces`. In a library where photon
  looks for faces the launch's pass is still recognising them for a moment, and the
  panel says "yet" until its last report - which a test of the line has to wait for.
- **A fixture's photos have no camera and no copy**, so the tests of those links write
  the columns themselves before the launch.
- **A keyword's field scrolled out of sight keeps the keyboard.** Everything else of the
  panel that is out of sight is not handed to egui; the field was among it at first, egui
  lets go of a field that is not drawn, and the next letter typed was the viewer's key.
- **The field's placeholder could not be read in a light window**, seen in the
  picture: egui paints a placeholder in the theme's "weak" text colour whatever colour
  its words are given, and the window's theme is not the viewer's glass's. The field is
  drawn in the dark theme's visuals, and `theme::install` gives placeholders the
  palette's dim text - which every field here had asked for and none had got.

**Probes**: a hundred and sixty-one, each rule broken in turn with the test that was to
fail. Fourteen passed the first time, and each was something:

- *Tests that could not see their rule, and were made to*: a shutter speed is rounded to
  the whole number it is read as (every case was exact); two people of one name are two
  (by their keys); another photo begins with an empty field at the top (the test looked
  for the field where it was out of sight, and scrolled a panel that had nothing to
  scroll - it saw neither of its two rules); a press on the field itself does not leave
  it (a click gave the keyboard back, so only the press shows it); the panel's button in
  the bar is lit while it is open; the keyboard is the grid's in the frame after the
  viewer closed over a keyword (twice: a key two frames on is answered either way, and a
  button let go of is a click elsewhere, at which egui takes the keyboard itself).
- *A rule with no test*: things side by side go on to the next line.
- *Lines that did nothing, and went*: Escape taken out of the frame by the field (the
  viewer has read the frame's keys by then, and reads none while a keyword is typed);
  a guard for a first thing too wide for its line (nothing is wider than the column it
  is wrapped to); "unless All photos is where the user is" before a copy is located
  (`go` asks for no step to where the user is, and drops a search half typed, which is
  wanted); a filter that handed the panel's data over only while it was open.
- *A test sharpened until it found a bug*: "a link cut off by the panel's edge is not
  pressed where it is not seen" pressed twenty points away from the link and passed
  without the cut. Pressed three points off, in the panel's margin, it failed with the
  cut in place: egui hands a press that hits nothing to a widget within five points.
  That is what the panel's ground is for, which a probe had just shown to "do nothing" -
  in the viewer, where the photo's surface lies under the panel. It stays, with its
  reason, and that test.
- *A probe that passed for the wrong reason*: taking the "is it in sight" test off a
  link changed nothing a test could see, and all of those tests were taken out - until
  a test of the whole application found a line that was scrolled out of sight. They are
  what keeps such a thing from AccessKit; the link probed was in a row guarded twice.
  They are back, and taking them out did show what the field's own had cost (above).

## What the review found

An independent read of the whole branch (a fresh reviewer, its own worktree and build),
with a failing test for each finding: twenty-seven tests, and twenty-three probes of
which sixteen passed. Nothing it called critical. Fixed in the commit after the
feature's, each with the reviewer's test or one of its kind.

**To fix before merging**

- **What the panel wrote went to the photo stepped to in the same frame.** The frame's
  steps were acted on before the panel's presses, and the writes went by the viewer's
  photo: a tick with an arrow key in its frame was the next photo's, whose panel nobody
  had seen; a tick in the frame of the Escape that closes was not written. The grid's
  rule for a key that writes, missed here. They are written first now, to the photo the
  panel was drawn for.
- **Who had the keyboard after a step of the wheel from the keyword's field was a
  race.** A step leaves nothing to draw until the library has answered, egui lets go of
  a field that is not drawn, and so the field kept the keyboard when the answer came
  within a frame and lost it when it did not - "hills" typed next was a keyword, or `h`
  hid the photo. The keyboard is the field's by the panel's own word now, across the
  frames with nothing to draw. (The reviewer's test asked only that it be one or the
  other; the Svelte viewer always loses it. Keeping it is the choice that hides no
  photo, and lets a keyword be typed for photo after photo.)

**Smaller, fixed**

- The field shared one undo history across photos, as the sidebar's field and the
  picker's once did.
- Everything laid out "to break anywhere" was cut in the middle of a word: egui breaks
  an over-long word by itself, and told to break anywhere never looks for a word's end.
- A link to where the user already is closed the viewer without handing its photo back.
- A notch of the wheel moved the panel by the system's lines, not by "egui's own" as
  this plan said; and by none at all at a Windows set to scroll none.
- A keyword the photo already has was written again, and the grid built anew for it.
- A position that is a tie in binary was rounded to the even digit, where `toFixed`
  rounds it up.
- Only the first button's press left the field.
- Tests for five rules its probes found none for: the field's arrow keys, Enter while
  the field has not the keyboard, a trackpad's points, the make found in the model in
  whatever case, and more kinds of thing scrolled out of the panel.

**A test of the zoom's that the gate failed once**, older than the review's findings
and of none of them: "a star keeps the zoom and another picture is fitted again" asked
for the zoom the moment the turned picture had landed. The picture and what the library
says of the turned photo are two answers, from two tasks, and the zoom is put back by
the second: four runs in twenty-four on the branch as pushed read 1.25. It waits for
both now (none in twenty-four); for the frames between, the turned picture is drawn at
the old zoom, which is left as it is.

**Statements that were wrong, put right**: the notch (above); "the caret is the same in
both of egui's themes" - it is not, and the first version of the test that said so had
been right; "the photo is not handed back, the grid being left too"; "there is nothing
to put back" after a refusal; what `wrap` and `leave` said of themselves.

**Found and left, for the user to weigh**

- A double click on a copy's link lands its second click on the grid, and selects the
  photo under it in the copy's place.
- A letter in the very frame of the click that gives the field the keyboard is the
  viewer's key too.
- "lisbon" typed where the library has "Lisbon" on other photos writes a second
  keyword; the picker writes the library's.
- A keyword the library refuses is not given back to the field.
- A field scrolled out of sight has the keyboard and no caret to be seen.
- A name in a right-to-left script is turned round in the panel and right in the bar.
- No test draws the scroll mark or the histogram's columns, shows a video's "Length",
  or reads "No faces found yet." through the whole application.
- Older than this pull request: a write refused by another connection's lock is refused
  at once where it reads before it writes, not after the five seconds a write waits;
  and a wheel delta that is not a number panics inside egui.
