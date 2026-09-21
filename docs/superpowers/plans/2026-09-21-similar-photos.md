# Similar Photos Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Widen the Duplicates view from byte-identical files to photos that *look* the same — the re-saved, re-exported and resized copies the README currently admits it misses.

**Architecture:** A 64-bit difference hash per photo, computed from the **already-cached 256 px grid thumbnail** rather than from the source file, so no photo is decoded twice. A post-scan pass in the engine hashes what is missing, bands the hashes into 16-bit buckets to find candidate pairs, resolves them with union-find, and writes each member's group into `items.similar_group`. The Duplicates view's SQL filter widens to "has an identical twin **or** a `similar_group`", so it stays a plain filter and keeps the folder-first order everything downstream depends on.

**Tech Stack:** Rust (`photon-core`: a new `similar.rs` pass and `library/similar.rs` queries; `photon-app`: engine wiring and IPC), SQLite (schema 12), Svelte 5 + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-21-photon-similar-photos-design.md`

---

## Spec deviation — read this before Task 1

**The spec says the hash is computed "in the thumbnail renderer". That does not work, and this plan does it differently.**

`thumbs/service.rs`'s `process_item` begins:

```rust
let fp = item.thumb_key();
if !cache.is_complete(fp) {
    match render(cache, Path::new(&item.path), item.orientation, item.edit) { ... }
}
```

The render is **skipped entirely when the thumbnail is already cached**. Every existing photon library has a complete thumbnail cache, so the renderer would never run for any already-indexed photo and no existing library would ever grow a single perceptual hash. Worse, the worker only visits rows the pending sweep enqueues, and those rows are `Ready`, so nothing would enqueue them either.

**What this plan does instead:** a post-scan pass, beside the duplicate pass it already mirrors, which reads each candidate's **cached grid thumbnail** (a 256 px WebP, roughly a millisecond to decode) and hashes that.

This keeps the spec's actual intent — *the source file is never decoded a second time* — and gains three things the spec's version could not have: it works on existing libraries, it needs no change to the thumbnail worker at all, and it sits next to `hash_duplicates` where the spec already argues this kind of work belongs ("in the engine rather than the scanner, so that neither of `walk_tree`'s two callers can be forgotten, and because a duplicate is a fact about the whole library").

Everything else in the spec stands, including the consequence it already records: a photo has no perceptual hash until its thumbnail exists, so look-alikes in a freshly indexed folder appear as its thumbnails land.

**The other spec statement this changes:** the hash is of the photo *as photon shows it*, including its edit. That remains true and for a better reason — the grid thumbnail is keyed by `Item::thumb_key()`, which already mixes in the edit.

---

## Global Constraints

- **Schema 12.** `library/mod.rs` asserts the version twice (the opened version and `SchemaTooNew`'s `supported`); both move to 12 **by hand**. The migration tests seed from `MIGRATIONS[..N-1]`. The hardcoding is the tripwire — never loosen it to `MIGRATIONS.len()`.
- **The table count assertion does NOT change.** This migration only alters `items` and adds an index.
- **No `bump_thumb_gc_epoch`.** Neither new column is part of the thumbnail key, so neither write can orphan a thumbnail.
- **`update_items` must set both new columns to NULL**, exactly as it already does for `content_hash`. A row that keeps a stale hash after its bytes changed is never re-examined.
- **Distances are stored as the Hamming distance itself:** `0` (off), `3` (conservative, the default), `6` (loose). A stored value outside those is clamped into range, never refused.
- **Exact recall at distance ≤ 3 is a property of the code, not of this document.** Four 16-bit bands; two hashes within distance 3 must agree exactly on at least one band by pigeonhole. At distance 6 recall is best-effort and the UI says "finds most".
- **Case-insensitive matching is done in Rust, never in SQL** — there is no `COLLATE NOCASE` anywhere and `lower()` is ASCII-only without ICU.
- **Reads are pooled, writes are one connection.** `Library::reader()` returns a `Result`; `writer()` is a single mutexed connection.
- **The Rust gate, all of it, before any commit:** `cargo fmt --all` (run it, not just `--check`), then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate:** `npm run check` (**0 errors AND 0 warnings**) and `npm test`, from the repo root.
- **IPC is three files plus the mirror plus the mock:** `commands.rs` → `ipc.rs` → `app.rs`'s `generate_handler!` (forgetting it compiles fine and fails at runtime) → the hand-written, unvalidated `ui/src/lib/api.ts` → an answer in `crates/xtask/screenshots/mock.js`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not proof. Where a change genuinely cannot have one, say so in the commit message and why.
- **Comments carry reasoning, not mechanics**; a wrong justification is treated as a defect.
- **photon never deletes a photo.** This feature only ever *reports*.
- **Branch:** create `feat/similar-photos` off `main` before Task 1.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/photon-core/src/library/schema.rs` | migration 12 | 1 |
| `crates/photon-core/src/library/mod.rs` | version assertions | 1 |
| `crates/photon-core/src/library/items.rs` | NULL both columns in `update_items` | 1 |
| `crates/photon-core/src/similar.rs` | `dhash`, banding, union-find — pure | 2, 3 |
| `crates/photon-core/src/library/similar.rs` | the queries: candidates, writes, filter, lookups | 4 |
| `crates/photon-core/src/library/duplicates.rs` | widen the view filter | 4 |
| `crates/photon-core/src/similar.rs` (pass fn) | hash candidates from cached thumbnails | 5 |
| `crates/photon-app/src/engine.rs` | run the pass after every scan; keep a cache handle | 5 |
| `crates/photon-core/src/library/settings.rs` | `similar_distance` | 6 |
| `crates/photon-app/src/{commands,ipc,app}.rs` | IPC for the setting and the widened copies | 6, 7 |
| `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js` | mirror and mock | 6, 7 |
| `ui/src/lib/faces.ts` or the info-panel module | group copies into Identical / Looks the same | 8 |
| `ui/src/components/Viewer.svelte`, `Settings.svelte` | render the two groups; the setting control | 8 |
| `README.md`, `CLAUDE.md`, `crates/xtask/src/screenshots.rs` | docs, smoke checklist, a shot | 9 |

---

### Task 1: Schema 12

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (append to `MIGRATIONS`)
- Modify: `crates/photon-core/src/library/mod.rs` (two hardcoded version numbers)
- Modify: `crates/photon-core/src/library/items.rs:330-338` (`update_items`)
- Test: `crates/photon-core/src/library/schema.rs`'s `mod tests`, and `items.rs`'s

**Interfaces:**
- Consumes: nothing.
- Produces: columns `items.percep_hash INTEGER` and `items.similar_group INTEGER`, plus the partial index `items_similar_group`.

- [ ] **Step 1: Create the branch**

```bash
git checkout main && git pull
git checkout -b feat/similar-photos
```

- [ ] **Step 2: Write the failing tests**

