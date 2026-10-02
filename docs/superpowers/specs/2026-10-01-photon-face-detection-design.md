# Face detection

2026-10-01. Approved 2026-10-02; implemented on `feat/face-detection` (PR #146). Paragraphs
marked **As built** record where the implementation departs from the design above them.

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

**As built: 1280 and 320.** The spike measured small faces at 1280 and close-ups only at 320
(LFW), and so missed the other end: at 1280 a face that fills the frame is not found. The
whole-branch review measured it on 2026-10-02 with the real detector, on crops of the test
portrait:

| Face height at the model's input | At a 1280 input | The same crop at 640 or 320 |
|---|---|---|
| 480-590 px | found, 0.90-0.93 | found, 0.89-0.95 |
| about 690 px | found, 0.82 | found |
| 720-830 px | found, 0.76-0.78, the box drawn too small | found |
| 880-910 px | **not found** | found |

Of a 1600 px preview that is a face taller than about two thirds of the long side: a head
shot, a selfie, a baby close-up. Why the model stops there was not looked into; the
measurement is what the decision rests on.

The decision: **the detector runs twice per picture, at 1280 and at 320, and the two runs'
faces are merged through the one overlap suppression** (0.3, strongest first), compared in
fractions of the picture, which is the unit both runs share. 320 rather than 640 because it
costs a sixteenth of the large run where 640 costs a quarter. `DETECTOR_VERSION` stays 1,
nothing having been released. Measured after the change, on the inputs the spike used:

| | 1280 alone | 1280 and 320 |
|---|---|---|
| The portrait cropped to its face, at eight tightnesses, enlarged to 1600 px | 0 of 8 found | 8 of 8, 0.94-0.95 |
| The test portrait (960x640) | 1 face, 0.936 | 1 face, 0.945 |
| Solvay at 1600 px (29 people) | 29 | 29 |
| 23 photos with no frontal face, at 0.7 | 0 | 0 |
| One detection, release build, one thread | 399 ms | 420 ms |

A face both runs find is kept once, as the stronger box; for the portrait that is the small
run's, which sits 0.02 lower at the top than the large run's. The portrait at 1600 px shows
the same thing from the other side: the large run alone draws its box short (top 0.278
against 0.209), and with the small run it is where the face is.

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

**As built.** The second run at 320 adds 21 ms to a detection on one thread (399 ms to 420 ms
on the test portrait, release build), about 5%: ten minutes more on those 100,000 photos. The
spike's own table gave 37 ms for a 320 run. Memory was not measured again; the small run's
input and outputs are a sixteenth the size of the large run's.

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

### Limits found while building

- **A face lying on its side is mostly missed.** Measured on the test fixture on 2026-10-02:
  the portrait turned a quarter one way is found at 0.71, just over the threshold and with its
  box about 0.1 off; turned the other way it is not found at all. The pass reads previews,
  which have the EXIF orientation and photon's own turns applied, so this reaches only photos
  stored sideways with no orientation tag. Turning such a photo in photon clears its detections
  and its `face_version`, and it is detected again upright.

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

**As built.** The candidate query has no `online` condition, which the look-alike pass's list
has: the preview is in photon's own cache, so the photos of an unplugged drive are detected.
`face_progress`'s count also reads every live photo; it writes the `+` too and has its own plan
test (without it the plan is a scan of `items_size`).

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

**As built.** The model is optimised once for each of the two input sizes: 49 ms in a release
build.

**As built.** `detect` takes a `&DynamicImage`, not an `&RgbImage`: the cache hands back RGBA
for a photo with transparency and a greyscale picture is one channel, and converting is the
detector's business. `DETECTOR_VERSION` also covers the overlap limit.

`DETECTOR_VERSION` changes when the model, either input size or the threshold changes, which
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

**As built.** Steps 1 to 3 are done twice, with 1280 and with 320 as the square's side (the
model is loaded with each input fact). Each run's faces over the threshold are divided by the
size of the image as that run scaled it, which puts both runs' faces in fractions of the
image; any face with a number that is not finite is dropped; and step 4's suppression then
runs once over the two sets together, so a face both runs found comes out once. The
threshold is asked as "is the score at least 0.7", so a score that is not a number is not
kept.

### The model file

`crates/photon-core/models/face_detection_yunet_2023mar.onnx`, embedded with `include_bytes!`.
`THIRD-PARTY-NOTICES.md` gains YuNet's MIT licence and `tract`'s.

### Failure

`tract` is Rust and unwinds. The pass wraps each photo's detection in `catch_unwind`; a photo
whose detection panics or returns an error is logged and written as looked-at with no faces, so
it is not retried on every pass. Unlike rav1d there is no abort to guard against and no
crash-loop guard.

