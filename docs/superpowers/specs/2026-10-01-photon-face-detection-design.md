# Face detection

2026-10-01. Designed in conversation the same day; awaiting review of this file.

## What it is

photon finds the faces in the user's photos itself, on their computer, with a small neural
network run in pure Rust. Until now the only faces photon knew were the ones Picasa had recorded
in `.picasa.ini`.

This is the first of three stages towards grouping people automatically, as Picasa did:

1. **Detection** (this spec): find faces, store their rectangles, search by them, outline them
   in the viewer.
2. **Embeddings and clustering**: a vector per face, grouped into unnamed people.
3. **People UI**: naming, merging and splitting groups, and reconciling them with Picasa's
   contacts.

Each stage has its own spec and plan. This one is releasable on its own.

Detection is **off until the user switches it on**. Nothing changes for anyone on upgrade.

## What the user gets

- A switch in Settings, "Find faces", off by default.
- While it runs, a progress line ("Finding faces: 12,400 of 98,000").
- `has:face`, `faces:N` and `faces:N+` in search.
- Unnamed outlines on detected faces in the viewer, beside Picasa's named ones.

## Why this is feasible: the spike

The rule that photon wraps no C/C++ SDK excludes ONNX Runtime, OpenCV and dlib, which almost
every face library is built on. A throwaway spike on 2026-10-01 asked whether a pure-Rust
inference engine could do the job. It can.

`tract-onnx` 0.23.8 (MIT/Apache) loads both models from OpenCV's model zoo unmodified:

| Model | File | Size | Licence | Used in |
|---|---|---|---|---|
| YuNet (detection) | `face_detection_yunet_2023mar.onnx` | 233 KB | MIT | this stage |
| SFace (embedding) | `face_recognition_sface_2021dec.onnx` | 38.7 MB | Apache 2.0 | stage 2 |

The measurements below were taken on a Ryzen AI Max (x86-64, Linux), release build. The test
data was the LFW dataset and the 1927 Solvay Conference photograph (29 people, 3000x2171).

### Input size decides what is found

The Solvay photo, letterboxed into the model's input:

| Input | Faces found (29 people) | Time, one thread |
|---|---|---|
| 320 | 1 | 37 ms |
| 640 | 30 (one extra) | 145 ms |
| 1280 | 29 | 465 ms |

A face narrower than about 15 px at the model's input is lost. A 256 px grid thumbnail is
therefore useless as input, and 640 puts a group photo's faces at 14-21 px, on the edge. The
pass uses 1280.

### The confidence threshold

Detections at each threshold, input 1280 (LFW at 320, its images being 250 px):

| Threshold | Solvay, from a 1600 px preview (29 people) | 23 photos with no face | LFW subjects found (401) |
|---|---|---|---|
| 0.5 | 29 | 3 false | 401 |
| 0.6 | 29 | 2 false | 401 |
| **0.7** | **29** | **0 false** | **401** |
| 0.8 | 29 | 0 false | 401 |
| 0.9 | 25 | 0 false | 399 |

The false detections below 0.7 were an ibex (0.48), a flower (0.61) and a woman seen from behind
(0.61). The Solvay faces score 0.89 to 0.94. The threshold is 0.7. The sample is small; the
stored score (below) lets it move without detecting again.

### Throughput and memory

Scaling a 1600 px preview to the input and detecting at 1280:

| Workers | Photos/s | Peak memory |
|---|---|---|
| 1 | 2.4 | 159 MB |
| 2 | 4.5 | 257 MB |
| 4 | 8.4 | 501 MB |
| 8 | 13.8 | 774 MB |

A worker holds 100-120 MB. With four workers, 100,000 photos take a little over three hours,
plus one WebP decode each.

### Stage 2's evidence, recorded here because the spike produced it

On LFW (600 people, 2,095 images), SFace embeddings by cosine similarity: at 0.363, OpenCV's
threshold, 98.7% of same-person pairs match and 0.11% of different-person pairs do; at 0.45,
95.7% and 0.003%; at 0.55, 85.2% and none in 2.2 million. Linking any two faces above the
threshold chained 1,997 faces into one cluster at 0.363. Stage 2 needs a stricter rule than
single linkage. An embedding costs 50-64 ms per face.

