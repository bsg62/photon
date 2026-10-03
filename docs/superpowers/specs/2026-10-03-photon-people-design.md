# People: recognising and naming

2026-10-03. Designed in conversation the same day and approved. The backend (plan 1) is built
on `feat/people`; where it departs from this text, "As built" near the end records what the
code does, and the data model below is the shipped schema.

## What it is

photon sorts the faces it finds into groups, one per person as far as it can tell, and the user
names them. A named group is a person: it appears in the People list, has a Person view, and
answers `person:` in search, alongside the people Picasa recorded.

This is stages 2 and 3 of the plan that began with face detection
(`2026-10-01-photon-face-detection-design.md`, released in 0.47.0). They are one spec because
neither is any use without the other.

It rides the existing "Find faces" switch. On means find, recognise and group; off deletes all
of it.

## What the user gets

- A **People page**, opened from the sidebar, with the faces photon found sorted into unnamed
  groups, each waiting for a name.
- **Naming** a group makes it a person. A name Picasa already uses is offered when the group's
  faces are ones Picasa named.
- **Suggestions**: a new face that looks like someone already named is offered for that person,
  to confirm or reject.
- **Corrections**: not this person, merge, ignore, rename, delete.
- Named people in the People list, the Person view, `person:` search and the viewer's name
  plates.

## The second spike

