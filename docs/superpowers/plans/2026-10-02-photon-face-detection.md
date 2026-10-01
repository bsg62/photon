# Face Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** photon finds faces in the user's photos with YuNet run through `tract`, stores their rectangles, makes them searchable (`has:face`, `faces:N`) and outlines them in the viewer, all behind a Settings switch that is off by default.

**Architecture:** A pure-Rust detector module (`photon_core::face_detect`) turns an image into rectangles. A background pass in the engine, modelled on the look-alike pass, reads each photo's cached 1600 px preview, detects, and writes to a new `detected_faces` table guarded against the photo or the switch changing mid-pass. One merge rule combines detections with Picasa's faces for both readers, the viewer and search.

**Tech Stack:** Rust (`tract-onnx` 0.23, rusqlite, `image`), Tauri 2 IPC, Svelte 5 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-01-photon-face-detection-design.md`. Read it before any task; the plan argues from it.

**About the code in this plan:** the `tract` calls, the output decoding and the letterbox arithmetic are lifted from the 2026-10-01 spike, which compiled and ran. Everything else was written against the repository as read on 2026-10-02 and has not been compiled. Where a block does not compile as written, fix it to match the stated interface and behaviour; do not change the interface without updating every later task that consumes it.

## Global Constraints

- **The gates** (`CLAUDE.md`, "Commands"): before every commit, `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`; and for UI changes `npm run check` (0 errors and 0 warnings) and `npm test`.
- **Every new test is shown to fail with its change reverted**, by an exact revert, and the commit message says so. After restoring a file from a pre-probe copy, `touch` it, or cargo keeps running the probed code. A probe that passes is a finding: stop and work out why.
- **Never launch the GUI.** Verification is the test suites, `svelte-check` and the screenshots.
- **No system library dependencies.** `tract`'s native code is assembly built with `cc`; nothing else native is added.
- Detector input is **1280**; score threshold **0.7**; overlap limit **0.3**; `DETECTOR_VERSION` is **1**.
- Schema goes **23 → 24**. Update the literal numbers in `library/mod.rs`; do not loosen them.
- The setting key is `face_detection`; absent means **off**.
- Workers: half of `available_parallelism`, at least 1, at most 4.
- Detection rectangles are fractions of the picture **as shown** (edit applied). Picasa's `faces` rows are fractions of the **unedited** picture.
- Rust mirrors in `ui/src/lib/api.ts` change in the same commit as the Rust struct.
- A new IPC command needs `commands.rs`, `ipc.rs`, the handler list in `app.rs`, `api.ts`, and an answer in `crates/xtask/screenshots/mock.js`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Work on the branch `feat/face-detection`, which already holds the spec.

## Review Focus

Inputs the spec implies and a person will meet, each pinned by a test in the task named:

1. **A photo smaller than the model's input** (a 300 px original, a tiny icon): never scaled up, no panic, fractions still of the photo. Task 3.
2. **An extreme panorama** (1600x40): the scaled short side must not round to zero. Task 3.
3. **A preview with an alpha channel or in greyscale** (`ThumbCache::read` returns RGBA for a PNG with alpha): converted, not rejected. Task 3.
4. **Switching on with nothing to do** (an empty library, or every photo already checked): the pass ends, a final progress event with `running: false` is sent, and the UI never shows "0 of 0". Tasks 7 and 10.
5. **Malformed face terms** (`faces:`, `faces:+`, `faces:-1`, `faces:99999999999999999999`, `has:faces`): ignored like every other malformed term, never a panic and never "match everything". Task 8.

## File Structure

| File | Responsibility |
|---|---|
| `crates/photon-core/models/face_detection_yunet_2023mar.onnx` | The bundled model (new). |
| `crates/photon-core/src/face_detect/mod.rs` | `Rect`, `Detection`, `Detector`, `DETECTOR_VERSION`, the letterbox (new). |
| `crates/photon-core/src/face_detect/decode.rs` | Pure decoding of the model's outputs and overlap suppression (new). |
| `crates/photon-core/src/face_detect/merge.rs` | The one rule merging Picasa's faces with detections (new). |
| `crates/photon-core/src/face_detect/pass.rs` | The batch loop: candidates, workers, guarded write (new). |
| `crates/photon-core/src/library/detected_faces.rs` | Every query on `detected_faces` and `items.face_version` (new). |
| `crates/photon-core/src/library/schema.rs` | Migration 24. |
| `crates/photon-core/src/library/settings.rs` | The `face_detection` setting; off deletes. |
| `crates/photon-core/src/library/items.rs` | `update_items` and `set_item_edit` clear detections; `search_entries` reads face counts. |
| `crates/photon-core/src/search.rs` | `has:face`, `faces:N`, `faces:N+`. |
| `crates/photon-app/src/engine.rs` | The pass's thread, triggers, pacing, progress, shutdown. |
| `crates/photon-app/src/{commands,ipc,app,events}.rs` | The command pair, `ViewerItem.unnamed_faces`, the progress event. |
| `ui/src/lib/{api,faces,status,settings,library.svelte}.ts` | Mirrors, labels, the progress state. |
| `ui/src/components/{Viewer,Settings,StatusBar,SearchBar}.svelte` | Outlines, the switch, the progress line, the search hint. |
| `crates/xtask/screenshots/mock.js`, `crates/xtask/src/screenshots.rs` | Answers for the new commands; one new shot. |

---

### Task 1: `tract` builds on all three platforms

This is a gate. If CI fails on Windows MSVC or macOS arm64 for a reason that is `tract`'s, stop and report; do not start Task 2.

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/photon-core/Cargo.toml`
- Create: `crates/photon-core/models/face_detection_yunet_2023mar.onnx`
- Create: `crates/photon-core/models/README.md`
- Create: `crates/photon-core/src/face_detect/mod.rs`
- Modify: `crates/photon-core/src/lib.rs`
- Modify: `crates/photon-core/src/error.rs`
- Modify: `THIRD-PARTY-NOTICES.md`

**Interfaces:**
- Produces: `photon_core::face_detect::Detector`, with `Detector::new() -> crate::Result<Detector>`; `Error::FaceModel(String)`.

- [ ] **Step 1: Fetch the model and record where it came from**

```bash
mkdir -p crates/photon-core/models
curl -sSL -o crates/photon-core/models/face_detection_yunet_2023mar.onnx \
  https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx
wc -c crates/photon-core/models/face_detection_yunet_2023mar.onnx
```

Expected: `232589`.

`crates/photon-core/models/README.md`:

```markdown
# Bundled models

| File | What | Source | Licence |
|---|---|---|---|
| `face_detection_yunet_2023mar.onnx` | YuNet face detector, 232,589 bytes | [opencv_zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet), fetched 2026-10-01 | MIT, Copyright (c) 2020 Shiqi Yu |

Embedded in photon-core with `include_bytes!` (`src/face_detect/mod.rs`). Replacing a file
changes what every library has stored: bump `face_detect::DETECTOR_VERSION`.
```

- [ ] **Step 2: Add the dependency and the dev-profile overrides**

In `crates/photon-core/Cargo.toml`, after the `tempfile` line:

```toml
# Runs the bundled face detector (`face_detect`). Pure Rust but for tract-linalg's assembly
# kernels, which it compiles in with `cc`: no nasm, no system library.
tract-onnx = "0.23.8"
```

In the workspace `Cargo.toml`, at the end:

```toml
# tract unoptimised runs one face detection in about five seconds, against one with these:
# measured 2026-10-02 on the test fixture. Only the three crates that hold the arithmetic.
[profile.dev.package.tract-linalg]
opt-level = 3

[profile.dev.package.tract-core]
opt-level = 3

[profile.dev.package.tract-data]
opt-level = 3
```

- [ ] **Step 3: Write the failing test**

Create `crates/photon-core/src/face_detect/mod.rs`:

```rust
//! Finding faces in a picture. Pixels in, rectangles out: this module knows nothing about
//! the library, the thumbnail cache or the engine, and nothing outside it names `tract`.

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled model parses and optimises at the input size the pass uses. This is the
    /// test CI runs on Windows and macOS to show `tract` builds and runs there at all.
    #[test]
    fn the_bundled_model_loads() {
        Detector::new().unwrap();
    }
}
```

Add `pub mod face_detect;` to `crates/photon-core/src/lib.rs` after `pub mod export;`.

- [ ] **Step 4: Run it to see it fail**

Run: `cargo test -p photon-core --lib face_detect`
Expected: a compile error, `cannot find type Detector`. (A compile error is not the proof the convention asks for; this test's proof is CI itself.)

- [ ] **Step 5: Implement `Detector::new`**

Add to `crates/photon-core/src/error.rs`, in `Error`:

```rust
    /// The bundled face detector could not be loaded or run: a build fault, not something
    /// about the user's photo.
    #[error("face detection failed: {0}")]
    FaceModel(String),
```

Put above the tests in `face_detect/mod.rs`:

```rust
use crate::{Error, Result};
use tract_onnx::prelude::*;

/// YuNet, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");

/// The side of the square the model is run at. A face narrower than about 15 px at this
/// size is missed: at 640 a group photo's faces are 14-21 px wide, on the edge, and at 320
/// the 29 people of the spike's test photograph came out as one.
pub const INPUT: usize = 1280;

type Run = dyn Fn(Tensor) -> TractResult<TVec<TValue>> + Send + Sync;

/// The loaded model. Parsing and optimising it takes about 25 ms, so a pass makes one and
/// shares it between its workers.
pub struct Detector {
    run: Box<Run>,
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Detector {
    pub fn new() -> Result<Self> {
        // The file declares a 640 input and the shapes that follow from it; left in, tract
        // refuses any other size ("Impossible to unify 320 with 160").
        let model = tract_onnx::onnx()
            .with_ignore_output_shapes(true)
            .with_ignore_value_info(true)
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, INPUT, INPUT]).into())
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

- [ ] **Step 6: Run the test**

Run: `cargo test -p photon-core --lib face_detect`
Expected: PASS, `the_bundled_model_loads`.

- [ ] **Step 7: Notices**

Append to `THIRD-PARTY-NOTICES.md` two sections in the shape of the existing ones (`## libjpeg-turbo (mozjpeg)` is the model): `## tract`, with the MIT licence text from `~/.cargo/registry/src/*/tract-onnx-0.23.8/LICENSE-MIT`, and `## YuNet face detection model`, with the MIT text from `https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_detection_yunet/LICENSE` (Copyright (c) 2020 Shiqi Yu).

Run: `cargo run -p xtask -- metadata`
Expected: passes.

- [ ] **Step 8: Run the Rust gate, commit, push, and watch CI**

```bash
git add Cargo.toml Cargo.lock crates/photon-core THIRD-PARTY-NOTICES.md
git commit -m "build: tract and the YuNet model, loaded by a test

The gate for face detection: CI has to show tract builds and runs on
Windows MSVC and macOS arm64 before anything is built on it.

The test has no revert probe: it is the build that is under test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
git push -u origin feat/face-detection
gh pr create --draft --title "feat: face detection" --body "Draft. Spec: docs/superpowers/specs/2026-10-01-photon-face-detection-design.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks --watch
```

`git push` needs gh's credential helper in an agent session (`gh auth setup-git`). Straight after a push `gh pr checks` can answer "no checks reported"; wait until checks exist.

Expected: all three `rust (...)` jobs pass. **If one fails in `tract`, stop here and report.**

---

### Task 2: Decoding the model's outputs

**Files:**
- Create: `crates/photon-core/src/face_detect/decode.rs`
- Modify: `crates/photon-core/src/face_detect/mod.rs`

**Interfaces:**
- Produces, all `pub(crate)` in `face_detect::decode`:
  - `struct Raw { x: f32, y: f32, w: f32, h: f32, landmarks: [(f32, f32); 5], score: f32 }`, in input pixels
  - `struct Level<'a> { stride: usize, cls: &'a [f32], obj: &'a [f32], bbox: &'a [f32], kps: &'a [f32] }`
  - `fn decode_level(level: &Level, side: usize, threshold: f32, out: &mut Vec<Raw>)`
  - `fn suppress(faces: Vec<Raw>, max_iou: f32) -> Vec<Raw>`
  - `const SCORE_THRESHOLD: f32 = 0.7`, `const MAX_IOU: f32 = 0.3`

- [ ] **Step 1: Write the failing tests**

Create `crates/photon-core/src/face_detect/decode.rs` with only the tests:

```rust
//! The arithmetic between the model's twelve output tensors and faces in input pixels.
//! Pure, so each rule has a test that fails without it.

#[cfg(test)]
mod tests {
    use super::*;

    /// One level of `cols` x `cols` cells, silent but for cell `n`.
    struct Cell {
        cls: Vec<f32>,
        obj: Vec<f32>,
        bbox: Vec<f32>,
        kps: Vec<f32>,
    }

    fn cell(cols: usize, n: usize, cls: f32, obj: f32, bbox: [f32; 4], kps: [f32; 10]) -> Cell {
        let cells = cols * cols;
        let mut c = Cell {
            cls: vec![0.0; cells],
            obj: vec![0.0; cells],
            bbox: vec![0.0; cells * 4],
            kps: vec![0.0; cells * 10],
        };
        c.cls[n] = cls;
        c.obj[n] = obj;
        c.bbox[n * 4..n * 4 + 4].copy_from_slice(&bbox);
        c.kps[n * 10..n * 10 + 10].copy_from_slice(&kps);
        c
    }

    fn decode(c: &Cell, stride: usize, side: usize) -> Vec<Raw> {
        let mut out = Vec::new();
        let level = Level {
            stride,
            cls: &c.cls,
            obj: &c.obj,
            bbox: &c.bbox,
            kps: &c.kps,
        };
        decode_level(&level, side, SCORE_THRESHOLD, &mut out);
        out
    }

    /// Side 64 at stride 16 is a 4x4 grid; cell 6 is row 1, column 2. Its centre is
    /// (2 + 0.5, 1 + 0.25) cells = (40, 20) px, its size exp(ln 2) = 2 cells = 32 px wide
    /// and exp(0) = 1 cell = 16 px high, so the box starts at (24, 12).
    #[test]
    fn a_cell_becomes_a_box_in_input_pixels() {
        let c = cell(4, 6, 0.81, 1.0, [0.5, 0.25, 2f32.ln(), 0.0], [0.0; 10]);
        let faces = decode(&c, 16, 64);
        assert_eq!(faces.len(), 1);
        let f = &faces[0];
        assert!((f.x - 24.0).abs() < 1e-3, "x {}", f.x);
        assert!((f.y - 12.0).abs() < 1e-3, "y {}", f.y);
        assert!((f.w - 32.0).abs() < 1e-3, "w {}", f.w);
        assert!((f.h - 16.0).abs() < 1e-3, "h {}", f.h);
    }

    /// The same outputs at stride 8 (an 8x8 grid, cell 6 = row 0, column 6) land elsewhere
    /// and half the size: the stride and the column count both come from the level.
    #[test]
    fn the_stride_scales_and_places_the_box() {
        let c = cell(8, 6, 0.81, 1.0, [0.5, 0.25, 2f32.ln(), 0.0], [0.0; 10]);
        let f = &decode(&c, 8, 64)[0];
        // centre ((6 + 0.5) * 8, (0 + 0.25) * 8) = (52, 2); 16 x 8.
        assert!((f.x - 44.0).abs() < 1e-3, "x {}", f.x);
        assert!((f.y - -2.0).abs() < 1e-3, "y {}", f.y);
        assert!((f.w - 16.0).abs() < 1e-3, "w {}", f.w);
        assert!((f.h - 8.0).abs() < 1e-3, "h {}", f.h);
    }

    /// A landmark is the cell plus its offset, times the stride: no `exp`, unlike the size.
    #[test]
    fn landmarks_are_offsets_from_the_cell() {
        let mut kps = [0.0; 10];
        kps[0] = 0.5; // first point: x
        kps[1] = -0.5; // first point: y
        kps[8] = 1.0; // fifth point: x
        kps[9] = 2.0; // fifth point: y
        let c = cell(4, 6, 0.81, 1.0, [0.0; 4], kps);
        let f = &decode(&c, 16, 64)[0];
        assert_eq!(f.landmarks[0], ((2.0 + 0.5) * 16.0, (1.0 - 0.5) * 16.0));
        assert_eq!(f.landmarks[4], ((2.0 + 1.0) * 16.0, (1.0 + 2.0) * 16.0));
    }

    /// The score is the square root of class times object: 0.81 x 0.64 gives 0.72, which
    /// passes 0.7, where the plain product (0.52) or either mean would not agree.
    #[test]
    fn the_score_is_the_geometric_mean() {
        let c = cell(4, 6, 0.81, 0.64, [0.0; 4], [0.0; 10]);
        let faces = decode(&c, 16, 64);
        assert_eq!(faces.len(), 1);
        assert!((faces[0].score - 0.72).abs() < 1e-3, "{}", faces[0].score);
    }

    #[test]
    fn a_score_under_the_threshold_is_dropped() {
        // sqrt(0.6 * 0.8) = 0.693
        let c = cell(4, 6, 0.6, 0.8, [0.0; 4], [0.0; 10]);
        assert!(decode(&c, 16, 64).is_empty());
    }

    /// The model's sigmoid outputs can leave 0..1 by a rounding error; a class of 1.3 must
    /// not lift a weak object score over the threshold.
    #[test]
    fn scores_are_clamped_before_they_are_multiplied() {
        // Unclamped: sqrt(1.3 * 0.4) = 0.721. Clamped: sqrt(1.0 * 0.4) = 0.632.
        let c = cell(4, 6, 1.3, 0.4, [0.0; 4], [0.0; 10]);
        assert!(decode(&c, 16, 64).is_empty());
    }

    fn raw(x: f32, y: f32, w: f32, h: f32, score: f32) -> Raw {
        Raw {
            x,
            y,
            w,
            h,
            landmarks: [(0.0, 0.0); 5],
            score,
        }
    }

    /// Two boxes sharing more than 0.3 of their union are one face: the stronger stays.
    #[test]
    fn overlapping_boxes_keep_the_higher_score() {
        // 10x10 boxes offset by 2: intersection 80, union 120, 0.67.
        let kept = suppress(
            vec![raw(0.0, 0.0, 10.0, 10.0, 0.8), raw(2.0, 0.0, 10.0, 10.0, 0.9)],
            MAX_IOU,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].score, 0.9);
    }

    /// Two faces side by side, their boxes just touching, are two faces.
    #[test]
    fn boxes_under_the_limit_are_both_kept() {
        // Offset by 8: intersection 20, union 180, 0.11.
        let kept = suppress(
            vec![raw(0.0, 0.0, 10.0, 10.0, 0.8), raw(8.0, 0.0, 10.0, 10.0, 0.9)],
            MAX_IOU,
        );
        assert_eq!(kept.len(), 2);
    }
}
```

Add `mod decode;` to `face_detect/mod.rs`, under the `use` lines.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib face_detect::decode`
Expected: compile errors, `cannot find type Raw`, `Level`, `decode_level`, `suppress`.

- [ ] **Step 3: Implement**

Put above the tests in `decode.rs`:

```rust
/// A detection must score at least this. Measured 2026-10-01: at 0.7 all 29 faces of a
/// group photograph (0.89-0.94) and all 401 sampled LFW subjects are kept, and the three
/// false detections in 23 photos without a face (an ibex at 0.48, a flower and a woman seen
/// from behind at 0.61) are not. At 0.9 four of the 29 are lost.
pub(crate) const SCORE_THRESHOLD: f32 = 0.7;

/// Two boxes sharing more than this much of their union are one face. The model authors'
/// value.
pub(crate) const MAX_IOU: f32 = 0.3;

/// One face in the model's input pixels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Raw {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub landmarks: [(f32, f32); 5],
    pub score: f32,
}

