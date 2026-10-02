# People, plan 1 of 2: the backend

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** photon embeds every detected face with SFace, groups faces into people incrementally, lets commands name and correct the groups, and shows named people in the People list, the Person view, `person:` search and the viewer.

**Architecture:** A pure-Rust embedder (`photon_core::face_embed`) beside the detector, and a pure grouping rule (`photon_core::people`). The face pass gains two steps after detection: embed (its own batch loop, like detection's) and group (one thread, in face order). People are rows of a new `people` table; corrections are `Library` operations exposed as commands. Readers identify a person by a prefixed key, `p:<id>` or `c:<contact hash>`.

**Tech Stack:** Rust (`tract-onnx` 0.23, rusqlite, `image`), Tauri 2 IPC, TypeScript mirrors in `ui/src/lib/api.ts`.

**Spec:** `docs/superpowers/specs/2026-10-03-photon-people-design.md`. Read it before any task. This plan is the first of two; the People page (face crops, the page, its factory, the switch-off dialog, screenshots) is plan 2.

**About the code in this plan:** the alignment arithmetic and the SFace calls are lifted from the 2026-10-03 spike, which compiled and ran against photon 0.47.0. Everything else was written against the repository as read on 2026-10-03 and has not been compiled. Where a block does not compile, fix it to the stated interface and behaviour; do not change an interface in a task's **Produces** list without updating every later task that consumes it.

## Global Constraints

- **The gates** (`CLAUDE.md`, "Commands"): before every commit, `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`; for any change under `ui/` or to `api.ts`, also `npm run check` (0 errors AND 0 warnings) and `npm test`.
- **Every new test is shown to fail with its change reverted**, by an exact revert, restored from a copy and then `touch`ed. A probe that passes is a finding: report it, and add the input that makes the reverted code differ.
- **Never launch the GUI.**
- **No system library dependencies**; both models run on the CPU through `tract`.
- Grouping threshold **0.50** (`GROUP_SIMILARITY`); faces under **35 px** wide in the preview are not embedded (`MIN_FACE_PX`); `EMBEDDER_VERSION` is **1**; a face vector has **128** numbers, normalised, stored as 128 little-endian `f32` (512 bytes).
- Schema goes **24 → 25**. Update the literal numbers in `library/mod.rs` and every migration test's final version assert; do not loosen them.
- **Only confirmed faces carry a name**: the Person view, the People list's counts, `person:` search and the viewer's names use confirmed faces only.
- **Hidden and missing photos** keep their faces' groups but appear in no People reader: every new query a user sees filters `hidden = 0` and `missing_since IS NULL`.
- A person's key is `p:<id>` for a photon person or `c:<hash>` for a Picasa contact no person is linked to.
- Names are compared without case **in Rust** (`to_lowercase`), never with SQL `lower()` or `COLLATE NOCASE`.
- photon never writes names to `.picasa.ini` or to photos.
- A new IPC command needs `commands.rs`, `ipc.rs`, the handler list in `app.rs`, `api.ts`, and an answer in `crates/xtask/screenshots/mock.js`.
- Rust structs mirrored in `api.ts` change in the same commit.
- No plan labels ("Review focus N") in code comments; comments carry the reasoning.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Work on branch `feat/people`, which holds the spec.

## One deviation from the spec, decided here

The spec stores each group's centroid (`people.centroid`, `centroid_count`). This plan does not: **the centroids are computed at the start of each grouping step from the faces, by the counting rule, and kept in memory for that step.** A stored centroid has to be kept right by every writer that moves a face (every operation, the edit and file-change clearing in `items.rs`, the switch, the carry-over at a detector re-run), and one forgotten writer leaves it silently wrong, which no test of that writer would show. Computing it costs one read of the grouped faces' vectors when there is something to group (about 50 MB at 100,000 faces, well under a second), and nothing when there is not. Operations that move a face out of a group leave it ungrouped and request a pass, whose grouping step places it. Task 10 records this in the spec.

## Review Focus

1. **A face on a hidden photo** must not appear in any People reader: not in a strip, a count, an offer or a suggestion, and not as the face that makes a group "two or more". Task 6.
2. **A name typed in a different case** ("anna" when "Anna" exists, or a Picasa contact "ANNA") must merge into, or link to, that person, not create a second Anna. Task 6.
3. **Merging a person into themselves**, or into an unnamed group, must be refused without changing anything. Task 6.
4. **A face rejected for a group that is later merged into another person** must not be suggested for that person. Task 6.
5. **A confirmed face on a photo the detector re-runs over unchanged** must stay confirmed, and an edit must send it back to a suggestion. Task 4.

## File Structure

| File | Responsibility |
|---|---|
| `crates/photon-core/models/face_recognition_sface_2021dec.onnx` | The bundled embedding model (new). |
| `crates/photon-core/src/face_embed/mod.rs` | `Embedder`, `FaceBox`, `Embedding`, constants, `similarity`, blob conversion (new). |
| `crates/photon-core/src/face_embed/align.rs` | The similarity transform and the 112x112 sampling, pure (new). |
| `crates/photon-core/src/face_embed/pass.rs` | The embed batch loop, with the breaker (new). |
| `crates/photon-core/src/people/mod.rs` | The grouping rule, pure (new). |
| `crates/photon-core/src/library/people.rs` | The grouping step, the operations, the People page's reads (new). |
| `crates/photon-core/src/library/detected_faces.rs` | Embedding candidates and writes; the carry-over at a detector re-run. |
| `crates/photon-core/src/library/schema.rs`, `mod.rs`, `settings.rs` | Migration 25; the switch deletes people. |
| `crates/photon-core/src/library/faces.rs`, `items.rs` | The People list, the Person view, `person:` search, contacts linked by name. |
| `crates/photon-app/src/engine.rs`, `events.rs`, `commands.rs`, `ipc.rs`, `app.rs` | The two pass steps, `people_write`, progress phases, the commands. |
| `ui/src/lib/api.ts`, `status.ts`, `library.svelte.ts`, `ui/src/components/FolderTree.svelte`, `crates/xtask/screenshots/mock.js` | Mirrors and the person key. |

---

### Task 1: SFace builds and loads on all three platforms

A gate, as `tract` was for detection. If a 38.7 MB `include_bytes!` fails on a platform, stop and report.

**Files:**
- Create: `crates/photon-core/models/face_recognition_sface_2021dec.onnx`
- Modify: `crates/photon-core/models/README.md`, `THIRD-PARTY-NOTICES.md`
- Create: `crates/photon-core/src/face_embed/mod.rs`
- Modify: `crates/photon-core/src/lib.rs`

**Interfaces:**
- Produces: `photon_core::face_embed::Embedder` with `Embedder::new() -> crate::Result<Embedder>`; constants `EMBEDDER_VERSION: i64 = 1`, `MIN_FACE_PX: f32 = 35.0`, `DIM: usize = 128`; `pub type Embedding = [f32; DIM]`.

- [ ] **Step 1: Fetch the model**

```bash
curl -sSL -o crates/photon-core/models/face_recognition_sface_2021dec.onnx \
  https://github.com/opencv/opencv_zoo/raw/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx
wc -c crates/photon-core/models/face_recognition_sface_2021dec.onnx
```

Expected: `38696353`. Add a row to `crates/photon-core/models/README.md`'s table in the existing row's shape: the file, "SFace face recognition, 38,696,353 bytes", the opencv_zoo link (`models/face_recognition_sface`), fetched 2026-10-03, Apache 2.0. Extend the README's last paragraph: replacing this file means bumping `face_embed::EMBEDDER_VERSION`.

- [ ] **Step 2: The failing test**

Create `crates/photon-core/src/face_embed/mod.rs`:

```rust
//! Telling faces apart. A picture and one face's landmarks in, a vector out: two faces of
//! one person give vectors pointing nearly the same way. Like `face_detect`, this knows
//! nothing about the library, and nothing outside it names `tract`.

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled model parses and optimises. This is the test CI runs on Windows and
    /// macOS to show a 39 MB model embedded in the binary builds and loads there.
    #[test]
    fn the_bundled_model_loads() {
        Embedder::new().unwrap();
    }
}
```

Add `pub mod face_embed;` to `crates/photon-core/src/lib.rs` after `pub mod face_detect;`.

Run: `cargo test -p photon-core --lib face_embed` — expected: compile error, no `Embedder`.

- [ ] **Step 3: Implement**

Above the tests:

```rust
use crate::{Error, Result};
use tract_onnx::prelude::*;

/// SFace, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_recognition_sface_2021dec.onnx");

/// Which embedder made a face's vector: `detected_faces.embedding_version` records it.
/// Bump it when the model file, the reference points or the sampling in `align`, or
/// [`MIN_FACE_PX`] changes; every face is then embedded again.
pub const EMBEDDER_VERSION: i64 = 1;

/// The narrowest face, in pixels of the picture it is cut from, that is embedded.
/// Measured 2026-10-03 on LFW faces shrunk into a 1600 px picture: at 35 px the same person
/// is still matched 90.8% of the time at 0.45, at 25 px 80%, at 18 px 58%, and two 18 px
/// faces start matching strangers (0.6% at 0.363). Below this a face stays detected and
/// counted, and is never grouped.
pub const MIN_FACE_PX: f32 = 35.0;

/// The length of a face's vector.
pub const DIM: usize = 128;

pub type Embedding = [f32; DIM];

/// The model's input side.
const SIDE: usize = 112;

type Run = dyn Fn(Tensor) -> TractResult<TVec<TValue>> + Send + Sync;

/// The loaded model. Parsing and optimising it takes a few tens of milliseconds; a pass
/// makes one and shares it between its workers.
pub struct Embedder {
    run: Box<Run>,
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Embedder {
    pub fn new() -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        Ok(Self {
            run: Box::new(move |input| model.run(tvec!(input.into()))),
        })
    }
}
```

`run` is read from Task 3 on; until then clippy flags it. Put `#[allow(dead_code)]` on the field with a one-line comment naming Task 3, as detection's Task 1 did.

- [ ] **Step 4: Notices, gate, push, CI**

Append to `THIRD-PARTY-NOTICES.md` a section `## SFace face recognition model` in the shape of the existing YuNet section, with the Apache 2.0 notice from `https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_recognition_sface/LICENSE`. Run `cargo run -p xtask -- metadata`.

Run the Rust gate, then:

```bash
git add crates/photon-core THIRD-PARTY-NOTICES.md
git commit -m "build: the SFace model, loaded by a test

The gate for recognising people: CI has to show a 39 MB model embedded
with include_bytes! builds and loads on Windows and macOS.

The test has no revert probe: it is the build that is under test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
git -c credential.helper='!gh auth git-credential' push https://github.com/bsg62/photon.git HEAD:refs/heads/feat/people
gh pr create --draft --head feat/people --title "feat: recognise and name people" --body "Draft. Spec: docs/superpowers/specs/2026-10-03-photon-people-design.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks <number> --watch
```

Straight after a push `gh pr checks` can report nothing; wait until checks exist. **If a job fails because of the model's size, stop and report.** Record the release binary's growth: `ls -la` of `target/release/photon` before and after is not available here, so record the `.deb` size from CI's release-build artefact if the workflow keeps one, or say it was not measured.

---

### Task 2: Alignment

**Files:**
- Create: `crates/photon-core/src/face_embed/align.rs`
- Modify: `crates/photon-core/src/face_embed/mod.rs` (`mod align;`)

**Interfaces:**
- Produces (`pub(crate)` in `face_embed::align`): `REFERENCE: [(f32, f32); 5]`; `struct Similarity { a: f32, b: f32, tx: f32, ty: f32 }` with `apply(&self, (f32, f32)) -> (f32, f32)` and `invert(&self, (f32, f32)) -> (f32, f32)`; `fn fit(points: &[(f32, f32); 5]) -> Option<Similarity>`; `fn sample(image: &image::RgbImage, to_reference: &Similarity) -> Vec<f32>` (planar RGB, `3 * 112 * 112`, values 0..255).

- [ ] **Step 1: Failing tests**

```rust
//! Bringing a face to where the model expects it: the rotation, uniform scale and shift
//! that best take its five landmarks to the model's reference points, and the 112x112
//! picture sampled through it.

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn landmarks_at_the_reference_need_no_transform() {
        let s = fit(&REFERENCE).unwrap();
        assert!((s.a - 1.0).abs() < 1e-5 && s.b.abs() < 1e-5, "{s:?}");
        assert!(s.tx.abs() < 1e-3 && s.ty.abs() < 1e-3, "{s:?}");
    }

    /// A face turned 30 degrees, made 2.5 times larger and moved is brought back: each of
    /// its landmarks lands on its reference point.
    #[test]
    fn a_turned_scaled_shifted_face_is_brought_back() {
        let (angle, k, dx, dy) = (30f32.to_radians(), 2.5f32, 400.0f32, 250.0f32);
        let moved = REFERENCE.map(|(x, y)| {
            (
                k * (x * angle.cos() - y * angle.sin()) + dx,
                k * (x * angle.sin() + y * angle.cos()) + dy,
            )
        });
        let s = fit(&moved).unwrap();
        for (p, q) in moved.iter().zip(REFERENCE.iter()) {
            assert!(close(s.apply(*p), *q), "{:?} -> {:?}, want {q:?}", p, s.apply(*p));
        }
    }

    #[test]
    fn invert_undoes_apply() {
        let s = Similarity { a: 0.8, b: -0.3, tx: 12.0, ty: -7.0 };
        for p in [(0.0, 0.0), (55.5, 70.25), (-3.0, 400.0)] {
            assert!(close(s.invert(s.apply(p)), p));
        }
    }

    /// Five landmarks on one spot (a broken detection) have no transform.
    #[test]
    fn landmarks_on_one_spot_have_no_transform() {
        assert!(fit(&[(10.0, 10.0); 5]).is_none());
    }

    /// With no transform, sampling copies the picture: red then green then blue planes,
    /// each pixel where it was.
    #[test]
    fn the_identity_samples_the_picture_as_it_is() {
        let img = RgbImage::from_fn(112, 112, |x, y| Rgb([x as u8, y as u8, 7]));
        let planes = sample(&img, &Similarity { a: 1.0, b: 0.0, tx: 0.0, ty: 0.0 });
        let plane = 112 * 112;
        let at = |c: usize, x: usize, y: usize| planes[c * plane + y * 112 + x];
        assert_eq!((at(0, 30, 5), at(1, 30, 5), at(2, 30, 5)), (30.0, 5.0, 7.0));
        assert_eq!((at(0, 111, 100), at(1, 111, 100)), (111.0, 100.0));
    }

    /// What falls outside the picture is black.
    #[test]
    fn outside_the_picture_is_black() {
        let img = RgbImage::from_pixel(50, 50, Rgb([200, 200, 200]));
        let planes = sample(&img, &Similarity { a: 1.0, b: 0.0, tx: 0.0, ty: 0.0 });
        assert_eq!(planes[10 * 112 + 10], 200.0);
        assert_eq!(planes[100 * 112 + 100], 0.0);
    }
}
```

Run: `cargo test -p photon-core --lib face_embed::align` — expected: compile errors.

- [ ] **Step 2: Implement**

```rust
use image::RgbImage;

/// Where the model expects the eyes, the nose tip and the mouth corners, in its 112x112
/// input, as OpenCV's `FaceRecognizerSF` aligns them. The order is YuNet's: image-left eye
/// first.
pub(crate) const REFERENCE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

const SIDE: usize = 112;

/// `u = a x - b y + tx`, `v = b x + a y + ty`: a rotation and uniform scale (`a`, `b`)
/// and a shift, from the picture's pixels to the model's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Similarity {
    pub a: f32,
    pub b: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Similarity {
    pub fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (self.a * x - self.b * y + self.tx, self.b * x + self.a * y + self.ty)
    }

    pub fn invert(&self, (u, v): (f32, f32)) -> (f32, f32) {
        let det = self.a * self.a + self.b * self.b;
        let (u, v) = (u - self.tx, v - self.ty);
        ((self.a * u + self.b * v) / det, (-self.b * u + self.a * v) / det)
    }
}

/// The least-squares similarity taking `points` to [`REFERENCE`], or `None` when the
/// points do not span anything (all in one place).
pub(crate) fn fit(points: &[(f32, f32); 5]) -> Option<Similarity> {
    let mean = |p: &[(f32, f32); 5]| {
        let (sx, sy) = p.iter().fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
        (sx / 5.0, sy / 5.0)
    };
    let (px, py) = mean(points);
    let (qx, qy) = mean(&REFERENCE);
    let (mut along, mut across, mut spread) = (0.0f32, 0.0f32, 0.0f32);
    for ((x, y), (u, v)) in points.iter().zip(REFERENCE.iter()) {
        let (x, y, u, v) = (x - px, y - py, u - qx, v - qy);
        along += x * u + y * v;
        across += x * v - y * u;
        spread += x * x + y * y;
    }
    if spread < 1e-6 {
        return None;
    }
    let (a, b) = (along / spread, across / spread);
    Some(Similarity {
        a,
        b,
        tx: qx - (a * px - b * py),
        ty: qy - (b * px + a * py),
    })
}

/// The model's input: each of its 112x112 pixels looked up in `image` through the inverse
/// of `to_reference`, bilinearly, black outside the picture. Planar RGB, 0..255, as
/// OpenCV feeds the model (it swaps its BGR to RGB for this one).
pub(crate) fn sample(image: &RgbImage, to_reference: &Similarity) -> Vec<f32> {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let plane = SIDE * SIDE;
    let mut out = vec![0f32; 3 * plane];
    let pixel = |x: i64, y: i64, c: usize| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            image.get_pixel(x as u32, y as u32)[c] as f32
        }
    };
    for v in 0..SIDE {
        for u in 0..SIDE {
            let (x, y) = to_reference.invert((u as f32, v as f32));
            let (x0, y0) = (x.floor() as i64, y.floor() as i64);
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            for c in 0..3 {
                out[c * plane + v * SIDE + u] = pixel(x0, y0, c) * (1.0 - fx) * (1.0 - fy)
                    + pixel(x0 + 1, y0, c) * fx * (1.0 - fy)
                    + pixel(x0, y0 + 1, c) * (1.0 - fx) * fy
                    + pixel(x0 + 1, y0 + 1, c) * fx * fy;
            }
        }
    }
    out
}
```

Mark the items `#[allow(dead_code)]` with a comment naming Task 3, which calls them, if clippy flags them.

- [ ] **Step 3: Run, probe, commit**

Run: `cargo test -p photon-core --lib face_embed::align` — 6 passed.

Probes (exact change, run, see the named test fail, restore, `touch`):

| Change | Must fail |
|---|---|
| `across += x * v - y * u` → `across += 0.0` | `a_turned_scaled_shifted_face_is_brought_back` |
| `tx: qx - (a * px - b * py)` → `tx: qx` | `a_turned_scaled_shifted_face_is_brought_back` |
| `invert`'s `(-self.b * u + self.a * v)` → `(self.b * u + self.a * v)` | `invert_undoes_apply` |
| `if spread < 1e-6 { return None; }` removed | `landmarks_on_one_spot_have_no_transform` |
| in `sample`, `c * plane +` → `0 * plane + c +` | `the_identity_samples_the_picture_as_it_is` |
| in `pixel`, the bounds check returns `255.0` | `outside_the_picture_is_black` |

Gate, commit `feat(people): align a face to the embedding model's reference points` with the probe results.

---

### Task 3: The embedder, picture and face in, vector out

**Files:**
- Modify: `crates/photon-core/src/face_embed/mod.rs`
- Create: `crates/photon-core/testdata/faces/other.jpg`; modify `crates/photon-core/testdata/faces/README.md`

**Interfaces:**
- Consumes: `align::{fit, sample}`; `face_detect::{Detector, Rect}`.
- Produces: `pub struct FaceBox { pub rect: Rect, pub landmarks: [(f32, f32); 5] }` (Clone, Copy, Debug, PartialEq), with all coordinates fractions of the image; `Embedder::embed(&self, image: &DynamicImage, face: &FaceBox) -> Result<Option<Embedding>>`; `pub fn similarity(a: &[f32], b: &[f32]) -> f32` (cosine, 0 when either is zero); `pub fn to_blob(e: &Embedding) -> Vec<u8>`; `pub fn from_blob(bytes: &[u8]) -> Option<Embedding>` (`None` unless exactly 512 bytes).

- [ ] **Step 1: The second fixture**

```bash
curl -sSL -A "photon-tests/0.1 (https://github.com/bsg62/photon)" \
  -o crates/photon-core/testdata/faces/other.jpg \
  "https://thumb.wikimedia.org/wikipedia/commons/thumb/b/bf/Gray-haired_man_portrait_%28Unsplash%29.jpg/960px-Gray-haired_man_portrait_%28Unsplash%29.jpg"
file crates/photon-core/testdata/faces/other.jpg
```

Expected: a 960x640 JPEG. Credit it in `testdata/faces/README.md`'s table: [Gray-haired man portrait (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Gray-haired_man_portrait_(Unsplash).jpg), Commons' 960 px rendering, Foto Sushi, CC0, "a second person, for telling two people apart".

- [ ] **Step 2: Failing tests** (in `mod.rs`'s tests)

```rust
    use crate::face_detect::{Detector, Rect};
    use image::DynamicImage;
    use std::path::Path;

    fn fixture(name: &str) -> DynamicImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/faces").join(name);
        image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The one face in a picture, as the pass would hand it over.
    fn the_face(detector: &Detector, img: &DynamicImage) -> FaceBox {
        let faces = detector.detect(img).unwrap();
        assert_eq!(faces.len(), 1, "{faces:?}");
        FaceBox { rect: faces[0].rect, landmarks: faces[0].landmarks }
    }

    fn vector(embedder: &Embedder, detector: &Detector, img: &DynamicImage) -> Embedding {
        embedder.embed(img, &the_face(detector, img)).unwrap().expect("large enough")
    }

    /// The pipeline is wired right: one person's face, the photo shrunk to half and saved
    /// again as a JPEG, still points the same way; another person's does not. (Whether
    /// recognition is good is the spike's evidence, in the spec, not this test's.)
    #[test]
    fn the_same_face_matches_and_another_does_not() {
        let (embedder, detector) = (Embedder::new().unwrap(), Detector::new().unwrap());
        let portrait = fixture("portrait.jpg");
        let copy = {
            let small = portrait.resize(480, 480, image::imageops::FilterType::Triangle);
            let mut bytes = Vec::new();
            small
                .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
                .unwrap();
            image::load_from_memory(&bytes).unwrap()
        };
        let a = vector(&embedder, &detector, &portrait);
        let b = vector(&embedder, &detector, &copy);
        let other = vector(&embedder, &detector, &fixture("other.jpg"));
        assert!(similarity(&a, &b) > 0.9, "same face: {}", similarity(&a, &b));
        assert!(similarity(&a, &other) < 0.363, "two people: {}", similarity(&a, &other));
    }

    #[test]
    fn a_vector_has_unit_length() {
        let (embedder, detector) = (Embedder::new().unwrap(), Detector::new().unwrap());
        let a = vector(&embedder, &detector, &fixture("portrait.jpg"));
        let len = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-4, "{len}");
    }

    /// The floor is measured in pixels of the picture the face is cut from.
    #[test]
    fn a_face_under_the_floor_is_not_embedded() {
        let embedder = Embedder::new().unwrap();
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(1000, 1000));
        let at = |width: f64| FaceBox {
            rect: Rect { left: 0.5, top: 0.5, right: 0.5 + width, bottom: 0.5 + width },
            landmarks: [(0.51, 0.51), (0.53, 0.51), (0.52, 0.52), (0.51, 0.53), (0.53, 0.53)],
        };
        assert_eq!(embedder.embed(&img, &at(0.034)).unwrap(), None); // 34 px
        assert!(embedder.embed(&img, &at(0.036)).unwrap().is_some()); // 36 px
    }

    #[test]
    fn similarity_is_the_cosine() {
        assert!((similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
        assert!((similarity(&[2.0, 0.0], &[3.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn a_vector_round_trips_through_its_blob() {
        let mut e = [0f32; DIM];
        e[0] = 0.25;
        e[127] = -1.5;
        let blob = to_blob(&e);
        assert_eq!(blob.len(), 512);
        assert_eq!(from_blob(&blob), Some(e));
        assert_eq!(from_blob(&blob[..511]), None);
    }
```

Run: `cargo test -p photon-core --lib face_embed` — expected: compile errors.

- [ ] **Step 3: Implement** (in `mod.rs`)

```rust
use crate::face_detect::Rect;
use image::DynamicImage;

mod align;

/// One face as `detected_faces` stores it: fractions of the picture it was found in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    pub rect: Rect,
    pub landmarks: [(f32, f32); 5],
}

/// How alike two vectors point: the cosine, 1 for the same direction. Either may be an
/// unnormalised sum (a group's centroid); zero length gives 0.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let (la, lb) = (
        a.iter().map(|x| x * x).sum::<f32>().sqrt(),
        b.iter().map(|x| x * x).sum::<f32>().sqrt(),
    );
    if la == 0.0 || lb == 0.0 { 0.0 } else { dot / (la * lb) }
}

/// 128 little-endian `f32`, the form `detected_faces.embedding` holds.
pub fn to_blob(e: &Embedding) -> Vec<u8> {
    e.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(bytes: &[u8]) -> Option<Embedding> {
    if bytes.len() != DIM * 4 {
        return None;
    }
    let mut e = [0f32; DIM];
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        e[i] = f32::from_le_bytes(chunk.try_into().ok()?);
    }
    Some(e)
}

impl Embedder {
    /// The face's vector, or `None` when it is narrower than [`MIN_FACE_PX`] in `image`.
    pub fn embed(&self, image: &DynamicImage, face: &FaceBox) -> Result<Option<Embedding>> {
        let (w, h) = (image.width() as f32, image.height() as f32);
        if ((face.rect.right - face.rect.left) as f32) * w < MIN_FACE_PX {
            return Ok(None);
        }
        let points = face.landmarks.map(|(x, y)| (x * w, y * h));
        let to_reference = align::fit(&points)
            .ok_or_else(|| model_error("a face's landmarks are all in one place"))?;
        let input = align::sample(&image.to_rgb8(), &to_reference);
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, SIDE, SIDE), input)
            .map_err(model_error)?
            .into();
        let out = (self.run)(tensor).map_err(model_error)?;
        let raw: Vec<f32> = out
            .first()
            .ok_or_else(|| model_error("no output"))?
            .to_plain_array_view::<f32>()
            .map_err(model_error)?
            .iter()
            .copied()
            .collect();
        if raw.len() != DIM {
            return Err(model_error(format!("{} numbers, expected {DIM}", raw.len())));
        }
        let len = raw.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !len.is_finite() || len == 0.0 {
            return Err(model_error("a vector that is not a number"));
        }
        let mut e = [0f32; DIM];
        for (slot, x) in e.iter_mut().zip(&raw) {
            *slot = x / len;
        }
        Ok(Some(e))
    }
}
```

Remove the `#[allow(dead_code)]`s left by Tasks 1 and 2.

- [ ] **Step 4: Run, probe, commit**

Run: `cargo test -p photon-core --lib face_embed` — all pass. Record the two similarities the first test printed (re-run it with `-- --nocapture` and a temporary `eprintln!` if needed, removed before commit). **If the same face is not above 0.9 or the two people are not below 0.363, do not loosen the test: report the numbers.**

| Change | Must fail |
|---|---|
| `x * w, y * h` → `x * h, y * w` in `points` | `the_same_face_matches_and_another_does_not` (the portrait is not square) |
| `*slot = x / len` → `*slot = *x` | `a_vector_has_unit_length` |
| `< MIN_FACE_PX` → `< 0.0` | `a_face_under_the_floor_is_not_embedded` |
| `align::sample(&image.to_rgb8(), …)` with the planes reversed (B, G, R) | **record the result**: a face may still match itself. If nothing fails, say so; the channel order is the model's training input, not something a fixture discriminates. |

Gate, commit `feat(people): embed a face` with the probe results and the two measured similarities.

---

### Task 4: Schema 25, embedding storage, and what clears what

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs`, `mod.rs`, `settings.rs`, `detected_faces.rs`
- Modify: `crates/photon-core/src/error.rs`

**Interfaces:**
- Consumes: `face_embed::{FaceBox, Embedding, to_blob, EMBEDDER_VERSION}`; `face_detect::merge::same_face`.
- Produces, on `Library` (in `detected_faces.rs`):
  - `pub struct EmbedCandidate { pub item_id: i64, pub thumb_key: u64, pub size: i64, pub mtime_ms: i64, pub edit: Edit, pub faces: Vec<(i64, FaceBox)> }` (Clone, Debug), re-exported from `library`
  - `embed_candidates(&self, after_item: i64, limit: usize, version: i64) -> Result<Vec<EmbedCandidate>>`
  - `write_embeddings(&self, batch: &[(EmbedCandidate, Vec<(i64, Option<Embedding>)>)], version: i64) -> Result<usize>` (faces written; 0 with the switch off)
  - `embed_progress(&self, version: i64) -> Result<(u64, u64)>` (faces with a current version, all faces, on live images)
- Produces in `error.rs`: `Error::EmptyPersonName`, `Error::NotAPerson(i64)`, `Error::PersonNamed(i64)`.

- [ ] **Step 1: The migration**

Append to `MIGRATIONS`:

```rust
    r#"
-- People: groups of faces photon takes for one person, named by the user or not. See
-- `photon_core::people` and `library/people.rs`. No centroid is stored: it is computed from
-- the faces at the start of each grouping step, so no writer can leave it out of date.
CREATE TABLE people (
    id      INTEGER PRIMARY KEY,
    name    TEXT,
    ignored INTEGER NOT NULL DEFAULT 0
);
-- A person's Picasa contacts. A table, not a column: a Picasa library can hold two
-- contacts for one human. A contact belongs to at most one person.
CREATE TABLE person_contacts (
    contact   TEXT PRIMARY KEY,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE
);
CREATE INDEX person_contacts_person ON person_contacts(person_id);
-- A face's vector (128 little-endian f32), the embedder that made it (set even when the
-- face was too small to have one), its group, whether the user put it there, and whether
-- it is to be left out of grouping.
ALTER TABLE detected_faces ADD COLUMN embedding BLOB;
ALTER TABLE detected_faces ADD COLUMN embedding_version INTEGER;
ALTER TABLE detected_faces ADD COLUMN person_id INTEGER REFERENCES people(id) ON DELETE SET NULL;
ALTER TABLE detected_faces ADD COLUMN confirmed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE detected_faces ADD COLUMN ignored INTEGER NOT NULL DEFAULT 0;
CREATE INDEX detected_faces_person ON detected_faces(person_id);
-- "Not this person": the face is never put in that group again.
CREATE TABLE face_rejections (
    face_id   INTEGER NOT NULL REFERENCES detected_faces(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    PRIMARY KEY (face_id, person_id)
);
"#,
```

Tripwires: in `library/mod.rs` `24` → `25` twice, add `'people', 'person_contacts', 'face_rejections'` to the table list and `13` → `16`; every `assert_eq!(version, 24);` at the end of a migration test in `schema.rs` → `25`. Add a `migration_25_…` test in the shape of `migration_24_adds_detected_faces_to_a_populated_library`: seed `MIGRATIONS[..24]` with one item and one detected face, open, and assert the face has `embedding_version IS NULL`, `person_id IS NULL`, `confirmed = 0`, and the three tables exist.

- [ ] **Step 2: Errors**

In `error.rs`'s `Error`:

```rust
    #[error("a person needs a name")]
    EmptyPersonName,
    /// A merge into, or an operation on, something that is not a named person.
    #[error("that is not a named person")]
    NotAPerson(i64),
    /// Ignoring a named person: delete them first, which makes them a group again.
    #[error("a named person cannot be ignored; delete them first")]
    PersonNamed(i64),
```

- [ ] **Step 3: Failing tests** (in `detected_faces.rs`'s tests, using its `seeded`, `face`, `V` helpers)

```rust
    use crate::face_embed::{EMBEDDER_VERSION as EV, Embedding, FaceBox};

    fn unit(i: usize) -> Embedding {
        let mut e = [0f32; 128];
        e[i] = 1.0;
        e
    }

    /// One photo with two detected faces, ready to embed.
    fn detected(lib: &Library, ids: &[i64]) {
        let c = lib.face_candidates(0, 10, V).unwrap();
        let batch: Vec<_> = c.iter().map(|c| (c.clone(), vec![face(0.1), face(0.5)])).collect();
        lib.write_face_batch(&batch, V).unwrap();
        assert!(!ids.is_empty());
    }

    #[test]
    fn faces_to_embed_are_listed_by_photo_with_their_boxes() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        detected(&lib, &ids);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        assert_eq!(c.iter().map(|c| c.item_id).collect::<Vec<_>>(), ids);
        assert_eq!(c[0].faces.len(), 2);
        assert_eq!(c[0].faces[0].1.rect, face(0.1).rect);
        assert_eq!(c[0].faces[0].1.landmarks, face(0.1).landmarks);
        // Paging is by photo.
        assert_eq!(lib.embed_candidates(ids[0], 10, EV).unwrap().len(), 1);
    }

    #[test]
    fn an_embedded_face_is_no_longer_a_candidate() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib, &ids);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        let faces: Vec<_> = c[0].faces.iter().map(|(id, _)| *id).collect();
        let written = lib
            .write_embeddings(&[(c[0].clone(), vec![(faces[0], Some(unit(0))), (faces[1], None)])], EV)
            .unwrap();
        assert_eq!(written, 2);
        assert!(lib.embed_candidates(0, 10, EV).unwrap().is_empty(), "a face too small is looked at too");
        assert_eq!(lib.embed_progress(EV).unwrap(), (2, 2));
    }

    #[test]
    fn embeddings_are_refused_with_the_switch_off_or_the_photo_moved_on() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib, &ids);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        let one = vec![(c[0].faces[0].0, Some(unit(0)))];
        lib.set_item_edit(ids[0], Edit { turns: 1, crop: None }).unwrap();
        assert_eq!(lib.write_embeddings(&[(c[0].clone(), one.clone())], EV).unwrap(), 0);
        let (_dir2, lib2, ids2) = seeded(&["a.jpg"]);
        detected(&lib2, &ids2);
        let c2 = lib2.embed_candidates(0, 10, EV).unwrap();
        lib2.set_face_detection(false).unwrap();
        let one2 = vec![(c2[0].faces[0].0, Some(unit(0)))];
        assert_eq!(lib2.write_embeddings(&[(c2[0].clone(), one2)], EV).unwrap(), 0);
    }

    /// A detector re-run over an unchanged picture keeps what the user did: the face at
    /// the same place inherits its group, its confirmation and its rejections.
    #[test]
    fn a_detector_rerun_carries_a_confirmed_face_over() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib, &ids);
        let (keep, gone) = {
            let c = lib.embed_candidates(0, 10, EV).unwrap();
            (c[0].faces[0].0, c[0].faces[1].0)
        };
        let w = lib.writer();
        w.execute("INSERT INTO people (id, name) VALUES (7, 'Anna'), (8, NULL)", []).unwrap();
        w.execute("UPDATE detected_faces SET person_id = 7, confirmed = 1 WHERE id = ?1", [keep]).unwrap();
        w.execute("INSERT INTO face_rejections VALUES (?1, 8)", [keep]).unwrap();
        w.execute("UPDATE detected_faces SET ignored = 1 WHERE id = ?1", [gone]).unwrap();
        drop(w);
        // The same picture, detected again by a newer detector: one face where the
        // confirmed one was, nudged, and nothing where the other was.
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        let mut moved = face(0.1);
        moved.rect.left += 0.01;
        moved.rect.right += 0.01;
        lib.write_face_batch(&[(c[0].clone(), vec![moved])], V + 1).unwrap();
        let r = lib.reader().unwrap();
        let (person, confirmed, ignored, rejected): (Option<i64>, i64, i64, i64) = r
            .query_row(
                "SELECT person_id, confirmed, ignored,
                        (SELECT count(*) FROM face_rejections WHERE face_id = f.id AND person_id = 8)
                 FROM detected_faces f",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((person, confirmed, ignored, rejected), (Some(7), 1, 0, 1));
    }

    /// An edit is not a re-run: the picture changed, and the user's confirmation goes with
    /// its rows (the face comes back as a suggestion once grouped again).
    #[test]
    fn an_edit_drops_the_confirmation() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib, &ids);
        lib.writer().execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", []).unwrap();
        lib.writer().execute("UPDATE detected_faces SET person_id = 7, confirmed = 1", []).unwrap();
        lib.set_item_edit(ids[0], Edit { turns: 1, crop: None }).unwrap();
        let n: i64 = lib.reader().unwrap()
            .query_row("SELECT count(*) FROM detected_faces WHERE confirmed = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn switching_off_deletes_people_links_and_rejections() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib, &ids);
        let w = lib.writer();
        w.execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", []).unwrap();
        w.execute("INSERT INTO person_contacts VALUES ('ada', 7)", []).unwrap();
        w.execute("INSERT INTO face_rejections SELECT id, 7 FROM detected_faces", []).unwrap();
        drop(w);
        lib.set_face_detection(false).unwrap();
        let r = lib.reader().unwrap();
        for table in ["people", "person_contacts", "face_rejections", "detected_faces"] {
            let n: i64 = r.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    #[test]
    fn the_embedding_candidates_are_found_through_the_face_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {EMBED_CANDIDATES_SQL}")).unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params![0, EV, 10], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(plan.iter().any(|s| s.contains("detected_faces_item")), "{plan:?}");
    }
```

Run: `cargo test -p photon-core --lib detected_faces` — expected: compile errors.

- [ ] **Step 4: Implement**

In `settings.rs`, `set_face_detection`'s `if !enabled` block gains, before the existing delete:

```rust
            tx.execute("DELETE FROM face_rejections", [])?;
            tx.execute("DELETE FROM person_contacts", [])?;
            tx.execute("DELETE FROM people", [])?;
```

and its doc comment says that people the user named go too (the UI asks first; plan 2).

In `detected_faces.rs`:

```rust
/// Photos, in id order after a given one, with at least one face whose
/// `embedding_version` is not the current one; each with every such face. Driven from
/// `detected_faces` through its item index: a library has far fewer faces than photos.
const EMBED_CANDIDATES_SQL: &str = "SELECT f.id, f.item_id, f.left, f.top, f.right, f.bottom, f.landmarks,
            i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE f.item_id IN (SELECT DISTINCT item_id FROM detected_faces
                         WHERE item_id > ?1 AND embedding_version IS NOT ?2
                         ORDER BY item_id LIMIT ?3)
       AND f.embedding_version IS NOT ?2
       AND i.missing_since IS NULL AND i.thumb_state = 1
     ORDER BY f.item_id, f.id";

const EMBED_PROGRESS_SQL: &str = "SELECT count(*) FILTER (WHERE f.embedding_version IS ?1), count(*)
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE i.missing_since IS NULL";

fn landmarks_from_blob(b: &[u8]) -> [(f32, f32); 5] {
    let f = |i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap_or([0; 4]));
    [(f(0), f(1)), (f(2), f(3)), (f(4), f(5)), (f(6), f(7)), (f(8), f(9))]
}
```

`EmbedCandidate` as in **Produces**. `embed_candidates` runs the query and folds consecutive rows of one `item_id` into one candidate, computing `thumb_key` as `face_candidates` does. A `landmarks` blob shorter than 40 bytes gives zeros, which `align::fit` refuses, so that face fails rather than panics.

`write_embeddings`: one transaction; return 0 if `face_detection_on` is false. Per candidate, check the photo is unchanged with `SELECT 1 FROM items WHERE id = ?1 AND size = ?2 AND mtime_ms = ?3 AND missing_since IS NULL AND edit_turns = ?4 AND edit_crop IS ?5` (skip it if not), then per face `UPDATE detected_faces SET embedding = ?2, embedding_version = ?3 WHERE id = ?1 AND item_id = ?4`, counting rows changed. `None` writes `embedding = NULL` with the version set.

`embed_progress` runs `EMBED_PROGRESS_SQL`, as `face_progress` does.

**The carry-over**, in `write_face_batch`, replacing the bare `delete.execute(params![candidate.id])?;` and the insert loop for a marked photo:

```rust
                // What the user did to the faces this picture had, to hand to the faces
                // found at the same places. The picture is unchanged (the guard above
                // checked size, mtime and edit), so a face at the same place is the same
                // face; an edit or a new file clears the rows before this and carries
                // nothing.
                let old: Vec<Carried> = read_carried(&tx, candidate.id)?;
                delete.execute(params![candidate.id])?;
                let mut taken = vec![false; old.len()];
                for face in faces {
                    insert.execute(params![/* as before */])?;
                    let id = tx.last_insert_rowid();
                    if let Some(i) = old.iter().enumerate().position(|(i, o)| {
                        !taken[i] && crate::face_detect::merge::same_face(&o.rect, &face.rect)
                    }) {
                        taken[i] = true;
                        carry(&tx, id, &old[i])?;
                    }
                }