**As built.** Reading the preview has a guard of its own: its decode goes through the `webp`
crate, and a panic there would otherwise end the pass's thread on the same photo every time.
A preview whose read panics is treated as one that cannot be read - skipped, still a
candidate - not as a detection that failed.

**Addendum: the breaker.** Marking a failed photo is wrong when the fault is the detector's
and every photo hits it (the model failing on this machine): the whole library would be
marked checked with no faces, the pass would finish normally and nothing would tell the user;
recovery would be switching the feature off and on. So a batch is not written, and
`pass::run` returns `Error::FaceModel` ("all N photos detected in a batch failed; nothing was marked"),
when *every photo detected in it failed* (error or panic) and at least `BREAKER_FLOOR` (8) did.
"Detected" means the preview was read and the detector called: a photo skipped for an
unreadable preview, or not reached because the pass was cancelled, counts neither way, and a
photo detected with no faces is a success. Nothing of the batch is marked, so its photos stay
candidates; batches already written stay written. The engine logs the error and ends the pass
through `end_face_pass` as for any other, so the progress line clears. Because nothing is
marked, each later trigger (a scan's end, the thumbnail queue draining, the switch turned on,
startup) loads the model and fails one batch again, which is the intended bounded cost - no
retry limit, persisted flag or setting. The floor exists because with a lower one a single bad photo alone in a final batch
would never be marked and would be retried on every pass, which is what marking a failed photo
prevents; it is 8 and not the whole batch so that a small library, or the tail of a large one,
can still trip it. The gap is real: fewer than 8 photos, all failing, are still marked. So is the cost in the
other direction: that every detected photo failing means the detector is failing is what the
rule assumes, not something a count can tell. `run` returns at the tripped batch and the next
pass starts from the same photos, so if a batch's detected photos are 8 or more genuinely bad
files with no success among them, it trips on them every pass and no photo after them (a higher
id) is ever detected. Improbable - each has a readable preview and the detector must fail on
every one - but it follows from the rule.

A detection whose box, landmarks or score is not a finite number is dropped by the detector.
SQLite binds NaN as NULL and `detected_faces` refuses it, which would fail the whole batch,
and the next pass lists the same batch first.

## The pass

### Requesting it

`Engine::request_face_pass` is built like `request_similar_pass`: a thread named
`photon-face-pass`, one pass at a time behind its own lock, a request that finds one running
sets a flag and the runner goes round again. It returns at once when the setting is off.

It is counted in the counter `request_similar_pass` uses, renamed from `similar_passes` to say
it covers both, so `shutdown`'s bounded wait and the tests' `Fixture::settle` cover it without a
second mechanism.

**As built.** The counter is `background_passes`. Both passes are spawned through one
function, `spawn_pass`, which counts the thread and refuses it once shutting down;
`wait_for_similar_pass` and `stop_similar_pass` became `wait_for_passes` and `stop_passes`.

It is requested:

- at the end of every `run_scan` that requests the look-alike pass;
- when the thumbnail queue goes quiet after readying thumbnails (`start_thumb_hashing`'s
  thread), which is the trigger that matters, since a photo is a candidate only once its preview
  exists;
- when the setting is switched on.

Resuming needs nothing more: every launch's startup scans request it, and `face_version`
records where it stopped.

**As built.** Resuming did need more. A scan that finds its root still offline requests no
pass, so a launch with every drive unplugged never resumed, though the previews are in
photon's cache. `startup` requests a pass itself, once the first grid is built.

This is fewer places than the look-alike pass is requested in: an edit, a folder's removal and
a change of the look-alike distance request that pass and no face pass. An edited photo is
detected again through the thumbnail queue's drain.

### Running it

1. Read a batch of candidates, id and thumbnail key, in id order from after the last id the
   pass has read.
2. Workers decode each cached preview and call `detect`. The worker count is half of
   `available_parallelism`, at least 1 and at most 4, bounding the pass at about 500 MB.
3. Write the batch in one transaction: the detections, and `face_version` for every photo
   looked at - unless the batch trips the breaker (see "Failure"), when it writes nothing and
   the pass ends with an error.
4. Repeat until a batch is empty. The cancel flag is checked between photos.

A preview that cannot be read or decoded is skipped and the row stays a candidate, as the
look-alike pass treats a thumbnail it cannot read. Paging by id is what keeps that from looping:
a skipped photo is not read again until the next pass.

**As built.** A candidate's thumbnail is ready, so its files existed and were renamed into
place; the realistic causes of an unreadable preview are a file removed from the cache, which
is detected once it is rendered again, and a file libwebp refuses. The second is permanent:
nothing re-renders a file that exists, so it costs one failed read on every pass and the
progress count stops short of the total by that photo.