/// The four outputs for one stride, flattened: one value per cell for `cls` and `obj`,
/// four for `bbox`, ten for `kps`, cells in row order.
pub(crate) struct Level<'a> {
    pub stride: usize,
    pub cls: &'a [f32],
    pub obj: &'a [f32],
    pub bbox: &'a [f32],
    pub kps: &'a [f32],
}

/// Appends the faces of one level, for a square input `side` pixels wide.
pub(crate) fn decode_level(level: &Level<'_>, side: usize, threshold: f32, out: &mut Vec<Raw>) {
    let cols = side / level.stride;
    let stride = level.stride as f32;
    for n in 0..cols * cols {
        // Clamped first: a sigmoid that rounds past 1 must not carry a weak partner over
        // the threshold.
        let score = (level.cls[n].clamp(0.0, 1.0) * level.obj[n].clamp(0.0, 1.0)).sqrt();
        if score < threshold {
            continue;
        }
        let (row, col) = ((n / cols) as f32, (n % cols) as f32);
        let b = &level.bbox[n * 4..n * 4 + 4];
        let (cx, cy) = ((col + b[0]) * stride, (row + b[1]) * stride);
        let (w, h) = (b[2].exp() * stride, b[3].exp() * stride);
        let k = &level.kps[n * 10..n * 10 + 10];
        let mut landmarks = [(0.0, 0.0); 5];
        for (i, point) in landmarks.iter_mut().enumerate() {
            *point = ((col + k[2 * i]) * stride, (row + k[2 * i + 1]) * stride);
        }
        out.push(Raw {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
            landmarks,
            score,
        });
    }
}

fn iou(a: &Raw, b: &Raw) -> f32 {
    let w = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let h = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let both = w * h;
    both / (a.w * a.h + b.w * b.h - both)
}

/// Keeps the strongest of each set of boxes that overlap by more than `max_iou`.
pub(crate) fn suppress(mut faces: Vec<Raw>, max_iou: f32) -> Vec<Raw> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Raw> = Vec::new();
    for face in faces {
        if kept.iter().all(|k| iou(k, &face) <= max_iou) {
            kept.push(face);
        }
    }
    kept
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib face_detect::decode`
Expected: 8 passed.

- [ ] **Step 5: Probe each rule**

For each line below, make exactly that change, run the named test, see it FAIL, and restore (then `touch` the file):

| Change | Test that must fail |
|---|---|
| `let cols = side / level.stride;` → `let cols = 4;` | `the_stride_scales_and_places_the_box` |
| `b[2].exp() * stride` → `b[2] * stride` | `a_cell_becomes_a_box_in_input_pixels` |
| remove `.sqrt()` | `the_score_is_the_geometric_mean` |
| remove both `.clamp(0.0, 1.0)` | `scores_are_clamped_before_they_are_multiplied` |
| `if score < threshold { continue; }` removed | `a_score_under_the_threshold_is_dropped` |
| `(col + k[2 * i]) * stride` → `k[2 * i] * stride` | `landmarks_are_offsets_from_the_cell` |
| `b.score.total_cmp(&a.score)` → `a.score.total_cmp(&b.score)` | `overlapping_boxes_keep_the_higher_score` |
| `<= max_iou` → `<= 0.0` | `boxes_under_the_limit_are_both_kept` |

- [ ] **Step 6: Gate and commit**

```bash
git add crates/photon-core/src/face_detect
git commit -m "feat(faces): decode YuNet's outputs into boxes and landmarks

Each rule reverted in turn fails its own test: the column count, exp on
the size, the square root and the clamp on the score, the threshold, the
landmark's cell offset, the suppression order and its limit.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The detector, image in and fractions out

**Files:**
- Modify: `crates/photon-core/src/face_detect/mod.rs`
- Create: `crates/photon-core/testdata/faces/portrait.jpg`
- Create: `crates/photon-core/testdata/faces/README.md`

**Interfaces:**
- Consumes: `decode::{Raw, Level, decode_level, suppress, SCORE_THRESHOLD, MAX_IOU}`; `crate::decode::shrink_within(img: &DynamicImage, max_edge: u32) -> DynamicImage`.
- Produces:
  - `pub const DETECTOR_VERSION: i64 = 1;`
  - `pub struct Rect { pub left: f64, pub top: f64, pub right: f64, pub bottom: f64 }` (`Clone, Copy, Debug, PartialEq, Serialize`)
  - `pub struct Detection { pub rect: Rect, pub landmarks: [(f32, f32); 5], pub score: f32 }` (`Clone, Debug, PartialEq`)
  - `impl Detector { pub fn detect(&self, image: &DynamicImage) -> Result<Vec<Detection>> }`

- [ ] **Step 1: Add the fixture**

```bash
mkdir -p crates/photon-core/testdata/faces
curl -sSL -A "photon-tests/0.1 (https://github.com/bsg62/photon)" \
  -o crates/photon-core/testdata/faces/portrait.jpg \
  "https://thumb.wikimedia.org/wikipedia/commons/thumb/9/91/Portrait_of_a_man_%28Unsplash%29.jpg/960px-Portrait_of_a_man_%28Unsplash%29.jpg"
file crates/photon-core/testdata/faces/portrait.jpg
```

Expected: `JPEG image data ... 960x640`, about 90 KB. (On 2026-10-02 the file was 90,402 bytes, sha256 `d7afcc1c…283f8`. Wikimedia may render the thumbnail afresh; the tests below allow for that with a tolerance.)

`crates/photon-core/testdata/faces/README.md`:

```markdown
# Face detection fixtures

| File | Source | Author | Licence |
|---|---|---|---|
| `portrait.jpg` | [Portrait of a man (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Portrait_of_a_man_(Unsplash).jpg), Commons' 960 px rendering | William Stitt | CC0 |

One face, looking at the camera. On 2026-10-02 the detector put it at left 0.317, top 0.209,
right 0.638, bottom 0.896 with a score of 0.936.

The photo without a face is `crates/xtask/screenshots/photos/14.jpg` (credited there).
```

- [ ] **Step 2: Write the failing tests**

Add to the `tests` module in `face_detect/mod.rs`:

```rust
    use image::{DynamicImage, GenericImageView};
    use std::path::Path;

    fn fixture(rel: &str) -> DynamicImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn portrait() -> DynamicImage {
        fixture("testdata/faces/portrait.jpg")
    }

    /// The fixture's one face, where `testdata/faces/README.md` records it.
    fn assert_the_portraits_face(faces: &[Detection]) {
        assert_eq!(faces.len(), 1, "{faces:?}");
        let r = faces[0].rect;
        for (got, want) in [(r.left, 0.317), (r.top, 0.209), (r.right, 0.638), (r.bottom, 0.896)] {
            assert!((got - want).abs() < 0.05, "{r:?}");
        }
        assert!(faces[0].score >= 0.85, "{}", faces[0].score);
    }

    #[test]
    fn a_portrait_has_one_face_where_it_is() {
        let faces = Detector::new().unwrap().detect(&portrait()).unwrap();
        assert_the_portraits_face(&faces);
        // The first two landmarks are the eyes: inside the box, in its upper half, the
        // image-left one first.
        let r = faces[0].rect;
        let (a, b) = (faces[0].landmarks[0], faces[0].landmarks[1]);
        assert!(a.0 < b.0);
        for (x, y) in [a, b] {
            assert!((r.left..r.right).contains(&(x as f64)), "{x}");
            assert!((r.top..(r.top + r.bottom) / 2.0).contains(&(y as f64)), "{y}");
        }
    }

    #[test]
    fn a_landscape_has_no_face() {
        let img = fixture("../xtask/screenshots/photos/14.jpg");
        assert!(Detector::new().unwrap().detect(&img).unwrap().is_empty());
    }

    /// The portrait turned on its side is a portrait-format image: the padding moves from
    /// the bottom of the input to its right, and the face must come back at the same place
    /// in the turned picture. Fractions are of the image, never of the padded square.
    #[test]
    fn fractions_are_of_the_image_whatever_its_shape() {
        let detector = Detector::new().unwrap();
        let upright = detector.detect(&portrait()).unwrap();
        let turned = detector.detect(&portrait().rotate90()).unwrap();
        assert_eq!((upright.len(), turned.len()), (1, 1));
        // A quarter turn clockwise sends (x, y) to (1 - y, x).
        let (u, t) = (upright[0].rect, turned[0].rect);
        for (got, want) in [
            (t.left, 1.0 - u.bottom),
            (t.top, u.left),
            (t.right, 1.0 - u.top),
            (t.bottom, u.right),
        ] {
            assert!((got - want).abs() < 0.05, "upright {u:?} turned {t:?}");
        }
    }

    /// Review focus 1: a picture smaller than the input is padded, never scaled up, and
    /// its fractions are still of the picture. A third of the size, the same face.
    #[test]
    fn a_small_picture_is_not_scaled_up() {
        let small = portrait().resize(320, 320, image::imageops::FilterType::Triangle);
        assert_eq!(small.dimensions(), (320, 213));
        let faces = Detector::new().unwrap().detect(&small).unwrap();
        assert_the_portraits_face(&faces);
    }

    /// Review focus 2: a strip whose short side would scale to under a pixel.
    #[test]
    fn a_thin_strip_is_detected_without_a_zero_side() {
        let strip = DynamicImage::ImageRgb8(image::RgbImage::new(6400, 2));
        assert!(Detector::new().unwrap().detect(&strip).unwrap().is_empty());
        let dot = DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        assert!(Detector::new().unwrap().detect(&dot).unwrap().is_empty());
    }

    /// Review focus 3: the cache hands back RGBA for a photo with transparency, and a
    /// greyscale picture is one channel. Both are the same face.
    #[test]
    fn alpha_and_greyscale_pictures_are_read() {
        let detector = Detector::new().unwrap();
        let rgba = DynamicImage::ImageRgba8(portrait().to_rgba8());
        assert_the_portraits_face(&detector.detect(&rgba).unwrap());
        let grey = DynamicImage::ImageLuma8(portrait().to_luma8());
        assert_eq!(detector.detect(&grey).unwrap().len(), 1);
    }
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p photon-core --lib face_detect::tests`
Expected: compile errors, `no method named detect`, `cannot find type Detection`.

- [ ] **Step 4: Implement**

Add to `face_detect/mod.rs`, above `Detector`:

```rust
use decode::{Level, MAX_IOU, Raw, SCORE_THRESHOLD};
use image::DynamicImage;
use serde::Serialize;

/// Which detector looked at a photo: `items.face_version` records it. Bump it when the
/// model file, [`INPUT`], or the threshold or overlap limit in `decode` changes; every
/// photo is then detected again, as `EXIF_VERSION` re-reads metadata.
pub const DETECTOR_VERSION: i64 = 1;

/// A rectangle as fractions of a picture, 0..1 from its left and top.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// One face, in fractions of the image it was found in.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub rect: Rect,
    /// The model's five points, in its order: the two eyes (the image-left one first), the
    /// nose tip, the two mouth corners (image-left first).
    pub landmarks: [(f32, f32); 5],
    pub score: f32,
}

/// The strides of the model's three output levels, in output order.
const STRIDES: [usize; 3] = [8, 16, 32];
```

And inside `impl Detector`:

```rust
    /// The faces in `image`, strongest first.
    pub fn detect(&self, image: &DynamicImage) -> Result<Vec<Detection>> {
        // Scaled down to fit and never up: a face is found by its size in pixels, and
        // enlarging a small picture only invents them.
        let fitted = crate::decode::shrink_within(image, INPUT as u32).to_rgb8();
        // A 6400x2 strip scales to 1280x0.4. `shrink_within` may round that to nothing;
        // there is no face in it either way.
        let (w, h) = fitted.dimensions();
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        // Top left of a black square, as the model was trained: BGR, 0..255, planar.
        let plane = INPUT * INPUT;
        let mut input = vec![0f32; 3 * plane];
        for (x, y, px) in fitted.enumerate_pixels() {
            let at = y as usize * INPUT + x as usize;
            input[at] = px[2] as f32;
            input[plane + at] = px[1] as f32;
            input[2 * plane + at] = px[0] as f32;
        }
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, INPUT, INPUT), input)
            .map_err(model_error)?
            .into();
        let out = (self.run)(tensor).map_err(model_error)?;
        // Twelve outputs: class, object, box and landmarks, each at the three strides.
        if out.len() != 12 {
            return Err(model_error(format!("{} outputs, expected 12", out.len())));
        }
        let flat = |i: usize| -> Result<Vec<f32>> {
            Ok(out[i]
                .to_plain_array_view::<f32>()
                .map_err(model_error)?
                .iter()
                .copied()
                .collect())
        };
        let mut raw: Vec<Raw> = Vec::new();
        for (i, stride) in STRIDES.into_iter().enumerate() {
            let (cls, obj, bbox, kps) = (flat(i)?, flat(3 + i)?, flat(6 + i)?, flat(9 + i)?);
            let level = Level {
                stride,
                cls: &cls,
                obj: &obj,
                bbox: &bbox,
                kps: &kps,
            };
            decode::decode_level(&level, INPUT, SCORE_THRESHOLD, &mut raw);
        }
        let (w, h) = (w as f32, h as f32);
        let unit = |v: f32, of: f32| (v / of).clamp(0.0, 1.0);
        Ok(decode::suppress(raw, MAX_IOU)
            .into_iter()
            .map(|f| Detection {
                rect: Rect {
                    left: unit(f.x, w) as f64,
                    top: unit(f.y, h) as f64,
                    right: unit(f.x + f.w, w) as f64,
                    bottom: unit(f.y + f.h, h) as f64,
                },
                landmarks: f.landmarks.map(|(x, y)| (unit(x, w), unit(y, h))),
                score: f.score,
            })
            .collect())
    }
```