In `crates/photon-core/src/library/items.rs`'s test module — read the neighbouring tests first and match how they build a library and rescan a file (there is already a test that a replaced file loses its `content_hash`; **find it and model this on it**, because it is the same shape):

```rust
    /// A rewritten file must lose both derived hashes, exactly as it loses `content_hash`.
    /// A row that kept a stale perceptual hash would never be a candidate again, and one
    /// that kept its group would stay grouped with photos it no longer resembles.
    #[test]
    fn replacing_a_file_clears_its_similarity_columns() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib.insert_items(&[new_item(folder, "/pics/a.jpg", 10, 100)]).unwrap();
        let id = ids[0];
        lib.writer()
            .execute(
                "UPDATE items SET percep_hash = 42, similar_group = 7 WHERE id = ?1",
                [id],
            )
            .unwrap();

        // The same row, with new bytes: a size and mtime the scanner would report.
        let replaced = new_item(folder, "/pics/a.jpg", 20, 200);
        lib.update_items(&[(id, replaced)]).unwrap();

        let (ph, sg): (Option<i64>, Option<i64>) = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT percep_hash, similar_group FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ph, None, "percep_hash survived a replacement");
        assert_eq!(sg, None, "similar_group survived a replacement");
    }
```

`new_item`, `seed_folder` and `temp_library` come from `crate::testutil`; check the exact signatures in that file — `new_item`'s argument order for size and mtime must match what is already used in `items.rs`'s tests, and adjust the call above if it differs.

In `schema.rs`'s test module, update the two existing version assertions and add:

```rust
    #[test]
    fn migration_12_adds_the_similarity_columns_to_an_existing_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..11] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 11i64).unwrap();
        drop(conn);

        let lib = Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        // Both columns exist and are NULL for every pre-existing row.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE percep_hash IS NOT NULL OR similar_group IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }
```

Read the existing migration tests in that file first — they already establish this shape, and yours must match their imports and helpers.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p photon-core similarity_columns && cargo test -p photon-core migration_12`
Expected: FAIL — `no such column: percep_hash`.

- [ ] **Step 4: Write the migration**

Append to `MIGRATIONS` in `crates/photon-core/src/library/schema.rs`:

```rust
    r#"
-- Look-alikes: photos that are the same picture without being the same bytes - a re-saved
-- JPEG, an emailed copy at 2048px, a re-export from another program. `content_hash` cannot
-- see those, and the README says so.
--
-- `percep_hash` is a 64-bit difference hash of the photo's own 256px grid thumbnail, so it
-- costs no decode the thumbnail has not already paid for, and it is of the photo *as photon
-- shows it* (the thumbnail is keyed by `Item::thumb_key()`, which mixes in the edit).
-- NULL until that thumbnail exists.
--
-- `similar_group` holds the smallest item id in the photo's look-alike group, or NULL when
-- the photo resembles nothing. It is recomputed wholesale after each scan rather than
-- updated in place: an incremental version has to reason about a group *splitting* when a
-- photo is purged, which is the kind of state that goes quietly wrong.
ALTER TABLE items ADD COLUMN percep_hash INTEGER;
ALTER TABLE items ADD COLUMN similar_group INTEGER;

-- Serves the widened Duplicates filter. Partial, because almost every row is NULL.
CREATE INDEX items_similar_group ON items(similar_group) WHERE similar_group IS NOT NULL;
"#,
```

In `crates/photon-core/src/library/mod.rs`, change the two hardcoded `11`s to `12` — the opened version and `SchemaTooNew`'s `supported`. **Leave the table count assertion alone**; this migration creates no table.

In `crates/photon-core/src/library/items.rs`, in `update_items`'s UPDATE (line ~336), extend the reset:

```rust
                        content_hash = NULL, percep_hash = NULL, similar_group = NULL
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p photon-core`
Expected: PASS.

- [ ] **Step 6: Prove the reset test discriminates**

Temporarily remove `percep_hash = NULL, similar_group = NULL` from the UPDATE (hand-edit; never a loose `sed`, which would hit the neighbouring `content_hash` writer and fail ten tests, proving nothing).
Run: `cargo test -p photon-core replacing_a_file_clears`
Expected: FAIL on `percep_hash survived a replacement`. Restore and confirm PASS.

- [ ] **Step 7: Run the Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/library/
git commit -m "feat(core): schema 12 - columns for look-alike photos

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The difference hash

**Files:**
- Create: `crates/photon-core/src/similar.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod similar;`)
- Test: inside `similar.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn dhash(img: &image::DynamicImage) -> u64`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    /// A gradient with a bright blob, at whatever size is asked for. The same picture at two
    /// resolutions must hash to (nearly) the same value - that is the whole property.
    fn picture(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let blob = if (fx - 0.3).abs() < 0.12 && (fy - 0.6).abs() < 0.12 { 90.0 } else { 0.0 };
            let v = (fx * 160.0 + fy * 60.0 + blob).min(255.0) as u8;
            *px = Rgb([v, v.wrapping_add(20), 255 - v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    fn distance(a: u64, b: u64) -> u32 {
        (a ^ b).count_ones()
    }

    #[test]
    fn the_same_picture_at_a_quarter_size_hashes_the_same() {
        let big = dhash(&picture(800, 600));
        let small = dhash(&picture(200, 150));
        assert!(
            distance(big, small) <= 3,
            "distance {} between the same picture at two sizes",
            distance(big, small)
        );
    }

    #[test]
    fn a_mirrored_picture_is_not_a_look_alike() {
        let normal = dhash(&picture(400, 300));
        let flipped = dhash(&picture(400, 300).fliph());
        assert!(
            distance(normal, flipped) > 6,
            "a mirror image hashed within {} of the original",
            distance(normal, flipped)
        );
    }

    #[test]
    fn two_unrelated_pictures_are_far_apart() {
        let gradient = dhash(&picture(400, 300));
        let mut noise = RgbImage::new(400, 300);
        for (x, y, px) in noise.enumerate_pixels_mut() {
            let v = ((x * 37 + y * 101) % 256) as u8;
            *px = Rgb([v, 255 - v, v / 2]);
        }
        let other = dhash(&DynamicImage::ImageRgb8(noise));
        assert!(
            distance(gradient, other) > 6,
            "unrelated pictures hashed within {}",
            distance(gradient, other)
        );
    }

    /// The bit that would catch a hash that is accidentally constant, which every other
    /// test here would pass.
    #[test]
    fn a_flat_image_and_a_gradient_do_not_share_a_hash() {
        let flat = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
        assert_ne!(dhash(&flat), dhash(&picture(64, 64)));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p photon-core dhash`
Expected: FAIL to compile — `cannot find function dhash`.

- [ ] **Step 3: Write the implementation**

Create `crates/photon-core/src/similar.rs`:

```rust
//! Finding photos that are the same picture without being the same bytes.
//!
//! `duplicates.rs` answers "are these the same file"; this answers "are these the same
//! photo". A re-saved JPEG, an emailed copy at 2048px and a re-export from another program
//! are all different files with different byte hashes, and the README has always admitted
//! photon could not see them.
//!
//! The hash is a 64-bit **difference hash**: reduce the picture to 9x8 greyscale and set one
//! bit per horizontal neighbour pair according to which is brighter. It is the cheapest hash
//! with the property that matters - invariant to scale and to re-compression, because it
//! records the *relationships* between regions rather than their values. It is deliberately
//! not invariant to a mirror or a rotation: those are different photographs to a person
//! looking for a duplicate.

use image::{DynamicImage, imageops::FilterType};

/// Width of the reduced image. One more than the 8 columns of bits, because each bit
/// compares a pixel with its right-hand neighbour.
const REDUCED_W: u32 = 9;
const REDUCED_H: u32 = 8;

/// A 64-bit difference hash of `img`.
///
/// The reduction does the work: at 9x8 greyscale, JPEG ringing, a WebP re-encode and a
/// resize all vanish, while the arrangement of light and dark survives. `FilterType::Triangle`
/// matches what `decode_oriented` already uses, so a photo reduced here and a photo reduced
/// on the way to a thumbnail agree.
pub fn dhash(img: &DynamicImage) -> u64 {
    let small = img
        .resize_exact(REDUCED_W, REDUCED_H, FilterType::Triangle)
        .to_luma8();
    let mut hash = 0u64;
    for y in 0..REDUCED_H {
        for x in 0..(REDUCED_W - 1) {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            hash = (hash << 1) | u64::from(left > right);
        }
    }
    hash
}
```

Add `pub mod similar;` to `crates/photon-core/src/lib.rs`, in alphabetical position with the others.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core dhash`
Expected: PASS, 4 tests.

- [ ] **Step 5: Prove the resize test discriminates**

Temporarily change `resize_exact(REDUCED_W, REDUCED_H, ...)` to `resize_exact(REDUCED_W, REDUCED_H, FilterType::Nearest)`.
Run: `cargo test -p photon-core the_same_picture_at_a_quarter_size`
Expected: it may still pass — **nearest-neighbour is a weaker reduction, not a broken one.** If it passes, that is a finding, not a failure: record it and instead probe by replacing the comparison `left > right` with `left >= right`, which changes the hash of any flat region. Confirm a test fails, then restore by hand and re-run.

Whatever you probe, **report which probe you ran, what it did, and whether it discriminated.** A probe that passes is a finding here, not a formality.

- [ ] **Step 6: Run the Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-core/src/similar.rs crates/photon-core/src/lib.rs
git commit -m "feat(core): a difference hash for look-alike photos

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Banding and grouping

**Files:**
- Modify: `crates/photon-core/src/similar.rs`
- Test: inside `similar.rs`

**Interfaces:**
- Consumes: `dhash` from Task 2 (not called here, but the same module).
- Produces:
  - `pub fn group(hashes: &[(i64, u64)], distance: u32) -> Vec<(i64, i64)>` — pairs of `(item_id, group_id)` where `group_id` is the smallest id in the group. Photos with no look-alike are **absent** from the result.
  - `pub const EXACT_RECALL_DISTANCE: u32 = 3;`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_photo_with_no_look_alike_is_not_in_any_group() {
        let out = group(&[(1, 0x0000_0000_0000_0000), (2, 0xffff_ffff_ffff_ffff)], 3);
        assert!(out.is_empty());
    }

    #[test]
    fn a_close_pair_shares_the_smaller_id_as_its_group() {
        // One bit apart.
        let out = group(&[(7, 0b1010), (4, 0b1011)], 3);
        let mut out = out;
        out.sort();
        assert_eq!(out, vec![(4, 4), (7, 4)]);
    }

    #[test]
    fn groups_are_transitive() {
        // A~B and B~C at distance 3 each, A~C at 6 - all three must land in one group.
        let a = 0u64;
        let b = 0b111u64;
        let c = 0b111_111u64;
        let mut out = group(&[(1, a), (2, b), (3, c)], 3);
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1), (3, 1)]);
    }

    /// The pigeonhole argument, made a property of the code rather than of a comment:
    /// four 16-bit bands, so two hashes within distance 3 must agree exactly on some band.
    /// Brute-force every pair and assert the banded filter found all of them.
    #[test]
    fn banding_finds_every_pair_within_the_exact_recall_distance() {
        let mut hashes: Vec<(i64, u64)> = Vec::new();
        let base = 0x0123_4567_89ab_cdefu64;
        for i in 0..64i64 {
            // Flip up to three scattered bits, so pairs land at a range of small distances.
            let h = base ^ (1u64 << (i % 64)) ^ (1u64 << ((i * 7) % 64)) ^ (1u64 << ((i * 13) % 64));
            hashes.push((i + 1, h));
        }
        let grouped = group(&hashes, EXACT_RECALL_DISTANCE);
        let in_a_group: std::collections::HashSet<i64> = grouped.iter().map(|(id, _)| *id).collect();

        for (ia, ha) in &hashes {
            for (ib, hb) in &hashes {
                if ia >= ib {
                    continue;
                }
                if (ha ^ hb).count_ones() <= EXACT_RECALL_DISTANCE {
                    assert!(
                        in_a_group.contains(ia) && in_a_group.contains(ib),
                        "pair ({ia}, {ib}) at distance {} was missed",
                        (ha ^ hb).count_ones()
                    );
                }
            }
        }
    }

    #[test]
    fn distance_zero_groups_nothing() {
        let out = group(&[(1, 5), (2, 5)], 0);
        assert!(out.is_empty(), "distance 0 means the feature is off");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p photon-core -- group banding`
Expected: FAIL to compile — `cannot find function group`.

- [ ] **Step 3: Write the implementation**

Append to `crates/photon-core/src/similar.rs`:

```rust
use std::collections::HashMap;

/// The largest distance at which grouping has **complete** recall.
///
/// Candidate pairs are found by splitting each hash into four 16-bit bands and bucketing by
/// band. Two hashes within Hamming distance 3 must agree exactly on at least one band -
/// four bands, at most three differing bits, so some band has none. That makes the buckets
/// an exact filter at this distance, not a heuristic. Above it, a pair is found only if it
/// happens to share a band, which is why the UI says "finds most" rather than "finds all".
pub const EXACT_RECALL_DISTANCE: u32 = 3;

const BANDS: u32 = 4;
const BAND_BITS: u32 = 16;

/// Groups look-alikes, returning `(item_id, group_id)` for every photo that has at least one.
///
/// `group_id` is the smallest item id in the group, so a group has a stable name that does
/// not depend on iteration order. A photo that resembles nothing is absent from the result
/// rather than present with a NULL - the caller clears the column wholesale first.
///
/// `distance` of 0 turns the feature off and returns nothing.
///
/// Whole-library, not incremental: a union-find over 100k rows is milliseconds, and the
/// incremental version has to reason about a group *splitting* when a photo is purged.
pub fn group(hashes: &[(i64, u64)], distance: u32) -> Vec<(i64, i64)> {
    if distance == 0 || hashes.len() < 2 {
        return Vec::new();
    }

    // Bucket by band. The key mixes the band's index in, so the same 16 bits in two
    // different bands do not collide into one bucket.
    let mut buckets: HashMap<(u32, u16), Vec<usize>> = HashMap::new();
    for (index, (_, hash)) in hashes.iter().enumerate() {
        for band in 0..BANDS {
            let shift = band * BAND_BITS;
            let key = ((hash >> shift) & 0xffff) as u16;
            buckets.entry((band, key)).or_default().push(index);
        }
    }

    let mut parent: Vec<usize> = (0..hashes.len()).collect();
    for indexes in buckets.values() {
        for (i, &a) in indexes.iter().enumerate() {
            for &b in &indexes[i + 1..] {
                if (hashes[a].1 ^ hashes[b].1).count_ones() <= distance {
                    union(&mut parent, a, b);
                }
            }
        }
    }

    // Each root takes the smallest item id beneath it; then every member that shares a root
    // with someone else takes that id.
    let mut smallest: HashMap<usize, i64> = HashMap::new();
    let mut members: HashMap<usize, u32> = HashMap::new();
    for index in 0..hashes.len() {
        let root = find(&mut parent, index);
        let id = hashes[index].0;
        smallest.entry(root).and_modify(|s| *s = (*s).min(id)).or_insert(id);
        *members.entry(root).or_insert(0) += 1;
    }

    let mut out = Vec::new();
    for index in 0..hashes.len() {
        let root = find(&mut parent, index);
        if members[&root] > 1 {
            out.push((hashes[index].0, smallest[&root]));
        }
    }
    out
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core -- group banding transitive`
Expected: PASS.

- [ ] **Step 5: Prove the banding test discriminates**

Temporarily change `const BANDS: u32 = 4;` to `const BANDS: u32 = 2;` (with `BAND_BITS` left at 16, so only the low 32 bits are banded).
Run: `cargo test -p photon-core banding_finds_every_pair`
Expected: FAIL, naming a missed pair. Restore by hand and confirm PASS.

This is the test that makes the pigeonhole argument a property of the code. If it does **not** fail, stop and report it — it means the fixture's hashes do not exercise the bands, and the test is not defending what it claims.

- [ ] **Step 6: Run the Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-core/src/similar.rs
git commit -m "feat(core): band and group look-alike hashes

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The queries

**Files:**
- Create: `crates/photon-core/src/library/similar.rs`
- Modify: `crates/photon-core/src/library/mod.rs` (add `mod similar;` and re-export)
- Modify: `crates/photon-core/src/library/duplicates.rs` (widen the filter, extend `ItemCopy`)
- Test: inside `library/similar.rs`

**Interfaces:**
- Consumes: schema 12's columns (Task 1).
- Produces:
  - `pub struct SimilarCandidate { pub id: i64, pub thumb_key: u64, pub size: i64, pub mtime_ms: i64 }`
  - `Library::similar_candidates(&self) -> Result<Vec<SimilarCandidate>>`
  - `Library::set_percep_hash(&self, candidate: &SimilarCandidate, hash: u64) -> Result<bool>`
  - `Library::percep_hashes(&self) -> Result<Vec<(i64, u64)>>`
  - `Library::set_similar_groups(&self, groups: &[(i64, i64)]) -> Result<()>`
  - `Library::similar_of(&self, item_id: i64) -> Result<Vec<ItemCopy>>`
  - `ItemCopy` gains `pub width: u32`, `pub height: u32`
  - `duplicates::DUPLICATE_FILTER` widens (keep the name)

- [ ] **Step 1: Write the failing tests**

Model the setup on `library/duplicates.rs`'s existing test module — read it first; it already seeds folders and items and is the same shape.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    #[test]
    fn a_photo_with_no_thumbnail_is_not_a_candidate() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        lib.insert_items(&[new_item(folder, "/pics/a.jpg", 10, 100)]).unwrap();
        // thumb_state defaults to Pending, so nothing is ready to hash.
        assert!(lib.similar_candidates().unwrap().is_empty());
    }

    #[test]
    fn a_ready_photo_without_a_hash_is_a_candidate_and_takes_one() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib.insert_items(&[new_item(folder, "/pics/a.jpg", 10, 100)]).unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();

        let candidates = lib.similar_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(lib.set_percep_hash(&candidates[0], 0xdead_beef).unwrap());

        assert!(lib.similar_candidates().unwrap().is_empty(), "still a candidate after hashing");
        assert_eq!(lib.percep_hashes().unwrap(), vec![(ids[0], 0xdead_beef)]);
    }

    /// The same guard `set_content_hash` has: the pass reads long after the row was listed,
    /// and a hash computed from the old thumbnail must not land on a row whose file moved.
    #[test]
    fn a_hash_is_refused_once_the_row_has_moved_on() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib.insert_items(&[new_item(folder, "/pics/a.jpg", 10, 100)]).unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();
        let candidate = lib.similar_candidates().unwrap().remove(0);

        lib.update_items(&[(ids[0], new_item(folder, "/pics/a.jpg", 20, 200))]).unwrap();
        assert!(!lib.set_percep_hash(&candidate, 0xdead_beef).unwrap());
    }

    #[test]
    fn groups_are_replaced_wholesale() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/pics/a.jpg", 10, 100),
                new_item(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])]).unwrap();
        assert_eq!(lib.similar_of(ids[0]).unwrap().len(), 1);

        // A later pass finds nothing similar: the old groups must go, not linger.
        lib.set_similar_groups(&[]).unwrap();
        assert!(lib.similar_of(ids[0]).unwrap().is_empty());
    }

    #[test]
    fn the_duplicates_view_counts_look_alikes_too() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/pics/a.jpg", 10, 100),
                new_item(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 0);
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])]).unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 2, "look-alikes are not in the view");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p photon-core -- similar_candidates percep_hash similar_of look_alikes`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Create `crates/photon-core/src/library/similar.rs`:

```rust
//! The queries behind look-alike photos. The hashing and grouping are `crate::similar`;
//! this is only what they read and write, and what the UI asks afterwards.

use super::{Library, duplicates::ItemCopy};
use crate::Result;
use crate::media::fingerprint;
use crate::edit::Edit;
use rusqlite::params;

/// A photo whose thumbnail exists but whose perceptual hash does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimilarCandidate {
    pub id: i64,
    /// The cache key of the grid thumbnail to hash - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// The fingerprint the hash will be stored against; see [`Library::set_percep_hash`].
    pub size: i64,
    pub mtime_ms: i64,
}

/// Photos that have a look-alike. Written as a filter so the Duplicates view stays a plain
/// filter, keeping the folder-first order and the sidebar agreement everything downstream
/// is built on.
pub(crate) const SIMILAR_FILTER: &str = "i.similar_group IS NOT NULL";