### Two guards on the write

- **The photo moved on.** A row whose thumbnail key (`Item::thumb_key`: fingerprint and edit) is
  no longer the one the batch read is skipped. A file rewritten or an edit made during the pass
  would otherwise have the old picture's rectangles stored against the new one.
- **The switch went off.** The batch transaction reads the setting first and writes nothing when
  it is off. Writes are one mutexed connection, so the off-switch's delete and a batch cannot
  interleave: nothing is written after "off".

Each has a test that fails with the guard removed.

**As built.** The first guard compares what the key is made of rather than the key: the row is
written only if its size, modification time and edit are still the ones listed and it is not
missing.

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

**As built.** The rebuild at the end of the pass and its not announcing a data change are
tested. The rebuild during the pass is not: its only trigger is a 30-second wait.

### Progress

A `face_progress` event, throttled like scan progress, carries `checked` and `total` (live
images with the current version, and live images). The last one of a pass carries
`running: false`. `api.ts` mirrors it in the same commit.

**As built.** A pass that ends with the switch off sends the cleared event (zeros, not running)
itself, although the command has already sent one: the batch the switch interrupted can report
"running" after the command's event, and that report would otherwise be left standing.

A pass asks for one candidate before it loads the model. With none it sends its last event
and returns, so the pass requested after every scan of a finished library costs two queries (the
candidate probe, which walks the table when nothing matches, and the count in that last
event) and not a model load. A library holding a photo whose preview can never be read never
gets this path, since that photo is always a candidate. With one it sends a `running: true` event at once: the first batch's report
is 64 detections away, eight seconds on four workers and a minute on a small machine, and a
switch that shows nothing for that long gets toggled again. A detector that fails to load
sends the last event too. A pass the quit ends sends no last count and makes no rebuild; each
reads the whole library inside `shutdown`'s bounded wait.

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

**As built.**

- The setting is read by its own command, `face_detection`, as every other setting is, rather
  than travelling with others. That makes two commands, each in the same five places.
- The switch is in a Settings section of its own, People.
- The mirror, the stored write and the mirror's restore when that write fails are serialised by
  a mutex in the engine, `face_write`. Commands run concurrently, and two toggles close
  together each moved the mirror and then queued for the library's writer in either order,
  leaving the mirror on one answer and the database on the other. The lock is released before
  the pass request, the rebuild and the event.

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

**As built.** The function is the module `face_detect::merge`. Its `shown` is the single
mapping of a Picasa rectangle into the picture as shown: the viewer's named faces go through
it as well as the unnamed ones and search's count.

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

**As built.** No `picture.ts` test was added. `pictureChanged` takes a `Pick` of `ViewerItem`
that has no face field, so it cannot see a face and there is no behaviour for a test to
discriminate; the comment on the type says the omission is deliberate.

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
  **As built:** the test that turned the portrait on its side was replaced by one that puts it
  on a taller canvas, which moves the padding from the bottom of the input to its right just
  the same. Turned, the test was measuring the model, which does not find a sideways face
  reliably (see "Limits found while building").
- End to end on the real model: a small CC0 portrait added as a fixture, credited as the
  screenshot photos are, yields one face about where it is; a landscape yields none.
  **As built**, also: the portrait cropped to its face and enlarged to 1600 px (found only by
  the small run); the portrait at 1600 px; and the portrait at a sixth of its size in a
  1600 px picture (found only by the large run, and the one test that pins the large run's
  fractions for a picture larger than the input).

**The pass** (`pass.rs`, stand-in detectors)

- The breaker: a full batch of failures is not written (`face_candidates` still lists every
  photo, no version set, `on_batch` not called) and `run` errs; the floor (8 trip, 7 are
  marked); a batch with one success is written as before; a detection with no faces is a
  success; panics count like errors; photos with unreadable previews, and photos a cancel
  never reached, count neither way; a tripped second batch leaves the first one's writes; and
  the decision function directly, on literals.

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
- `picture.ts`: a change in `unnamedFaces` is not a picture change. **As built:** not written;
  see "The viewer".

**What has no test**

The viewer's outlines and the Settings switch are component markup and effect wiring; they are
covered by `svelte-check`, the screenshots (`viewer-info-*` gains an unnamed face in `mock.js`,
the Settings shot shows the switch) and the smoke checklist.

**As built.** There is one viewer-info shot, `viewer-info-light` (the viewer is dark in both
themes), and it shows the unnamed face. The switch is in its own Settings section, so the
existing Settings shots cannot show it: one shot was added, `settings-people-light`, making
twenty-nine.

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