`shrink_within` is `pub(crate)` in `crate::decode` and returns the image unchanged when it already fits; confirm that by reading it. If it enlarges, or panics on a zero side, guard here rather than changing it: the thumbnail renderer depends on its behaviour.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-core --lib face_detect`
Expected: all pass. If `a_portrait_has_one_face_where_it_is` fails on the numbers, print the detection: a fresh Wikimedia rendering moves them by a few thousandths, not by 0.05. A larger difference is a bug in the channel order or the mapping.

- [ ] **Step 6: Probe**

| Change | Test that must fail |
|---|---|
| `input[at] = px[2]` and `input[2 * plane + at] = px[0]` swapped (RGB instead of BGR) | Run all; **record what happens.** A face is a face in either order, so this may pass. If it does, say so in the commit message rather than inventing a test: the order is the model's documented training input, not something a fixture discriminates. |
| `unit(f.x, w)` → `unit(f.x, INPUT as f32)` (and the other three) | `a_small_picture_is_not_scaled_up` |
| `unit(f.y, h)` → `unit(f.y, w)` | `fractions_are_of_the_image_whatever_its_shape` |
| remove the `if w == 0 || h == 0` guard | `a_thin_strip_is_detected_without_a_zero_side`, **if** `shrink_within` returns a zero side for it. If the test passes without the guard, `shrink_within` never does: remove the guard and its comment, and keep the test. |
| `.to_rgb8()` → `.as_rgb8().unwrap().clone()` | `alpha_and_greyscale_pictures_are_read` |

- [ ] **Step 7: Gate and commit**

```bash
git add crates/photon-core/src/face_detect crates/photon-core/testdata/faces
git commit -m "feat(faces): detect faces in an image

<the probe results, one line each; say which passed and why that is acceptable>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Schema 24 and the library's side

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs`
- Modify: `crates/photon-core/src/library/mod.rs`
- Create: `crates/photon-core/src/library/detected_faces.rs`
- Modify: `crates/photon-core/src/library/settings.rs`
- Modify: `crates/photon-core/src/library/items.rs` (`update_items` near line 513, `set_item_edit` near line 803)

**Interfaces:**
- Consumes: `face_detect::{Detection, Rect}`; `Edit`, `Crop::to_db`, `edit_from_db`, `media::fingerprint` as `library/similar.rs` uses them.
- Produces, on `Library`:
  - `pub fn face_detection(&self) -> Result<bool>`
  - `pub fn set_face_detection(&self, enabled: bool) -> Result<()>` (off deletes everything)
  - `pub fn face_candidates(&self, after_id: i64, limit: usize, version: i64) -> Result<Vec<FaceCandidate>>`
  - `pub fn write_face_batch(&self, batch: &[(FaceCandidate, Vec<Detection>)], version: i64) -> Result<usize>` (photos written; 0 with the setting off)
  - `pub fn face_progress(&self, version: i64) -> Result<(u64, u64)>` (checked, total)
  - `pub fn item_detected_faces(&self, item_id: i64) -> Result<Vec<Rect>>`
- Produces: `pub struct FaceCandidate { pub id: i64, pub thumb_key: u64, pub size: i64, pub mtime_ms: i64, pub edit: Edit }` (`Clone, Debug, PartialEq`), re-exported from `library`.
- Produces: `pub(crate) fn face_detection_on(conn: &Connection) -> rusqlite::Result<bool>` in `library::settings`.

- [ ] **Step 1: Write the failing tests**

Create `crates/photon-core/src/library/detected_faces.rs` with the tests only:

```rust
//! The faces photon found itself (`face_detect`), and which photos it has looked at. The
//! detecting is `crate::face_detect`; this is what it reads and writes.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;
    use crate::face_detect::{DETECTOR_VERSION, Detection, Rect};
    use crate::library::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};

    const V: i64 = DETECTOR_VERSION;

    fn face(left: f64) -> Detection {
        Detection {
            rect: Rect {
                left,
                top: 0.2,
                right: left + 0.1,
                bottom: 0.4,
            },
            landmarks: [(0.1, 0.2), (0.3, 0.4), (0.5, 0.6), (0.7, 0.8), (0.9, 1.0)],
            score: 0.9,
        }
    }

    /// A library with the setting on and `names.len()` photos, thumbnails ready.
    fn seeded(names: &[&str]) -> (tempfile::TempDir, Library, Vec<i64>) {
        let (dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, dir.path());
        let items: Vec<_> = names
            .iter()
            .map(|n| new_item(folder, &format!("{}/{n}", dir.path().display()), 1))
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        lib.set_face_detection(true).unwrap();
        (dir, lib, ids)
    }

    fn rows(lib: &Library) -> i64 {
        lib.reader()
            .unwrap()
            .query_row("SELECT count(*) FROM detected_faces", [], |r| r.get(0))
            .unwrap()
    }

    fn version_of(lib: &Library, id: i64) -> Option<i64> {
        lib.reader()
            .unwrap()
            .query_row("SELECT face_version FROM items WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_ready_photo_is_a_candidate_until_it_is_written() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        let candidates = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(candidates.iter().map(|c| c.id).collect::<Vec<_>>(), ids);

        let written = lib
            .write_face_batch(&[(candidates[0].clone(), vec![face(0.1), face(0.5)])], V)
            .unwrap();
        assert_eq!(written, 1);
        assert_eq!(rows(&lib), 2);
        assert_eq!(version_of(&lib, ids[0]), Some(V));
        let left = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(left.iter().map(|c| c.id).collect::<Vec<_>>(), [ids[1]]);
    }

    /// A photo with no face is written too, or it would be detected on every pass.
    #[test]
    fn a_photo_without_faces_is_marked_as_looked_at() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(lib.write_face_batch(&[(c[0].clone(), vec![])], V).unwrap(), 1);
        assert_eq!(version_of(&lib, ids[0]), Some(V));
        assert!(lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    /// Each of the candidate query's conditions, by breaking one at a time.
    #[test]
    fn what_is_not_a_candidate() {
        let (_dir, lib, ids) = seeded(&["pending.jpg", "video.mp4", "missing.jpg", "old.jpg"]);
        let w = lib.writer();
        w.execute("UPDATE items SET thumb_state = 0 WHERE id = ?1", [ids[0]]).unwrap();
        w.execute("UPDATE items SET kind = 1 WHERE id = ?1", [ids[1]]).unwrap();
        w.execute("UPDATE items SET missing_since = 5 WHERE id = ?1", [ids[2]]).unwrap();
        w.execute("UPDATE items SET face_version = ?2 WHERE id = ?1", [ids[3], V]).unwrap();
        drop(w);
        assert!(lib.face_candidates(0, 10, V).unwrap().is_empty());
        // A newer detector looks again at what an older one saw.
        let again = lib.face_candidates(0, 10, V + 1).unwrap();
        assert_eq!(again.iter().map(|c| c.id).collect::<Vec<_>>(), [ids[3]]);
    }

    #[test]
    fn candidates_page_by_id() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg", "c.jpg"]);
        let first = lib.face_candidates(0, 2, V).unwrap();
        assert_eq!(first.iter().map(|c| c.id).collect::<Vec<_>>(), ids[..2]);
        let rest = lib.face_candidates(first[1].id, 2, V).unwrap();
        assert_eq!(rest.iter().map(|c| c.id).collect::<Vec<_>>(), ids[2..]);
    }

    /// The pass reads the preview long after it listed the row. An edit in between makes
    /// what it detected a picture the row no longer shows.
    #[test]
    fn a_batch_is_refused_for_a_photo_edited_since_it_was_listed() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.set_item_edit(ids[0], Edit { turns: 1, crop: None }).unwrap();
        assert_eq!(lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V).unwrap(), 0);
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    /// As above for the file itself changing.
    #[test]
    fn a_batch_is_refused_for_a_photo_whose_file_changed() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.writer()
            .execute("UPDATE items SET mtime_ms = mtime_ms + 1 WHERE id = ?1", [ids[0]])
            .unwrap();
        assert_eq!(lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V).unwrap(), 0);
        assert_eq!(rows(&lib), 0);
    }

    /// Off means no face data is kept: a batch that was in flight when the user switched
    /// off writes nothing.
    #[test]
    fn a_batch_writes_nothing_with_the_setting_off() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.set_face_detection(false).unwrap();
        assert_eq!(lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V).unwrap(), 0);
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn switching_off_deletes_every_detection() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)])], V).unwrap();
        assert!(lib.face_detection().unwrap());

        lib.set_face_detection(false).unwrap();
        assert!(!lib.face_detection().unwrap());
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn the_setting_is_off_until_set() {
        let (_dir, lib) = temp_library();
        assert!(!lib.face_detection().unwrap());
    }

    #[test]
    fn an_edit_clears_the_photos_detections() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(
            &[(c[0].clone(), vec![face(0.1)]), (c[1].clone(), vec![face(0.2)])],
            V,
        )
        .unwrap();
        lib.set_item_edit(ids[0], Edit { turns: 1, crop: None }).unwrap();
        assert!(lib.item_detected_faces(ids[0]).unwrap().is_empty());
        assert_eq!(version_of(&lib, ids[0]), None);
        assert_eq!(lib.item_detected_faces(ids[1]).unwrap().len(), 1, "the other photo's stay");
    }

    #[test]
    fn a_rewritten_file_clears_the_photos_detections() {
        let (dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)])], V).unwrap();
        let (_, folder) = seed_folder(&lib, dir.path());
        let mut changed = new_item(folder, &format!("{}/a.jpg", dir.path().display()), 1);
        changed.size += 1;
        lib.update_items(&[(ids[0], changed)]).unwrap();
        assert!(lib.item_detected_faces(ids[0]).unwrap().is_empty());
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn progress_counts_live_images() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg", "gone.jpg", "clip.mp4"]);
        let w = lib.writer();
        w.execute("UPDATE items SET missing_since = 5 WHERE id = ?1", [ids[2]]).unwrap();
        w.execute("UPDATE items SET kind = 1 WHERE id = ?1", [ids[3]]).unwrap();
        drop(w);
        assert_eq!(lib.face_progress(V).unwrap(), (0, 2));
        let c = lib.face_candidates(0, 1, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![])], V).unwrap();
        assert_eq!(lib.face_progress(V).unwrap(), (1, 2));
    }

    #[test]
    fn rectangles_and_landmarks_round_trip() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.25)])], V).unwrap();
        assert_eq!(lib.item_detected_faces(ids[0]).unwrap(), [face(0.25).rect]);
        let blob: Vec<u8> = lib
            .reader()
            .unwrap()
            .query_row("SELECT landmarks FROM detected_faces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blob.len(), 40);
        assert_eq!(f32::from_le_bytes(blob[0..4].try_into().unwrap()), 0.1);
        assert_eq!(f32::from_le_bytes(blob[36..40].try_into().unwrap()), 1.0);
    }

    /// The candidate list reads every live photo, so it must walk the table by id, not the
    /// partial index `items_size` (`library/mod.rs` has why the bare term picks it).
    #[test]
    fn the_candidate_list_walks_the_table_by_id() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CANDIDATES_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params![0, V, 10], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|s| s.contains("INTEGER PRIMARY KEY")),
            "expected a walk by rowid, got {plan:?}"
        );
        assert!(!plan.iter().any(|s| s.contains("items_")), "{plan:?}");
    }
}
```

`insert_items(&[NewItem]) -> Result<Vec<i64>>` returns the new ids in order; `update_items` takes `&[(i64, NewItem)]`.

In `crates/photon-core/src/library/mod.rs`: add `mod detected_faces;` beside the other `mod` lines and `pub use detected_faces::FaceCandidate;` beside the other re-exports. In the same file change `assert_eq!(version, 23);` to `24`, `supported: 23` to `supported: 24`, add `'detected_faces'` to the table-name list and change `assert_eq!(tables, 12);` to `13`.

Add a migration test to `schema.rs`'s tests, after `migration_22_...`:

```rust
    /// A library at schema 23 with photos in it comes out with the table, and every photo
    /// unlooked-at.
    #[test]
    fn migration_24_adds_detected_faces_to_a_populated_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..23] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 23i64).unwrap();
        conn.execute_batch(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
             INSERT INTO folders (id, watched_id, path, name) VALUES (1, 1, '/p', 'p');
             INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, taken_at)
             VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1);",
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (faces, version): (i64, Option<i64>) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM detected_faces), face_version FROM items",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((faces, version), (0, None));
        let user: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(user, 24);
    }
```

The `INSERT`s name the columns that are `NOT NULL` without a default in the schema as read; if SQLite refuses one, read the `CREATE TABLE` in `MIGRATIONS[0]` and add what it asks for. Also change the two `assert_eq!(version, 23);` lines at the end of the older migration tests in `schema.rs` (they assert the version the upgrade ends at) to `24`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib detected_faces`
Expected: compile errors for `CANDIDATES_SQL`, `face_candidates`, and the rest.

- [ ] **Step 3: The migration**

Append to `MIGRATIONS` in `schema.rs`:

```rust
    r#"
-- The faces photon finds itself (`face_detect`), beside the ones Picasa recorded in `faces`.
-- A table of its own: `set_item_faces` replaces a photo's Picasa faces wholesale on every
-- reread of its INI, and a later stage hangs an embedding on a detected face by `id`.
--
-- The rectangle is fractions of the picture AS SHOWN, edit applied, because that is the
-- preview the detector reads. `faces` rows are fractions of the unedited picture and are
-- mapped through the edit on read. `face_detect::merge` is the one place the two meet.
--
-- `landmarks` is the model's five points as ten little-endian f32 fractions.
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
-- Which `face_detect::DETECTOR_VERSION` last looked at the photo, faces or none; NULL for
-- never. No index: the candidate query walks the table by id.
ALTER TABLE items ADD COLUMN face_version INTEGER;
"#,
```

- [ ] **Step 4: The setting**

In `settings.rs`, beside the other key constants:

```rust
/// Whether photon looks for faces itself. Absent is off: the pass costs hours on a large
/// library and computes data about the people in it, so nothing runs until asked.
const FACE_DETECTION: &str = "face_detection";
```

Beside `bump_thumb_gc_epoch`:

```rust
/// Whether face detection is on, read on `conn` so a writer can ask inside its own
/// transaction: `write_face_batch` must not store a face after the user switched off.
pub(crate) fn face_detection_on(conn: &Connection) -> rusqlite::Result<bool> {
    use rusqlite::OptionalExtension;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [FACE_DETECTION],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("1"))
}
```

In `impl Library`:

```rust
    /// Whether photon looks for faces itself.
    pub fn face_detection(&self) -> Result<bool> {
        Ok(face_detection_on(&*self.reader()?)?)
    }

    /// Switches face detection. Off deletes every detection and every record of a photo
    /// having been looked at, in the transaction that stores the setting: "off" means
    /// photon keeps no face data of its own, and switching back on detects again.
    pub fn set_face_detection(&self, enabled: bool) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        set_setting(&tx, FACE_DETECTION, if enabled { "1" } else { "0" })?;
        if !enabled {
            tx.execute("DELETE FROM detected_faces", [])?;
            tx.execute(
                "UPDATE items SET face_version = NULL WHERE face_version IS NOT NULL",
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
```

If `self.reader()?` does not deref to `Connection` that way, use the form the neighbouring `setting` method uses.

- [ ] **Step 5: The queries**

Put above the tests in `detected_faces.rs`:

```rust
use super::Library;
use super::items::edit_from_db;
use super::settings::face_detection_on;
use crate::Result;
use crate::edit::{Crop, Edit};
use crate::face_detect::{Detection, Rect};
use crate::media::fingerprint;
use rusqlite::params;

/// A photo the detector has not looked at, or that an older detector did.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceCandidate {
    pub id: i64,
    /// The cache key of the preview to read - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// What the detections will be stored against; see [`Library::write_face_batch`].
    pub size: i64,
    pub mtime_ms: i64,
    pub edit: Edit,
}

/// Live images whose thumbnail is ready and whose `face_version` is not the current one,
/// after a given id. `thumb_state = 1` is the point, as it is for the look-alike pass:
/// detection reads the cached preview, so a library from before this feature fills in
/// without a source file being decoded.
///
/// Not `w.online = 1`, unlike that pass's list: the preview is in photon's own cache, so a
/// drive that is unplugged can still be detected.
///
/// The `+` keeps this a walk of the table by id. Without statistics the bare term has
/// SQLite walk a partial index on that predicate instead (`library/mod.rs`).
const CANDIDATES_SQL: &str = "SELECT id, path, size, mtime_ms, edit_turns, edit_crop FROM items
     WHERE id > ?1 AND +missing_since IS NULL AND thumb_state = 1
       -- A poster frame is not the video.
       AND kind = 0
       AND face_version IS NOT ?2
     ORDER BY id LIMIT ?3";

fn landmarks_blob(points: &[(f32, f32); 5]) -> [u8; 40] {
    let mut blob = [0u8; 40];
    for (i, (x, y)) in points.iter().enumerate() {
        blob[i * 8..i * 8 + 4].copy_from_slice(&x.to_le_bytes());
        blob[i * 8 + 4..i * 8 + 8].copy_from_slice(&y.to_le_bytes());
    }
    blob
}

impl Library {
    /// Up to `limit` candidates with an id above `after_id`, in id order. The pass pages
    /// with the last id it read, so a photo it had to skip is not handed back in the same
    /// pass.
    pub fn face_candidates(
        &self,
        after_id: i64,
        limit: usize,
        version: i64,
    ) -> Result<Vec<FaceCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(CANDIDATES_SQL)?;
        let rows = stmt
            .query_map(params![after_id, version, limit as i64], |r| {
                let path: String = r.get(1)?;
                let size: i64 = r.get(2)?;
                let mtime_ms: i64 = r.get(3)?;
                let edit = edit_from_db(r.get(4)?, r.get(5)?);
                Ok(FaceCandidate {
                    id: r.get(0)?,
                    thumb_key: edit.thumb_key(fingerprint(&path, size, mtime_ms)),
                    size,
                    mtime_ms,
                    edit,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores what the detector found, and that it looked, for each photo of the batch.
    /// Returns how many photos were written.
    ///
    /// **Nothing is written with the setting off.** It is read here, inside the
    /// transaction, and writes are one connection: the switch's delete and a batch that was
    /// being detected when the user threw it cannot interleave, so no face lands after
    /// "off".
    ///
    /// **A photo that moved on is skipped,** by the guard `set_percep_hash` uses and for
    /// its reason: the preview was read long after the row was listed, and a file rewritten
    /// or an edit made in between makes these the faces of a picture the row no longer
    /// shows. The row keeps its cleared `face_version` and the next pass looks at the new
    /// picture.
    pub fn write_face_batch(
        &self,
        batch: &[(FaceCandidate, Vec<Detection>)],
        version: i64,
    ) -> Result<usize> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if !face_detection_on(&tx)? {
            return Ok(0);
        }
        let mut written = 0;
        {
            let mut mark = tx.prepare_cached(
                "UPDATE items SET face_version = ?2
                 WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL
                   AND edit_turns = ?5 AND edit_crop IS ?6",
            )?;
            let mut delete = tx.prepare_cached("DELETE FROM detected_faces WHERE item_id = ?1")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO detected_faces (item_id, left, top, right, bottom, landmarks, score)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (candidate, faces) in batch {
                let marked = mark.execute(params![
                    candidate.id,
                    version,
                    candidate.size,
                    candidate.mtime_ms,
                    candidate.edit.turns,
                    candidate.edit.crop.map(Crop::to_db),
                ])?;
                if marked != 1 {
                    continue;
                }
                written += 1;
                // An older detector's faces, when the version moved.
                delete.execute(params![candidate.id])?;
                for face in faces {
                    insert.execute(params![
                        candidate.id,
                        face.rect.left,
                        face.rect.top,
                        face.rect.right,
                        face.rect.bottom,
                        landmarks_blob(&face.landmarks).as_slice(),
                        face.score as f64,
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(written)
    }

    /// How many live images the current detector has looked at, and how many there are.
    /// The total includes photos whose thumbnail is not ready yet, so the progress line
    /// can sit below its end while thumbnails are still being made.
    pub fn face_progress(&self, version: i64) -> Result<(u64, u64)> {
        let conn = self.reader()?;
        let (checked, total): (i64, i64) = conn.query_row(
            "SELECT count(*) FILTER (WHERE face_version IS ?1), count(*) FROM items
             WHERE +missing_since IS NULL AND kind = 0",
            params![version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((checked as u64, total as u64))
    }

    /// The faces photon detected on one photo, in the picture as shown.
    pub fn item_detected_faces(&self, item_id: i64) -> Result<Vec<Rect>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT left, top, right, bottom FROM detected_faces WHERE item_id = ?1 ORDER BY id",
        )?;
        let rects = stmt
            .query_map(params![item_id], |r| {
                Ok(Rect {
                    left: r.get(0)?,
                    top: r.get(1)?,
                    right: r.get(2)?,
                    bottom: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rects)
    }
}
```

- [ ] **Step 6: The two clearing writers**

In `items.rs`, `set_item_edit`: add `face_version = NULL` to the `UPDATE`'s `SET` list after `similar_group = NULL`, and inside `if changed > 0 {`, before the epoch bump:

```rust
            // The detections describe the picture as shown, as the perceptual hash does:
            // a turn or a crop makes them rectangles on a picture photon no longer shows.
            tx.execute("DELETE FROM detected_faces WHERE item_id = ?1", params![id])?;
```

In `update_items`: add `face_version = NULL` to its `UPDATE`'s `SET` list after `similar_group = NULL`, and in the loop that executes it per item, after `stmt.execute(...)`, run a second prepared statement `DELETE FROM detected_faces WHERE item_id = ?1` with the id. Prepare it once beside `stmt`, in the same transaction.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p photon-core --lib detected_faces && cargo test -p photon-core --lib library`
Expected: all pass, including `open_creates_schema_and_is_idempotent` and `refuses_newer_schema` at 24.

- [ ] **Step 8: Probe**

| Change | Test that must fail |
|---|---|
| remove `if !face_detection_on(&tx)? { return Ok(0); }` | `a_batch_writes_nothing_with_the_setting_off` |
| remove `AND edit_turns = ?5 AND edit_crop IS ?6` from `mark` (and the two params) | `a_batch_is_refused_for_a_photo_edited_since_it_was_listed` |
| remove `AND size = ?3 AND mtime_ms = ?4` (and the two params) | `a_batch_is_refused_for_a_photo_whose_file_changed` |
| remove the `DELETE` added to `set_item_edit` | `an_edit_clears_the_photos_detections` |
| remove `face_version = NULL` from `set_item_edit` | `an_edit_clears_the_photos_detections` |
| remove the `DELETE` added to `update_items` | `a_rewritten_file_clears_the_photos_detections` |
| remove the two statements in `set_face_detection`'s `if !enabled` | `switching_off_deletes_every_detection` |
| `+missing_since` → `missing_since` in `CANDIDATES_SQL` | `the_candidate_list_walks_the_table_by_id`. **If it passes**, the `id > ?1 ORDER BY id` shape already forces the rowid walk and the `+` is not load-bearing here: remove the `+` and the sentence in the comment that claims it, and say so in the commit. A wrong justification is a defect. |
| remove `AND thumb_state = 1`, `AND kind = 0`, `AND +missing_since IS NULL`, and change `IS NOT ?2` to `IS NULL`, one at a time | `what_is_not_a_candidate` each time |

- [ ] **Step 9: Gate and commit**