### What the spike did not show

- `tract` building on Windows MSVC and macOS arm64. Its only native code is assembly kernels
  compiled with `cc`.
- Detection on real family photographs: children, profiles, faces partly hidden. LFW is press
  photographs of adults.

## Decisions

- **Pure-Rust inference with `tract`.** The dependency rule stands. `tract`'s assembly is
  vendored and compiled in with `cc`, like the C photon already carries.
- **The models are bundled**, not downloaded. photon has no network access at runtime and this
  feature does not add any. The installer grows by roughly 20 MB in this stage (`tract`) and by
  a further 38.7 MB in stage 2 (SFace).
- **Off until enabled.** The pass costs hours of CPU on a large library and computes data about
  the people in it; neither happens unasked.
- **Off means gone.** Switching off deletes every detection.
- **Detect from the cached preview, not the source file.** Every ready photo has a 1600 px
  preview with its edit applied. Reading it means no source is decoded a second time, a library
  from before this feature fills in from its cache, and the scanner is not involved, so neither
  of `walk_tree`'s callers can be forgotten. Detecting inside the thumbnail renderer would skip
  every photo whose thumbnail is already cached, which is the trap recorded for the perceptual
  hash.
- **A new table**, not rows in Picasa's `faces`: `set_item_faces` replaces a photo's faces
  wholesale on every INI reread, and stage 2's embedding belongs to a detected face.
- **Picasa's faces win.** A detection lying over a Picasa face is not shown or counted twice.
- **Images only.** Videos are skipped in this stage.

## Data model (schema 24)

### `detected_faces`

```sql
CREATE TABLE detected_faces (
    id        INTEGER PRIMARY KEY,
    item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    left      REAL NOT NULL,
    top       REAL NOT NULL,
    right     REAL NOT NULL,
    bottom    REAL NOT NULL,
    landmarks BLOB NOT NULL,
    score     REAL NOT NULL
);
CREATE INDEX detected_faces_item ON detected_faces(item_id);
```

- The rectangle is fractions of the picture **as shown**, edit applied. Picasa's `faces` rows
  are fractions of the unedited picture and are mapped through the edit on read. The two tables
  are in different frames on purpose; the merge (below) is the one place they meet.
- `landmarks` is YuNet's five points (the two eyes, the nose tip, the two mouth corners) as ten
  little-endian `f32` fractions, in the model's order. Stage 2 aligns a face by them before
  embedding it; storing them now means the whole library is not detected again then.
- `score` is the detector's confidence. Only detections at or above the threshold are stored.
- `id` is the face's own identity, for stage 2's embedding and stage 3's assignment to a person.

### `items.face_version`

`INTEGER`, `NULL` by default. `NULL` means the detector has not looked at the photo. Otherwise
it is the `face_detect::DETECTOR_VERSION` that did, whether or not it found a face.

A photo is a candidate when all of these hold: its `face_version` is `NULL` or behind the
current version, its thumbnail state is ready, it is an image, and it is not missing. Hidden
photos are candidates, as they are for hashing, so unhiding is instant.

The candidate query reads every live photo, so it writes `+missing_since IS NULL` and has a plan
test.

### Writers that clear a photo's detections

Each deletes the photo's `detected_faces` rows and sets `face_version` to `NULL`:

- `update_items`, when the file's fingerprint changes, beside the `content_hash` and
  `percep_hash` it already clears.
- `set_item_edit`, because the rectangles describe the picture as shown.

Purging an item cascades. A test pins each of the two writers.

### The setting

`face_detection`, a boolean in `settings`, absent meaning off.

Switching it off deletes every `detected_faces` row and nulls every `face_version` in the
transaction that stores the setting.

### Tripwires

The literal schema version (twice) and the table count in `library/mod.rs` move to 24 and one
more table. The migration test seeds from `MIGRATIONS[..23]`.