A throwaway spike on 2026-10-03 measured the two things the design rests on. It used photon's
shipped detector (0.47.0), so the landmarks are the real ones, and SFace
(`face_recognition_sface_2021dec.onnx`, 38.7 MB, Apache 2.0, from OpenCV's model zoo) through
`tract`, on the LFW dataset. Similarity is the cosine of two face vectors.

### How small a face can be

The same faces shrunk into a 1600 px picture, as a face in a group photo appears in a preview,
detected there, and compared with the same people at LFW's own size (about 100 px):

| Face width | Detected | Same person matched at 0.45 | Strangers matched at 0.45 |
|---|---|---|---|
| ~100 px | all | 93.6% | 0.003% |
| 80 px | 574 of 574 | 94.7% | 0.003% |
| 60 px | 574 of 574 | 94.9% | 0.003% |
| 45 px | 574 of 574 | 93.0% | 0.004% |
| 35 px | 572 of 574 | 90.8% | 0.002% |
| 25 px | 545 of 574 | 80.2% | 0.002% |
| 18 px | 370 of 574 | 57.8% | 0.003% |

Two faces of 18 px compared with each other also match strangers at 0.6% at the looser 0.363.
Recognition holds to about 35 px and falls off below. **A face under 35 px wide in the preview
is not recognised.**

### Which grouping rule

2,674 faces: 500 people with up to 12 photos each (1,974 photos) and 700 strangers with one.
"Misplaced" is a face in a group that is mostly someone else; "whole" is a person who ended in
exactly one group.

| Rule, threshold | Misplaced | Whole (of 500) | Largest group |
|---|---|---|---|
| Single link, 0.363 | 2,434 | 496 | 2,438 |
| Single link, 0.55 | 2 | 422 | 12 |
| Complete link, 0.45 | 8 | 437 | 12 |
| Average link, 0.45 | 9 | 463 | 12 |
| Average link, 0.50 | 3 | 440 | 12 |
| Chinese whispers, 0.50 | 6 | 457 | 12 |
| Nearest group so far, 0.45 | 27-40 | 468-469 | 13-20 |
| **Nearest group so far, 0.50** | **6-11** | **446-448** | **12** |
| Nearest group so far, 0.55 | 2 | 411-414 | 12 |

"Nearest group so far" was run in three random orders. Two faces are misplaced under every rule
at every strict threshold, which points at the dataset's labels.

### What follows

- **The threshold is 0.50.** Below it mixed groups rise quickly; above it people split for
  little gain.
- **The rule is "nearest group so far".** It is within a few faces of the best batch rules, and
  it is the only one that fits. The batch rules compare every face with every other, which at
  100,000 faces is tens of gigabytes, and they regroup from nothing each time, so a group would
  change identity under the user between passes. Adding each new face to the nearest existing
  group keeps groups where they are and costs one comparison per group.
- **Splitting is the cheap error.** About one person in ten lands in two groups at 0.50; a
  merge fixes that in one action. A mixed group needs faces removed one at a time.

### What the spike did not show

LFW is press photographs of adults facing the camera. Children, the same person decades apart,
profiles and partly hidden faces will split into more groups than these numbers say. Nothing
was measured on a family library.

## Decisions

- **Groups first, like Picasa.** photon groups; the user names; later matches are suggestions.
- **One list of people.** A person is one entry whether Picasa or the user named them. The link
  between a photon person and a Picasa contact lives in photon's library; Picasa's tables stay
  a read-only mirror of the INI.
- **Only confirmed faces carry a name.** A face photon matched to a named person is a
  suggestion until confirmed and is counted nowhere but on the People page.
- **Incremental grouping** at 0.50, faces under 35 px left out, as measured.
- **Both models on the CPU through `tract`.** GPU inference was considered and left out: no one
  route covers three platforms, it cannot be tested in CI, and it speeds up only the first run.
- **The embedding model is bundled.** No download, no network access.
- **Switching off deletes everything**, people the user named included, after a confirmation
  that says how many.
- **Names are never written** to `.picasa.ini` or to the photos.

## Data model (schema 25)

### `people`

```sql
CREATE TABLE people (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT,
    ignored INTEGER NOT NULL DEFAULT 0
);
```

A row is a set of faces photon takes for one person. `name` is `NULL` for an unnamed group.
`ignored` marks a group the user does not want to name.

A group's centroid is the sum of the vectors of the faces that count towards it, and how many.
For an unnamed or ignored group every member counts. For a named person only confirmed faces
count, so suggestions cannot drag the person towards themselves. *As built:* the centroid is
not a column. It is computed from the faces at the start of each grouping step (see "As
built").

### `person_contacts`

```sql
CREATE TABLE person_contacts (
    contact   TEXT PRIMARY KEY,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE
);
CREATE INDEX person_contacts_person ON person_contacts(person_id);
```

Links a person to Picasa contact hashes. A table, not a column: Picasa libraries can hold two
contacts for one human, and merging two people each linked to one must work. A contact belongs
to at most one person.

### `detected_faces`, rebuilt

Migration 25 rebuilds the table migration 24 created (released in 0.47.0), keeping every row
and its id, because `AUTOINCREMENT` cannot be added to an existing table:

```sql
CREATE TABLE detected_faces (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id           INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    left              REAL NOT NULL,
    top               REAL NOT NULL,
    right             REAL NOT NULL,
    bottom            REAL NOT NULL,
    landmarks         BLOB NOT NULL,
    score             REAL NOT NULL,
    embedding         BLOB,
    embedding_version INTEGER,
    person_id         INTEGER REFERENCES people(id) ON DELETE SET NULL,
    confirmed         INTEGER NOT NULL DEFAULT 0,
    ignored           INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX detected_faces_item ON detected_faces(item_id);
CREATE INDEX detected_faces_person ON detected_faces(person_id);
```

The new columns:

| Column | Meaning |
|---|---|
| `embedding BLOB` | 128 little-endian `f32`, normalised. `NULL` until computed, for a face too small, and for one the embedder failed on. |
| `embedding_version INTEGER` | The `face_embed::EMBEDDER_VERSION` that looked at the face, set even when it was too small. `NULL` for not looked at. |
| `person_id INTEGER REFERENCES people(id) ON DELETE SET NULL` | The group or person, or `NULL`. |
| `confirmed INTEGER NOT NULL DEFAULT 0` | The user put the face there, by naming its group or confirming a suggestion. |
| `ignored INTEGER NOT NULL DEFAULT 0` | This face is not to be grouped or suggested. |

### `face_rejections`

```sql
CREATE TABLE face_rejections (
    face_id   INTEGER NOT NULL REFERENCES detected_faces(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    PRIMARY KEY (face_id, person_id)
);
```

"Not this person." The face is never put in that group again.

### Size

512 bytes per face, for the vector. 100,000 faces add about 50 MB to `library.db`.

### What clears what

- **An edit or a changed file** deletes the photo's detections, as in 0.47.0, and with them
  their membership, confirmations and rejections. Re-detected and re-embedded, the faces are
  grouped again, so a face confirmed as Anna comes back as a suggestion for Anna. A recorded
  limit: carrying a confirmation across a turn or a crop means mapping rectangles through the
  edit, for a case one click fixes.
- **A detector re-run on an unchanged picture** (a `DETECTOR_VERSION` bump) must not do that to
  a whole library. When `write_face_batch` replaces a photo's rows, a new face inherits
  `person_id`, `confirmed`, `ignored` and the rejections of the old face at the same place,
  by `face_detect::merge::same_face`. Its embedding is computed afresh. *As built:* best
  overlap first, and the old vector is carried until then (As built 12).
- **A group left with no faces** is deleted, unless it is named or linked to a contact.
- **Switching off** deletes `people`, `person_contacts` and `face_rejections` with the
  detections, in the transaction that stores the setting.
- **A renamed or moved file or folder** is a purge and a new row, as for albums and edits, so
  its faces are detected again and lose their confirmations. A recorded limit.
- **A named person whose confirmed faces are all gone** - rejected, cleared by an edit, or
  purged - has no average, and draws no suggestions until a group is named for them again
  (or merged into them). A recorded limit.

### Tripwires

The literal schema version and the table count in `library/mod.rs` move to 25 and three more
tables. Each new whole-library read has a plan test.

## The embedder: `photon_core::face_embed`

Like the detector, it knows nothing about the library.

```rust
pub const EMBEDDER_VERSION: i64 = 1;
pub const MIN_FACE_PX: f32 = 35.0;

impl Embedder {
    pub fn new() -> Result<Self>;
    /// `None` for a face narrower than `MIN_FACE_PX` in `image`.
    pub fn embed(&self, image: &DynamicImage, face: &FaceBox) -> Result<Option<[f32; 128]>>;
}
```

`FaceBox` is the rectangle and five landmarks as fractions of the image, which is what
`detected_faces` stores.

### One face

1. The face's width in pixels is its rectangle's width times the image's. Under `MIN_FACE_PX`:
   `None`.
2. Find the similarity transform (rotation, uniform scale, shift) that takes the five
   landmarks, least squares, to the model's reference points in its 112x112 input:
   (38.2946, 51.6963), (73.5318, 51.5014), (56.0252, 71.7366), (41.5493, 92.3655),
   (70.7299, 92.2041).
3. Sample the 112x112 input through the inverse transform, bilinearly, black outside the
   image. Channels are RGB, values 0 to 255, as OpenCV's `FaceRecognizerSF` feeds the model.
4. Run the model, and divide the 128 outputs by their length. A vector with a non-finite value
   is an error.

Comparing two faces is then one dot product.

### The model file

`crates/photon-core/models/face_recognition_sface_2021dec.onnx`, embedded with
`include_bytes!`, credited in `models/README.md` and `THIRD-PARTY-NOTICES.md` (Apache 2.0).

`EMBEDDER_VERSION` changes when the model, the reference points, the sampling or
`MIN_FACE_PX` changes, which embeds every face again.

## Grouping: `photon_core::people`

A pure rule and the library operations that apply it.

### The rule

A face that has an embedding, is not ignored and has no group is given one:

1. Compare it with every group's centroid, by cosine. Skip a group the face has rejected, and a
   group whose `centroid_count` is zero.
2. If the best is at least `GROUP_SIMILARITY` (0.50), the face joins that group, unconfirmed,
   and where the group is unnamed or ignored its vector is added to the centroid.
3. Otherwise it starts a new unnamed group, whose centroid is the face.

Faces are taken in id order and the step runs on one thread, so the outcome does not depend on
which worker finished first.

An ignored group takes part like any other: a stranger who recurs is absorbed without asking
again.

### Picasa's names offered for a group

For an unnamed group: among its faces, take those that are the same face as a Picasa face with
a named contact on the same photo (`merge::same_face`, the Picasa rectangle mapped through the
photo's edit by `merge::shown`). If one contact accounts for more than half of them, the group
is offered that contact's name.

Nothing is linked by this. Accepting the offer is naming the group. *As built:* see As built 4.

## The operations

Each is a function on `Library` in one transaction, a command in the app, and takes the
engine's `people_write` lock, which the grouping step also takes. Each ends by rebuilding
through `refresh_after_write`.

- **Name a group.** An empty name is refused. If the name, compared without case in Rust, is
  the name of a person, the group is merged into that person. If it is the name of a Picasa
  contact no person is linked to, the group becomes a person with that name linked to every
  unlinked contact of that name. Otherwise the group becomes a person. In every case the
  group's faces become confirmed and the centroid is recomputed from them.
- **Confirm** faces: `confirmed = 1`; the person's centroid gains them.
- **Not this person**, for faces: record the rejection, take the face out (and out of the
  centroid if it counted), and group it again by the rule, which now skips that group.
- **Merge** A into B, where B is a named person: A's faces move to B keeping `confirmed`; A's rejections and contact links
  move to B; B's centroid is recomputed; A is deleted. Merging an unnamed group into a person
  confirms its faces, as naming does. *As built:* a face of A rejected from B is left
  ungrouped instead (As built 11).
- **Ignore** a group, or undo it: set or clear `people.ignored`. Ignoring a named person is
  refused; delete them first.
- **Ignore** faces, or undo it: set `ignored`, clear `person_id` and `confirmed`, and take the
  face out of its centroid. Undoing clears `ignored`, and the face is grouped by the rule.
- **Rename**: as naming; a taken name merges. *As built:* a rename does not confirm the
  person's suggestions (As built 3).
- **Delete** a person: the row becomes an unnamed group. `name` is cleared, its contact links
  are removed, its faces become unconfirmed, and its centroid is recomputed over all of them.

**Contacts are linked by name.** When a person is named or renamed, and when
`upsert_contacts` records a contact, an unlinked contact whose name equals a person's is linked
to them. The user typing a name Picasa uses is the user saying it is the same person; that is
the same rule naming a group applies.

There is no undo beyond these.

## The pass

`run_face_pass` gains two steps after detection, behind the same switch, lock and counter.

### Embed

1. List photos, in id order from after the last id read, that have a face whose
   `embedding_version` is not current, with each such face's id, rectangle and landmarks, and
   the photo's thumbnail key.
2. Workers decode each cached preview once and embed its faces. Same worker count as detection.
3. One transaction a batch writes `embedding` and `embedding_version`.

A separate step from detection, at the cost of decoding the preview again for photos with
faces: a library already detected by 0.47.0 gets embeddings without being detected again.

About 55 ms a face: 150,000 faces take about 35 minutes on four workers.

### Group

After each embed batch is written, and once at the start of the step for faces left ungrouped
by an earlier pass or an operation: every face with an embedding, no group and not ignored is
grouped by the rule, in id order, in one transaction a batch, under `people_write`.

The centroids are read once at the start of the step into memory and written back with each
batch.

*As built:* the centroids are computed from the faces at the start of each run and nothing is
written back (As built 1); during the embed step the runs are paced rather than one per batch
(As built 6); and the run for faces left ungrouped comes at the end of the pass, after the
embed step, whenever a face with a vector has no group and is not ignored.

### Guards

- Nothing is written with the switch off, read inside the write's transaction.
- A face whose row is gone, or whose photo's thumbnail key moved since it was listed, is
  skipped.
- The breaker covers embedding: a batch in which every face embedded failed, at least 8 of
  them, is not written and ends the pass.
- A preview that cannot be read is skipped and its faces stay candidates.

### The refresh chain

Grouping does not change what the People list, the Person view, `person:` or the viewer read:
they read confirmed faces only, and grouping writes none - only suggestions, new unnamed groups
and the removal of emptied ones. Those are read by `people_page`, and the People page and its
sidebar count of groups to name refetch on `data_changed`. So, unlike detection, grouping is
announced as a data change: the step rebuilds through `refresh_grid`, which marks `data_dirty`
and moves `counts_epoch`. Once when the pass ends if anything was grouped, and during the pass
at most every 30 seconds.

### Progress

`FaceProgress` gains `phase`: `detecting` or `recognising`. In the second, `checked` and
`total` count faces with a current `embedding_version` and all faces on live images. The status
line reads "Recognising people: 1,200 of 8,000 faces".

### Upgrade

A 0.47.0 library with the switch on starts embedding at first launch. The user opted in to
finding faces; the Settings text is updated to say what that now includes, and the release
notes say it plainly.

## Readers

### Hidden and missing photos

A face on a hidden or missing photo keeps its group, so unhiding is instant, but it appears
nowhere: not in a strip, a count, a suggestion, or as a group's only visible face. Every query
below filters on `hidden = 0` and `missing_since IS NULL`, and a test pins each, as
`library/hidden.rs` does for the views.

### The key of a person

The Person view's argument, `GridInfo.person` and the People list's entries use one string:
`p:<id>` for a photon person, or `c:<hash>` for a Picasa contact no person is linked to. Both
are prefixed, so nothing depends on what a contact hash may contain. An argument with neither
prefix names no one and gives an empty grid, as an unknown album id does.

### The Person view and `person:`

A person's photos are those with a confirmed face of theirs, and those where Picasa recorded a
contact linked to them. `person:` matches the names of both. The driver of the grid query is
filtered the same way as the outer `WHERE`, as for every membership view.

### The People list

Named people, with a count of distinct visible photos by the rule above, and Picasa contacts
linked to no person, as today. Sorted by name in Rust.

### The viewer

`ViewerItem.faces` gains the photo's confirmed faces of named people, with the name and the
person's key. A detection confirmed as a person is no longer in `unnamed_faces`. Where Picasa
recorded the same face under a linked contact, it is shown once. *As built:* under any named
contact, linked or not, and over an unnamed Picasa face too (As built 8).

Unnamed outlines stay as they are and are not clickable.

## The People page

Clicking "People" in the sidebar opens it in the main area, in place of the grid. It is a view
of its own in the UI's navigation, not a `GridView`: it has no grid rows. *As built:* drawn over
the grid, which stays mounted beneath it (As built 18).

Four sections:

1. **Unnamed.** Groups of two or more faces, largest first. Each is a strip of faces, the first
   of them with "Show all N", and a name box. Where Picasa's names point at a contact the box
   holds that name with "Yes, this is Anna" and a line saying why. Groups of one face are
   collected at the end under "N single faces", closed by default. *As built:* "Show more (N
   left)", 200 faces a click, and only the 200 largest groups listed (As built 19).
2. **Suggestions.** For each named person with unconfirmed faces: those faces, dashed, with
   "Confirm all".
3. **People.** Each named person: a strip of confirmed faces, Rename, "Merge into…", Delete.
4. **Ignored.** Closed by default; groups and faces, each with "Stop ignoring".

In any strip, clicking faces selects them and shows an action bar in that strip: "Not this
person" and "Ignore these faces" (and "Confirm" in Suggestions). Double-clicking a face opens
its photo in the viewer.

Typing a taken name into a group's box merges; the box says so before the name is committed
("Add to Anna").

The sidebar's People row shows the number of unnamed groups of two or more. Clicking a person's
name opens the Person view, as today.

### Loading

The page asks for the sections with the first faces of each strip. "Show all" pages through a
group's faces. The page refetches on a library change that carries `data_changed`. *As built:*
"Show more", and at most once a second (As built 19).

### Face crops

A new route, `/face/<face id>/<thumb key>`, serves a square crop around the face from the
cached preview: the rectangle widened by 30% a side, clamped to the picture, scaled to 96 px.
Rendered on request, not cached on disk; the response is `immutable`, since a face id with its
photo's thumbnail key names one picture. No photo file is read.

### Logic and markup

The page's behaviour (selection per strip, the action bar, paging, the name box's choice
between creating and merging, optimistic removal of a face acted on) is a factory in a
`.svelte.ts` module tested with the client runtime, like `createCropTool`. The component holds
markup and effect wiring. *As built:* tested with the server runtime (As built 20).

### Switching off

When named people exist, switching "Find faces" off first asks, in a dialog rendered in
`App.svelte` beside Settings: "This deletes N people you named and everything photon found."
The count comes from a `face_data_summary` command. With none, it switches off as today.

## IPC

Commands, each through `commands.rs`, `ipc.rs`, `app.rs`, `api.ts` and `mock.js`:
`people_page`, `person_faces` (a page of one group's faces), `name_person`, `confirm_faces`,
`reject_faces`, `ignore_person`, `ignore_faces`, `merge_people`, `rename_person`,
`delete_person`, `face_data_summary`. `set_person_view` takes the key.

## Build

The model adds 38.7 MB to the binary. `include_bytes!` of a file that size is checked on all
three CI platforms by the plan's first task, as `tract` was.

## Documentation

- `CLAUDE.md`: the two new pass steps and that grouping is a data change; `people_write`; the
  rule and its two constants with their evidence; only confirmed faces carry a name; the key
  of a person; what clears what, and the carry-over at a detector re-run; hidden photos in the
  People queries; the bundled model.
- `README.md`, the website, `THIRD-PARTY-NOTICES.md`, `models/README.md`.
- `docs/smoke-checklist.md`: the People page end to end on a real library.

## Testing

Every new test is shown to fail with its change reverted.

**The embedder**

- The transform: landmarks already at the reference points give the identity; the same
  landmarks rotated, scaled and shifted give the transform that undoes it.
- A face under `MIN_FACE_PX` gives `None`; one just over gives a vector.
- On the real model: the fixture portrait and a resized, re-encoded copy of it are above 0.9;
  the portrait and a second CC0 portrait of another person are below 0.363. This shows the
  pipeline is wired correctly. The evidence that recognition is good is the spike's tables.

**The rule**

- Joins at 0.50 and not just below; takes the nearer of two groups; skips a rejected group;
  skips a group with no centroid; an ignored group absorbs; a named person's centroid does not
  move for an unconfirmed face.
- The Picasa offer: more than half; exactly half offers nothing; a face cropped away does not
  count.

**The operations**, each on real rows: name; name into an existing person; name into an
unlinked contact; confirm; reject and regroup; merge with both sides' links and rejections;
ignore and its undo, for a group and for a face; rename; delete; contacts linked by name on
`upsert_contacts`.

**The clearing rules**: an edit sends a confirmed face back to a suggestion; a detector re-run
carries confirmation over by position; an emptied unnamed group is deleted and a named one is
not; switching off deletes all four tables' rows.

**The pass**: embeds a 0.47.0-shaped library without detecting again; groups in id order
whatever the workers' order; the guards; the breaker; grouping announces a data change.

**Readers**: the Person view and `person:` over confirmed faces and linked contacts, and not
over suggestions; the People list's counts; hidden and missing photos in every People query;
the viewer's names.

**The page's factory**: selection, the action bar's states, paging, create-or-merge, removal.

**What has no test**: the page's markup and the switch-off dialog, covered by `svelte-check`,
new screenshots of the page in both themes and of the dialog, and the smoke checklist.

## Shape of the work

Two plans and two pull requests, one release.

1. **Backend**: schema 25, the embedder, the pass's two steps, the rule, the operations and
   their commands, the readers.
2. **The People page**: the face-crop route, the page and its factory, the sidebar, the
   switch-off dialog, the screenshots.

Nothing is released until both are merged: the first alone groups faces with no way to see or
name them.

## Rollout

Schema 25 makes the release a minor, and an older photon refuses the library.

The release notes say: recognition starts by itself where "Find faces" is already on; nothing
leaves the computer; the installers' measured growth; a first run's time; that this was
measured on press photographs, and children and photos decades apart will split into more
groups; that nothing was run in the app by a person.

## As built

The backend (plan 1) was built on 2026-10-03. Where it departs from the text above, the code
is what is recorded here; each was decided during the work, after review.

1. **Centroids are not stored.** No `centroid` or `centroid_count` column: at the start of each
   grouping step, every group's sum is computed from its faces by the counting rule above and
   kept in memory for that step. A stored centroid has to be kept right by every writer that
   moves a face (every operation, the clearing on an edit or a changed file in `items.rs`, the
   switch, the carry-over at a detector re-run), and one forgotten writer leaves it silently
   wrong, which no test of that writer would show. Computing it costs one read of the grouped
   faces' vectors when there is something to group, and nothing when there is not. So "the
   centroid is recomputed" in the operations means nothing is written: the next step reads the
   faces as they now are, and "a group whose `centroid_count` is zero" in the rule is a group
   nothing counts towards.
2. **`people.id` and `detected_faces.id` are `INTEGER PRIMARY KEY AUTOINCREMENT`**, and
   migration 25 rebuilds `detected_faces` to get it (the data model above). The People page
   holds these ids and acts on them later, and an id the UI holds must never name a different
   row: without `AUTOINCREMENT` SQLite hands the highest deleted id to the next insert, and the
   grouping step deletes empty groups and creates new ones, as a re-detection deletes a photo's
   faces and inserts new ones.
3. **Renaming a named person does not confirm their suggestions**; naming an unnamed group
   confirms its faces. "Rename: as naming" holds for the name and the merge a taken name makes,
   not for the confirmation: confirming suggestions the user has not looked at would put
   strangers under a name.
4. **The Picasa name offered for a group counts only faces Picasa recorded under a named
   contact.** A group's face that sits on no Picasa face, or only on one whose contact no INI
   names, does not vote, and an unnamed Picasa face at the same place does not hide a named one.
   A contact linked to a person offers the person's name.
5. **Every named person is listed in the page's People section**, with a `face_count` of 0 and
   an empty strip when none of their confirmed faces is visible (all rejected, hidden or
   missing). Out of the page they could not be renamed, merged or deleted, while the switch-off
   count still included them. The sidebar's People list is unchanged: it lists a person only
   with a visible photo.
6. **Grouping during the embed step is paced.** Not after every embed batch: the first batch
   that writes is grouped at once, each later run once `GROUP_COST_FACTOR` (9) times the last
   run's own duration has passed since it ended (`GroupPacer` in `engine.rs`), so grouping is at
   most about a tenth of the step. Each run reads every grouped face's vector, and run after
   every batch a first recognition would read about 40 GB at 100,000 photos. The groups are
   never kept between runs, only the timing: an edit or a changed file deletes detections
   without `people_write`, so groups held over could name deleted faces. The step that ends the
   pass places whatever a paced-out batch left.
7. **The embed candidate query applies its eligibility filters inside the page's `LIMIT`.** The
   photo's conditions (live, preview ready) are in the subquery that picks the page's photos,
   faces driving through a `CROSS JOIN` and each photo read by id. Outside it, a page made of
   missing photos came back empty while later photos had faces to embed, and the pass reads an
   empty page as no work.
8. **`person:` and the viewer.** `person:` gives a contact linked to a person the person's name
   too, so a renamed person's Picasa-only photos are found by the new name. The viewer draws a
   face once: a confirmed detection over a named Picasa face (linked to a person or not) leaves
   the plate to Picasa's, which carries the person's key and name when its contact is linked;
   one over an unnamed Picasa face takes that face's outline away and shows the person's plate.
9. **A face narrower than `MIN_FACE_PX` (35) in the decoded preview** is marked looked-at
   (`embedding_version` set) with no vector, and is never grouped. It still counts as a face
   for `has:face` and `faces:N`.
10. **The embedding breaker counts faces, not photos.** The floor is detection's, 8, and a batch
    trips it when every face the model was asked about failed. One group photo with 8 or more
    faces that all fail (landmarks the aligner refuses), in a batch where no other face is
    asked about, trips it alone, on every pass, and no photo with a higher id is embedded. The
    guard in "Guards" is otherwise as written.
11. **A merge leaves out a face rejected from the person it merges into.** Grouping places a
    face taken out of Anna in another group; naming that group "Anna", or merging it into her,
    moved it back to her, confirmed, while the rejection stood. Such a face is left ungrouped
    by the merge, and the grouping step places it, passing Anna over. Naming a group that no
    longer exists (emptied and deleted by a grouping run, or by the switch) is refused.
12. **The detector re-run hands faces back by fit, with their vectors.** Each pair of an old
    and a new face that `same_face` accepts is a candidate, assigned best overlap (intersection
    over union) first, each face on either side used once: first-match in id order let a small
    face whose centre lies inside a large one take the large face's name. The new face gets
    the old vector without an `embedding_version`, so it is embedded again and meanwhile counts
    towards its group's average; a face is placed only by a vector made for it (the ungrouped
    test asks for a version, and for a vector of the right length).
13. **The choosing is kept cheap, not moved out of the write.** `people::Group` keeps its sum's
    length, and the dot product sums in eight lanes: about 9 ns a comparison, against 95 ns
    recomputing both lengths, timed in release on x86-64.
14. **The viewer and an unlinked contact.** A detection confirmed as Ben over a Picasa face
    named for a contact Anna that no person is linked to shows only Anna's plate in the viewer
    (As built 8), while Ben's Person view lists the photo.
15. **The switch-off question is the native `ask` dialog, from Settings,** not an overlay in
    `App.svelte`. The overlay rule exists because an in-page dialog is made inert by its own
    opening; a native dialog is modal over the whole window, so it does not arise, and it is
    how photon asks every other destructive question. Delete and Merge on the page ask the same
    way.
16. **"Confirm all" confirms the faces on screen** (the strip plus whatever "Show more" has
    loaded), and says how many when that is fewer than the person's suggestions ("Confirm these
    12"): confirming faces the user has not looked at would put strangers under a name, which is
    why renaming does not confirm suggestions (As built 3).
17. **Opening a face's photo, and single faces.** The photo is found in the grid's current view,
    or the grid switches to All photos, and the viewer opens over the page, which keeps its
    state. A single face can be named: `PageFace.personId` is its group, the first face's group
    is named and the others' groups merge into it.
18. **The grid stays mounted under the page.** The spec and the plan said it unmounts. It does
    not: it is drawn beneath the page with `visibility: hidden` and `inert`. A remounted grid
    starts at the top after the launch restore is long done, and its "remember the folder at the
    top" effect then overwrote the user's place with the first folder on every return;
    `display: none` drops the layout box, resets `scrollTop` and shows the ResizeObserver a zero
    width, which trips the same write.
19. **The page at 150,000 faces.** Measured by the `people_150k_faces` bench in
    `benches/grid.rs` (`cargo bench -p photon-core --bench grid -- people`), in release on a
    24-thread AMD Ryzen AI MAX PRO 390: 150,000 faces with vectors on the 100,000-photo bench
    library, in 30,000 groups shaped like a first recognition (20 named people of 1,500 faces,
    30 unnamed groups of 500, 4,950 of 13, 15,000 pairs, 10,000 single faces, 650 faces
    ignored one by one; 1% of the photos hidden), written straight into the database.
    `people_page(12)` takes 148 ms and `people_to_name()` 70 ms. The bench has no Picasa
    faces, so no offer is worked out; with the cap below, that cost falls on the 200 listed
    groups' photos alone. Three things keep that from being paid back to back:
    - **Unnamed lists the 200 largest groups** (`LISTED_GROUPS`) and `unnamed_count` counts
      them all; the page says "Showing the 200 largest groups. Name or ignore some to see the
      rest." past it, as it does for single faces. `people_to_name` still counts every group.
      A group on screen that others' growth pushes out of the 200 leaves the page at the next
      reload; one that enters is appended, since the page keeps the order it opened with, so "largest first" holds only when the page opens and the heading no
      longer claims it.
    - **Library changes reload the page at most once a second** (`createPeoplePage().changed`,
      trailing): a scan announces a data change with each rebuild. The page's own actions
      reload at once.
    - **A reload keeps the order of the groups on screen**: the backend's order (largest
      first) is taken once, when the page opens; after that a group keeps its place and a new
      one is added at the end. Re-sorted on every reload, the keyed rows moved under the user,
      taking focus out of the name being typed and putting another group under the pointer.
20. **`createPeoplePage` is tested with the server runtime**, not the client one ("Logic and
    markup" said client). Its tests are `people-page.svelte.test.ts`, in the `node` vitest
    project, where a `.svelte.ts` module compiles for Svelte's server runtime and effects never
    run. That is enough because the factory holds no effect and no derived value: its state is
    read back through plain getters and functions, which run the same on either runtime, and
    its timers are `setTimeout`, driven by vitest's fake timers. What needs the client runtime
    is a reaction re-running (`page-signals.client.test.ts` is the case that showed it); the
    page's reactions - the reload on `dataVersion`, the switch read - live in
    `PeoplePage.svelte`, which no test renders.

## Not in this design

- Naming a face from the viewer.
- Hints that two groups look like one person.
- Writing names to `.picasa.ini` or to photos.
- GPU inference.
- Faces in videos.
- Carrying a confirmation across a turn or a crop.
- Undo.