```bash
git add crates/photon-core/src/library
git commit -m "feat(faces): schema 24, detected_faces and the face_detection setting

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The merge rule

**Files:**
- Create: `crates/photon-core/src/face_detect/merge.rs`
- Modify: `crates/photon-core/src/face_detect/mod.rs` (`pub mod merge;`)

**Interfaces:**
- Consumes: `face_detect::Rect`; `Edit::map_rect(self, (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)>`.
- Produces, in `face_detect::merge`:
  - `pub fn shown(edit: Edit, picasa: Rect) -> Option<Rect>`
  - `pub fn same_face(a: &Rect, b: &Rect) -> bool`
  - `pub fn unmatched(picasa_shown: &[Rect], detected: &[Rect]) -> Vec<Rect>`
  - `pub fn count(edit: Edit, picasa: &[Rect], detected: &[Rect]) -> u32`

- [ ] **Step 1: Write the failing tests**

Create `merge.rs` with:

```rust
//! The one rule for "the faces on this photo": Picasa's, plus every detection that is not
//! one of Picasa's. The viewer and search both go through it, so they cannot disagree.
//!
//! It is also the one place the two frames meet. A `faces` row is fractions of the
//! unedited picture; a `detected_faces` row is fractions of the picture as shown.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Crop, Edit};

    fn r(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_detection_over_a_picasa_face_is_the_same_face() {
        let picasa = [r(0.30, 0.20, 0.50, 0.50)];
        let detected = [r(0.32, 0.22, 0.48, 0.47)];
        assert!(unmatched(&picasa, &detected).is_empty());
    }

    #[test]
    fn a_detection_beside_a_picasa_face_is_another_face() {
        let picasa = [r(0.10, 0.20, 0.30, 0.50)];
        let detected = [r(0.60, 0.20, 0.80, 0.50)];
        assert_eq!(unmatched(&picasa, &detected), detected);
    }

    /// Picasa draws a face generously, hair and chin; the detector draws it tight. The
    /// tight box is a fifth of the loose one's area, which an overlap ratio of 0.3 would
    /// call two faces. Either centre inside the other box calls it one.
    #[test]
    fn boxes_of_different_tightness_around_one_face_match() {
        let loose = r(0.20, 0.10, 0.60, 0.70);
        let tight = r(0.34, 0.30, 0.50, 0.56);
        assert!(same_face(&loose, &tight));
        assert!(same_face(&tight, &loose));
    }

    /// Two people cheek to cheek: the boxes overlap, but neither centre is in the other.
    #[test]
    fn overlapping_boxes_of_two_people_do_not_match() {
        let a = r(0.20, 0.20, 0.50, 0.60);
        let b = r(0.45, 0.20, 0.75, 0.60);
        assert!(!same_face(&a, &b));
    }

    /// A face Picasa recorded in the half that a crop removed is not in the picture as
    /// shown: it is not counted, and it must not swallow a detection either.
    #[test]
    fn a_picasa_face_cropped_away_is_gone() {
        // Keep the left half.
        let edit = Edit {
            turns: 0,
            crop: Some(Crop::from_db(Crop::pack(0, 0, 32768, 65535))),
        };
        let cropped_away = r(0.70, 0.20, 0.90, 0.50);
        assert_eq!(shown(edit, cropped_away), None);
        // Something detected in the kept half, in the shown frame.
        let detected = [r(0.70, 0.20, 0.90, 0.50)];
        assert_eq!(count(edit, &[cropped_away], &detected), 1);
    }

    #[test]
    fn a_picasa_face_is_mapped_through_a_turn() {
        let edit = Edit { turns: 1, crop: None };
        // A quarter turn clockwise sends (l, t, r, b) to (1 - b, l, 1 - t, r).
        assert_eq!(shown(edit, r(0.1, 0.2, 0.3, 0.6)), Some(r(0.4, 0.1, 0.8, 0.3)));
    }

    #[test]
    fn the_count_is_picasas_faces_and_the_detections_that_are_not_theirs() {
        let edit = Edit::default();
        let picasa = [r(0.10, 0.20, 0.30, 0.50)];
        let detected = [r(0.12, 0.22, 0.28, 0.48), r(0.60, 0.20, 0.80, 0.50)];
        assert_eq!(count(edit, &picasa, &detected), 2);
        assert_eq!(count(edit, &picasa, &[]), 1);
        assert_eq!(count(edit, &[], &detected), 2);
        assert_eq!(count(edit, &[], &[]), 0);
    }
}
```

`Crop::from_db` and a packing helper may be spelled differently in `edit.rs`; build the crop the way `edit.rs`'s own tests do. The crop wanted is left 0, top 0, right half of `CROP_UNIT`, bottom `CROP_UNIT`.

Add `pub mod merge;` to `face_detect/mod.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib face_detect::merge`
Expected: compile errors for `unmatched`, `same_face`, `shown`, `count`.

- [ ] **Step 3: Implement**

```rust
use super::Rect;
use crate::edit::Edit;

/// A Picasa rectangle in the picture as shown, or `None` when the edit crops its centre
/// away - `viewer_item`'s rule, which this now holds for it.
pub fn shown(edit: Edit, picasa: Rect) -> Option<Rect> {
    let (left, top, right, bottom) =
        edit.map_rect((picasa.left, picasa.top, picasa.right, picasa.bottom))?;
    Some(Rect {
        left,
        top,
        right,
        bottom,
    })
}

fn centre(r: &Rect) -> (f64, f64) {
    ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
}

fn holds(r: &Rect, (x, y): (f64, f64)) -> bool {
    (r.left..=r.right).contains(&x) && (r.top..=r.bottom).contains(&y)
}

/// Whether two rectangles are the same face: either one's centre lies inside the other.
///
/// Not an overlap ratio. Picasa draws a face with its hair and chin and the detector draws
/// it tight, so one face's two boxes can share a fifth of their union - less than two
/// neighbours' boxes do. A centre is where the face is, whatever the box's generosity.
pub fn same_face(a: &Rect, b: &Rect) -> bool {
    holds(a, centre(b)) || holds(b, centre(a))
}

/// The detections that are none of Picasa's faces. Both in the picture as shown.
pub fn unmatched(picasa_shown: &[Rect], detected: &[Rect]) -> Vec<Rect> {
    detected
        .iter()
        .filter(|d| !picasa_shown.iter().any(|p| same_face(p, d)))
        .copied()
        .collect()
}

/// How many faces a photo has: Picasa's that the edit still shows, and the detections
/// that are none of them. `picasa` is in the unedited picture, as `faces` stores it.
pub fn count(edit: Edit, picasa: &[Rect], detected: &[Rect]) -> u32 {
    let picasa: Vec<Rect> = picasa.iter().filter_map(|p| shown(edit, *p)).collect();
    (picasa.len() + unmatched(&picasa, detected).len()) as u32
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib face_detect::merge`
Expected: 7 passed.

- [ ] **Step 5: Probe**

| Change | Test that must fail |
|---|---|
| `same_face` body → `holds(a, centre(b))` only | `boxes_of_different_tightness_around_one_face_match` (its second assert) |
| `same_face` body → an IoU above 0.3 | `boxes_of_different_tightness_around_one_face_match` |
| `same_face` body → "the rectangles intersect" | `overlapping_boxes_of_two_people_do_not_match` |
| `count`: `filter_map(|p| shown(edit, *p))` → `map(|p| *p)` | `a_picasa_face_cropped_away_is_gone` |
| `unmatched`: `!picasa_shown.iter().any(...)` → `true` | `a_detection_over_a_picasa_face_is_the_same_face` |

- [ ] **Step 6: Gate and commit**

```bash
git add crates/photon-core/src/face_detect
git commit -m "feat(faces): one rule merging Picasa's faces with detections

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The pass

**Files:**
- Create: `crates/photon-core/src/face_detect/pass.rs`
- Modify: `crates/photon-core/src/face_detect/mod.rs` (`pub mod pass;`)

**Interfaces:**
- Consumes: `Library::{face_candidates, write_face_batch}`, `FaceCandidate`; `ThumbCache::read(&self, fp: u64, size: ThumbSize) -> Result<DynamicImage>` (`pub(crate)`), `ThumbSize::Preview`; `Detection`.
- Produces, in `face_detect::pass`:
  - `pub const BATCH: usize = 64;`
  - `pub type Detect<'a> = dyn Fn(&DynamicImage) -> Result<Vec<Detection>> + Sync + 'a;`
  - `pub fn run(lib: &Library, cache: &ThumbCache, version: i64, workers: usize, detect: &Detect<'_>, cancel: &(dyn Fn() -> bool + Sync), on_batch: &mut dyn FnMut(usize)) -> Result<usize>` (photos written in all; `on_batch` gets each batch's count)
  - `pub fn workers() -> usize`

The detector is passed as a function so these tests run without the model: a stand-in that returns one face, or panics.

- [ ] **Step 1: Write the failing tests**

Create `pass.rs` with:

```rust
//! One pass over the photos the detector has not looked at: read each one's cached
//! preview, detect, write. The engine owns the thread, the triggers and the progress; this
//! is the loop, here so it can be run against a library and a cache without an engine.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face_detect::{DETECTOR_VERSION, Rect};
    use crate::library::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use image::RgbImage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const V: i64 = DETECTOR_VERSION;

    struct Fixture {
        _dir: tempfile::TempDir,
        lib: Library,
        cache: ThumbCache,
        ids: Vec<i64>,
    }

    /// `n` photos, thumbnails ready and cached, the setting on.
    fn fixture(n: usize) -> Fixture {
        let (dir, lib) = temp_library();
        let cache = ThumbCache::new(dir.path().join("thumbs"));
        let (_, folder) = seed_folder(&lib, dir.path());
        let items: Vec<_> = (0..n)
            .map(|i| new_item(folder, &format!("{}/{i}.jpg", dir.path().display()), 1))
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        lib.set_face_detection(true).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        let picture = DynamicImage::ImageRgb8(RgbImage::new(64, 48));
        for c in lib.face_candidates(0, n, V).unwrap() {
            cache.store(c.thumb_key, &picture, &picture).unwrap();
        }
        Fixture {
            _dir: dir,
            lib,
            cache,
            ids,
        }
    }

    fn one_face(_: &DynamicImage) -> Result<Vec<Detection>> {
        Ok(vec![Detection {
            rect: Rect {
                left: 0.1,
                top: 0.1,
                right: 0.2,
                bottom: 0.2,
            },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        }])
    }

    fn never() -> bool {
        false
    }

    fn faces(lib: &Library) -> i64 {
        lib.reader()
            .unwrap()
            .query_row("SELECT count(*) FROM detected_faces", [], |r| r.get(0))
            .unwrap()
    }

    /// More photos than a batch, so the loop pages: every one is detected, once.
    #[test]
    fn every_candidate_is_detected_once() {
        let f = fixture(BATCH + 5);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        let mut batches = Vec::new();
        let written = run(&f.lib, &f.cache, V, 3, &detect, &never, &mut |n| batches.push(n)).unwrap();
        assert_eq!(written, BATCH + 5);
        assert_eq!(batches, [BATCH, 5]);
        assert_eq!(calls.load(Ordering::SeqCst), BATCH + 5);
        assert_eq!(faces(&f.lib), (BATCH + 5) as i64);
        assert!(f.lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    /// A preview that is not in the cache is skipped and the photo stays a candidate - and
    /// the pass ends all the same, rather than asking for it again for ever.
    #[test]
    fn a_missing_preview_is_skipped_and_stays_a_candidate() {
        let f = fixture(3);
        let gone = f.lib.face_candidates(0, 10, V).unwrap()[1].clone();
        std::fs::remove_file(f.cache.path_for(gone.thumb_key, ThumbSize::Preview)).unwrap();
        let written = run(&f.lib, &f.cache, V, 1, &one_face, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 2);
        let left = f.lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(left.iter().map(|c| c.id).collect::<Vec<_>>(), [gone.id]);
    }

    /// A photo the detector panics on is written as looked-at with no faces, so it is not
    /// tried again on every pass, and the photos around it are detected.
    #[test]
    fn a_panic_in_the_detector_costs_one_photo() {
        let f = fixture(3);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                panic!("a photo the model cannot take");
            }
            one_face(img)
        };
        let written = run(&f.lib, &f.cache, V, 1, &detect, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 3);
        assert_eq!(faces(&f.lib), 2);
        assert!(f.lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    #[test]
    fn an_error_from_the_detector_costs_one_photo() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(crate::Error::FaceModel("no".into()));
            }
            one_face(img)
        };
        assert_eq!(run(&f.lib, &f.cache, V, 1, &detect, &never, &mut |_| {}).unwrap(), 2);
        assert_eq!(faces(&f.lib), 1);
    }

    /// Cancelled before it starts, the pass reads nothing and writes nothing.
    #[test]
    fn a_cancelled_pass_does_nothing() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        assert_eq!(run(&f.lib, &f.cache, V, 2, &detect, &|| true, &mut |_| {}).unwrap(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.lib.face_candidates(0, 10, V).unwrap().len(), 2);
    }

    /// Cancelled in the middle, the pass stops at the photo it is on.
    #[test]
    fn cancelling_stops_between_photos() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        let cancel = || calls.load(Ordering::SeqCst) >= 3;
        run(&f.lib, &f.cache, V, 1, &detect, &cancel, &mut |_| {}).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn an_empty_library_is_a_pass_with_nothing_to_do() {
        let f = fixture(0);
        let mut batches = 0;
        assert_eq!(run(&f.lib, &f.cache, V, 4, &one_face, &never, &mut |_| batches += 1).unwrap(), 0);
        assert_eq!(batches, 0);
        assert!(f.ids.is_empty());
    }

    #[test]
    fn workers_are_between_one_and_four() {
        assert!((1..=4).contains(&workers()));
    }
}
```

Add `pub mod pass;` to `face_detect/mod.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib face_detect::pass`
Expected: compile errors for `run`, `BATCH`, `workers`.

- [ ] **Step 3: Implement**

```rust
use super::Detection;
use crate::Result;
use crate::library::{FaceCandidate, Library};
use crate::thumbs::cache::{ThumbCache, ThumbSize};
use image::DynamicImage;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Photos per write. One transaction a batch: small enough that a quit loses under a
/// minute of work, large enough that the writer is not taken per photo.
pub const BATCH: usize = 64;

/// What finds the faces in a preview: `Detector::detect`, or a test's stand-in.
pub type Detect<'a> = dyn Fn(&DynamicImage) -> Result<Vec<Detection>> + Sync + 'a;

/// Half the machine's cores, at least one and at most four. A worker holds 100-120 MB at
/// the detector's input size (measured: 159 MB with one, 501 MB with four), so the cap is
/// what bounds the pass's memory; the half is what leaves the machine usable for the
/// hours a first pass over a large library takes.
pub fn workers() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, 4)
}

/// Detects every candidate, in batches of [`BATCH`]. Returns how many photos were written;
/// `on_batch` is told each batch's count as it lands.
///
/// **Pages by id.** A photo whose preview cannot be read is skipped without being written,
/// so it is still a candidate - asked for again from the start, it would be handed back for
/// ever. From after the last id read, it waits for the next pass.
pub fn run(
    lib: &Library,
    cache: &ThumbCache,
    version: i64,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize> {
    let mut after = 0;
    let mut total = 0;
    while !cancel() {
        let candidates = lib.face_candidates(after, BATCH, version)?;
        let Some(last) = candidates.last() else {
            break;
        };
        after = last.id;
        let found = detect_batch(&candidates, cache, workers, detect, cancel);
        let batch: Vec<(FaceCandidate, Vec<Detection>)> = candidates
            .into_iter()
            .zip(found)
            .filter_map(|(candidate, faces)| Some((candidate, faces?)))
            .collect();
        let written = lib.write_face_batch(&batch, version)?;
        total += written;
        on_batch(written);
    }
    Ok(total)
}

/// One answer per candidate, in order: the faces found, or `None` for a photo not looked
/// at (its preview unreadable, or the pass cancelled before its turn).
fn detect_batch(
    candidates: &[FaceCandidate],
    cache: &ThumbCache,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Vec<Option<Vec<Detection>>> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<Option<Vec<Detection>>>> = Mutex::new(vec![None; candidates.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, candidates.len().max(1)) {
            scope.spawn(|| {
                loop {
                    if cancel() {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(candidate) = candidates.get(i) else {
                        return;
                    };
                    let faces = detect_one(candidate, cache, detect);
                    found.lock().unwrap_or_else(|e| e.into_inner())[i] = faces;
                }
            });
        }
    });
    found.into_inner().unwrap_or_else(|e| e.into_inner())
}

fn detect_one(
    candidate: &FaceCandidate,
    cache: &ThumbCache,
    detect: &Detect<'_>,
) -> Option<Vec<Detection>> {
    // Unreadable is usually a cache still being written: left a candidate, as the
    // look-alike pass leaves a thumbnail it cannot read.
    let preview = match cache.read(candidate.thumb_key, ThumbSize::Preview) {
        Ok(preview) => preview,
        Err(err) => {
            tracing::debug!(id = candidate.id, %err, "could not read a preview to detect faces");
            return None;
        }
    };
    // tract is Rust and unwinds, so a photo it panics on costs that photo. It is written
    // as looked-at with no faces: the same picture would panic on every pass.
    match catch_unwind(AssertUnwindSafe(|| detect(&preview))) {
        Ok(Ok(faces)) => Some(faces),
        Ok(Err(err)) => {
            tracing::warn!(id = candidate.id, %err, "face detection failed on a photo");
            Some(Vec::new())
        }
        Err(_) => {
            tracing::warn!(id = candidate.id, "face detection panicked on a photo");
            Some(Vec::new())
        }
    }
}
```

Check the paths: `ThumbCache` and `ThumbSize` are imported in `similar.rs` (`use crate::thumbs::...`); use the same path. `Detection` needs `Clone` for `vec![None; n]`, which Task 3 gave it.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib face_detect::pass`
Expected: 8 passed. The panic test prints a panic message to stderr; that is the test working.

- [ ] **Step 5: Probe**

| Change | Test that must fail |
|---|---|
| `after = last.id;` removed | `a_missing_preview_is_skipped_and_stays_a_candidate` (it never ends: run it with `timeout 20 cargo test ...` and take the timeout as the failure) |
| `catch_unwind(AssertUnwindSafe(|| detect(&preview)))` → `Ok(detect(&preview))` | `a_panic_in_the_detector_costs_one_photo` |
| `Ok(Err(err)) => { ...; Some(Vec::new()) }` → `None` | `an_error_from_the_detector_costs_one_photo` |
| `Err(err) => { ...; return None; }` → `return Some(Vec::new())` | `a_missing_preview_is_skipped_and_stays_a_candidate` |
| `while !cancel()` → `loop` | `a_cancelled_pass_does_nothing` |
| remove `if cancel() { return; }` in the worker | `cancelling_stops_between_photos` |

- [ ] **Step 6: Gate and commit**

```bash
git add crates/photon-core/src/face_detect
git commit -m "feat(faces): the detection pass over cached previews

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: The engine: thread, triggers, switch, progress

**Files:**
- Modify: `crates/photon-app/src/engine.rs`
- Modify: `crates/photon-app/src/events.rs`
- Modify: `crates/photon-app/src/app.rs`
- Modify: `crates/photon-app/src/commands.rs`
- Modify: `crates/photon-app/src/ipc.rs`
- Modify: `crates/photon-app/src/testutil.rs`
- Modify: `ui/src/lib/api.ts`
- Modify: `crates/xtask/screenshots/mock.js`

**Interfaces:**
- Consumes: `photon_core::face_detect::{Detector, DETECTOR_VERSION, pass}`; `Library::{face_detection, set_face_detection, face_progress}`; `photon_core::search::Query` (its `needs().faces` arrives in Task 8; until then `view_reads_faces` returns `false`, see Step 5).
- Produces:
  - `events::FaceProgress { pub checked: u64, pub total: u64, pub running: bool }`, serde camelCase; `Events::face_progress(&self, FaceProgress)`; `Recorded::Face(FaceProgress)`; Tauri event name `face-progress`.
  - `Engine::request_face_pass(self: &Arc<Self>)`; `Engine::set_face_detection(self: &Arc<Self>, enabled: bool) -> photon_core::Result<()>`; `Engine::face_detection(&self) -> bool`.
  - Commands `face_detection() -> bool` and `set_face_detection(enabled: bool) -> ()`.
  - `api.faceDetection()`, `api.setFaceDetection(enabled)`, `events.onFaceProgress(cb)`, `interface FaceProgress { checked: number; total: number; running: boolean }`.
- Renames: `Engine.similar_passes` → `background_passes`; `wait_for_similar_pass` → `wait_for_passes`; `stop_similar_pass` → `stop_passes`. Every use moves with them, tests included.

- [ ] **Step 1: The event**

In `events.rs`, after `ExportProgress`:

```rust
/// How far the face pass has got: live images the current detector has looked at, of all
/// live images. `running` is false on a pass's last event, which is what clears the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceProgress {
    pub checked: u64,
    pub total: u64,
    pub running: bool,
}
```

Add `fn face_progress(&self, event: FaceProgress);` to `Events`, `Face(FaceProgress)` to `Recorded`, and to `impl Events for Recorder`:

```rust
    fn face_progress(&self, e: FaceProgress) {
        self.0.lock().push(Recorded::Face(e));
    }
```

In `app.rs`, beside the other event-name constants add `const FACE_PROGRESS: &str = "face-progress";`, import `FaceProgress`, and in `impl Events for TauriEvents`:

```rust
    fn face_progress(&self, e: FaceProgress) {
        self.emit(FACE_PROGRESS, e);
    }
```

- [ ] **Step 2: The rename, with no behaviour change**

In `engine.rs` rename the field `similar_passes` to `background_passes`, `wait_for_similar_pass` to `wait_for_passes` and `stop_similar_pass` to `stop_passes`, at every use (`grep -n "similar_passes\|wait_for_similar_pass\|stop_similar_pass" crates/photon-app/src`), including `testutil.rs`'s `settle`. Reword the field's doc comment to say it counts every thread `spawn_pass` has let through, the look-alike pass's and the face pass's.

Then pull the counting out of `request_similar_pass` so both passes share it. Replace its body from `self.similar_passes.fetch_add` to the end with a call, and add the helper, keeping the long comment about the order of the count and the flag on the helper:

```rust
    pub fn request_similar_pass(self: &Arc<Self>) {
        self.spawn_pass("photon-similar-pass", |engine| {
            engine.hash_after_scan(&engine.shutting_down)
        });
    }

    /// Runs `pass` on a thread of its own, counted in `background_passes` from before the
    /// thread exists until it returns, however it ends.
    ///
    /// <the existing comment on why the count is raised before the flag is read, moved here>
    fn spawn_pass(self: &Arc<Self>, name: &str, pass: fn(&Arc<Engine>)) {
        self.background_passes.fetch_add(1, Ordering::SeqCst);
        if self.shutting_down.load(Ordering::SeqCst) {
            self.background_passes.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        // Lowers the count however the pass ends, a panic included, and also when the
        // thread never starts: made out here and moved in, it is dropped with the closure
        // if the spawn fails. Left raised, the count holds every later `shutdown` to its
        // full timeout.
        struct Done(Arc<Engine>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.background_passes.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let done = Done(Arc::clone(self));
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || pass(&done.0))
            .expect("failed to spawn a pass thread");
    }
```

Run: `cargo test -p photon-app`
Expected: everything that passed before still passes. Commit this step on its own:

```bash
git commit -am "refactor(engine): one counted spawn for background passes

No behaviour change: request_similar_pass's counting moves into
spawn_pass, and the counter and its two waits are renamed for the face
pass that will share them.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: A face fixture for engine tests**

In `crates/photon-app/src/testutil.rs`:

```rust
/// photon-core's face fixture: one person looking at the camera
/// (`crates/photon-core/testdata/faces/README.md`).
pub fn portrait_jpeg() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../photon-core/testdata/faces/portrait.jpg");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
```

- [ ] **Step 4: Write the failing tests**

Add to `engine.rs`'s tests (they use `fixture`, `jpeg`, `portrait_jpeg` from `testutil`, and `Recorded`):

```rust
    fn detected(f: &Fixture) -> i64 {
        f.engine
            .lib
            .reader()
            .unwrap()
            .query_row("SELECT count(*) FROM detected_faces", [], |r| r.get(0))
            .unwrap()
    }

    /// Scans, waits for the thumbnails, and lets every pass that follows finish.
    fn scanned(f: &Fixture) {
        f.add_photos();
        f.engine.thumbs.wait_idle();
        f.settle();
    }

    /// Off by default: a scan, its thumbnails and every pass after them leave no face data.
    #[test]
    fn nothing_is_detected_until_the_switch_is_on() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        assert!(!f.engine.face_detection());
        assert_eq!(detected(&f), 0);
    }

    /// Switching on is itself a request: the photos already there are detected without a
    /// scan happening by.
    #[test]
    fn switching_on_detects_the_photos_already_there() {
        let f = fixture(&[
            ("a/face.jpg", &portrait_jpeg()),
            ("a/plain.jpg", &jpeg(200, 150)),
        ]);
        scanned(&f);

        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();

        assert_eq!(detected(&f), 1);
        let (checked, total) = f
            .engine
            .lib
            .face_progress(photon_core::face_detect::DETECTOR_VERSION)
            .unwrap();
        assert_eq!((checked, total), (2, 2));
    }

    /// With the switch on, a photo that arrives later is detected by the pass its own
    /// scan and thumbnail bring, with nothing else asking.
    #[test]
    fn a_photo_scanned_with_the_switch_on_is_detected() {
        let f = fixture(&[("a/plain.jpg", &jpeg(200, 150))]);
        f.engine.set_face_detection(true).unwrap();
        f.engine.start_thumb_hashing(std::time::Duration::from_millis(20));
        let watched = f.add_photos();
        std::fs::write(f.photos.join("a").join("face.jpg"), portrait_jpeg()).unwrap();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        f.engine.thumbs.wait_idle();
        // The drain's settle, then the pass it requests.
        std::thread::sleep(std::time::Duration::from_millis(200));
        f.settle();
        assert_eq!(detected(&f), 1);
    }

    #[test]
    fn switching_off_leaves_no_detections() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(detected(&f), 1);

        f.engine.set_face_detection(false).unwrap();
        f.engine.wait_for_passes();
        assert_eq!(detected(&f), 0);
        assert!(!f.engine.face_detection());
    }

    /// Detections are read by the grid of a face search and by the viewer, never by an
    /// album, person, tag or folder count: the pass's rebuild must not send the UI to
    /// refetch the sidebar.
    #[test]
    fn a_face_pass_does_not_announce_a_data_change() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        // Anything pending from the scan is announced by this.
        f.engine.refresh_grid().unwrap();
        let before = f.events.all().len();

        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();

        let after: Vec<_> = f.events.all().split_off(before);
        let libraries: Vec<_> = after
            .iter()
            .filter_map(|e| match e {
                Recorded::Library(l) => Some(*l),
                _ => None,
            })
            .collect();
        assert!(!libraries.is_empty(), "the pass rebuilt nothing: {after:?}");
        assert!(libraries.iter().all(|l| !l.data_changed), "{libraries:?}");
    }

    /// Review focus 4: with nothing to detect the pass still ends with an event that says
    /// it is not running, so a progress line cannot be left standing.
    #[test]
    fn a_pass_with_nothing_to_do_still_says_it_ended() {
        let f = fixture(&[]);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        let last = f
            .events
            .all()
            .into_iter()
            .rev()
            .find_map(|e| match e {
                Recorded::Face(p) => Some(p),
                _ => None,
            })
            .expect("no face progress was sent");
        assert_eq!(
            last,
            FaceProgress {
                checked: 0,
                total: 0,
                running: false
            }
        );
    }

    /// The pass ends with what it reached, running false.
    #[test]
    fn progress_ends_at_the_count_checked() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        let faces: Vec<FaceProgress> = f
            .events
            .all()
            .into_iter()
            .filter_map(|e| match e {
                Recorded::Face(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            faces.last(),
            Some(&FaceProgress {
                checked: 1,
                total: 1,
                running: false
            })
        );
    }

    #[test]
    fn a_face_pass_is_not_requested_once_shutting_down() {
        let f = fixture(&[("a/face.jpg", &portrait_jpeg())]);
        scanned(&f);
        f.engine.set_face_detection(true).unwrap();
        f.engine.wait_for_passes();
        f.engine.lib.set_face_detection(false).unwrap();
        f.engine.lib.set_face_detection(true).unwrap(); // every photo a candidate again
        f.engine.shutdown();
        f.engine.request_face_pass();
        f.engine.wait_for_passes();
        assert_eq!(detected(&f), 0);
    }
```

`fixture(&[])` with no files must give an empty `photos` directory; it does as written in `testutil.rs`. If `refresh_grid` is private to the module, the tests are in it and may call it.

- [ ] **Step 5: Run them to see them fail**

Run: `cargo test -p photon-app --lib face`
Expected: compile errors for `set_face_detection`, `face_detection`, `request_face_pass`, `Recorded::Face`.

- [ ] **Step 6: Implement the engine side**

Constants, beside `THROTTLE`:

```rust
/// How often a running face pass rebuilds the grid, and only while the view is a search
/// that reads faces. A pass over a large library runs for hours; rebuilding every few
/// seconds for all of it would be its largest cost, for nothing anyone is looking at.
const FACE_REBUILD_EVERY: Duration = Duration::from_secs(30);

/// How often a running face pass reports its progress. Each report counts the library.
const FACE_PROGRESS_EVERY: Duration = Duration::from_secs(1);
```

Fields on `Engine`, after `hash_requested`:

```rust
    /// The `face_detection` setting, mirrored so a running pass can be cancelled without a
    /// database read per photo. `set_face_detection` writes both; the stored value is the
    /// authority (`write_face_batch` reads it in its own transaction).
    face_enabled: AtomicBool,
    /// Held by the one running face pass, as `hashing` is by the look-alike pass.
    face_pass: Mutex<()>,
    /// A request that found the pass running: the runner goes round again.
    face_requested: AtomicBool,
```

In `Engine::open`, read the setting before `lib` moves into the struct (`let face_enabled = lib.face_detection()?;`, beside `let sort = lib.grid_sort()?;`) and initialise `face_enabled: AtomicBool::new(face_enabled)`, `face_pass: Mutex::new(())`, `face_requested: AtomicBool::new(false)`.

Methods, after `request_similar_pass`:

```rust
    /// Whether photon looks for faces itself.
    pub fn face_detection(&self) -> bool {
        self.face_enabled.load(Ordering::SeqCst)
    }

    /// Switches face detection. On requests a pass; off cancels the running one and
    /// deletes what was found.
    ///
    /// The mirror moves first, so a pass in flight stops at its next photo rather than
    /// detecting through the delete. A batch it had already detected is refused by
    /// `write_face_batch`, which reads the stored setting inside its own transaction.
    pub fn set_face_detection(self: &Arc<Self>, enabled: bool) -> Result<()> {
        let was = self.face_enabled.swap(enabled, Ordering::SeqCst);
        if let Err(err) = self.lib.set_face_detection(enabled) {
            self.face_enabled.store(was, Ordering::SeqCst);
            return Err(err);
        }
        if enabled {
            self.request_face_pass();
        } else {
            self.refresh_after_write("switching face detection off");
            self.events.face_progress(FaceProgress {
                checked: 0,
                total: 0,
                running: false,
            });
        }
        Ok(())
    }

    /// Requests a face pass, on its own thread: a no-op with the switch off. Coalesces
    /// with one already running, like `request_similar_pass`.
    pub fn request_face_pass(self: &Arc<Self>) {
        if !self.face_detection() {
            return;
        }
        self.spawn_pass("photon-face-pass", |engine| engine.detect_faces());
    }

    /// One pass at a time, `hash_after_scan`'s way: a request that finds the pass running
    /// sets the flag and leaves, and the runner goes round while it is set, so photos
    /// whose thumbnails became ready after its last batch are not left for the next scan.
    fn detect_faces(&self) {
        self.face_requested.store(true, Ordering::Release);
        loop {
            let Some(guard) = self.face_pass.try_lock() else {
                return;
            };
            while self.face_requested.swap(false, Ordering::AcqRel) {
                self.run_face_pass();
            }
            drop(guard);
            if !self.face_requested.load(Ordering::Acquire) {
                return;
            }
        }
    }

    fn face_cancelled(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst) || !self.face_enabled.load(Ordering::SeqCst)
    }

    /// Whether the grid on screen is a search whose query reads faces: the one view a
    /// detection changes while the pass is still running.
    fn view_reads_faces(&self) -> bool {
        let state = self.state.lock();
        state.view == GridView::Search && photon_core::search::Query::parse(&state.arg).needs().faces
    }

    fn send_face_progress(&self, running: bool) {
        match self.lib.face_progress(photon_core::face_detect::DETECTOR_VERSION) {
            Ok((checked, total)) => self.events.face_progress(FaceProgress {
                checked,
                total,
                running,
            }),
            Err(err) => tracing::warn!(%err, "could not count the face pass's progress"),
        }
    }

    fn run_face_pass(&self) {
        use photon_core::face_detect::{DETECTOR_VERSION, Detector, pass};
        if self.face_cancelled() {
            return;
        }
        let detector = match Detector::new() {
            Ok(detector) => detector,
            Err(err) => {
                tracing::warn!(%err, "the face detector could not be loaded");
                return;
            }
        };
        let mut last_rebuild = Instant::now();
        let mut last_progress: Option<Instant> = None;
        // Detections written since the last rebuild: what the end of the pass owes the grid.
        let mut unshown = false;
        let result = pass::run(
            &self.lib,
            &self.cache,
            DETECTOR_VERSION,
            pass::workers(),
            &|preview| detector.detect(preview),
            &|| self.face_cancelled(),
            &mut |written| {
                if last_progress.is_none_or(|t| t.elapsed() >= FACE_PROGRESS_EVERY) {
                    self.send_face_progress(true);
                    last_progress = Some(Instant::now());
                }
                if written == 0 {
                    return;
                }
                unshown = true;
                if last_rebuild.elapsed() >= FACE_REBUILD_EVERY && self.view_reads_faces() {
                    // Derived: a detection is read by a face search's grid and by the
                    // viewer, and by no album, person, tag, tag rule or folder count.
                    if let Err(err) = self.refresh_grid_derived() {
                        tracing::warn!(%err, "grid refresh failed");
                    }
                    last_rebuild = Instant::now();
                    unshown = false;
                }
            },
        );
        if let Err(err) = result {
            tracing::warn!(%err, "the face pass failed");
        }
        if unshown && let Err(err) = self.refresh_grid_derived() {
            tracing::warn!(%err, "grid refresh failed");
        }
        // After a switch-off the command has already sent the cleared line.
        if self.face_enabled.load(Ordering::SeqCst) {
            self.send_face_progress(false);
        }
    }