## The detector: `photon_core::face_detect`

The module knows nothing about the library, the cache or the engine. Nothing outside it names
`tract`. (`faces` is already the name of the Picasa table module.)

```rust
pub const DETECTOR_VERSION: i64 = 1;

pub struct Detection {
    pub rect: Rect,              // fractions of the image given
    pub landmarks: [(f32, f32); 5],
    pub score: f32,
}

impl Detector {
    pub fn new() -> Result<Self>;
    pub fn detect(&self, image: &RgbImage) -> Result<Vec<Detection>>;
}
```

`Detector::new` parses and optimises the model, about 25 ms, once per pass. A `Detector` is
shared between the pass's workers.

`DETECTOR_VERSION` changes when the model, the input size or the threshold changes, which
detects the whole library again, as `EXIF_VERSION` does for metadata.

### One image

1. Scale the image to fit 1280x1280, never up, and place it at the top left of a black
   1280x1280 input. Channels are BGR, values 0 to 255, as the model was trained.
2. Run the model. The file declares a 640 input, so it is loaded with
   `with_ignore_output_shapes(true)` and `with_ignore_value_info(true)` and given the 1280 input
   fact; without them `tract` refuses the size.
3. Decode the twelve outputs: for each of the strides 8, 16 and 32, a class score, an object
   score, a box and five landmarks per grid cell. The score is the square root of class times
   object, each clamped to 0..1. The box centre is the cell plus the offset, times the stride;
   its size is `exp` of the output, times the stride. A landmark is the cell plus its offset,
   times the stride.
4. Keep scores of 0.7 or more. Take them in descending score and drop any whose overlap
   (intersection over union) with one already kept exceeds 0.3, the model authors' value.
5. Divide by the scale and the scaled image's size, giving fractions of the image passed in.

### The model file

`crates/photon-core/models/face_detection_yunet_2023mar.onnx`, embedded with `include_bytes!`.
`THIRD-PARTY-NOTICES.md` gains YuNet's MIT licence and `tract`'s.

### Failure

`tract` is Rust and unwinds. The pass wraps each photo's detection in `catch_unwind`; a photo
whose detection panics or returns an error is logged and written as looked-at with no faces, so
it is not retried on every pass. Unlike rav1d there is no abort to guard against and no
crash-loop guard.

## The pass

### Requesting it

`Engine::request_face_pass` is built like `request_similar_pass`: a thread named
`photon-face-pass`, one pass at a time behind its own lock, a request that finds one running
sets a flag and the runner goes round again. It returns at once when the setting is off.

It is counted in the counter `request_similar_pass` uses, renamed from `similar_passes` to say
it covers both, so `shutdown`'s bounded wait and the tests' `Fixture::settle` cover it without a
second mechanism.

It is requested:

- at the end of every `run_scan` that requests the look-alike pass;
- when the thumbnail queue goes quiet after readying thumbnails (`start_thumb_hashing`'s
  thread), which is the trigger that matters, since a photo is a candidate only once its preview
  exists;
- when the setting is switched on.

Resuming needs nothing more: every launch's startup scans request it, and `face_version`
records where it stopped.

### Running it

1. Read a batch of candidates, id and thumbnail key, in id order from after the last id the
   pass has read.
2. Workers decode each cached preview and call `detect`. The worker count is half of
   `available_parallelism`, at least 1 and at most 4, bounding the pass at about 500 MB.
3. Write the batch in one transaction: the detections, and `face_version` for every photo
   looked at.
4. Repeat until a batch is empty. The cancel flag is checked between photos.

A preview that cannot be read or decoded is skipped and the row stays a candidate, as the
look-alike pass treats a thumbnail it cannot read. Paging by id is what keeps that from looping:
a skipped photo is not read again until the next pass.

### Two guards on the write

- **The photo moved on.** A row whose thumbnail key (`Item::thumb_key`: fingerprint and edit) is
  no longer the one the batch read is skipped. A file rewritten or an edit made during the pass
  would otherwise have the old picture's rectangles stored against the new one.