```

with

```rust
struct Carried {
    rect: Rect,
    person_id: Option<i64>,
    confirmed: bool,
    ignored: bool,
    rejected: Vec<i64>,
}

fn read_carried(tx: &rusqlite::Connection, item_id: i64) -> rusqlite::Result<Vec<Carried>> {
    let mut faces = tx.prepare_cached(
        "SELECT id, left, top, right, bottom, person_id, confirmed, ignored
         FROM detected_faces WHERE item_id = ?1 ORDER BY id",
    )?;
    let mut rejections =
        tx.prepare_cached("SELECT person_id FROM face_rejections WHERE face_id = ?1")?;
    let rows: Vec<(i64, Carried)> = faces
        .query_map(params![item_id], |r| {
            Ok((
                r.get(0)?,
                Carried {
                    rect: Rect { left: r.get(1)?, top: r.get(2)?, right: r.get(3)?, bottom: r.get(4)? },
                    person_id: r.get(5)?,
                    confirmed: r.get::<_, i64>(6)? == 1,
                    ignored: r.get::<_, i64>(7)? == 1,
                    rejected: Vec::new(),
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|(id, mut c)| {
            c.rejected = rejections
                .query_map(params![id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(c)
        })
        .collect()
}

fn carry(tx: &rusqlite::Connection, face_id: i64, from: &Carried) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE detected_faces SET person_id = ?2, confirmed = ?3, ignored = ?4 WHERE id = ?1",
        params![face_id, from.person_id, from.confirmed as i64, from.ignored as i64],
    )?;
    for person in &from.rejected {
        tx.execute(
            "INSERT OR IGNORE INTO face_rejections (face_id, person_id) VALUES (?1, ?2)",
            params![face_id, person],
        )?;
    }
    Ok(())
}
```

Re-export `EmbedCandidate` from `library/mod.rs`.

- [ ] **Step 5: Run, probe, commit**

Run: `cargo test -p photon-core --lib detected_faces && cargo test -p photon-core --lib library`.

| Change | Must fail |
|---|---|
| remove the `face_detection_on` check in `write_embeddings` | `embeddings_are_refused_with_the_switch_off_or_the_photo_moved_on` |
| remove the unchanged-photo check in `write_embeddings` | the same test |
| `carry(&tx, id, &old[i])?;` removed | `a_detector_rerun_carries_a_confirmed_face_over` |
| `carry`'s rejection loop removed | the same test (its `rejected` column) |
| the three new `DELETE`s in `set_face_detection` removed | `switching_off_deletes_people_links_and_rejections` |
| `AND f.embedding_version IS NOT ?2` (the outer one) removed | `an_embedded_face_is_no_longer_a_candidate` |

Gate, commit `feat(people): schema 25, face vectors, and what survives a re-run` with the probe results.

---

### Task 5: The grouping rule

**Files:**
- Create: `crates/photon-core/src/people/mod.rs`
- Modify: `crates/photon-core/src/lib.rs` (`pub mod people;`)

**Interfaces:**
- Consumes: `face_embed::{similarity, DIM}`.
- Produces: `pub const GROUP_SIMILARITY: f32 = 0.50;`; `pub struct Group { pub id: i64, pub sum: Vec<f32>, pub count: usize }` (Clone, Debug); `pub enum Choice { Join(i64), New }` (Debug, PartialEq); `pub fn choose(face: &[f32], groups: &[Group], rejected: &HashSet<i64>) -> Choice`; `pub fn counts_toward_centroid(named: bool, confirmed: bool) -> bool`; `impl Group { pub fn add(&mut self, face: &[f32]) }`.

- [ ] **Step 1: Failing tests**

```rust
//! Which group a face belongs in. Pure: vectors and group sums in, a choice out. The
//! library applies it (`library/people.rs`), one face at a time in id order.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A unit vector at `deg` degrees in the first two dimensions: two of them have a
    /// similarity of cos(difference).
    fn at(deg: f32) -> Vec<f32> {
        let mut v = vec![0f32; 128];
        v[0] = deg.to_radians().cos();
        v[1] = deg.to_radians().sin();
        v
    }

    fn group(id: i64, deg: f32) -> Group {
        Group { id, sum: at(deg), count: 1 }
    }

    const NONE: &[i64] = &[];

    fn rejected(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    /// cos(59°) = 0.515 joins; cos(61°) = 0.485 does not.
    #[test]
    fn a_face_joins_at_the_threshold_and_not_below() {
        assert_eq!(choose(&at(59.0), &[group(1, 0.0)], &rejected(NONE)), Choice::Join(1));
        assert_eq!(choose(&at(61.0), &[group(1, 0.0)], &rejected(NONE)), Choice::New);
    }

    #[test]
    fn the_nearest_group_wins() {
        let groups = [group(1, 0.0), group(2, 30.0)];
        assert_eq!(choose(&at(20.0), &groups, &rejected(NONE)), Choice::Join(2));
    }

    #[test]
    fn a_rejected_group_is_passed_over() {
        let groups = [group(1, 0.0), group(2, 40.0)];
        assert_eq!(choose(&at(5.0), &groups, &rejected(&[1])), Choice::Join(2));
        assert_eq!(choose(&at(5.0), &[group(1, 0.0)], &rejected(&[1])), Choice::New);
    }

    #[test]
    fn a_group_with_no_centroid_is_passed_over() {
        let empty = Group { id: 1, sum: vec![0f32; 128], count: 0 };
        assert_eq!(choose(&at(0.0), &[empty], &rejected(NONE)), Choice::New);
    }

    /// The comparison is with the group's average direction, not its first face: a group
    /// built from faces at 0° and 40° sits at 20°.
    #[test]
    fn a_group_is_compared_by_its_average() {
        let mut g = group(1, 0.0);
        g.add(&at(40.0));
        assert_eq!(g.count, 2);
        // 75° is 0.26 from the first face alone and 0.57 from the average at 20°.
        assert_eq!(choose(&at(75.0), &[g], &rejected(NONE)), Choice::Join(1));
        assert_eq!(choose(&at(75.0), &[group(1, 0.0)], &rejected(NONE)), Choice::New);
    }

    #[test]
    fn only_confirmed_faces_move_a_named_person() {
        assert!(counts_toward_centroid(false, false));
        assert!(counts_toward_centroid(false, true));
        assert!(counts_toward_centroid(true, true));
        assert!(!counts_toward_centroid(true, false));
    }
}
```

Run: `cargo test -p photon-core --lib people` — compile errors.

- [ ] **Step 2: Implement**

```rust
use crate::face_embed::similarity;
use std::collections::HashSet;

/// How alike a face and a group's average must be for the face to join it. Measured
/// 2026-10-03 on 2,674 LFW faces (500 people with several photos, 700 with one): at 0.50
/// this rule misplaced 6-11 faces and kept 446-448 of the 500 people in one group; at
/// 0.45, 27-40 and 468; at 0.55, 2 and 411-414. Below 0.50 mixed groups rise quickly,
/// above it people split for little gain - and a split is one merge to fix, where a
/// mixed group is faces removed one at a time.
pub const GROUP_SIMILARITY: f32 = 0.50;

/// A group as the rule sees it: the sum of the vectors that count towards it.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: i64,
    pub sum: Vec<f32>,
    pub count: usize,
}

impl Group {
    pub fn add(&mut self, face: &[f32]) {
        for (s, x) in self.sum.iter_mut().zip(face) {
            *s += x;
        }
        self.count += 1;
    }
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    Join(i64),
    New,
}

/// The group `face` joins: the most alike, if at least [`GROUP_SIMILARITY`], passing over
/// groups the face was rejected from and groups nothing counts towards.
pub fn choose(face: &[f32], groups: &[Group], rejected: &HashSet<i64>) -> Choice {
    groups
        .iter()
        .filter(|g| g.count > 0 && !rejected.contains(&g.id))
        .map(|g| (g.id, similarity(&g.sum, face)))
        .filter(|(_, s)| *s >= GROUP_SIMILARITY)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(Choice::New, |(id, _)| Choice::Join(id))
}

/// Whether a face counts towards its group's average: every face of an unnamed or ignored
/// group, and only the confirmed faces of a named person, so that suggestions cannot pull
/// a person towards themselves.
pub fn counts_toward_centroid(named: bool, confirmed: bool) -> bool {
    !named || confirmed
}
```

- [ ] **Step 3: Run, probe, commit**

| Change | Must fail |
|---|---|
| `*s >= GROUP_SIMILARITY` → `*s > 0.0` | `a_face_joins_at_the_threshold_and_not_below` |
| `max_by` → `min_by` | `the_nearest_group_wins` |
| drop `&& !rejected.contains(&g.id)` | `a_rejected_group_is_passed_over` |
| drop `g.count > 0 &&` | `a_group_with_no_centroid_is_passed_over` (the zero sum gives similarity 0, so **record** whether it fails; if not, say so: `similarity` already returns 0 for a zero vector, and the `count` check is then belt and braces — keep it and say why in its comment) |
| `!named \|\| confirmed` → `true` | `only_confirmed_faces_move_a_named_person` |

Gate, commit `feat(people): the grouping rule`.

---

### Task 6: People in the library: grouping step, operations, page reads

The largest task. Split the commit in two if it helps review: the grouping step and the operations first, the page reads second.

**Files:**
- Create: `crates/photon-core/src/library/people.rs`
- Modify: `crates/photon-core/src/library/mod.rs` (`mod people;`, re-exports), `faces.rs` (`upsert_contacts` links by name)

**Interfaces:**
- Consumes: `people::{choose, Choice, Group, counts_toward_centroid}`; `face_embed::{from_blob, DIM}`; `face_detect::merge::{same_face, shown}`; `Error::{EmptyPersonName, NotAPerson, PersonNamed}`.
- Produces, on `Library`:
  - `group_ungrouped_faces(&self, cancel: &dyn Fn() -> bool) -> Result<usize>` (faces placed)
  - `has_ungrouped_faces(&self) -> Result<bool>`
  - `name_group(&self, group: i64, name: &str) -> Result<i64>` (the person the group ended in)
  - `rename_person(&self, person: i64, name: &str) -> Result<i64>`
  - `confirm_faces(&self, faces: &[i64]) -> Result<()>`
  - `reject_faces(&self, faces: &[i64]) -> Result<()>`
  - `merge_people(&self, from: i64, into: i64) -> Result<()>`
  - `set_person_ignored(&self, person: i64, ignored: bool) -> Result<()>`
  - `set_faces_ignored(&self, faces: &[i64], ignored: bool) -> Result<()>`
  - `delete_person(&self, person: i64) -> Result<()>`
  - `named_people_count(&self) -> Result<i64>`
  - `people_page(&self, strip: usize) -> Result<PeoplePage>`
  - `person_faces(&self, person: i64, which: FaceFilter, offset: usize, limit: usize) -> Result<Vec<PageFace>>`
- Produces types (serde camelCase, re-exported from `library`):
  - `PageFace { id: i64, item_id: i64, thumb_key: String, confirmed: bool }` (`thumb_key` in `grid::hex_key` form)
  - `Offer { name: String, contact: String, faces: i64 }`
  - `PageGroup { id: i64, name: Option<String>, face_count: i64, faces: Vec<PageFace>, offer: Option<Offer> }`
  - `PeoplePage { unnamed: Vec<PageGroup>, single_faces: Vec<PageFace>, single_count: i64, suggestions: Vec<PageGroup>, people: Vec<PageGroup>, ignored_groups: Vec<PageGroup>, ignored_faces: Vec<PageFace> }`
  - `enum FaceFilter { All, Confirmed, Unconfirmed }` (Deserialize, lowercase)

**Visibility.** Every read in this task that a user sees joins `items` and requires `i.hidden = 0 AND i.missing_since IS NULL`. The grouping step does not: hidden photos' faces are grouped, so unhiding is instant.

- [ ] **Step 1: Test helpers**

At the top of `people.rs`'s tests:

```rust
    use super::*;
    use crate::face_detect::{DETECTOR_VERSION as V, Detection, Rect};
    use crate::face_embed::{EMBEDDER_VERSION as EV, to_blob};
    use crate::media::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};

    /// A unit vector at `deg` degrees in the first two dimensions.
    fn at(deg: f32) -> [f32; 128] {
        let mut v = [0f32; 128];
        v[0] = deg.to_radians().cos();
        v[1] = deg.to_radians().sin();
        v
    }

    struct L {
        _dir: tempfile::TempDir,
        lib: Library,
        items: Vec<i64>,
    }

    /// One photo per entry of `faces`, each with faces at those angles, detected and
    /// embedded, the switch on, nothing grouped yet. Returns the face ids in order.
    fn library(faces: &[&[f32]]) -> (L, Vec<i64>) {
        let (dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, dir.path());
        let items = lib
            .insert_items(
                &(0..faces.len())
                    .map(|i| new_item(folder, &format!("{}/{i}.jpg", dir.path().display()), 1))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        for id in &items {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        lib.set_face_detection(true).unwrap();
        let c = lib.face_candidates(0, 100, V).unwrap();
        let batch: Vec<_> = c
            .iter()
            .zip(faces)
            .map(|(c, angles)| {
                let dets = angles
                    .iter()
                    .enumerate()
                    .map(|(k, _)| Detection {
                        rect: Rect { left: 0.1 * k as f64, top: 0.1, right: 0.1 * k as f64 + 0.08, bottom: 0.2 },
                        landmarks: [(0.0, 0.0); 5],
                        score: 0.9,
                    })
                    .collect();
                (c.clone(), dets)
            })
            .collect();
        lib.write_face_batch(&batch, V).unwrap();
        let mut ids = Vec::new();
        for (item, angles) in items.iter().zip(faces) {
            let rows: Vec<i64> = {
                let r = lib.reader().unwrap();
                let mut s = r.prepare("SELECT id FROM detected_faces WHERE item_id = ?1 ORDER BY id").unwrap();
                s.query_map([item], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
            };
            for (id, deg) in rows.iter().zip(angles.iter()) {
                lib.writer()
                    .execute(
                        "UPDATE detected_faces SET embedding = ?2, embedding_version = ?3 WHERE id = ?1",
                        rusqlite::params![id, to_blob(&at(*deg)), EV],
                    )
                    .unwrap();
                ids.push(*id);
            }
        }
        (L { _dir: dir, lib, items }, ids)
    }

    fn never() -> bool {
        false
    }

    fn person_of(lib: &Library, face: i64) -> (Option<i64>, bool) {
        lib.reader()
            .unwrap()
            .query_row(
                "SELECT person_id, confirmed FROM detected_faces WHERE id = ?1",
                [face],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1)),
            )
            .unwrap()
    }
```

- [ ] **Step 2: Failing tests for the grouping step and the operations**

```rust
    #[test]
    fn alike_faces_share_a_group_and_others_start_their_own() {
        let (l, f) = library(&[&[0.0], &[10.0], &[90.0]]);
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 3);
        let (a, b, c) = (person_of(&l.lib, f[0]).0, person_of(&l.lib, f[1]).0, person_of(&l.lib, f[2]).0);
        assert!(a.is_some() && a == b && c != a, "{a:?} {b:?} {c:?}");
        assert!(!l.lib.has_ungrouped_faces().unwrap());
    }

    #[test]
    fn grouping_is_in_face_order_and_repeatable() {
        let (l, f) = library(&[&[0.0], &[40.0], &[75.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        // 0° starts a group; 40° joins it (0.77); the group's average is then at 20°, and
        // 75° is 55° from it (0.57): it joins too, which it would not have against 0° alone.
        let groups: Vec<_> = f.iter().map(|id| person_of(&l.lib, *id).0).collect();
        assert!(groups.iter().all(|g| *g == groups[0]), "{groups:?}");
    }

    #[test]
    fn an_ignored_face_and_a_face_without_a_vector_are_not_grouped() {
        let (l, f) = library(&[&[0.0, 5.0]]);
        l.lib.writer().execute("UPDATE detected_faces SET ignored = 1 WHERE id = ?1", [f[0]]).unwrap();
        l.lib.writer().execute("UPDATE detected_faces SET embedding = NULL WHERE id = ?1", [f[1]]).unwrap();
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 0);
    }

    #[test]
    fn nothing_is_grouped_with_the_switch_off() {
        let (l, _f) = library(&[&[0.0]]);
        l.lib.writer().execute("UPDATE settings SET value = '0' WHERE key = 'face_detection'", []).unwrap();
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 0);
    }

    #[test]
    fn naming_a_group_confirms_its_faces() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        assert_eq!(l.lib.name_group(group, "  Anna ").unwrap(), group);
        assert_eq!(person_of(&l.lib, f[0]), (Some(group), true));
        assert_eq!(person_of(&l.lib, f[1]), (Some(group), true));
        let name: String = l.lib.reader().unwrap()
            .query_row("SELECT name FROM people WHERE id = ?1", [group], |r| r.get(0)).unwrap();
        assert_eq!(name, "Anna", "trimmed");
    }

    #[test]
    fn an_empty_name_is_refused() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        assert!(matches!(l.lib.name_group(group, "   "), Err(Error::EmptyPersonName)));
    }

    /// Review focus 2's case, in its own words: a name typed in another case is the same
    /// person.
    #[test]
    fn a_taken_name_in_any_case_merges() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (person_of(&l.lib, f[0]).0.unwrap(), person_of(&l.lib, f[1]).0.unwrap());
        l.lib.name_group(a, "Anna").unwrap();
        assert_eq!(l.lib.name_group(b, "anna").unwrap(), a);
        assert_eq!(person_of(&l.lib, f[1]), (Some(a), true));
        let groups: i64 = l.lib.reader().unwrap().query_row("SELECT count(*) FROM people", [], |r| r.get(0)).unwrap();
        assert_eq!(groups, 1);
    }

    #[test]
    fn a_name_picasa_uses_links_the_contact() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.upsert_contacts(&HashMap::from([("h1".to_string(), "ANNA".to_string())])).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(group, "Anna").unwrap();
        let linked: i64 = l.lib.reader().unwrap()
            .query_row("SELECT person_id FROM person_contacts WHERE contact = 'h1'", [], |r| r.get(0)).unwrap();
        assert_eq!(linked, group);
    }

    #[test]
    fn a_contact_recorded_later_is_linked_by_name() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(group, "Anna").unwrap();
        l.lib.upsert_contacts(&HashMap::from([("h2".to_string(), "anna".to_string())])).unwrap();
        let linked: i64 = l.lib.reader().unwrap()
            .query_row("SELECT person_id FROM person_contacts WHERE contact = 'h2'", [], |r| r.get(0)).unwrap();
        assert_eq!(linked, group);
    }

    /// A face matched to a named person later is a suggestion, and does not move them.
    #[test]
    fn a_later_match_is_a_suggestion() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        l.lib.writer().execute("UPDATE detected_faces SET embedding_version = NULL WHERE id = ?1", [f[1]]).unwrap();
        // Only the first is grouped and named; the second arrives afterwards.
        l.lib.writer().execute("UPDATE detected_faces SET embedding_version = ?2 WHERE id = ?1", rusqlite::params![f[1], EV]).unwrap();
        l.lib.writer().execute("UPDATE detected_faces SET ignored = 1 WHERE id = ?1", [f[1]]).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib.writer().execute("UPDATE detected_faces SET ignored = 0 WHERE id = ?1", [f[1]]).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), false));
        l.lib.confirm_faces(&[f[1]]).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), true));
    }

    #[test]
    fn a_rejected_face_is_regrouped_elsewhere() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[1]).0.unwrap();
        l.lib.reject_faces(&[f[1]]).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (None, false));
        l.lib.group_ungrouped_faces(&never).unwrap();
        let now = person_of(&l.lib, f[1]).0.unwrap();
        assert_ne!(now, group);
    }

    /// Review focus 3.
    #[test]
    fn merging_into_oneself_or_into_an_unnamed_group_is_refused() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (person_of(&l.lib, f[0]).0.unwrap(), person_of(&l.lib, f[1]).0.unwrap());
        l.lib.name_group(a, "Anna").unwrap();
        assert!(matches!(l.lib.merge_people(a, a), Err(Error::NotAPerson(_))));
        assert!(matches!(l.lib.merge_people(a, b), Err(Error::NotAPerson(_))));
        assert_eq!(person_of(&l.lib, f[0]), (Some(a), true), "unchanged");
    }

    /// Review focus 4: a rejection follows the group into the person it is merged into.
    #[test]
    fn a_rejection_survives_a_merge() {
        let (l, f) = library(&[&[0.0], &[10.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        let anna = person_of(&l.lib, f[2]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib.reject_faces(&[f[1]]).unwrap();
        l.lib.merge_people(group, anna).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_ne!(person_of(&l.lib, f[1]).0, Some(anna));
        assert_eq!(person_of(&l.lib, f[0]), (Some(anna), true), "merged faces are confirmed");
    }

    #[test]
    fn a_named_person_cannot_be_ignored_but_a_group_can() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (person_of(&l.lib, f[0]).0.unwrap(), person_of(&l.lib, f[1]).0.unwrap());
        l.lib.name_group(a, "Anna").unwrap();
        assert!(matches!(l.lib.set_person_ignored(a, true), Err(Error::PersonNamed(_))));
        l.lib.set_person_ignored(b, true).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert!(page.unnamed.iter().all(|g| g.id != b) && page.single_faces.iter().all(|x| x.id != f[1]));
        assert!(page.ignored_groups.iter().any(|g| g.id == b));
        l.lib.set_person_ignored(b, false).unwrap();
        assert!(l.lib.people_page(8).unwrap().ignored_groups.is_empty());
    }

    #[test]
    fn an_ignored_face_leaves_its_group_and_comes_back() {
        let (l, f) = library(&[&[0.0], &[5.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.set_faces_ignored(&[f[1]], true).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (None, false));
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]).0, None, "an ignored face stays out");
        l.lib.set_faces_ignored(&[f[1]], false).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]).0, person_of(&l.lib, f[0]).0);
    }

    #[test]
    fn deleting_a_person_makes_them_a_group_again() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.upsert_contacts(&HashMap::from([("h1".to_string(), "Anna".to_string())])).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let p = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(p, "Anna").unwrap();
        l.lib.delete_person(p).unwrap();
        assert_eq!(person_of(&l.lib, f[0]), (Some(p), false));
        let (name, links): (Option<String>, i64) = l.lib.reader().unwrap()
            .query_row("SELECT name, (SELECT count(*) FROM person_contacts) FROM people WHERE id = ?1", [p], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((name, links), (None, 0));
    }

    #[test]
    fn an_emptied_group_goes_and_a_named_one_stays() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (person_of(&l.lib, f[0]).0.unwrap(), person_of(&l.lib, f[1]).0.unwrap());
        l.lib.name_group(b, "Ben").unwrap();
        l.lib.set_faces_ignored(&[f[0], f[1]], true).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let ids: Vec<i64> = {
            let r = l.lib.reader().unwrap();
            let mut s = r.prepare("SELECT id FROM people ORDER BY id").unwrap();
            s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(ids, [b], "the unnamed group {a} is gone");
    }