const CANDIDATES_SQL: &str = "SELECT i.id, i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM items i
     JOIN folders f ON f.id = i.folder_id
     JOIN watched_folders w ON w.id = f.watched_id
     WHERE i.missing_since IS NULL AND i.percep_hash IS NULL
       AND i.thumb_state = 1 AND w.online = 1
     ORDER BY i.id";

impl Library {
    /// Live photos whose thumbnail is ready but which have no perceptual hash yet.
    ///
    /// `thumb_state = 1` (Ready) is the point: the hash is taken from the cached 256px grid
    /// thumbnail, so a photo whose thumbnail has not been rendered has nothing to hash. It
    /// becomes a candidate as soon as it does, which is why an existing library fills in
    /// without anything being re-decoded.
    pub fn similar_candidates(&self) -> Result<Vec<SimilarCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(CANDIDATES_SQL)?;
        let rows = stmt
            .query_map([], |r| {
                let path: String = r.get(1)?;
                let size: i64 = r.get(2)?;
                let mtime_ms: i64 = r.get(3)?;
                let turns: u8 = r.get(4)?;
                let crop: Option<i64> = r.get(5)?;
                let edit = Edit::from_db(turns, crop);
                Ok(SimilarCandidate {
                    id: r.get(0)?,
                    thumb_key: edit.thumb_key(fingerprint(&path, size, mtime_ms)),
                    size,
                    mtime_ms,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores a perceptual hash, but only against the fingerprint it was computed for -
    /// the same guard [`Library::set_content_hash`] has, and for the same reason.
    pub fn set_percep_hash(&self, candidate: &SimilarCandidate, hash: u64) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET percep_hash = ?2
             WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL",
            params![candidate.id, hash as i64, candidate.size, candidate.mtime_ms],
        )?;
        Ok(changed == 1)
    }

    /// Every live photo's perceptual hash. One integer per photo, so a 100k library is
    /// 1.6 MB - which is what makes grouping in Rust affordable and an SQL index pointless.
    pub fn percep_hashes(&self) -> Result<Vec<(i64, u64)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, percep_hash FROM items
             WHERE percep_hash IS NOT NULL AND missing_since IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let hash: i64 = r.get(1)?;
                Ok((r.get::<_, i64>(0)?, hash as u64))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replaces every group in one transaction.
    ///
    /// Wholesale rather than incremental: the pass recomputes the whole library, and
    /// clearing first is what lets a group *shrink* - a photo that no longer resembles
    /// anything must lose its group, and an UPDATE of only the new members would leave it
    /// pointing at a group it is no longer in.
    pub fn set_similar_groups(&self, groups: &[(i64, i64)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute("UPDATE items SET similar_group = NULL WHERE similar_group IS NOT NULL", [])?;
        {
            let mut stmt = tx.prepare_cached("UPDATE items SET similar_group = ?2 WHERE id = ?1")?;
            for (id, group) in groups {
                stmt.execute(params![id, group])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The other live photos that look like `item_id`, by path.
    pub fn similar_of(&self, item_id: i64) -> Result<Vec<ItemCopy>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT o.id, o.path, o.width, o.height FROM items i
             JOIN items o ON o.similar_group = i.similar_group AND o.id <> i.id
             WHERE i.id = ?1 AND i.similar_group IS NOT NULL AND o.missing_since IS NULL
             ORDER BY o.path",
        )?;
        let rows = stmt
            .query_map([item_id], |r| {
                Ok(ItemCopy {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    width: r.get(2)?,
                    height: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
```

Check `Edit::from_db`'s and `Edit::thumb_key`'s exact signatures in `crates/photon-core/src/edit.rs` before writing the candidate query — `edit_from_db` may be a free function in `items.rs` rather than an associated one. Use whichever the codebase already provides; **do not add a second way to build an `Edit` from its columns.**

In `crates/photon-core/src/library/duplicates.rs`:

1. Add `pub width: u32, pub height: u32` to `ItemCopy`, and select them in `COPIES_SQL` (`SELECT o.id, o.path, o.width, o.height`).
2. Widen the view filter, keeping the name and extending the comment:

```rust
/// The photos that have at least one byte-identical twin **or** a look-alike, as a grid
/// filter. Applied to the driver as well as the outer `WHERE`, like every membership view,
/// so a folder is placed by its oldest matching photo and the sidebar agrees with the grid.
///
/// The identical half counts live rows only: a photo whose one twin has gone missing is no
/// longer a duplicate of anything the user can find. The look-alike half is a single column
/// read, because `crate::similar` has already done the grouping.
pub(crate) const DUPLICATE_FILTER: &str = "AND (i.content_hash IN (
    SELECT content_hash FROM items
    WHERE content_hash IS NOT NULL AND missing_since IS NULL
    GROUP BY content_hash HAVING COUNT(*) > 1)
    OR i.similar_group IS NOT NULL)";
```

In `crates/photon-core/src/library/mod.rs`, add `mod similar;` and `pub use similar::SimilarCandidate;` beside the existing duplicates re-exports.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core`
Expected: PASS. **Some existing Duplicates tests may now need a look** — if one fails, read it before changing it: a test that asserted "only byte-identical photos are in this view" is asserting the old contract and its expectation genuinely changes, but a test that fails for any other reason is a real regression. Say which you found in your report.

- [ ] **Step 5: Prove the widened filter discriminates**

Temporarily revert `DUPLICATE_FILTER` to its original form (hand-edit).
Run: `cargo test -p photon-core the_duplicates_view_counts_look_alikes_too`
Expected: FAIL, `look-alikes are not in the view`. Restore and confirm PASS.

- [ ] **Step 6: Add the index plan test**

The spec requires the partial index to get a plan test, as `the_recent_view_is_served_by_its_index` does for Recent. Find that test, read how it asserts on `EXPLAIN QUERY PLAN`, and write the equivalent for a Duplicates query, asserting the plan uses `items_similar_group` and has no temp b-tree. Run it, then **temporarily drop the index from the migration** and confirm the plan test fails. Restore.

- [ ] **Step 7: Run the Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/library/
git commit -m "feat(core): queries for look-alike photos, and widen the Duplicates view

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The pass, and the engine

**Files:**
- Modify: `crates/photon-core/src/similar.rs` (add the pass)
- Modify: `crates/photon-app/src/engine.rs:147-148` (keep a cache handle), `:1202`, `:1227-1249`
- Test: inside `similar.rs`, and `crates/photon-app/src/engine.rs`'s tests

**Interfaces:**
- Consumes: `dhash`, `group`, `EXACT_RECALL_DISTANCE` (Tasks 2-3); the `Library` methods from Task 4; `ThumbCache::path_for`.
- Produces: `pub fn update(lib: &Library, cache: &ThumbCache, distance: u32, cancel: &AtomicBool) -> Result<u64>` — hashes every candidate whose thumbnail can be read, regroups the whole library, and returns how many rows took a new hash.

- [ ] **Step 1: Write the failing test**

In `similar.rs`'s test module. It needs a real cached thumbnail, so build one through `ThumbCache` the way `thumbs/cache.rs`'s own tests do — read those first for the exact helper names.

```rust
    /// The end-to-end property: two files that are the same picture at different sizes, both
    /// with thumbnails, end up in one group; an unrelated third does not.
    #[test]
    fn the_pass_groups_the_same_picture_at_two_sizes() {
        // Build a library with three photos, render their thumbnails, run the pass.
        // (Construct via temp_library/seed_folder/insert_items, write real JPEG bytes with
        //  testutil::jpeg_bytes at two sizes for the pair and a visually different third,
        //  then ThumbCache::generate each one and mark the rows Ready.)
        // Assert: the pair shares a similar_group, the third has none.
    }
```

**This is the one test in this plan written as a sketch rather than as final code**, because it depends on `testutil`'s exact fixture helpers and on how `thumbs/cache.rs`'s tests build a cache — both of which you must read. Write it out fully before implementing. If `testutil::jpeg_bytes` cannot produce two visibly-same-but-differently-sized pictures, add a helper beside it rather than inlining image construction in the test.

Keep the fixtures **small**: a fixture of several megapixels costs seconds in a debug build, and a thin strip proves a resolution claim as well as a square does.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p photon-core the_pass_groups`
Expected: FAIL — `update` does not exist.

- [ ] **Step 3: Write the pass**

Append to `crates/photon-core/src/similar.rs`:

```rust
use crate::{Result, library::Library, thumbs::{ThumbCache, ThumbSize}};
use std::sync::atomic::{AtomicBool, Ordering};

/// Hashes every photo that has a thumbnail but no hash, then regroups the whole library.
///
/// The hash comes from the **cached 256px grid thumbnail**, not from the photo: the
/// thumbnail is the reduced image this hash wants, it is already on disk, and decoding it
/// costs about a millisecond against the ~175ms a source decode costs. That is also what
/// makes an existing library fill in - the renderer never runs again for a photo whose
/// thumbnail is already cached, so a hash computed there would never have been computed at
/// all (see the plan's spec-deviation note).
///
/// A thumbnail that cannot be read is skipped and the row stays a candidate: it is usually a
/// cache still being written, and the next scan's pass tries again. Cancelling stops between
/// photos; what was hashed so far is kept.
pub fn update(lib: &Library, cache: &ThumbCache, distance: u32, cancel: &AtomicBool) -> Result<u64> {
    let mut hashed = 0;
    for candidate in lib.similar_candidates()? {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = cache.path_for(candidate.thumb_key, ThumbSize::Grid);
        match image::open(&path) {
            Ok(img) => {
                if lib.set_percep_hash(&candidate, dhash(&img))? {
                    hashed += 1;
                }
            }
            Err(err) => {
                tracing::debug!(id = candidate.id, %err, "could not read a thumbnail to hash");
            }
        }
    }

    // Regroup unconditionally, not only when something was hashed: the setting may have
    // changed, or a photo may have been purged out of a group since the last pass.
    lib.set_similar_groups(&group(&lib.percep_hashes()?, distance))?;
    Ok(hashed)
}
```

Check that `ThumbSize` is exported from `crate::thumbs`; if `path_for` is not public on `ThumbCache`, it already is (`cache.rs:65`).

- [ ] **Step 4: Wire the engine**

In `crates/photon-app/src/engine.rs`:

1. At line ~147, keep a handle: `let cache = Arc::new(ThumbCache::new(config.cache_dir.clone()));` then pass `cache.clone()` to `ThumbService::start`, and store `cache` on the `Engine` struct as `cache: Arc<ThumbCache>`.
2. Rename `hash_duplicates` to `hash_after_scan` and run both passes inside its existing guard, so one `hashing` mutex and one `hash_requested` flag cover both. Inside the `while self.hash_requested.swap(...)` loop, after the existing duplicate call:

```rust
                let distance = self.lib.similar_distance().unwrap_or(EXACT_RECALL_DISTANCE as i64) as u32;
                match photon_core::similar::update(&self.lib, &self.cache, distance, cancel) {
                    Ok(0) => {}
                    Ok(_) => {
                        if let Err(err) = self.refresh_grid() {
                            tracing::warn!(%err, "grid refresh failed");
                        }
                    }
                    Err(err) => tracing::warn!(%err, "look-alike hashing failed"),
                }
```

`similar_distance()` arrives in Task 6; until then, use `EXACT_RECALL_DISTANCE as u32` directly and leave a comment saying Task 6 replaces it. **Update the call site at line 1202 and the doc comment above the method**, which currently describes only the duplicate pass.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p photon-core the_pass_groups && cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Prove the pass test discriminates**

Temporarily make `update` return before calling `set_similar_groups`.
Run: `cargo test -p photon-core the_pass_groups`
Expected: FAIL — the pair has no group. Restore and confirm PASS.

- [ ] **Step 7: Run the Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/similar.rs crates/photon-app/src/engine.rs
git commit -m "feat: find look-alikes after every scan

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The setting

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs`
- Modify: `crates/photon-app/src/{commands,ipc,app}.rs`
- Modify: `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js`
- Modify: `crates/photon-app/src/engine.rs` (use the real setting)

**Interfaces:**
- Consumes: `EXACT_RECALL_DISTANCE`.
- Produces: `Library::similar_distance() -> Result<i64>`, `Library::set_similar_distance(i64) -> Result<i64>` (returns the clamped value); commands `similar_distance` / `set_similar_distance`; TS `api.similarDistance()` / `api.setSimilarDistance(distance)`.

- [ ] **Step 1: Write the failing test**

In `settings.rs`'s test module, modelled on `the_slideshow_interval_defaults_persists_and_is_clamped_both_ways` (read it first):

```rust
    #[test]
    fn the_similar_distance_defaults_persists_and_is_clamped_both_ways() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.similar_distance().unwrap(), 3, "conservative is the default");
        assert_eq!(lib.set_similar_distance(6).unwrap(), 6);
        assert_eq!(lib.similar_distance().unwrap(), 6);
        assert_eq!(lib.set_similar_distance(0).unwrap(), 0, "off is a real choice");
        assert_eq!(lib.set_similar_distance(99).unwrap(), 6, "clamped to loose");
        assert_eq!(lib.set_similar_distance(-1).unwrap(), 0, "clamped to off");
        // Written by something other than the setter - clamped on read, as the table is
        // plain text an older or newer photon may have written.
        lib.set_setting(SIMILAR_DISTANCE, "40").unwrap();
        assert_eq!(lib.similar_distance().unwrap(), 6);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p photon-core similar_distance`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

In `settings.rs`, beside the other key constants:

```rust
/// How far apart two perceptual hashes may be and still count as the same picture:
/// 0 off, 3 conservative, 6 loose. Stored as the distance itself rather than a name,
/// because the distance is what the pass uses and a name would need a second table to
/// interpret it.
const SIMILAR_DISTANCE: &str = "similar_distance";
/// Conservative: the distance at which grouping has complete recall.
pub const SIMILAR_DISTANCE_DEFAULT: i64 = 3;
/// Off, conservative, loose. Clamped rather than refused, both ways.
pub const SIMILAR_DISTANCE_RANGE: std::ops::RangeInclusive<i64> = 0..=6;
```

and, in `impl Library`, a getter and setter that clamp on the way in **and** on the way out, exactly as `slideshow_interval_s` does. Add a private `clamp_distance` beside `clamp_interval`.

Then the IPC quartet, following `slideshow_interval` exactly: `commands.rs` (`pub fn similar_distance(engine: &Engine) -> CmdResult<i64>` and `set_similar_distance(engine: &Engine, distance: i64) -> CmdResult<i64>`), the two delegating wrappers in `ipc.rs`, two entries in `app.rs`'s `generate_handler!`, the two calls in `ui/src/lib/api.ts` (`similarDistance: () => invoke<number>('similar_distance')`, `setSimilarDistance: (distance: number) => invoke<number>('set_similar_distance', { distance })`), and answers in `mock.js` (`similar_distance: () => 3, set_similar_distance: (a) => a.distance`).

Finally, replace the placeholder in `engine.rs` from Task 5 with the real `self.lib.similar_distance()`.

- [ ] **Step 4: Run the gates**

Run: `cargo test --workspace && cargo test -p xtask && npm run check`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 5: Prove the mock test discriminates**

Delete the two `mock.js` lines, run `cargo test -p xtask`, confirm it fails naming both commands, restore by hand, confirm PASS.

- [ ] **Step 6: Run the full Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add -A
git commit -m "feat: a setting for how alike is alike enough

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Copies, in two kinds

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (`viewer_item`, ~line 417)
- Modify: `ui/src/lib/api.ts`
- Modify: `crates/xtask/screenshots/mock.js`

**Interfaces:**
- Consumes: `Library::copies_of`, `Library::similar_of`, the widened `ItemCopy` (Task 4).
- Produces: `ViewerItem.copies` entries gain `kind: 'identical' | 'similar'`, `width`, `height`.

- [ ] **Step 1: Write the failing test**

In `crates/photon-app/src/commands.rs`'s test module (read how its existing `viewer_item` tests build an engine):

```rust
    #[test]
    fn the_viewer_lists_identical_copies_before_look_alikes() {
        // Build an engine with three photos: two byte-identical, one a look-alike of the
        // first. Assert viewer_item(first).copies has the identical one first with
        // kind Identical, then the look-alike with kind Similar and its dimensions.
    }
```

Write it out fully against the existing test helpers before implementing.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p photon-app identical_copies_before_look_alikes`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

In `commands.rs`, add a serde enum beside the other IPC types:

```rust
/// What kind of relationship a listed copy has to the photo on screen.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CopyKind {
    /// The same bytes.
    Identical,
    /// The same picture, different bytes - a resize or a re-save.
    Similar,
}
```

Give the serialised copy struct `kind`, `width` and `height`, and in `viewer_item` build the list as `copies_of` (each `Identical`) followed by `similar_of` (each `Similar`), filtering out of the second list any id already in the first — a photo can be both, and listing it twice would be a bug the user sees.

Mirror it in `ui/src/lib/api.ts`:

```typescript
export type CopyKind = 'identical' | 'similar';

export interface ItemCopy {
  id: number;
  path: string;
  kind: CopyKind;
  width: number;
  height: number;
}
```

and update `mock.js`'s `viewer_item` answer to include the new fields, with at least one of each kind so the screenshots show both groups.

Note `ui/tsconfig.json` includes `src/**/*.ts`, so **test files are typechecked too** — adding fields to `ItemCopy` breaks every `ItemCopy` literal in the UI tests, and `npm run check` fails on it. Fix those literals in the same commit.

- [ ] **Step 4: Run the gates**

Run: `cargo test --workspace && npm run check && npm test`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 5: Prove the ordering test discriminates**

Temporarily swap the two lists so look-alikes come first.
Run: `cargo test -p photon-app identical_copies_before_look_alikes`
Expected: FAIL. Restore and confirm PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add -A
git commit -m "feat: tell identical copies from look-alikes in the info panel

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: The UI

**Files:**
- Modify: the info-panel module under `ui/src/lib/` that shapes `ViewerItem` for display (find it: `grep -rn 'copies' ui/src/lib ui/src/components`)
- Modify: `ui/src/components/Viewer.svelte`
- Modify: `ui/src/components/Settings.svelte`
- Test: a `.ts` test beside the module

**Interfaces:**
- Consumes: `ItemCopy` with `kind`/`width`/`height` (Task 7); `api.similarDistance` / `setSimilarDistance` (Task 6).
- Produces: a pure grouping helper plus its test; markup in the two components.

- [ ] **Step 1: Write the failing test**

The grouping is logic and belongs in a pure module, because **vitest runs with `environment: 'node'` and a `.svelte` file cannot be rendered**. Put it beside the other info-panel helpers:

```typescript
describe('copy groups', () => {
  it('splits copies into identical and look-alike, keeping order', () => {
    const copies: ItemCopy[] = [
      { id: 2, path: '/a/b.jpg', kind: 'identical', width: 4000, height: 3000 },
      { id: 3, path: '/c/d.jpg', kind: 'similar', width: 2048, height: 1536 },
      { id: 4, path: '/e/f.jpg', kind: 'similar', width: 800, height: 600 },
    ];
    expect(copyGroups(copies)).toEqual([
      { kind: 'identical', label: 'Identical', copies: [copies[0]] },
      { kind: 'similar', label: 'Looks the same', copies: [copies[1], copies[2]] },
    ]);
  });

  it('omits a group with no members', () => {
    const copies: ItemCopy[] = [
      { id: 2, path: '/a/b.jpg', kind: 'identical', width: 10, height: 10 },
    ];
    expect(copyGroups(copies).map((g) => g.kind)).toEqual(['identical']);
  });

  it('has nothing to show for a photo with no copies', () => {
    expect(copyGroups([])).toEqual([]);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `npm test -w ui -- <the test file>`
Expected: FAIL — `copyGroups` is not exported.

- [ ] **Step 3: Implement `copyGroups`, then the markup**

Write the helper. Then in `Viewer.svelte`'s info panel, render each group with its label as a heading and each entry as it renders copies today, adding the pixel dimensions to a look-alike entry (`{width} × {height}`) — the spec's reason is that at that point the question is almost always "which of these is the big one". Keep the existing click-to-locate behaviour on every entry.

In `Settings.svelte`, under an appropriate section, add an **Off / Conservative / Loose** control. **Follow the existing theme and size controls exactly**: `role="group"` with `aria-pressed` on independent buttons, ordinary Tab stops, no `role="radiogroup"` and no arrow-key handling — `role="radio"` without roving-tabindex key handling lies to a screen reader. Copy the `.segmented` rules, and note that Svelte scopes styles per component, so any declaration the source control inherits from `Settings.svelte`'s generic `button` rule (`border`, `cursor`, `transition`, and the `prefers-reduced-motion` override) must be restated if you put the control in a new component. Give Loose a hint saying it finds *most* look-alikes, not all.

`ui/src/lib/no-literals.test.ts` fails on a colour literal, a named colour, a colour function used as a component colour, a glyph icon, or a `var(--x)` that `tokens.css` does not declare.

- [ ] **Step 4: Run the gates**

Run: `npm test -w ui && npm run check`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 5: Prove the grouping test discriminates**

Temporarily make `copyGroups` return a single group containing everything.
Run: `npm test -w ui -- <the test file>`
Expected: FAIL. Restore and confirm PASS.

- [ ] **Step 6: Commit**

The markup has no test and cannot have one; say so in the commit message and why.

```bash
git add -A
git commit -m "feat(ui): show look-alikes beside identical copies

The grouping is a pure helper with its own tests; the panel markup and the
Settings control have none, because vitest runs under node and a .svelte file
cannot be rendered. svelte-check, no-literals.test.ts and the smoke checklist
cover them.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Docs, screenshots and the smoke checklist

**Files:**
- Modify: `README.md` (the Duplicates section and the smoke checklist)
- Modify: `CLAUDE.md` (the duplicate-finder paragraph; **and see the carried-over note below**)
- Modify: `crates/xtask/src/screenshots.rs` (`SHOTS`)

- [ ] **Step 1: Update the README's Duplicates section**

It currently ends "Resized or re-saved versions are different files and are not reported." That sentence is now wrong and is the reason this feature exists. Rewrite the section to describe both kinds, say that look-alikes are found from the thumbnails photon has already made (so nothing is re-read), and explain the Off / Conservative / Loose setting in the user's terms — including that Loose finds most look-alikes rather than all, which is a real limit and should not be hidden.

- [ ] **Step 2: Update CLAUDE.md's duplicate-finder paragraph**

The architecture section's paragraph beginning "**The duplicate finder hashes after the scan, in the engine.**" must now cover both passes, `items.percep_hash` / `items.similar_group`, the fact that **the perceptual hash comes from the cached grid thumbnail** (and why: the renderer is skipped for an already-cached thumbnail, so a hash computed there would never be computed for an existing library), and that `update_items` must NULL all three derived columns.

- [ ] **Step 3: Carry over the note from the tile-size branch**

The grid tile size work found a fact worth recording that belongs to no feature: **when a scroll container's content shrinks, the browser clamps `scrollTop` to the new maximum synchronously, during the very layout that reading `scrollTop` forces.** A guard that asked "has the viewport moved since I pinned it?" was therefore always false on a size decrease, and the grid jumped to the end of the library. Add it to CLAUDE.md's styling/layout section, in its voice, as a trap for anything that restores a scroll position after changing content height.

Commit this as **its own commit**, separate from the feature, so it can be cherry-picked or reverted independently.

- [ ] **Step 4: Add a screenshot**

Add one `SHOTS` entry showing the Duplicates view with both kinds present (the mock's `viewer_item` gained both in Task 7; check whether the sidebar's Duplicates row needs a `duplicateCount` in `mock.js` to appear). Follow the existing entries' shape. Update the count in `CLAUDE.md` — it says sixteen PNGs and will say seventeen.

Generate it (`cargo run -p xtask -- screenshots --only <name>`; needs Chromium on `PATH` or in `CHROMIUM`) and **look at it** with the Read tool. If Chromium is unavailable, say so and skip the generation, not the entry.

- [ ] **Step 5: Add the smoke-checklist entries**

In the README's `## Manual smoke checklist`. At minimum:

```markdown
- Save a copy of one photo at half its size into a watched folder, let the scan finish, and
  check both turn up under Duplicates with the copy marked "Looks the same" and its
  dimensions shown.
- Set Find look-alikes to Off in Settings and check Duplicates falls back to byte-identical
  files only, then set it back.
- Open a photo that has both an identical copy and a look-alike, and check the info panel
  lists them under two headings with the identical one first.
- On a library indexed by an older photon, check look-alikes appear without anything being
  re-scanned — the hashes come from thumbnails that already exist.
```

Do not pad beyond what needs eyes.

- [ ] **Step 6: Run every gate and chore**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
cargo run -p xtask -- versions
cargo run -p xtask -- metadata
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "docs: look-alike photos, and a screenshot of both kinds

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

**Do not push and do not open a PR** — the controller handles both.

---

## Self-Review

**Spec coverage.** Every section maps to a task: the dHash and its placement (Tasks 2 and 5, with the deviation argued at the top); banding with the pigeonhole property (Task 3); union-find into `similar_group` (Tasks 3-4); the widened Duplicates filter keeping the view a plain filter (Task 4); `update_items` NULLing the columns (Task 1); the pass running beside `hash_duplicates` in the engine for the reasons the spec gives (Task 5); schema 12 with the partial index and its plan test (Tasks 1 and 4); no `bump_thumb_gc_epoch` (stated in Global Constraints, nothing to do); the settings triple and its IPC (Task 6); `ItemCopy` gaining `kind` and dimensions (Task 7); the info panel's two groups and the Settings control (Task 8); README and CLAUDE.md (Task 9). The spec's six named tests all appear: `dhash_survives_a_resize` and `dhash_separates_different_photos` in Task 2, `bands_find_every_pair_within_three` in Task 3, `groups_are_transitive` in Task 3, `a_replaced_file_loses_its_similar_group` in Task 1, `off_restores_todays_duplicates_view` in Task 6's clamp test plus Task 4's filter test.

**Type consistency.** `u64` is the hash everywhere in Rust and is cast to `i64` only at the SQLite boundary (SQLite has no unsigned integer); `SimilarCandidate` carries `thumb_key` because the pass needs a cache path, not a file path; `ItemCopy` is one struct shared by `copies_of` and `similar_of`, with `CopyKind` applied by the command layer rather than stored, because it is a property of the *query*, not of the row.

**Two soft spots, named rather than hidden.**

1. **Task 5's test is a sketch.** It needs real cached thumbnails and depends on `testutil` helpers and `thumbs/cache.rs`'s test setup that I have not read line by line. Its implementer must write it out fully before implementing, and should report if the fixtures cannot express "the same picture at two sizes" — that would be a finding about `testutil`, not about this feature.
2. **`similar_of`'s query joins `items` to itself on `similar_group`.** With the partial index that is cheap, but it is the one new query that runs per viewer open. If the plan test in Task 4 shows a scan rather than an index seek, say so rather than accepting it.