- **The switch went off.** The batch transaction reads the setting first and writes nothing when
  it is off. Writes are one mutexed connection, so the off-switch's delete and a batch cannot
  interleave: nothing is written after "off".

Each has a test that fails with the guard removed.

### The refresh chain

A detection changes what a face search returns and what the viewer draws. It changes no album,
person, tag, tag rule or folder count and nothing Settings reads through `data_changed`, so the
pass rebuilds through `refresh_grid_derived` and does not move `counts_epoch`.

- One rebuild when the pass ends, if it wrote anything.
- During the pass, at most one every 30 seconds, and only while the current view is a search
  whose query reads faces (`Query::needs().faces`). Rebuilding a 300,000-photo grid every few
  seconds for three hours would be the pass's largest cost, for nothing visible.

Consequence: a photo already open in the viewer when its detection lands shows its outlines at
the next rebuild, not at once. Any photo opened afterwards has them.

### Progress

A `face_progress` event, throttled like scan progress, carries `checked` and `total` (live
images with the current version, and live images). The last one of a pass carries
`running: false`. `api.ts` mirrors it in the same commit.

### Quitting

`shutdown` cancels the pass and waits a bounded time, as for the look-alike pass. A worker
finishes the photo it is on, under half a second. A batch not yet written is done again next
launch.

## The switch

Settings gains "Find faces", with a line beneath it saying that it runs in the background, can
take hours on a large library, stays on this computer, and that switching it off deletes what
it found. While a pass runs, the progress is shown beside it.

One command, `set_face_detection(enabled)`: `commands.rs`, `ipc.rs`, the handler list in
`app.rs`, `api.ts`, and an answer in `mock.js`. The current value travels with the settings the
dialog already reads.

- **On:** store the setting, request a pass.
- **Off:** cancel the running pass, store the setting and delete the detections in one
  transaction, rebuild through `refresh_after_write`.

The engine keeps the setting in an atomic beside the stored value, set by the command and read
at `open`, so the pass's cancel check does not read the database per photo. The batch
transaction's own read of the stored setting is the authority.

## Readers

### One rule for the faces on a photo

A pure function in `face_detect` merges the two sources. `viewer_item` and search both call it,
so they cannot disagree.

- Picasa's faces are mapped through the photo's edit into the picture as shown
  (`Edit::map_rect`); one whose centre is cropped away is dropped, as today.
- A detection is the same face as a Picasa face when either rectangle's centre lies inside the
  other rectangle. Picasa's boxes and YuNet's are drawn to different tightness, which an
  overlap ratio would be sensitive to.
- The result is every Picasa face, then every detection that matches none of them.

### A change for Picasa libraries

Today a Picasa face whose contact no INI names is left out of the viewer. Under this rule it is
a face: outlined without a name and counted by search, with detection on or off. Kept
invisible, it would hide the detection lying over it and that person would have no outline at
all.

### The viewer

- `ViewerItem` gains `unnamed_faces: Vec<Rect>` (`unnamedFaces` in `api.ts`): unnamed Picasa
  faces and unmatched detections, in the picture as shown. `faces` stays the named ones.
- They are drawn with the outline named faces have and no name plate, and like them only while
  the info panel is open. They are not interactive in this stage.
- The info panel's people list ends with "N faces not named" when there are any. Its empty
  state, "No faces named in Picasa.", appears only when there are no faces of either kind.
- **Faces must not reload the photo.** `pictureChanged` does not compare `faces` or
  `unnamedFaces`; a detection landing while the user looks at a photo must not blank it and
  reset the zoom. `picture.ts`'s tests gain that case.

### Search

- `has:face`: at least one face.
- `faces:N`: exactly N. `faces:N+`: N or more. A value that is not a number is ignored, like
  every other malformed term.
- Both negate with a leading `-` and are added to the prefix suggestions.
- `Needs` gains `faces`. When set, `search_entries` reads both tables' rectangles, and the edit
  of each photo that has a Picasa face, inside the search's existing snapshot, and counts
  through the merge. A query without these terms reads nothing more than it does today.