```

Until Task 8 adds `Needs::faces`, write `view_reads_faces` as `false` with a `// Task 8` line of its own; Task 8 replaces the body with the one above. (If Tasks 7 and 8 are executed in the other order, write the real body now.)

**The triggers.** Add `self.request_face_pass();` directly after `self.request_similar_pass();` at the end of `run_scan` (inside `if !cancelled && !still_offline`) and add `engine.request_face_pass();` directly after `engine.request_similar_pass();` in `start_thumb_hashing`'s loop. Not after an edit or a folder removal: an edit's re-render reaches the pass through the drain, and a removal leaves nothing to detect. Extend `start_thumb_hashing`'s doc comment by a sentence saying the face pass rides the same signal, for the same reason: a photo is a candidate only once its preview exists.

**Shutdown.** In `stop_passes`, after the wait on `hashing`, wait on the face pass against the same deadline:

```rust
        match self.face_pass.try_lock_until(deadline) {
            Some(guard) => drop(guard),
            None => tracing::warn!(
                "a face pass did not stop within {budget:?}; leaving it to finish on its own"
            ),
        }
```

Import `FaceProgress` from `crate::events`.

- [ ] **Step 7: The commands**

`commands.rs`, after `set_similar_distance`:

```rust
/// Whether photon looks for faces itself.
pub fn face_detection(engine: &Engine) -> CmdResult<bool> {
    Ok(engine.face_detection())
}

/// Switches face detection. On starts a pass in the background; off stops it and deletes
/// what it found. Returns without waiting for either.
pub fn set_face_detection(engine: &Arc<Engine>, enabled: bool) -> CmdResult<()> {
    engine.set_face_detection(enabled)?;
    Ok(())
}
```

`ipc.rs`, after `set_similar_distance`:

```rust
#[tauri::command(async)]
pub fn face_detection(engine: Eng<'_>) -> Result<bool, AppError> {
    commands::face_detection(&engine)
}

#[tauri::command(async)]
pub fn set_face_detection(engine: Eng<'_>, enabled: bool) -> Result<(), AppError> {
    commands::set_face_detection(engine.inner(), enabled)
}
```

`app.rs`, in `generate_handler!` after `ipc::set_similar_distance,`:

```rust
            ipc::face_detection,
            ipc::set_face_detection,
```

`ui/src/lib/api.ts`: beside `ExportProgress`,

```ts
/** Mirrors `events::FaceProgress`: live images the detector has looked at, of all live
 *  images. `running` is false on a pass's last event. */
export interface FaceProgress { checked: number; total: number; running: boolean }
```

in `api`, after `setSimilarDistance`:

```ts
  faceDetection: () => invoke<boolean>('face_detection'),
  setFaceDetection: (enabled: boolean) => invoke<void>('set_face_detection', { enabled }),
```

in `events`, after `onExportProgress`:

```ts
  onFaceProgress: (cb: (e: FaceProgress) => void): Promise<UnlistenFn> =>
    listen<FaceProgress>('face-progress', (e) => cb(e.payload)),
```

`crates/xtask/screenshots/mock.js`, in `canned` after `set_similar_distance`:

```js
    face_detection: () => true,
    set_face_detection: () => null,
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p photon-app && cargo test -p xtask && npm run check`
Expected: all pass. `a_photo_scanned_with_the_switch_on_is_detected` leans on a 200 ms sleep for the drain's 20 ms settle; if it is flaky, replace the sleep with a loop that calls `f.settle()` until `detected(&f) == 1` or two seconds pass.

- [ ] **Step 9: Probe**

| Change | Test that must fail |
|---|---|
| `request_face_pass`: remove the `if !self.face_detection() { return; }` **and** `run_face_pass`'s `if self.face_cancelled() { return; }` | `nothing_is_detected_until_the_switch_is_on`. **Expect it to pass anyway**: `write_face_batch` refuses with the setting off, so the rows never land. That is the library guard doing its job and the engine checks being an optimisation. Say so in the commit; do not write a test that counts detector calls to force a failure. |
| `set_face_detection`: remove `self.request_face_pass();` | `switching_on_detects_the_photos_already_there` |
| remove `engine.request_face_pass();` from `start_thumb_hashing` and `self.request_face_pass();` from `run_scan` | `a_photo_scanned_with_the_switch_on_is_detected` |
| remove only the `run_scan` one | **record the result.** If the test still passes the drain alone carries it; keep both triggers (the spec names both, and a scan whose thumbnails were all cached readies nothing and never drains) and add a test for that case: a library scanned with the switch off, its cache complete, switched on in the database only (`f.engine.lib.set_face_detection(true)` and the mirror set through a second `Engine::open` on the same directories), then one `start_scan`; `detected` must be 1. |
| `self.refresh_grid_derived()` → `self.refresh_grid()` in both places in `run_face_pass` | `a_face_pass_does_not_announce_a_data_change` |
| remove `if self.face_enabled.load(...) { self.send_face_progress(false); }` | `a_pass_with_nothing_to_do_still_says_it_ended` |
| `spawn_pass`: remove the `shutting_down` check | `a_face_pass_is_not_requested_once_shutting_down` |
| `set_face_detection`: remove `self.face_enabled.swap(...)` (store nothing) | `switching_off_leaves_no_detections` (its last assert) |

- [ ] **Step 10: Gate and commit**

```bash
git add crates/photon-app crates/xtask ui/src/lib/api.ts
git commit -m "feat(faces): the face pass in the engine, behind its switch

<probe results, including the ones that passed and why>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Search by faces

**Files:**
- Modify: `crates/photon-core/src/search.rs`
- Modify: `crates/photon-core/src/library/items.rs` (`search_entries`, near line 1061)
- Modify: `crates/photon-core/src/library/detected_faces.rs`
- Modify: `crates/photon-app/src/engine.rs` (`view_reads_faces`, if Task 7 left it `false`)
- Modify: `ui/src/components/SearchBar.svelte`

**Interfaces:**
- Consumes: `face_detect::merge::count(edit, picasa, detected) -> u32`, `Rect`.
- Produces:
  - `Term::Faces { min: u32, max: Option<u32> }` (private, as `Term` is)
  - `Needs.faces: bool`
  - `Fields.faces: u32`, `Haystacks.faces: u32`
  - `pub(crate) fn search_face_counts(conn: &Connection) -> Result<HashMap<i64, u32>>` in `library::detected_faces`

- [ ] **Step 1: Write the failing tests**

In `search.rs`'s tests, in the style of the `has:gps` ones:

```rust
    fn with_faces(n: u32) -> Fields<'static> {
        Fields {
            any: &["a.jpg"],
            faces: n,
            ..Fields::default()
        }
    }

    fn face_match(query: &str, faces: u32) -> bool {
        Query::parse(query).matches(&with_faces(faces))
    }

    #[test]
    fn has_face_asks_for_at_least_one() {
        assert!(face_match("has:face", 1));
        assert!(face_match("has:face", 7));
        assert!(!face_match("has:face", 0));
        assert!(face_match("-has:face", 0));
        assert!(!face_match("-has:face", 2));
        assert!(face_match("HAS:Face", 1));
    }

    #[test]
    fn faces_asks_for_a_count() {
        assert!(face_match("faces:2", 2));
        assert!(!face_match("faces:2", 1));
        assert!(!face_match("faces:2", 3));
        assert!(face_match("faces:0", 0));
        assert!(!face_match("faces:0", 1));
    }

    #[test]
    fn faces_with_a_plus_asks_for_at_least_that_many() {
        assert!(face_match("faces:3+", 3));
        assert!(face_match("faces:3+", 12));
        assert!(!face_match("faces:3+", 2));
        assert!(face_match("-faces:3+", 2));
    }

    /// Review focus 5. A term that says nothing is dropped, like every other dangling
    /// piece - and a query of nothing but dropped terms is empty, which matches no photo
    /// rather than every photo.
    #[test]
    fn malformed_face_terms_are_ignored() {
        for q in ["faces:", "faces:+", "faces:-1", "faces:two", "faces:99999999999999999999", "has:faces", "faces:2++"] {
            assert!(Query::parse(q).is_empty(), "{q}");
            // Beside a real term it changes nothing.
            let both = format!("a.jpg {q}");
            assert!(Query::parse(&both).matches(&with_faces(0)), "{both}");
        }
    }

    #[test]
    fn only_a_face_term_needs_the_face_counts() {
        assert!(Query::parse("has:face").needs().faces);
        assert!(Query::parse("zoo faces:2+").needs().faces);
        assert!(Query::parse("-has:face").needs().faces);
        assert!(!Query::parse("zoo person:anna has:gps").needs().faces);
    }