```

`HashMap` and `Error` need importing in the tests (`use std::collections::HashMap; use crate::Error;`).

- [ ] **Step 3: Failing tests for the page reads**

```rust
    #[test]
    fn the_page_sorts_groups_into_its_sections() {
        let (l, f) = library(&[&[0.0], &[5.0], &[90.0], &[180.0], &[182.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let ben = person_of(&l.lib, f[3]).0.unwrap();
        l.lib.name_group(ben, "Ben").unwrap();
        // A later face for Ben, unconfirmed: put it there by hand as the step would.
        l.lib.writer().execute("UPDATE detected_faces SET confirmed = 0 WHERE id = ?1", [f[4]]).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(page.unnamed.len(), 1, "the pair at 0° and 5°");
        assert_eq!(page.unnamed[0].face_count, 2);
        assert_eq!(page.single_count, 1, "the face at 90°");
        assert_eq!(page.people.len(), 1);
        assert_eq!(page.people[0].face_count, 1, "confirmed only");
        assert_eq!(page.suggestions.len(), 1);
        assert_eq!(page.suggestions[0].faces.iter().map(|x| x.id).collect::<Vec<_>>(), [f[4]]);
    }

    /// Review focus 1: a hidden photo's face is in no section and no count, and does not
    /// make a group "two or more".
    #[test]
    fn a_hidden_photo_is_in_no_section() {
        let (l, f) = library(&[&[0.0], &[5.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.set_hidden(&[l.items[1]], true).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert!(page.unnamed.is_empty(), "one visible face is not a group of two");
        assert_eq!(page.single_count, 1);
        assert!(page.single_faces.iter().all(|x| x.id != f[1]));
    }

    #[test]
    fn a_strip_is_the_first_faces_and_the_rest_page() {
        let (l, f) = library(&[&[0.0], &[1.0], &[2.0], &[3.0], &[4.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(2).unwrap();
        assert_eq!(page.unnamed[0].faces.len(), 2);
        assert_eq!(page.unnamed[0].face_count, 5);
        let g = page.unnamed[0].id;
        let rest = l.lib.person_faces(g, FaceFilter::All, 2, 10).unwrap();
        assert_eq!(rest.iter().map(|x| x.id).collect::<Vec<_>>(), f[2..]);
    }

    /// Picasa's name is offered when more than half of a group's faces that sit on a
    /// named Picasa face are that contact's.
    #[test]
    fn picasa_names_are_offered_by_majority() {
        let (l, f) = library(&[&[0.0], &[3.0], &[6.0]]);
        l.lib.upsert_contacts(&HashMap::from([
            ("h1".to_string(), "Anna".to_string()),
            ("h2".to_string(), "Ben".to_string()),
        ])).unwrap();
        // The detected face sits at left 0, top 0.1, right 0.08, bottom 0.2 on each photo.
        let at_the_face = |contact: &str| crate::picasa::Face {
            contact: contact.into(), left: 0.0, top: 0.1, right: 0.08, bottom: 0.2,
        };
        l.lib.set_item_faces(&[
            (l.items[0], vec![at_the_face("h1")]),
            (l.items[1], vec![at_the_face("h1")]),
            (l.items[2], vec![at_the_face("h2")]),
        ]).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(8).unwrap();
        let offer = page.unnamed[0].offer.as_ref().unwrap();
        assert_eq!((offer.name.as_str(), offer.contact.as_str(), offer.faces), ("Anna", "h1", 2));
        // Two against one is a majority; one against one is not.
        l.lib.set_item_faces(&[(l.items[1], vec![])]).unwrap();
        assert!(l.lib.people_page(8).unwrap().unnamed[0].offer.is_none());
        assert_eq!(f.len(), 3);
    }
```

`set_hidden` and `set_item_faces` are existing `Library` methods; check their exact names and signatures in `library/hidden.rs` and `library/faces.rs` and adjust the calls.

Run: `cargo test -p photon-core --lib library::people` — compile errors.

- [ ] **Step 4: Implement**

Module doc and the grouping step:

```rust
//! People: the grouping step that places faces, the operations the user corrects them
//! with, and what the People page reads. The rule is `crate::people`.

use super::Library;
use super::items::edit_from_db;
use super::settings::face_detection_on;
use crate::face_detect::{Rect, merge};
use crate::face_embed::{DIM, from_blob};
use crate::people::{Choice, Group, choose, counts_toward_centroid};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Faces per write of the grouping step: the writer is held for one batch, not the whole
/// first run over a library.
const GROUP_BATCH: usize = 512;

impl Library {
    /// Places every face that has a vector, no group and is not ignored, by the rule, in
    /// face order. Returns how many were placed.
    ///
    /// The groups' averages are computed here from their faces, not stored: a stored one
    /// would have to be kept right by every writer that moves a face. Groups left with no
    /// face are deleted first, unless named or linked to a Picasa contact.
    pub fn group_ungrouped_faces(&self, cancel: &dyn Fn() -> bool) -> Result<usize> {
        {
            let mut conn = self.writer();
            let tx = conn.transaction()?;
            if !face_detection_on(&tx)? {
                return Ok(0);
            }
            tx.execute(
                "DELETE FROM people WHERE name IS NULL
                   AND id NOT IN (SELECT person_id FROM person_contacts)
                   AND id NOT IN (SELECT person_id FROM detected_faces WHERE person_id IS NOT NULL)",
                [],
            )?;
            tx.commit()?;
        }
        let (mut groups, rejected) = self.groups_and_rejections()?;
        let ungrouped: Vec<(i64, Vec<f32>)> = {
            let conn = self.reader()?;
            let mut stmt = conn.prepare(
                "SELECT id, embedding FROM detected_faces
                 WHERE person_id IS NULL AND ignored = 0 AND embedding IS NOT NULL ORDER BY id",
            )?;
            stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
                .filter_map(|row| {
                    row.map(|(id, b)| from_blob(&b).map(|e| (id, e.to_vec()))).transpose()
                })
                .collect::<rusqlite::Result<_>>()?
        };
        let mut placed = 0;
        for chunk in ungrouped.chunks(GROUP_BATCH) {
            if cancel() {
                break;
            }
            let mut conn = self.writer();
            let tx = conn.transaction()?;
            if !face_detection_on(&tx)? {
                return Ok(placed);
            }
            for (face, vector) in chunk {
                let none = HashSet::new();
                let refused = rejected.get(face).unwrap_or(&none);
                let group = match choose(vector, &groups.list, refused) {
                    Choice::Join(id) => id,
                    Choice::New => {
                        tx.execute("INSERT INTO people (name) VALUES (NULL)", [])?;
                        let id = tx.last_insert_rowid();
                        groups.insert(id, false);
                        id
                    }
                };
                // Only a row still ungrouped is placed: an operation may have moved it
                // since it was read.
                let moved = tx.execute(
                    "UPDATE detected_faces SET person_id = ?2, confirmed = 0
                     WHERE id = ?1 AND person_id IS NULL AND ignored = 0",
                    params![face, group],
                )?;
                if moved == 1 {
                    groups.add(group, vector, false);
                    placed += 1;
                }
            }
            tx.commit()?;
        }
        Ok(placed)
    }

    pub fn has_ungrouped_faces(&self) -> Result<bool> {
        Ok(self.reader()?.query_row(
            "SELECT EXISTS (SELECT 1 FROM detected_faces
                 WHERE person_id IS NULL AND ignored = 0 AND embedding IS NOT NULL)",
            [],
            |r| r.get(0),
        )?)
    }

    fn groups_and_rejections(&self) -> Result<(Groups, HashMap<i64, HashSet<i64>>)> {
        let conn = self.reader()?;
        let mut groups = Groups::default();
        let mut stmt = conn.prepare("SELECT id, name IS NOT NULL FROM people")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)))? {
            let (id, named) = row?;
            groups.insert(id, named);
        }
        let mut stmt = conn.prepare(
            "SELECT person_id, embedding, confirmed FROM detected_faces
             WHERE person_id IS NOT NULL AND embedding IS NOT NULL",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, i64>(2)? == 1))
        })? {
            let (person, blob, confirmed) = row?;
            if let Some(e) = from_blob(&blob) {
                groups.add(person, &e, confirmed);
            }
        }
        let mut rejected: HashMap<i64, HashSet<i64>> = HashMap::new();
        let mut stmt = conn.prepare("SELECT face_id, person_id FROM face_rejections")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (face, person) = row?;
            rejected.entry(face).or_default().insert(person);
        }
        Ok((groups, rejected))
    }
}

/// The groups as the rule sees them, and which are named (whose average only confirmed
/// faces move).
#[derive(Default)]
struct Groups {
    list: Vec<Group>,
    at: HashMap<i64, (usize, bool)>,
}

impl Groups {
    fn insert(&mut self, id: i64, named: bool) {
        self.at.insert(id, (self.list.len(), named));
        self.list.push(Group { id, sum: vec![0f32; DIM], count: 0 });
    }

    fn add(&mut self, id: i64, vector: &[f32], confirmed: bool) {
        if let Some(&(i, named)) = self.at.get(&id)
            && counts_toward_centroid(named, confirmed)
        {
            self.list[i].add(vector);
        }
    }
}
```

The operations (each one transaction on the writer; names compared in Rust):

```rust
/// A name as stored: trimmed; empty is refused.
fn clean(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() { Err(Error::EmptyPersonName) } else { Ok(name.to_string()) }
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The named person, other than `except`, whose name is `name` in any case.
fn person_named(tx: &Connection, name: &str, except: i64) -> rusqlite::Result<Option<i64>> {
    let mut stmt = tx.prepare_cached("SELECT id, name FROM people WHERE name IS NOT NULL AND id != ?1")?;
    let found = stmt
        .query_map(params![except], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .find(|(_, n)| same_name(n, name))
        .map(|(id, _)| id);
    Ok(found)
}

/// Links every Picasa contact no person has to the named person whose name it carries.
pub(crate) fn link_contacts_by_name(tx: &Connection) -> rusqlite::Result<()> {
    let people: Vec<(i64, String)> = tx
        .prepare_cached("SELECT id, name FROM people WHERE name IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let unlinked: Vec<(String, String)> = tx
        .prepare_cached(
            "SELECT hash, name FROM contacts WHERE hash NOT IN (SELECT contact FROM person_contacts)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (hash, name) in unlinked {
        if let Some((id, _)) = people.iter().find(|(_, n)| same_name(n, &name)) {
            tx.execute(
                "INSERT OR IGNORE INTO person_contacts (contact, person_id) VALUES (?1, ?2)",
                params![hash, id],
            )?;
        }
    }
    Ok(())
}

fn is_named(tx: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(tx
        .query_row("SELECT name IS NOT NULL FROM people WHERE id = ?1", params![id], |r| r.get(0))
        .optional()?
        .unwrap_or(false))
}

fn merge(tx: &Connection, from: i64, into: i64) -> rusqlite::Result<()> {
    let from_named = is_named(tx, from)?;
    tx.execute(
        "UPDATE detected_faces SET person_id = ?2,
             confirmed = CASE WHEN ?3 THEN confirmed ELSE 1 END
         WHERE person_id = ?1",
        params![from, into, from_named],
    )?;
    tx.execute(
        "UPDATE OR IGNORE face_rejections SET person_id = ?2 WHERE person_id = ?1",
        params![from, into],
    )?;
    tx.execute("UPDATE person_contacts SET person_id = ?2 WHERE person_id = ?1", params![from, into])?;
    tx.execute("DELETE FROM people WHERE id = ?1", params![from])?;
    Ok(())
}

impl Library {
    pub fn name_group(&self, group: i64, name: &str) -> Result<i64> {
        let name = clean(name)?;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let person = if let Some(existing) = person_named(&tx, &name, group)? {
            merge(&tx, group, existing)?;
            existing
        } else {
            tx.execute("UPDATE people SET name = ?2, ignored = 0 WHERE id = ?1", params![group, name])?;
            tx.execute("UPDATE detected_faces SET confirmed = 1 WHERE person_id = ?1", params![group])?;
            group
        };
        link_contacts_by_name(&tx)?;
        tx.commit()?;
        Ok(person)
    }

    pub fn rename_person(&self, person: i64, name: &str) -> Result<i64> {
        self.name_group(person, name)
    }

    pub fn confirm_faces(&self, faces: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        for face in faces {
            tx.execute(
                "UPDATE detected_faces SET confirmed = 1 WHERE id = ?1
                   AND person_id IN (SELECT id FROM people WHERE name IS NOT NULL)",
                params![face],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn reject_faces(&self, faces: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        for face in faces {
            tx.execute(
                "INSERT OR IGNORE INTO face_rejections (face_id, person_id)
                 SELECT id, person_id FROM detected_faces WHERE id = ?1 AND person_id IS NOT NULL",
                params![face],
            )?;
            tx.execute(
                "UPDATE detected_faces SET person_id = NULL, confirmed = 0 WHERE id = ?1",
                params![face],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn merge_people(&self, from: i64, into: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if from == into || !is_named(&tx, into)? {
            return Err(Error::NotAPerson(into));
        }
        merge(&tx, from, into)?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_person_ignored(&self, person: i64, ignored: bool) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if ignored && is_named(&tx, person)? {
            return Err(Error::PersonNamed(person));
        }
        tx.execute("UPDATE people SET ignored = ?2 WHERE id = ?1", params![person, ignored])?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_faces_ignored(&self, faces: &[i64], ignored: bool) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        for face in faces {
            if ignored {
                tx.execute(
                    "UPDATE detected_faces SET ignored = 1, person_id = NULL, confirmed = 0 WHERE id = ?1",
                    params![face],
                )?;
            } else {
                tx.execute("UPDATE detected_faces SET ignored = 0 WHERE id = ?1", params![face])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_person(&self, person: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute("UPDATE people SET name = NULL WHERE id = ?1", params![person])?;
        tx.execute("DELETE FROM person_contacts WHERE person_id = ?1", params![person])?;
        tx.execute("UPDATE detected_faces SET confirmed = 0 WHERE person_id = ?1", params![person])?;
        tx.commit()?;
        Ok(())
    }

    pub fn named_people_count(&self) -> Result<i64> {
        Ok(self.reader()?.query_row("SELECT count(*) FROM people WHERE name IS NOT NULL", [], |r| r.get(0))?)
    }
}
```

In `faces.rs`, `upsert_contacts`: inside its transaction, after the insert loop and before `tx.commit()`, call `super::people::link_contacts_by_name(&tx)?;`, with a sentence in its doc comment: a contact carrying a person's name is that person.

The page reads. `PageFace`, `Offer`, `PageGroup`, `PeoplePage`, `FaceFilter` as in **Produces**, `#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]` (and `Deserialize`, `#[serde(rename_all = "lowercase")]` for `FaceFilter`). Implement `people_page` by reading every visible grouped face once, then sorting into sections in Rust:

```rust
/// Every face on a visible photo with its group, in face order: what the page is built
/// from. Visible means not hidden and not missing; the grouping step is the one place
/// that sees hidden photos' faces.
const PAGE_FACES_SQL: &str = "SELECT f.id, f.item_id, f.person_id, f.confirmed, f.ignored,
            f.left, f.top, f.right, f.bottom,
            i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE (f.person_id IS NOT NULL OR f.ignored = 1)
       AND i.hidden = 0 AND +i.missing_since IS NULL
     ORDER BY f.id";
```

Then, in Rust:
- Read `people` (`id`, `name`, `ignored`).
- Per group: visible faces, split into confirmed and unconfirmed.
- `unnamed`: unnamed, not ignored, two or more visible faces, largest first (ties by id); `faces` = the first `strip`.
- `single_faces` / `single_count`: unnamed, not ignored, exactly one visible face; `single_faces` holds at most 200.
- `suggestions`: named people with unconfirmed visible faces; `faces` = those, first `strip`; `face_count` = their number.
- `people`: named people with confirmed visible faces, sorted by name in Rust; `faces` = confirmed, first `strip`; `face_count` = confirmed.
- `ignored_groups`: ignored with visible faces; `ignored_faces`: visible faces with `ignored = 1`, at most 200.
- `thumb_key`: `grid::hex_key(edit.thumb_key(fingerprint(path, size, mtime_ms)))`.

The offer, for each `unnamed` group (computed only for those):

```rust
/// Picasa's name for a group, when more than half of its faces that sit on a face Picasa
/// named are that contact's. A contact linked to a person offers the person's name.
fn offer(
    faces: &[(i64 /*item*/, Rect, Edit)],
    picasa: &HashMap<i64, Vec<(Rect, String /*contact*/)>>,
    names: &HashMap<String, String>, // contact -> name to offer
) -> Option<Offer> {
    let mut votes: HashMap<&str, i64> = HashMap::new();
    let mut total = 0;
    for (item, rect, edit) in faces {
        let Some(on_photo) = picasa.get(item) else { continue };
        if let Some((_, contact)) = on_photo.iter().find(|(p, _)| {
            merge::shown(*edit, *p).is_some_and(|shown| merge::same_face(&shown, rect))
        }) && names.contains_key(contact.as_str())
        {
            *votes.entry(contact.as_str()).or_default() += 1;
            total += 1;
        }
    }
    let (contact, n) = votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))?;
    (2 * n > total).then(|| Offer { name: names[contact].clone(), contact: contact.to_string(), faces: n })
}
```

`picasa` is read once for the items of the unnamed groups' faces: `SELECT item_id, left, top, right, bottom, contact FROM faces WHERE item_id IN (…)` (chunk the id list at 500). `names` is every named contact (`contacts`), with a linked contact's name replaced by its person's name.

`person_faces(person, which, offset, limit)`: the same visible-face query restricted to `f.person_id = ?1` and the filter (`confirmed = 1`, `confirmed = 0`, or either), `ORDER BY f.id LIMIT ?2 OFFSET ?3`.

- [ ] **Step 5: Run, probe, commit**

Run: `cargo test -p photon-core --lib library::people && cargo test -p photon-core --lib faces`.

| Change | Must fail |
|---|---|
| `same_name`: compare without `to_lowercase` | `a_taken_name_in_any_case_merges`, `a_name_picasa_uses_links_the_contact` |
| `merge_people`: drop `from == into \|\|` | `merging_into_oneself_or_into_an_unnamed_group_is_refused` |
| `merge`: drop the `face_rejections` update | `a_rejection_survives_a_merge` |
| `merge`: `CASE WHEN ?3 THEN confirmed ELSE 1 END` → `confirmed` | `a_rejection_survives_a_merge` (its "confirmed" assert) |
| `groups.add(group, vector, false)` removed in the step | `grouping_is_in_face_order_and_repeatable` |
| `counts_toward_centroid` ignored in `Groups::add` (always add) | `a_later_match_is_a_suggestion` (**record**: a single suggestion may not move the person enough to change a test's outcome; if nothing fails, add a case where a named person's suggestions would pull a later face away from them, and say so) |
| `PAGE_FACES_SQL`: drop `i.hidden = 0 AND` | `a_hidden_photo_is_in_no_section` |
| `offer`: `2 * n > total` → `n > 0` | `picasa_names_are_offered_by_majority` |
| the empty-group `DELETE` removed | `an_emptied_group_goes_and_a_named_one_stays` |
| `link_contacts_by_name` call removed from `upsert_contacts` | `a_contact_recorded_later_is_linked_by_name` |
| `set_person_ignored`'s named check removed | `a_named_person_cannot_be_ignored_but_a_group_can` |

Gate, commit(s) `feat(people): group faces, name and correct people` and `feat(people): what the People page reads`, with the probe results.

---

### Task 7: The embed pass

**Files:**
- Create: `crates/photon-core/src/face_embed/pass.rs`
- Modify: `crates/photon-core/src/face_embed/mod.rs` (`pub mod pass;`), `crates/photon-core/src/face_detect/pass.rs` (`BREAKER_FLOOR` → `pub(crate)` if it is not)

**Interfaces:**
- Consumes: `Library::{embed_candidates, write_embeddings}`, `EmbedCandidate`, `FaceBox`, `Embedding`; `face_detect::pass::{workers, BREAKER_FLOOR}`; `ThumbCache::read`.
- Produces: `pub type Embed<'a> = dyn Fn(&DynamicImage, &FaceBox) -> Result<Option<Embedding>> + Sync + 'a;`; `pub fn run(lib: &Library, cache: &ThumbCache, version: i64, workers: usize, embed: &Embed<'_>, cancel: &(dyn Fn() -> bool + Sync), on_batch: &mut dyn FnMut(usize)) -> Result<usize>` (faces written).

The loop has the detection pass's shape exactly: page by photo id; workers over the photos of a batch; per photo read the preview once (guarded; unreadable or panicking read → the photo is skipped and its faces stay candidates); per face embed (guarded; `Ok(Some)` → a vector, `Ok(None)` → too small, `Err` or panic → failed); the breaker over the batch's faces: if every face the model was asked about failed (`Failed` count equals `Embedded + Failed` count) and at least `BREAKER_FLOOR` did, the batch is not written and `run` returns `Error::FaceModel("all N faces embedded in a batch failed; nothing was marked")`; otherwise write with `write_embeddings` (`Failed` and too small both write `None`), call `on_batch(written)`.

Read `face_detect/pass.rs` before writing this: copy its structure, its comments' reasoning where it applies (say so where it does not), and its tests' fixture style. Do not factor a shared generic loop out of the two: they differ in what a candidate is and what an outcome is, and the duplication is two short functions; a shared loop is a later refactor if a third pass appears.

- [ ] **Step 1: Failing tests** (fixture: photos with detected faces, previews cached, as `face_detect::pass`'s tests make them; stand-in embedders)

```rust
    #[test]
    fn every_face_is_embedded_once()                      // >BATCH photos, 2 faces each; batches as expected; candidates empty after
    #[test]
    fn a_face_too_small_is_marked_without_a_vector()      // stand-in returns Ok(None) for one face: version set, embedding NULL
    #[test]
    fn a_missing_preview_skips_the_photo()                // remove one preview: its faces stay candidates, the pass ends
    #[test]
    fn a_panic_costs_one_face()                           // stand-in panics on one face: that face marked without a vector, others embedded
    #[test]
    fn a_batch_where_every_face_fails_is_not_written()    // all fail, >= 8: Err(Error::FaceModel(m)) with "nothing was marked"; nothing written
    #[test]
    fn seven_failing_faces_are_written()                  // 7 faces all failing: written as looked-at
    #[test]
    fn too_small_faces_do_not_count_towards_the_breaker() // 8 failing + 3 too small: trips; 7 failing + 3 too small: does not
    #[test]
    fn each_photo_gets_its_own_vectors()                  // 3 workers, previews of distinct widths, the stand-in encodes the width in the vector: each face's stored vector is its photo's
```

Write each in full in the style of `face_detect::pass`'s tests (`fixture_with`, the counting stand-ins, `tripped`); the comments above say what each asserts.

- [ ] **Step 2: Implement**, **Step 3: Run five times** (`for i in 1 2 3 4 5; do cargo test -p photon-core --lib face_embed::pass 2>&1 | grep "test result"; done`), **Step 4: Probe** each rule as detection's Task 6 did (the paging cursor, the read guard, the embed guard, the breaker's floor and its "every", too-small not counting, alignment), **Step 5: Commit** `feat(people): the embed pass`.

---

### Task 8: The engine and the commands

**Files:**
- Modify: `crates/photon-app/src/engine.rs`, `events.rs`, `commands.rs`, `ipc.rs`, `app.rs`, `testutil.rs`
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/status.ts`, `ui/src/lib/status.test.ts`, `crates/xtask/screenshots/mock.js`

**Interfaces:**
- Consumes: Tasks 4, 6, 7.
- Produces:
  - `events::FacePhase { Detecting, Recognising }` (serde lowercase); `FaceProgress` gains `pub phase: FacePhase`; `api.ts`: `phase: 'detecting' | 'recognising'`.
  - `Engine.people_write: Mutex<()>`; `Engine::write_people<T>(self: &Arc<Self>, what: &'static str, op: impl FnOnce(&Library) -> photon_core::Result<T>) -> photon_core::Result<T>`.
  - Commands (each in `commands.rs`, `ipc.rs`, `app.rs`, `api.ts`, `mock.js`):

| Command | Arguments | Returns | `api.ts` |
|---|---|---|---|
| `people_page` | `strip: usize` | `PeoplePage` | `peoplePage(strip)` |
| `person_faces` | `person: i64, which: FaceFilter, offset: usize, limit: usize` | `Vec<PageFace>` | `personFaces(person, which, offset, limit)` |
| `name_person` | `person: i64, name: String` | `i64` | `namePerson(person, name)` |
| `rename_person` | `person: i64, name: String` | `i64` | `renamePerson(person, name)` |
| `confirm_faces` | `faces: Vec<i64>` | `()` | `confirmFaces(faces)` |
| `reject_faces` | `faces: Vec<i64>` | `()` | `rejectFaces(faces)` |
| `merge_people` | `from: i64, into: i64` | `()` | `mergePeople(from, into)` |
| `ignore_person` | `person: i64, ignored: bool` | `()` | `ignorePerson(person, ignored)` |
| `ignore_faces` | `faces: Vec<i64>, ignored: bool` | `()` | `ignoreFaces(faces, ignored)` |
| `delete_person` | `person: i64` | `()` | `deletePerson(person)` |
| `face_data_summary` | — | `FaceDataSummary { named_people: i64 }` | `faceDataSummary()` |

  Every operation goes through `write_people`. The two reads (`people_page`, `person_faces`) call the library directly.

- [ ] **Step 1: The progress phase.** `FaceProgress { phase, checked, total, running }`. `FACE_PROGRESS_CLEARED` gets `phase: FacePhase::Detecting`. `send_face_progress(&self, phase: FacePhase, running: bool)` counts with `face_progress(DETECTOR_VERSION)` for detecting and `embed_progress(EMBEDDER_VERSION)` for recognising. `status.ts`'s `faceStatus` labels "Finding faces: N of M" or "Recognising people: N of M faces"; add a test for the second label and probe it.

- [ ] **Step 2: `write_people`.**

```rust
    /// Runs a correction of the People data under `people_write`, which the grouping step
    /// also takes, so a pass cannot place a face in a group the user is merging or
    /// deleting. Then the rebuild every data write ends with, and a request for a pass:
    /// faces an operation leaves ungrouped (rejected, unignored) are placed by its
    /// grouping step. Taken before the library's locks; nothing else is held across it.
    pub fn write_people<T>(
        self: &Arc<Self>,
        what: &'static str,
        op: impl FnOnce(&Library) -> photon_core::Result<T>,
    ) -> photon_core::Result<T> {
        let result = {
            let _serialised = self.people_write.lock();
            op(&self.lib)?
        };
        self.refresh_after_write(what);
        self.request_face_pass();
        Ok(result)
    }
```

- [ ] **Step 3: The pass's two new steps.** In `run_face_pass`:

1. Ask for work of all three kinds before loading anything: `face_candidates(0, 1, DETECTOR_VERSION)`, `embed_candidates(0, 1, EMBEDDER_VERSION)` and `has_ungrouped_faces()`. With none, `end_face_pass` as now and return.
2. Detection as now, if it has work.
3. If not cancelled and `embed_candidates(0, 1, …)` is non-empty (re-asked: detection just made faces): send `recognising, running`; load `Embedder::new()` (a failure ends the pass as the detector's does); run `face_embed::pass::run` with `on_batch` that, after each batch, takes `people_write` and calls `group_ungrouped_faces(&cancel)`, sets `regrouped = true` when it placed any, sends throttled progress, and rebuilds through `refresh_grid` (a data change: grouping moves the People list and the Person view) at most every `FACE_REBUILD_EVERY`.
4. If not cancelled: once more, under `people_write`, `group_ungrouped_faces` for faces an operation left ungrouped.
5. `end_face_pass(unshown, regrouped)`: when `regrouped`, rebuild through `refresh_grid` instead of `refresh_grid_derived`; the final event carries the phase of the last step that ran.

`view_reads_faces` still decides the detection step's mid-pass rebuild; the grouping step's mid-pass rebuild is unconditional (paced), because the sidebar's People list reads it whatever the view.

- [ ] **Step 4: Tests** (engine; they run the real models, about two seconds each in a dev build):

```rust
    #[test]
    fn a_detected_face_is_embedded_and_grouped()           // portrait fixture, switch on, settle: one group with that face
    #[test]
    fn two_photos_of_one_face_share_a_group()             // portrait and a resized copy: one group of two
    #[test]
    fn grouping_announces_a_data_change()                 // the pass's LibraryChanged after grouping has data_changed: true
    #[test]
    fn the_last_event_is_recognising_not_running()        // after a pass that embedded: the last FaceProgress is phase Recognising, running false
    #[test]
    fn an_operation_requests_a_pass_that_regroups()       // reject a face through write_people; after settle it is in a new group
    #[test]
    fn a_library_detected_before_this_is_embedded()       // detections written with no embeddings (as 0.47.0 left them), switch on, settle: embedded without detecting again (count detector calls by checking face_version unchanged and ids unchanged)
```

Write each in full in the style of the face tests already in `engine.rs` (`fixture`, `portrait_jpeg`, `scanned`, `f.settle()`, `Recorded::Face`). A helper to read a face's group goes through the library's public methods (`people_page`), since `Library::reader()` is private to photon-core.

- [ ] **Step 5: The commands** as in the table. `FaceDataSummary` is `#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub struct FaceDataSummary { pub named_people: i64 }` in `commands.rs`. In `mock.js`: `people_page` answers a small page (two unnamed groups of three faces, one person "Anna" with two faces, one suggestion), `person_faces` answers `[]`, `face_data_summary` answers `{ namedPeople: 1 }`, and the operations go in `SILENT`. `api.ts` gets the types `PageFace`, `Offer`, `PageGroup`, `PeoplePage`, `FaceFilter` (`'all' | 'confirmed' | 'unconfirmed'`), `FaceDataSummary`, each mirroring its Rust struct field for field.

- [ ] **Step 6: Run, probe, commit.** Run the engine's face tests five times. Probes:

| Change | Must fail |
|---|---|
| `refresh_grid` → `refresh_grid_derived` after grouping | `grouping_announces_a_data_change` |
| remove the embed step's call to `group_ungrouped_faces` | `a_detected_face_is_embedded_and_grouped` |
| remove step 4 (the final grouping) | `an_operation_requests_a_pass_that_regroups` (**record**: the embed step does not run when nothing needs embedding, so this should fail; if it does not, find why) |
| `write_people` without `request_face_pass` | `an_operation_requests_a_pass_that_regroups` |
| the final event's phase hard-coded to detecting | `the_last_event_is_recognising_not_running` |

Both gates (Rust and UI), commit `feat(people): recognise people in the face pass; the commands`.

---

### Task 9: The readers

**Files:**
- Modify: `crates/photon-core/src/library/faces.rs`, `items.rs`
- Modify: `crates/photon-app/src/commands.rs`, `engine.rs` (`set_person_view`), `ipc.rs`
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/library.svelte.ts`, `ui/src/components/FolderTree.svelte`, `crates/xtask/screenshots/mock.js`, any `Person` or `ItemFace` literal in `ui/src/**/*.test.ts`

**Interfaces:**
- Produces: `Person { key: String, name: String, count: i64 }` (replacing `hash`); `ItemFace { key: String, name: String, left, top, right, bottom }` (replacing `hash`); `set_person_view(engine, person: &str)` (the Tauri argument renamed `person`); `api.setPersonView(key)`.

- [ ] **Step 1: Failing tests** (in `faces.rs`'s or `people.rs`'s tests, with Task 6's `library` helper or the existing ones)

```rust
    #[test]
    fn the_people_list_has_named_people_and_unlinked_contacts()
        // a photon person "Anna" (2 confirmed faces on 2 photos, 1 unconfirmed on a third),
        // a Picasa contact "Ben" with a face, a Picasa contact "anna" linked to Anna with a
        // face on a fourth photo: list = [Anna p:<id> count 3, Ben c:<hash> count 1]
    #[test]
    fn the_person_view_has_confirmed_faces_and_linked_contacts()
        // entries_for(GridView::Person, "p:<id>") = the three photos above, not the suggestion's
    #[test]
    fn a_contact_key_is_the_picasa_view_and_anything_else_is_empty()
        // "c:<hash>" = that contact's photos; "<hash>" (no prefix) and "p:x" = empty
    #[test]
    fn person_search_finds_a_photon_name()
        // person:anna finds the confirmed photos; not the suggestion's
    #[test]
    fn hidden_photos_are_in_no_person_reader()
        // hide one of Anna's photos: the list's count, the Person view and person: all drop it
```

Write each in full. The existing `people_with_counts` plan test and `PEOPLE_SQL` stay for the contacts half; add a plan test for the new people half (`+i.missing_since`).

- [ ] **Step 2: Implement**

- `people_with_counts` returns named people first computed from:

```sql
SELECT p.id, p.name, count(DISTINCT x.item_id)
FROM people p
JOIN (SELECT person_id, item_id FROM detected_faces WHERE confirmed = 1 AND person_id IS NOT NULL
      UNION ALL
      SELECT pc.person_id, f.item_id FROM person_contacts pc JOIN faces f ON f.contact = pc.contact) x
  ON x.person_id = p.id
JOIN items i ON i.id = x.item_id
WHERE p.name IS NOT NULL AND +i.missing_since IS NULL AND i.hidden = 0
GROUP BY p.id
```

with key `format!("p:{id}")`, plus `PEOPLE_SQL`'s rows for contacts not in `person_contacts` (add `AND c.hash NOT IN (SELECT contact FROM person_contacts)`), key `format!("c:{hash}")`; one list sorted as now.
- `entries_for(GridView::Person, arg)`: `arg.strip_prefix("p:")` parsed as `i64` → filter `AND i.id IN (SELECT item_id FROM detected_faces WHERE person_id = ?1 AND confirmed = 1 UNION SELECT f.item_id FROM faces f JOIN person_contacts pc ON pc.contact = f.contact WHERE pc.person_id = ?1)`; `arg.strip_prefix("c:")` → the existing contact filter; anything else → `Ok(Vec::new())`.
- `SEARCH_PEOPLE_SQL` gains `UNION ALL SELECT d.item_id, p.name FROM detected_faces d JOIN people p ON p.id = d.person_id WHERE d.confirmed = 1 AND p.name IS NOT NULL`.
- `viewer_item`: named Picasa faces get `key` = `p:<id>` and the person's name when their contact is linked, else `c:<hash>` and the contact's name. A new `Library::item_named_detected_faces(item_id) -> Result<Vec<(Rect, i64, String)>>` (confirmed faces of named people on the photo, rectangle as shown) adds them to `faces` with `p:<id>`, unless `merge::same_face` with a named Picasa face already listed; such faces are no longer in `unnamed_faces` (the `unmatched` input excludes them).
- UI: `Person.key`, `ItemFace.key`; `FolderTree.svelte` keys its `{#each}` and compares `library.info.person` by `person.key`; `library.svelte.ts`'s `setPersonView(key)` and the name lookup by key; `mock.js`'s `list_people` and viewer faces use `key` (`'c:a'`, `'p:1'`); fix every test literal `npm run check` reports.

- [ ] **Step 3: Run, probe, commit**

| Change | Must fail |
|---|---|
| the people query's `confirmed = 1` removed | `the_people_list_has_named_people_and_unlinked_contacts` |
| the Person view's `AND confirmed = 1` removed | `the_person_view_has_confirmed_faces_and_linked_contacts` |
| unprefixed argument treated as a contact hash | `a_contact_key_is_the_picasa_view_and_anything_else_is_empty` |
| `SEARCH_PEOPLE_SQL`'s `WHERE d.confirmed = 1` removed | `person_search_finds_a_photon_name` |
| the people query's `i.hidden = 0` removed | `hidden_photos_are_in_no_person_reader` |
| the unlinked-contact `NOT IN` removed | `the_people_list_has_named_people_and_unlinked_contacts` (Anna's contact listed twice) |

Both gates; `cargo run -p xtask -- screenshots --only viewer-info-light` and Read it (Anna's plate still there). Commit `feat(people): named people in the People list, the Person view, search and the viewer`.

---

### Task 10: Documentation, gates, push

**Files:** `CLAUDE.md`, `docs/superpowers/specs/2026-10-03-photon-people-design.md`, `docs/smoke-checklist.md`, `THIRD-PARTY-NOTICES.md` (already done in Task 1; check).

- [ ] **Step 1: `CLAUDE.md`**, each sentence checked against the code:
  - The face pass's two new steps after detection; grouping is a data change (`refresh_grid`), detection is not; `people_write` and `write_people`; operations request a pass, whose grouping step places what they left ungrouped.
  - The rule: `GROUP_SIMILARITY` 0.50 and `MIN_FACE_PX` 35 with their evidence in one line each; centroids computed per step, never stored, and why.
  - Only confirmed faces carry a name; the key of a person (`p:`/`c:`); contacts linked by name in `upsert_contacts` and on naming.
  - What clears what: an edit or file change drops confirmations; a detector re-run carries them by position in `write_face_batch`; the switch deletes `people`, `person_contacts`, `face_rejections`.
  - Hidden photos: grouped, in no People reader; every People query filters them.
  - The bundled SFace model under the dependency convention.
- [ ] **Step 2: The spec**: an "As built" note recording that centroids are computed per grouping step, not stored, and why (this plan's deviation section); any other deviation a task recorded.
- [ ] **Step 3: Smoke checklist**: a "People (backend)" section — on a real library with "Find faces" on: the status line's second phase; `person:` finds a person named through the commands is not possible by hand until plan 2, so list only what is visible: the second phase runs, the People list in the sidebar still shows Picasa's people, the viewer still shows their plates.
- [ ] **Step 4: Every gate**, then push and watch CI:

```bash
git -c credential.helper='!gh auth git-credential' push https://github.com/bsg62/photon.git HEAD:refs/heads/feat/people
gh pr checks <number> --watch
```

Do not mark the PR ready: plan 2 continues on the same branch, and nothing is released until both are merged.

---

## Self-review

**Spec coverage.** Data model (4, minus stored centroids: the deviation). Embedder (1-3). Grouping rule (5). Operations and Picasa offer (6). The pass's two steps, guards, breaker, refresh chain, progress (7, 8). Readers: hidden photos (6, 9), the key (9), Person view and `person:` (9), People list (9), viewer (9). IPC (8). Documentation (10). The People page, face crops, the switch-off dialog and screenshots of the page are plan 2.

**Type consistency.** `FaceBox` (Task 3) is what `EmbedCandidate.faces` carries (Task 4) and `Embed` takes (Task 7). `Embedding` = `[f32; 128]` throughout; `from_blob`/`to_blob` are the only conversions. `Group`, `Choice`, `choose`, `counts_toward_centroid` (Task 5) are what `group_ungrouped_faces` uses (Task 6). `PeoplePage` and friends (Task 6) are what `people_page` returns through the command (Task 8) and what `api.ts` mirrors. `Person.key` and `ItemFace.key` (Task 9) replace `hash` everywhere, including `FolderTree.svelte` and `mock.js`.

**Review Focus coverage.** 1 → `a_hidden_photo_is_in_no_section` (6) and `hidden_photos_are_in_no_person_reader` (9). 2 → `a_taken_name_in_any_case_merges`, `a_name_picasa_uses_links_the_contact` (6). 3 → `merging_into_oneself_or_into_an_unnamed_group_is_refused` (6). 4 → `a_rejection_survives_a_merge` (6). 5 → `a_detector_rerun_carries_a_confirmed_face_over`, `an_edit_drops_the_confirmation` (4).

**Known looseness, for the implementer to settle and report:** Task 7's and Task 8's tests are specified by name and assertion rather than written out, because they follow existing files' patterns (`face_detect/pass.rs`'s tests and the engine's face tests) that the implementer must read anyway; each still has to be shown failing under its revert.