- With detection off the terms count Picasa's faces alone.

**A limit, stated in the Settings text and the release notes:** while the pass is unfinished, a
photo it has not reached counts only its Picasa faces, so `-has:face` and `faces:0` include
photos that have not been checked yet.

## Build

`tract-onnx` is added to photon-core. Its `tract-linalg` compiles assembly kernels with `cc`;
no nasm, no system library, nothing for the `.deb`'s `ldd` check to find.

**The first task of the plan** is a branch that adds the dependency and one test that loads the
embedded model, pushed to CI before anything else is written. If Windows MSVC or macOS arm64
does not build, work stops there and the choice of engine is reconsidered.

`cargo run -p xtask -- metadata` must still pass with the new notices.

## Documentation

- `CLAUDE.md`: the pass and its two triggers; the two frames (`detected_faces` as shown,
  `faces` unedited) and the merge as their one meeting point; the writers that clear
  detections; `DETECTOR_VERSION`; `pictureChanged` not comparing faces; `tract` and the bundled
  model under the dependency convention.
- `README.md` and the website's feature list.
- `THIRD-PARTY-NOTICES.md`.
- `docs/smoke-checklist.md`: switching on and off, the progress line, outlines on a real group
  photo, both search terms, a Picasa library's unnamed faces.

## Testing

Every new test is shown to fail with its change reverted.

**The detector**

- The output decoding is a pure function, tested on hand-built tensors: one cell per stride at
  a known position, giving a box and landmarks computed by hand. Each rule (the grid position,
  the stride, `exp` on the size, the square root of the scores) has a case that differs without
  it.
- The overlap rule: two boxes over the limit keep the higher score; two under it keep both.
- The letterbox mapping: the same face in a landscape and a portrait image maps back to the
  same fractions of each.
- End to end on the real model: a small CC0 portrait added as a fixture, credited as the
  screenshot photos are, yields one face about where it is; a landscape yields none.

**The library**

- The migration, on a populated library.
- The candidate query: its plan, and each of its four conditions.
- `update_items` and `set_item_edit` each clear a photo's detections and version.
- Switching off clears everything.
- The batch write: skipped for a row whose thumbnail key moved; nothing written with the
  setting off.

**The engine**

- A scan followed by `settle` leaves detections on a fixture photo with the setting on, and
  none with it off.
- Switching on requests a pass; switching off during a pass leaves no rows behind.
- The pass rebuilds through the derived path: `data_changed` is not announced.

**Readers**

- The merge: a detection over a Picasa face is dropped; one beside it is kept; boxes of
  different tightness around one face match; a Picasa face cropped away does not swallow a
  detection.
- `viewer_item`: named and unnamed faces for a photo with both, and with an edit.
- Search: `has:face`, `faces:2`, `faces:2+`, `faces:0`, negation, a Picasa-only library, and
  that a query without the terms makes no side read.
- `picture.ts`: a change in `unnamedFaces` is not a picture change.

**What has no test**

The viewer's outlines and the Settings switch are component markup and effect wiring; they are
covered by `svelte-check`, the screenshots (`viewer-info-*` gains an unnamed face in `mock.js`,
the Settings shot shows the switch) and the smoke checklist.

## Rollout

Schema 24 makes the release a minor, and an older photon will refuse the library.

The release notes say: the feature is off until switched on; the first run takes hours on a
large library and can be interrupted; nothing leaves the computer; a face search is incomplete
until the pass finishes; unnamed Picasa faces are now outlined; the installers are about 20 MB
larger.

## Not in this design

- Recognising or grouping people, embeddings, the SFace model: stage 2.
- Naming, merging or clicking a detected face; any change to the People list: stage 3.
- Faces in videos.
- Writing detections to `.picasa.ini` or to the photo. They live only in `library.db`.
- A GPU. `tract` runs on the CPU.
- A user-facing threshold or input-size setting.