```

In `detected_faces.rs`'s tests:

```rust
    /// Picasa's faces and the detections are counted through the one merge rule, with the
    /// photo's edit: a detection lying over a Picasa face is the same face.
    #[test]
    fn search_counts_merge_both_sources() {
        let (_dir, lib, ids) = seeded(&["both.jpg", "detected.jpg", "picasa.jpg", "none.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(
            &[
                // Over the Picasa face below, and one more elsewhere.
                (c[0].clone(), vec![face(0.12), face(0.7)]),
                (c[1].clone(), vec![face(0.1), face(0.4), face(0.7)]),
            ],
            V,
        )
        .unwrap();
        let picasa = |contact: &str| crate::picasa::Face {
            contact: contact.into(),
            left: 0.10,
            top: 0.15,
            right: 0.25,
            bottom: 0.45,
        };
        lib.set_item_faces(&[(ids[0], vec![picasa("a")]), (ids[2], vec![picasa("b")])])
            .unwrap();

        let counts = search_face_counts(&lib.reader().unwrap()).unwrap();
        assert_eq!(counts.get(&ids[0]), Some(&2), "one shared, one detected only");
        assert_eq!(counts.get(&ids[1]), Some(&3));
        assert_eq!(counts.get(&ids[2]), Some(&1), "an unnamed Picasa face is a face");
        assert_eq!(counts.get(&ids[3]), None);
    }
```

And in `detected_faces.rs`'s tests, the search itself, through the view's own entry point:

```rust
    /// The whole path: a face search reads the counts and answers with the right photos.
    #[test]
    fn a_face_search_finds_photos_by_their_faces() {
        use crate::grid::GridView;
        let (_dir, lib, ids) = seeded(&["with.jpg", "without.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)]), (c[1].clone(), vec![])], V)
            .unwrap();
        let ids_for = |query: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(ids_for("has:face"), [ids[0]]);
        assert_eq!(ids_for("-has:face"), [ids[1]]);
        assert_eq!(ids_for("faces:1"), [ids[0]]);
        assert_eq!(ids_for("faces:2+"), Vec::<i64>::new());
        assert_eq!(ids_for("faces:0"), [ids[1]]);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib search && cargo test -p photon-core --lib detected_faces`
Expected: compile errors for `Fields::faces`, `Needs::faces`, `search_face_counts`.

- [ ] **Step 3: Implement the grammar**

In `search.rs`:

- `Term`, after `HasGps`:

```rust
    /// `has:face`, `faces:N` and `faces:N+`: how many faces the photo has, Picasa's and
    /// the ones photon found, counted once each (`face_detect::merge`).
    Faces {
        min: u32,
        max: Option<u32>,
    },
```

- `Needs` gains `pub faces: bool,`; in `needs()`'s `visit`, add `Term::Faces { .. } => needs.faces = true,` before the catch-all.
- `Term::matches`, after `Term::HasGps`:

```rust
            Term::Faces { min, max } => {
                haystacks.faces >= *min && max.is_none_or(|max| haystacks.faces <= max)
            }
```

- `Fields` gains `/// How many faces the photo has, for \`has:face\` and \`faces:\`.` `pub faces: u32,`; `Haystacks` gains `pub faces: u32,`; where `Query::matches` copies `fields.starred` into the haystacks add `haystacks.faces = fields.faces;`; where `Haystacks` resets `self.starred = false;` add `self.faces = 0;`.
- In the parser, `has:` becomes:

```rust
            match value {
                "gps" => vec![Term::HasGps],
                "face" => vec![Term::Faces { min: 1, max: None }],
                _ => Vec::new(),
            }
```

and before the final `else`, a new branch:

```rust
        } else if let Some(value) = prefixed("faces:") {
            faces(value).into_iter().collect()
```

with, beside `near` and `month_day`:

```rust
/// `faces:`'s value: `2` is exactly two, `2+` is two or more. Anything else is no term.
fn faces(value: &str) -> Option<Term> {
    let (digits, open) = match value.strip_suffix('+') {
        Some(digits) => (digits, true),
        None => (value, false),
    };
    // `parse` alone would take "+2"; a count is digits and nothing else.
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let min: u32 = digits.parse().ok()?;
    Some(Term::Faces {
        min,
        max: (!open).then_some(min),
    })
}
```

- The module doc's list of terms (near line 48) gains a line: `` - `has:face` keeps the photos with a face, and `faces:2` or `faces:3+` those with that many; Picasa's faces and the ones photon found, each counted once. ``

If the `value` the parser hands a prefixed term is already lowercased, `"face"` matches `HAS:Face` as the test wants; check how `"gps"` is matched and follow it.

- [ ] **Step 4: Implement the side read**

In `detected_faces.rs`:

```rust
use std::collections::HashMap;

/// How many faces each photo has, for search: Picasa's and the detected ones, merged by
/// `face_detect::merge::count` with the photo's edit, so search counts what the viewer
/// draws. Photos without a face are absent.
///
/// Read whole, and only for a query that asks (`Needs::faces`): a few numbers per face.
pub(crate) fn search_face_counts(conn: &rusqlite::Connection) -> Result<HashMap<i64, u32>> {
    use crate::face_detect::merge;
    let rect = |r: &rusqlite::Row<'_>, at: usize| -> rusqlite::Result<Rect> {
        Ok(Rect {
            left: r.get(at)?,
            top: r.get(at + 1)?,
            right: r.get(at + 2)?,
            bottom: r.get(at + 3)?,
        })
    };
    let mut detected: HashMap<i64, Vec<Rect>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT item_id, left, top, right, bottom FROM detected_faces")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        detected.entry(r.get(0)?).or_default().push(rect(r, 1)?);
    }
    // Picasa's rectangles are in the unedited picture, so each needs its photo's edit.
    let mut picasa: HashMap<i64, (Edit, Vec<Rect>)> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT f.item_id, f.left, f.top, f.right, f.bottom, i.edit_turns, i.edit_crop
         FROM faces f JOIN items i ON i.id = f.item_id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let edit = edit_from_db(r.get(5)?, r.get(6)?);
        picasa
            .entry(r.get(0)?)
            .or_insert_with(|| (edit, Vec::new()))
            .1
            .push(rect(r, 1)?);
    }
    let mut counts = HashMap::new();
    for (id, (edit, faces)) in &picasa {
        let found = detected.remove(id).unwrap_or_default();
        let n = merge::count(*edit, faces, &found);
        if n > 0 {
            counts.insert(*id, n);
        }
    }
    for (id, found) in detected {
        counts.insert(id, found.len() as u32);
    }
    Ok(counts)
}
```

In `items.rs`, `search_entries`: after the `albums` side read,

```rust
        let faces = if needs.faces {
            super::detected_faces::search_face_counts(&tx)?
        } else {
            HashMap::new()
        };
```

and in the row loop, beside `haystacks.starred = ...`:

```rust
            haystacks.faces = faces.get(&id).copied().unwrap_or(0);
```

Extend the comment above the side reads ("Read only for a query that names a person or an album") to include the face counts.

In `engine.rs`, give `view_reads_faces` its real body from Task 7 Step 6 if it was left `false`.

- [ ] **Step 5: The hint**

In `ui/src/components/SearchBar.svelte`, in the `title=` text, after `has:gps for photos that record where they were taken,` insert `has:face or faces:2+ for photos with people in them,`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p photon-core && cargo test -p photon-app && npm run check`
Expected: all pass.

- [ ] **Step 7: Probe**

| Change | Test that must fail |
|---|---|
| `max.is_none_or(|max| haystacks.faces <= max)` → `true` | `faces_asks_for_a_count` |
| `max: (!open).then_some(min)` → `max: Some(min)` | `faces_with_a_plus_asks_for_at_least_that_many` |
| remove the digits check in `faces()` | `malformed_face_terms_are_ignored` (`faces:-1` may still fail to parse as `u32`; the case that differs is `faces:+2`-shaped input. If nothing fails, add `"faces:+2"` to the test's list, see it fail, restore.) |
| `Term::Faces { .. } => needs.faces = true,` removed | `only_a_face_term_needs_the_face_counts` |
| `search_face_counts`: `merge::count(*edit, faces, &found)` → `(faces.len() + found.len()) as u32` | `search_counts_merge_both_sources` |
| `search_entries`: `if needs.faces` → `if false` | `a_face_search_finds_photos_by_their_faces` |

Also confirm, by reading rather than by test, that a query without a face term prepares neither side statement: the `if needs.faces` is the whole of it.

- [ ] **Step 8: Gate and commit**

```bash
git add crates/photon-core crates/photon-app ui/src/components/SearchBar.svelte
git commit -m "feat(search): has:face and faces:N

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: The viewer's unnamed faces

**Files:**
- Modify: `crates/photon-core/src/library/faces.rs`
- Modify: `crates/photon-app/src/commands.rs` (`ViewerItem` near line 173, `viewer_item` near line 706)
- Modify: `ui/src/lib/api.ts`
- Modify: `ui/src/lib/faces.ts`, `ui/src/lib/faces.test.ts`
- Modify: `ui/src/components/Viewer.svelte`
- Modify: `crates/xtask/screenshots/mock.js`
- Modify: any `ViewerItem` literal in `ui/src/**/*.test.ts` (`npm run check` names them)

**Interfaces:**
- Consumes: `Library::item_detected_faces`, `face_detect::merge::{shown, unmatched}`, `Rect`.
- Produces:
  - `Library::item_picasa_faces(&self, item_id: i64) -> Result<Vec<(Rect, bool)>>`: every Picasa face on the photo in the unedited picture, with whether its contact is named.
  - `ViewerItem.unnamed_faces: Vec<Rect>` (`unnamedFaces` on the wire and in `api.ts`).
  - `unnamedFacesLabel(count: number): string | null` in `ui/src/lib/faces.ts`.

**No test for `pictureChanged`.** The spec asks for a `picture.ts` case showing that a change in `unnamedFaces` is not a picture change. `pictureChanged` takes a `Pick` of six named fields, so it cannot see `faces` or `unnamedFaces` at all, and a test passing objects that differ only in them passes with or without any change: there is nothing to revert. Add one sentence to `picture.ts`'s doc comment instead ("Faces are not in `Picture` on purpose: a detection landing must not reload the photo the user is looking at") and say in the commit message that no test was added and why.

- [ ] **Step 1: Write the failing tests**

`crates/photon-core/src/library/faces.rs`, in its tests (`face`, `temp_library`, `seed_folder` and `new_item` are already there):

```rust
    /// Every Picasa face on the photo, named or not: an unnamed one is still a face, and
    /// the merge has to know it is there or it would hide the detection lying over it.
    #[test]
    fn item_picasa_faces_includes_the_unnamed() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada".to_string())]))
            .unwrap();
        lib.set_item_faces(&[(ids[0], vec![face("ada"), face("nobody")])])
            .unwrap();

        let faces = lib.item_picasa_faces(ids[0]).unwrap();
        assert_eq!(faces.len(), 2);
        assert_eq!(
            faces.iter().map(|(_, named)| *named).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(faces[1].0.left, 0.1);
    }
```

`crates/photon-app/src/commands.rs`, in its tests, beside `viewer_item_carries_camera_keywords_faces_and_albums`:

```rust
    /// The viewer gets Picasa's named faces as before, and beside them every face without
    /// a name: Picasa's unnamed ones and the detections that are none of Picasa's.
    #[test]
    fn viewer_item_carries_unnamed_faces() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            (
                "a/.picasa.ini",
                // Ada at the left, and a face whose contact no INI names at the right.
                b"[Contacts2]\nabc=Ada\n[a.jpg]\nfaces=rect64(1000200030006000),abc;rect64(c0002000f0006000),zzz\n",
            ),
        ]);
        f.add_photos();
        let id = f.ids()[0];
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine.lib.set_thumb_state(id, ThumbState::Ready, None).unwrap();
        let listed = f.engine.lib.face_candidates(0, 10, DETECTOR_VERSION).unwrap();
        let at = |left: f64| Detection {
            rect: Rect { left, top: 0.2, right: left + 0.1, bottom: 0.3 },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        };
        // One over Ada (0.0625..0.1875 wide), one over the unnamed face (0.75..0.9375),
        // one in the middle that is nobody Picasa knew.
        f.engine
            .lib
            .write_face_batch(&[(listed[0].clone(), vec![at(0.08), at(0.80), at(0.45)])], DETECTOR_VERSION)
            .unwrap();

        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces.len(), 1);
        assert_eq!(item.faces[0].name, "Ada");
        assert_eq!(item.unnamed_faces.len(), 2, "{:?}", item.unnamed_faces);
        // Picasa's unnamed face first, then the detection that matched nothing.
        assert!((item.unnamed_faces[0].left - 0.75).abs() < 0.001);
        assert!((item.unnamed_faces[1].left - 0.45).abs() < 0.001);
    }

    /// An edit maps Picasa's unnamed face like its named ones; a detection is already in
    /// the picture as shown and is not mapped again.
    #[test]
    fn unnamed_faces_follow_the_edit() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            ("a/.picasa.ini", b"[a.jpg]\nfaces=rect64(1000200030006000),zzz\n"),
        ]);
        f.add_photos();
        let id = f.ids()[0];
        let before = viewer_item(&f.engine, id).unwrap().unnamed_faces;
        f.engine.rotate_item(id, true).unwrap();
        let after = viewer_item(&f.engine, id).unwrap().unnamed_faces;
        assert_eq!((before.len(), after.len()), (1, 1));
        // A quarter turn clockwise sends (l, t, r, b) to (1 - b, l, 1 - t, r).
        assert!((after[0].left - (1.0 - before[0].bottom)).abs() < 0.001);
        assert!((after[0].top - before[0].left).abs() < 0.001);
    }
```

Use the rotate entry point the neighbouring edit test uses (`viewer_item_...` near line 1562 turns a photo); the rect64 values follow that test's `4000200080006000` form (four 16-bit hex fields: left, top, right, bottom).

`ui/src/lib/faces.test.ts`:

```ts
import { unnamedFacesLabel } from './faces';

describe('unnamedFacesLabel', () => {
  it('says nothing for none', () => {
    expect(unnamedFacesLabel(0)).toBeNull();
  });
  it('counts one and many', () => {
    expect(unnamedFacesLabel(1)).toBe('1 face not named');
    expect(unnamedFacesLabel(3)).toBe('3 faces not named');
    expect(unnamedFacesLabel(1200)).toBe(`${(1200).toLocaleString()} faces not named`);
  });
});
```

(Merge the import into the file's existing import from `./faces`.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-app --lib viewer_item ; npm test -w ui -- src/lib/faces.test.ts`
Expected: compile errors for `unnamed_faces` and `item_picasa_faces`; the vitest file fails on the missing export.

- [ ] **Step 3: Implement the backend**

`faces.rs`, after `item_faces`:

```rust
    /// Every face Picasa recorded on one photo, in the unedited picture, with whether its
    /// contact has a name. `item_faces` is the named ones, with the name; this is what the
    /// merge with photon's own detections needs, where an unnamed face is still a face.
    pub fn item_picasa_faces(&self, item_id: i64) -> Result<Vec<(Rect, bool)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT f.left, f.top, f.right, f.bottom,
                    EXISTS (SELECT 1 FROM contacts c WHERE c.hash = f.contact)
             FROM faces f WHERE f.item_id = ?1 ORDER BY f.rowid",
        )?;
        let faces = stmt
            .query_map(params![item_id], |r| {
                Ok((
                    Rect {
                        left: r.get(0)?,
                        top: r.get(1)?,
                        right: r.get(2)?,
                        bottom: r.get(3)?,
                    },
                    r.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(faces)
    }
```

with `use crate::face_detect::Rect;`.

`commands.rs`: on `ViewerItem`, after `faces`:

```rust
    /// Faces with no name: Picasa's whose contact no INI names, then the ones photon
    /// detected that are none of Picasa's. In the picture as shown, like `faces`.
    pub unnamed_faces: Vec<Rect>,
```

In `viewer_item`, after `faces` is built:

```rust
    // One rule for both readers (`face_detect::merge`): search counts exactly these.
    let picasa: Vec<(Rect, bool)> = engine
        .lib
        .item_picasa_faces(item.id)?
        .into_iter()
        .filter_map(|(rect, named)| Some((merge::shown(edit, rect)?, named)))
        .collect();
    let all: Vec<Rect> = picasa.iter().map(|(rect, _)| *rect).collect();
    let mut unnamed_faces: Vec<Rect> = picasa
        .iter()
        .filter(|(_, named)| !named)
        .map(|(rect, _)| *rect)
        .collect();
    unnamed_faces.extend(merge::unmatched(&all, &engine.lib.item_detected_faces(item.id)?));
```

and `unnamed_faces,` in the struct literal after `faces,`. Import `photon_core::face_detect::{Rect, merge}`. Update the doc comment on `ViewerItem.edit` to name `unnamed_faces` beside `faces`.

- [ ] **Step 4: Implement the UI**

`api.ts`: after `faces: ItemFace[];` in `ViewerItem`:

```ts
  /** Faces with no name: Picasa's unnamed ones, then the ones photon detected that are none
   *  of Picasa's. Fractions of the picture as shown, like `faces`. */
  unnamedFaces: FaceRect[];
```

and above `ItemFace`:

```ts
/** Mirrors `face_detect::Rect`: fractions of the picture, from its left and top. */
export interface FaceRect { left: number; top: number; right: number; bottom: number }
```

In the `edit` field's comment, name `unnamedFaces` beside `faces`.

`faces.ts`:

```ts
/** The info panel's line for the faces that have no name, or null when there are none. */
export function unnamedFacesLabel(count: number): string | null {
  if (count <= 0) return null;
  return count === 1 ? '1 face not named' : `${count.toLocaleString()} faces not named`;
}
```

and change the file's first comment to "Placing face rectangles over a photo shown with `object-fit: contain`."

`Viewer.svelte`: import `unnamedFacesLabel` beside `containedBox, faceBox`. Change `faceBoxes` to carry both kinds:

```ts
  const faceBoxes = $derived.by(() => {
    if (!item || !info || crop.active) return [];
    const image = containedBox(oriented.width, oriented.height, frameW, frameH);
    return [
      ...item.faces.map((f) => ({ name: f.name as string | null, box: faceBox(f, image) })),
      ...item.unnamedFaces.map((f) => ({ name: null, box: faceBox(f, image) })),
    ];
  });
  const unnamedLabel = $derived(item ? unnamedFacesLabel(item.unnamedFaces.length) : null);
```

In the markup, the name plate becomes conditional:

```svelte
            {#if face.name}<span class="face-name">{face.name}</span>{/if}
```

and the People block:

```svelte
      <h3>People</h3>
      {#if item.faces.length}
        <ul class="chips">
          {#each item.faces as face, i (i)}
            <li>{face.name}</li>
          {/each}
        </ul>
      {/if}
      {#if unnamedLabel}
        <p class="info-muted">{unnamedLabel}</p>
      {:else if !item.faces.length}
        <p class="info-muted">No faces named in Picasa.</p>
      {/if}
```

Change the comment above `oriented` from "the coordinates Picasa's faces are in" to "the coordinates the faces are in".

`mock.js`: in `viewerItem`, after the `faces:` line (line 123):

```js
      unnamedFaces: [{ left: 0.56, top: 0.3, right: 0.66, bottom: 0.5 }],
```

`picture.ts`: the sentence described at the top of this task.

Add `unnamedFaces: []` to every `ViewerItem` literal `npm run check` reports.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-core && cargo test -p photon-app && cargo test -p xtask && npm run check && npm test`
Expected: all pass.

- [ ] **Step 6: Probe**

| Change | Test that must fail |
|---|---|
| `merge::unmatched(&all, ...)` → `merge::unmatched(&[], ...)` | `viewer_item_carries_unnamed_faces` (4 unnamed, not 2) |
| `let all` built from the named faces only (`filter(|(_, named)| *named)`) | `viewer_item_carries_unnamed_faces` (the detection over the unnamed face appears as a third) |
| remove the `.filter(|(_, named)| !named)` chain (no unnamed Picasa faces) | `viewer_item_carries_unnamed_faces` |
| `merge::shown(edit, rect)?` → `rect` | `unnamed_faces_follow_the_edit` |
| `item_picasa_faces`: add `JOIN contacts c ON c.hash = f.contact` | `item_picasa_faces_includes_the_unnamed` |
| `unnamedFacesLabel`: drop the `count === 1` branch | `counts one and many` |

- [ ] **Step 7: See it**

Run: `cargo run -p xtask -- screenshots --only viewer-info-light` then `--only viewer-info-dark --no-build`, and Read both PNGs from `target/screenshots/`.
Expected: Anna's outline with her name plate, a second outline to its right with none, and "1 face not named" under her chip in the info panel, legible in both themes.

- [ ] **Step 8: Gate and commit**

```bash
git add crates ui
git commit -m "feat(viewer): outline faces that have no name

Picasa's unnamed faces and photon's own detections, merged by the rule
search counts with. A Picasa face no INI names was invisible until now.

No test for pictureChanged: it takes a Pick of six fields and cannot see
a face, so a test would pass with or without any change.

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: The switch and the progress line

**Files:**
- Modify: `ui/src/lib/status.ts`, `ui/src/lib/status.test.ts`
- Modify: `ui/src/lib/settings.ts`
- Modify: `ui/src/lib/library.svelte.ts`
- Modify: `ui/src/components/StatusBar.svelte`
- Modify: `ui/src/components/Settings.svelte`
- Modify: `crates/xtask/screenshots/mock.js`
- Modify: `crates/xtask/src/screenshots.rs`

**Interfaces:**
- Consumes: `api.faceDetection`, `api.setFaceDetection`, `events.onFaceProgress`, `FaceProgress` (Task 7).
- Produces:
  - `faceStatus(progress: FaceProgress | null): { label: string; fraction: number } | null` in `status.ts`
  - `library.faces: FaceProgress | null` (null unless a pass is running)
  - `SettingsSection` gains `'people'`

- [ ] **Step 1: Write the failing tests**

`ui/src/lib/status.test.ts`:

```ts
import { faceStatus } from './status';

describe('faceStatus', () => {
  it('is nothing when no pass is running', () => {
    expect(faceStatus(null)).toBeNull();
    expect(faceStatus({ checked: 5, total: 10, running: false })).toBeNull();
  });

  it('counts photos checked of all photos', () => {
    expect(faceStatus({ checked: 12400, total: 98000, running: true })).toEqual({
      label: `Finding faces: ${(12400).toLocaleString()} of ${(98000).toLocaleString()}`,
      fraction: 12400 / 98000,
    });
  });

  // Review focus 4: an empty library, or the first event before anything is counted.
  it('never shows 0 of 0', () => {
    expect(faceStatus({ checked: 0, total: 0, running: true })).toBeNull();
  });

  it('does not run past the end', () => {
    expect(faceStatus({ checked: 11, total: 10, running: true })?.fraction).toBe(1);
  });
});
```

(Merge the import into the file's existing import from `./status`.)

- [ ] **Step 2: Run it to see it fail**

Run: `npm test -w ui -- src/lib/status.test.ts`
Expected: FAIL, `faceStatus` is not exported.

- [ ] **Step 3: Implement**

`status.ts`: add `FaceProgress` to the type import, change the file's first comment to "What the status bar shows for a running scan or face pass.", and append:

```ts
/** The status bar's line for a running face pass, or null when there is nothing to say:
 *  no pass, or a library with no photo to check. `total` counts photos whose thumbnail is
 *  not made yet, so the bar can wait short of its end while thumbnails are rendering. */
export function faceStatus(progress: FaceProgress | null): { label: string; fraction: number } | null {
  if (!progress || !progress.running || progress.total <= 0) return null;
  return {
    label: `Finding faces: ${progress.checked.toLocaleString()} of ${progress.total.toLocaleString()}`,
    fraction: Math.min(1, progress.checked / progress.total),
  };
}
```

`library.svelte.ts`: beside `exporting`'s declaration,

```ts
  /** The running face pass's progress, or null when none is running. */
  faces = $state<FaceProgress | null>(null);
```

in the listener list after `events.onExportProgress(...)`:

```ts
        events.onFaceProgress((e) => {
          this.faces = e.running ? e : null;
        }),
```

and in `dispose()`, beside the other resets, `this.faces = null;`. Import `FaceProgress`.

`StatusBar.svelte`: import `faceStatus`, add `const faces = $derived(faceStatus(library.faces));`, and after the `{#each scans ...}` block:

```svelte
    {#if faces}
      <span class="scan" role="status">
        <span>{faces.label}</span>
        <progress aria-label="Face detection progress" value={faces.fraction} max="1"></progress>
      </span>
    {/if}
```

`settings.ts`: `SettingsSection` becomes

```ts
export type SettingsSection = 'folders' | 'appearance' | 'tags' | 'slideshow' | 'duplicates' | 'people' | 'statistics' | 'about';
```

`Settings.svelte`. In the script, after the `shuffle` block:

```ts
  /** Whether photon looks for faces itself. Null until read, like `interval`. */
  let findFaces = $state<boolean | null>(null);

  onMount(() => {
    api
      .faceDetection()
      .then((on) => (findFaces = on))
      .catch(library.reportError);
  });

  /** The box shows what is stored: put back if the store fails. */
  function saveFindFaces(e: Event & { currentTarget: HTMLInputElement }) {
    const field = e.currentTarget;
    const next = field.checked;
    api
      .setFaceDetection(next)
      .then(() => (findFaces = next))
      .catch((err) => {
        field.checked = findFaces ?? false;
        library.reportError(err);
      });
  }

  const faceProgress = $derived(faceStatus(library.faces));
```

Import `faceStatus` from `../lib/status`. In the nav, after the Duplicates button:

```svelte
        <button class:active={current === 'people'} aria-current={current === 'people'} onclick={() => (current = 'people')}>
          People
        </button>
```

In the content, after the `duplicates` branch and before `{:else}`:

```svelte
        {:else if current === 'people'}
          <h2>Find faces</h2>
          <p class="hint">
            photon looks for faces in your photos, so you can search for them (has:face, faces:2+) and see
            them outlined in the viewer's info panel. It works in the background and can take hours on a
            large library; you can quit and it carries on next time. Until it has finished, a search for
            photos without faces also finds photos it has not reached yet.
          </p>
          <p class="hint">
            Everything stays on this computer. Switching this off deletes what photon found; faces named in
            Picasa are not affected.
          </p>
          <label class="shuffle">
            <input type="checkbox" checked={findFaces ?? false} disabled={findFaces === null} onchange={saveFindFaces} />
            Find faces in my photos
          </label>
          {#if findFaces && faceProgress}
            <p class="hint" role="status">{faceProgress.label}</p>
          {/if}
```

`.shuffle` is the existing checkbox-row class; reuse it rather than adding a rule.

`mock.js`: add to `actions`, after `statistics`: `people: () => settingsSection('People'),`.

`screenshots.rs`: add to `SHOTS`, after `settings-dark`:

```rust
    Shot {
        name: "settings-people-light",
        query: "theme=light&do=people",
        dark: false,
    },
```

- [ ] **Step 4: Run the tests**

Run: `npm run check && npm test && cargo test -p xtask`
Expected: all pass, 0 warnings from `svelte-check`.

- [ ] **Step 5: Probe**

| Change | Test that must fail |
|---|---|
| `faceStatus`: drop `|| progress.total <= 0` | `never shows 0 of 0` |
| drop `!progress.running ||` | `is nothing when no pass is running` |
| `Math.min(1, ...)` → the bare division | `does not run past the end` |

The component wiring (the listener, the checkbox, the status bar's block) has no test: there is no component harness, and it is effect wiring. It is covered by `svelte-check`, the screenshot below and the smoke checklist. Say so in the commit.

- [ ] **Step 6: See it**

Run: `cargo run -p xtask -- screenshots --only settings-people-light`, and Read the PNG.
Expected: the People section selected, both paragraphs, the checkbox ticked (the mock answers `true`), nothing clipped.

- [ ] **Step 7: Gate and commit**

```bash
git add ui crates/xtask
git commit -m "feat(settings): a switch for finding faces, and its progress

The listener, the checkbox and the status bar's line are effect wiring
with no harness to render them; svelte-check, the settings-people
screenshot and the smoke checklist cover them.

<probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: Documentation, the full gates, and the review

**Files:**
- Modify: `CLAUDE.md`
- Modify: `README.md`
- Modify: `site/` (the feature list, if it has one that lists search terms or people)
- Modify: `docs/smoke-checklist.md`
- Modify: `docs/superpowers/specs/2026-10-01-photon-face-detection-design.md` (its first line)

- [ ] **Step 1: `CLAUDE.md`**

Make these edits, each in the section named:

- **Commands:** "twenty-eight PNGs" becomes "twenty-nine PNGs" (two places: the `screenshots` comment line and the Styling section's paragraph).
- **Architecture, after "The duplicate finder hashes after the scan..."**, a new paragraph:

  > **Face detection is a third pass, off until the user switches it on.** `Engine::request_face_pass` runs `photon_core::face_detect::pass::run` on its own thread, counted with the look-alike pass in `background_passes`, requested where that pass is at the end of `run_scan` and from the thumbnail queue's drain, and by the switch itself. It reads each photo's **cached 1600 px preview**, never the source, for the reason the perceptual hash reads the grid thumbnail: inside the renderer it would never run for a photo whose thumbnail is already cached. `items.face_version` records which `DETECTOR_VERSION` looked; bump the constant when the model, its input size or its threshold changes. `write_face_batch` has two guards, each with a test that fails without it: a row whose fingerprint or edit moved since it was listed is skipped, and nothing is written with the setting off, read inside the batch's own transaction so the switch's delete and a batch in flight cannot interleave. `update_items` and `set_item_edit` clear a photo's detections and its version, beside the hashes they already clear. The pass rebuilds through `refresh_grid_derived`: a detection is read by a face search's grid and by the viewer, never by a collection count. Off deletes every detection.
  >
  > **Two tables of faces, in two frames.** Picasa's `faces` rows are fractions of the *unedited* picture and are mapped through the edit on read; `detected_faces` rows are fractions of the picture *as shown*, because that is the preview the detector reads. `face_detect::merge` is the one place they meet and the one rule for "the faces on this photo" (either rectangle's centre inside the other is the same face); `viewer_item` and `search_face_counts` both go through it, so the viewer and `has:face`/`faces:N` cannot disagree. A new reader of either table goes through it too.

- **Search paragraph:** add `has:face` and `faces:N` to the list of terms asking about the photo, and the face counts to the side tables read only when a query names one (`Query::needs`).
- **"What reloads the viewer":** add a sentence: `pictureChanged` takes a `Pick` that leaves faces out on purpose, since a detection landing must not reload the photo on screen.
- **Conventions, "No system library dependencies":** add `tract` to the account: pure Rust but for `tract-linalg`'s assembly kernels, compiled in with `cc`, no nasm; the YuNet model is a file in `crates/photon-core/models/`, embedded with `include_bytes!`. Add that the three `tract` crates are built optimised in the dev profile, and why (five seconds a detection otherwise).
- **Conventions, "photon never writes to, moves or deletes photo files":** in the sentence listing what lives only in `library.db`, add photon's own face detections.

- [ ] **Step 2: `README.md` and the site**

Add face detection to the feature list in the README's own wording: off by default, on this computer only, searchable with `has:face` and `faces:2+`, outlines in the viewer. If `site/index.html` has a feature list, the same line there. Do not regenerate the site's screenshots.

- [ ] **Step 3: The smoke checklist**

Append a section to `docs/smoke-checklist.md` in the file's existing style:

```markdown
## Face detection

- [ ] Settings → People: the switch is off on a library that never had it on.
- [ ] Switch it on: the status bar shows "Finding faces: N of M" and N rises.
- [ ] Quit mid-pass and relaunch: it carries on from where it was, not from zero.
- [ ] Open a group photo with the info panel shown: every face has an outline; Picasa's named faces keep their name plates and are not outlined twice.
- [ ] `has:face` finds photos with people; `faces:2` and `faces:3+` find the right ones; `-has:face` finds landscapes.
- [ ] Turn or crop a photo with detected faces: after its thumbnail is remade, the outlines are on the faces again.
- [ ] Switch it off mid-pass: the progress line clears, the outlines of detected faces are gone, Picasa's remain.
- [ ] A Picasa library with unnamed faces, detection off: those faces are outlined without a name.
- [ ] The app stays usable (scrolling, opening photos) while a pass runs on a large library.
- [ ] Windows and macOS: the same switch-on, on a real library.
```

- [ ] **Step 4: The spec's first line**

Change it to: `2026-10-01. Approved 2026-10-02; implemented on \`feat/face-detection\` (PR #<the draft PR's number>).` and add under "Readers → The viewer" one line recording the deviation: no `picture.ts` test was added, because `pictureChanged`'s `Pick` cannot see a face. Under "The switch", record that the setting is read by its own command (`face_detection`), as every other setting is, rather than travelling with others.

- [ ] **Step 5: Every gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
cargo run -p xtask -- versions
cargo run -p xtask -- metadata
cargo run -p xtask -- screenshots
```

Expected: all pass; twenty-nine PNGs. Read `viewer-info-light.png`, `viewer-info-dark.png`, `settings-people-light.png` and `main-light.png` (the status bar, which should show no face line: the mock sends no progress).

- [ ] **Step 6: Commit and push**

```bash
git add CLAUDE.md README.md site docs
git commit -m "docs: face detection

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
git push
gh pr checks --watch
```

Expected: all checks pass on the three platforms.

- [ ] **Step 7: An independent read of the whole branch**

`CLAUDE.md` asks for one before a large branch merges. Give a fresh reviewer the diff against `main` and the spec, and point it at what a new feature *arms* in old code, not only the new code:

- `spawn_pass` and `stop_passes`: can a face pass outlive `shutdown`'s bound, or hold it to the full timeout on every quit?
- The drain thread now requests two passes per quiet period: what does that cost on a settled library with the switch on and nothing to detect (one `face_candidates` query per drain)?
- `set_item_edit` and `update_items` now delete from a second table in their transactions.
- `search_entries`' new side read, and whether any other reader of `faces` (the Person view, `people_with_counts`, `SEARCH_PEOPLE_SQL`) now disagrees with what the viewer draws.
- `Viewer.svelte`: does anything reload, refocus or re-layout when `unnamedFaces` changes on a re-read?
- The 30-second rebuild: a face search open for the length of a pass.

Fix what it finds, each fix with its own failing-first test where one is possible. Then mark the PR ready (`gh pr ready`) and stop: merging and releasing are the user's call.

---

## Self-review

**Spec coverage.** What the user gets: switch (10), progress line (7, 10), search terms (8), outlines (9). Data model (4). Detector (1–3). Pass, guards, refresh chain, progress, quitting (6, 7). The switch (7, 10). Merge and both readers (5, 8, 9). The Picasa-library change (9, with a smoke item). Build gate (1). Documentation (11). Rollout notes are release work, outside this plan; the plan ends at a ready PR.

**Deviations from the spec, each recorded in Task 11 Step 4 or in the task itself:**
- The setting has its own getter command, as every setting in this codebase does, rather than travelling with others.
- No `picture.ts` test (Task 9).
- The candidate query has no `w.online = 1` term, which the spec did not ask for either but the look-alike pass's query has: previews are in photon's cache, so an unplugged drive's photos can be detected.
- One new screenshot (`settings-people-light`): the spec's "the Settings shot shows the switch" cannot hold, because the switch is in its own section.

**Type consistency.** `Rect` (f64 fields) is one type from Task 3 through Tasks 4, 5, 8 and 9 and `FaceRect` in `api.ts`. `FaceCandidate` and `write_face_batch(&[(FaceCandidate, Vec<Detection>)], i64) -> Result<usize>` are the same in Tasks 4 and 6. `pass::run`'s argument order in Task 6 is the order Task 7 calls it with. `FaceProgress` has the same three fields in `events.rs`, `api.ts`, `status.ts` and `library.svelte.ts`. The renamed `wait_for_passes` is what Task 7's tests and `Fixture::settle` call.
