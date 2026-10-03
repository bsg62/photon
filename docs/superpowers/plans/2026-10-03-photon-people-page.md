# People Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the user the People page — the place where the face groups photon found are named, confirmed, corrected and ignored — with face crops, a sidebar entry that counts the groups waiting for a name, and a confirmation before switching "Find faces" off deletes the names.

**Architecture:** Plan 1 (backend, on `feat/people`, head `f1853c4`) built every command the page needs. This plan adds a face-crop route to `photon://`, a count for the sidebar, a `.svelte.ts` factory holding all of the page's behaviour (tested under vitest), the page's markup as three components, and the wiring that puts the page in the main area in place of the grid. The page is not a `GridView`: a small `mainPage` state chooses between the grid and the page, and the grid unmounts while the page is shown.

**Tech Stack:** Rust (photon-core, photon-app, xtask), Tauri 2 custom protocol, Svelte 5 runes + TypeScript, vitest, headless-Chromium screenshots.

**Spec:** `docs/superpowers/specs/2026-10-03-photon-people-design.md` — sections "The People page", "Loading", "Face crops", "Logic and markup", "Switching off", "IPC", "Documentation", "Testing", "Rollout", and "As built" 1-14. Read CLAUDE.md's "Recognising people", "IPC is three files per command", "TypeScript mirrors", "There is no component test harness", "Styling" (tokens, icons, focus ring, dialogs, `fitMenu`) and "Conventions" before starting any task.

## Global Constraints

- **photon never writes to photo files, and names are never written** to `.picasa.ini` or to the photos. Nothing in this plan writes outside `library.db`.
- **Only confirmed faces carry a name**; a suggestion is shown on the People page and nowhere else.
- **A face on a hidden or missing photo appears nowhere** on the page or in the sidebar count (`hidden = 0 AND missing_since IS NULL`).
- **IPC is three files per command** (`commands.rs`, `ipc.rs`, `app.rs`'s `generate_handler!`) plus the hand-written mirror in `ui/src/lib/api.ts` and an answer in `crates/xtask/screenshots/mock.js` (a test in `screenshots.rs` fails without it). Change Rust and TS in the same commit.
- **Colours are tokens** from `ui/src/tokens.css` only (`no-literals.test.ts` fails on a literal, a named colour, `color-mix` or an undeclared `var(--x)`); **icons are `Icon.svelte`** names already in `lib/icons.ts` (`user`, `check`, `x`, `eye-off`, `chevron-down`, `chevron-right` are there); no glyph icons.
- A container focused from script gets `.focus-container`; a context menu is placed by `use:fitMenu`; a pointer gesture is not introduced (selection is by click).
- **Confirmations use the native `ask` from `@tauri-apps/plugin-dialog`**, as every other confirmation in photon does (`FolderTree.svelte` delete album, `Settings.svelte` remove folder). See "Decisions made in planning".
- **Every new test must be shown to fail with its change reverted** (an exact replacement from a saved copy, then `touch` the restored file); a probe that passes is a finding — write an input that makes the reverted code differ. A change with no possible test (markup, an effect) says so in its commit message.
- **Never launch the GUI** (`npm run dev`, the app binary) to verify anything. Verification is the Rust gate, `npm run check` (0 errors, 0 warnings), `npm test`, and the screenshots.
- The Rust gate before every commit that touches Rust: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. The UI gate before every commit that touches `ui/`: `npm run check`, `npm test`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`. Do not push.
- Comments carry the reasoning, not the mechanics; a wrong justification is a defect.

## Decisions made in planning

These settle what the spec leaves open or where this plan departs from it. Task 6 records 1 and 2 in the spec's "As built".

1. **The switch-off confirmation is the native `ask` dialog, from Settings**, not an overlay in `App.svelte`. The spec put it in `App.svelte` because CLAUDE.md's overlay rule keeps dialogs out of inert containers; a native dialog is modal over the whole window, so the rule does not arise, and it is how photon asks every other destructive question. The deletion of people on the page (Delete, Merge) asks the same way.
2. **"Confirm all" confirms the faces on screen** (the strip plus whatever "Show all" has loaded), and its label says how many when that is fewer than the person's suggestions ("Confirm these 12"). Confirming faces the user has not looked at would put strangers under a name — the reason renaming does not confirm suggestions (As built 3).
3. **Clicking a face toggles its selection**; a selection lives in one strip at a time (clicking a face in another strip starts a new selection there). The action bar shows in the strip that holds the selection. Space toggles from the keyboard; Enter or a double-click opens the face's photo.
4. **Opening a face's photo** finds it in the grid's current view, or switches to All photos when it is not there (as "Locate in photon" does), and opens the viewer over the People page. Closing the viewer returns to the page, which keeps its state.
5. **The grid unmounts while the People page is shown.** Leaving the page is any sidebar view, a folder jump, typing a search, or a search started from the viewer or Statistics.
6. **Single faces can be named.** `PageFace` gains `personId` (the face's group); naming a selection of single faces names the first face's group and merges the others' groups into the result.
7. **The sidebar count** ("N to name") is its own command, `people_to_name`, read with the collections on every `data_changed`; it equals `people_page().unnamed.len()` by construction and a test pins that.
8. **A face crop is cut only from a cached preview.** A preview that is not cached, a face that is gone, or a key that is not the photo's current one is a 404, and the UI draws a placeholder; the route never asks the thumbnail service to render, so a page of crops cannot start a burst of renders.

## Review Focus

The five conditions the spec implies that no plan-1 test exercises and that are most likely to bite a person using the page, each with the test that pins it and the task that owns it:

1. **A reload lands while an action is in flight** (the backend's `data_changed` after an earlier action, or a scan): faces the user just rejected must not reappear, and once a reload started after the action has landed, the page shows the truth. → Task 3, `a reload started before an action ends does not bring its faces back` and `a reload started after it shows what the backend says`.
2. **The group on screen has changed under the user** (a grouping run emptied and deleted it, or merged faces out): naming it fails with `notAPerson`; the user sees the message and the page reloads, rather than a silent success or a stuck box. → Task 1, `merging_a_group_that_is_gone_is_refused` (backend) and Task 3, `a refused name reloads the page and reports the error`.
3. **A name typed in another case** ("anna" when Anna exists, "ÉMILE" for Émile): the box says "Add to Anna" before committing, exactly when the backend will merge. → Task 3, `nameChoice` tests in `people.test.ts` (case, Unicode case, surrounding spaces, the person being renamed).
4. **A crop that cannot be served** (preview not cached, face deleted by an edit, stale key): a 404, never a 500 and never a render; the UI draws a placeholder once and does not retry. → Task 2, `a_face_crop_is_cut_only_from_the_cached_preview` and `a_face_that_is_gone_is_not_found`.
5. **Switching off with named people, then cancelling**: the switch stays on and nothing is deleted; with no named people there is no dialog. → Task 5, the `switchOffWarning` test covers the wording; the flow itself is effect wiring in `Settings.svelte`, on the smoke checklist (Task 6), and says so in its commit.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/photon-core/src/library/people.rs` | `people_to_name`; `PageFace.person_id`; `merge_people` refuses a gone `from`; doc fixes | 1 |
| `crates/photon-core/src/people/mod.rs`, `face_embed/mod.rs` | doc fixes from plan 1's re-review | 1 |
| `crates/photon-app/src/error.rs` | error kinds for the three people errors | 1 |
| `crates/photon-app/src/{commands,ipc,app}.rs` | `people_to_name` command | 1 |
| `crates/photon-core/src/library/detected_faces.rs` | `face_crop_source` | 2 |
| `crates/photon-core/src/thumbs/face_crop.rs` (new) | the crop's geometry and render | 2 |
| `crates/photon-core/src/thumbs/{mod,cache,service}.rs` | expose `face_crop` | 2 |
| `crates/photon-app/src/protocol.rs` | `/face/<id>/<key>` | 2 |
| `crates/xtask/src/screenshots.rs` | the mock server answers `/face/` | 2 |
| `ui/src/lib/api.ts` | mirrors (`personId`, `peopleToName`) | 1 |
| `ui/src/lib/people.ts` (new) | pure helpers: `nameChoice`, `switchOffWarning`, `faceUrl`, `openFacePhoto` | 3 |
| `ui/src/lib/people-page.svelte.ts` (new) | the page's behaviour: `createPeoplePage` | 3 |
| `ui/src/components/FaceStrip.svelte` (new) | one strip of faces, its selection and action bar | 4 |
| `ui/src/components/NameBox.svelte` (new) | a name field that says what committing will do | 4 |
| `ui/src/components/PeoplePage.svelte` (new) | the four sections, empty states, menus | 4 |
| `ui/src/lib/main-page.svelte.ts` (new) | grid or People page | 5 |
| `ui/src/App.svelte`, `FolderTree.svelte`, `SearchBar.svelte`, `Grid.svelte`, `Settings.svelte`, `lib/library.svelte.ts` | wiring, sidebar row, switch-off confirmation, Settings text | 5 |
| `crates/xtask/src/screenshots.rs`, `crates/xtask/screenshots/mock.js` | two People page shots | 6 |
| `CLAUDE.md`, spec, `docs/smoke-checklist.md`, `README.md` | documentation | 6 |

---

### Task 1: Backend leftovers for the page

Small backend changes the page needs, plus the residual minors from plan 1's final re-review.

**Files:**
- Modify: `crates/photon-core/src/library/people.rs` (`PageFace`, `face_row`, `merge_people`, new `people_to_name` and its SQL, tests)
- Modify: `crates/photon-core/src/people/mod.rs`, `crates/photon-core/src/face_embed/mod.rs` (doc comments only)
- Modify: `crates/photon-app/src/error.rs`, `crates/photon-app/src/commands.rs`, `crates/photon-app/src/ipc.rs`, `crates/photon-app/src/app.rs`
- Modify: `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js`
- Test: in-module tests in `people.rs`, `error.rs` (or `commands.rs`'s tests, where the other kinds are asserted)

**Interfaces:**
- Produces: `PageFace { id, item_id, thumb_key, confirmed, person_id: Option<i64> }` → TS `PageFace { id; itemId; thumbKey; confirmed; personId: number | null }`.
- Produces: `Library::people_to_name(&self) -> Result<i64>`; command `people_to_name(engine) -> CmdResult<i64>`; TS `api.peopleToName(): Promise<number>`.
- Produces: AppError kinds `emptyPersonName`, `notAPerson`, `personNamed`.

- [ ] **Step 1: Failing test — `merge_people` refuses a gone `from`**

In `people.rs`'s tests, beside `naming_a_group_that_is_gone_is_refused`:

```rust
/// A group the page shows can be emptied and deleted by a grouping run before the user
/// merges it. Answered with success, the page would think the merge happened.
#[test]
fn merging_a_group_that_is_gone_is_refused() {
    let (l, faces) = library(&[&[0.0], &[0.0]]);
    l.lib.group_ungrouped_faces(&|| false).unwrap();
    let group = person_of(&l.lib, faces[0]);
    let anna = l.lib.name_group(group, "Anna").unwrap();
    assert!(matches!(
        l.lib.merge_people(9_999, anna),
        Err(Error::NotAPerson(9_999))
    ));
}
```

Use the module's existing helpers (`library`, and whatever the module calls the lookup of a face's group — read the tests around `naming_a_group_that_is_gone_is_refused` and use the same ones; if no `person_of` exists, read `person_id` with a one-line query as the neighbouring tests do).

- [ ] **Step 2: Run it, expect FAIL** (`cargo test -p photon-core --lib merging_a_group_that_is_gone`): today `merge_people` returns `Ok(())`.

- [ ] **Step 3: Implement** — in `merge_people`, after the `from == into || !is_named(...)` check, inside the same transaction:

```rust
if !tx
    .prepare_cached("SELECT 1 FROM people WHERE id = ?1")?
    .exists(params![from])?
{
    return Err(Error::NotAPerson(from));
}
```

Update `merge_people`'s doc comment: a `from` that is gone is refused, as naming a gone group is.

- [ ] **Step 4: Failing test — `people_to_name` equals the page's unnamed section**

```rust
/// The sidebar's count is the page's Unnamed section, counted: unnamed, not ignored,
/// two or more visible faces. Built so that every rule the page applies has a group that
/// only that rule leaves out.
#[test]
fn people_to_name_counts_the_unnamed_section() {
    // Six groups, each pair of faces on two photos, the groups' angles far enough apart
    // that none joins another (the rule is cosine >= 0.50 against the group's sum, so
    // keep every two groups more than 60 degrees apart; the module's `at(deg)` uses two
    // dimensions, where only five directions fit (0, 72, 144, 216, 288), so the sixth
    // group needs a vector along a third dimension - add a helper if none exists):
    // A (2 faces, counted), B (2 faces, counted), C (1 face), D (2 faces, ignored with
    // set_person_ignored), E (2 faces, the second photo hidden), F (2 faces, named "Anna").
    let (l, faces) = library(&[/* one entry per photo, as the neighbouring tests build */]);
    l.lib.group_ungrouped_faces(&|| false).unwrap();
    // ... ignore D, hide E's second photo, name F, with the module's helpers.
    let page = l.lib.people_page(8).unwrap();
    assert_eq!(page.unnamed.len(), 2, "{page:?}");
    assert_eq!(l.lib.people_to_name().unwrap(), 2);
}
```

The exact fixture is yours to build from the module's helpers; what the test must hold: one group for each rule (two faces → counted; one face → not; ignored → not; named → not; second face hidden → not), and `people_to_name()` equal to `people_page(..).unnamed.len()` with that value asserted literally. Check each angle pair groups the way you intend (the rule is cosine ≥ 0.50 against the group's sum) before relying on it.

- [ ] **Step 5: Run it, expect FAIL** (no such function — a compile error is not the proof; the probe in Step 8 is).

- [ ] **Step 6: Implement**

```rust
/// The People page's Unnamed section, counted: what the sidebar shows as "N to name".
/// Driven from the faces in person order through `detected_faces_person`, each photo read
/// by its id; the `+` is the whole-library convention (`library/mod.rs`).
const PEOPLE_TO_NAME_SQL: &str = "SELECT count(*) FROM (
     SELECT f.person_id FROM detected_faces f
     JOIN people p ON p.id = f.person_id
     JOIN items i ON i.id = f.item_id
     WHERE p.name IS NULL AND p.ignored = 0
       AND i.hidden = 0 AND +i.missing_since IS NULL
     GROUP BY f.person_id HAVING count(*) >= 2)";

impl Library {
    /// How many unnamed groups wait for a name: the page's Unnamed section, counted.
    pub fn people_to_name(&self) -> Result<i64> {
        Ok(self.reader()?.query_row(PEOPLE_TO_NAME_SQL, [], |r| r.get(0))?)
    }
}
```

Add a plan test beside `the_page_is_driven_from_the_faces`: `lib.query_plan(PEOPLE_TO_NAME_SQL, &[])` must not contain `SCAN i` (the items table walked) and must not contain `TEMP B-TREE` (the group-by sorted rather than read in index order). Print the plan in the assertion message. If the plan SQLite picks differs from "faces in person order", pin what it does pick, explain in the test's doc comment why it is acceptable, and say so in your report.

- [ ] **Step 7: `PageFace.person_id`** — add `pub person_id: Option<i64>` to `PageFace` (doc: "The face's group, which a single face is named through"), fill it in `face_row` from column 2 (already selected). Extend one existing page test (the one that checks `single_faces`) to assert a single face's `person_id` is its group's id.

- [ ] **Step 8: Probes** — revert each change in turn (exact replacement from a saved copy, then `touch`): the `from` check (Step 1's test must fail), `HAVING count(*) >= 2` → `>= 1` and `p.ignored = 0` removed and `i.hidden = 0` removed, one at a time (Step 4's test must fail for each), and `person_id` filled with `None` (Step 7's assertion must fail). Record each red/green.

- [ ] **Step 9: Error kinds** — in `crates/photon-app/src/error.rs`'s match:

```rust
EmptyPersonName => "emptyPersonName",
NotAPerson(_) => "notAPerson",
PersonNamed(_) => "personNamed",
```

Test beside the existing kind assertions in `commands.rs`'s tests (`rename_tag(...).unwrap_err().kind` is the pattern): `name_person(&f.engine, id, "  ")` gives `"emptyPersonName"`, `merge_people(&f.engine, 9_999, 9_998)` gives `"notAPerson"`. Probe by removing the two match arms.

- [ ] **Step 10: The command** — `commands.rs`:

```rust
/// How many unnamed groups wait for a name, for the sidebar's People row.
pub fn people_to_name(engine: &Engine) -> CmdResult<i64> {
    Ok(engine.lib.people_to_name()?)
}
```

`ipc.rs`: a `#[tauri::command(async)] pub fn people_to_name(engine: State<'_, Arc<Engine>>) -> CmdResult<i64>` wrapper in the style of `face_data_summary`'s. `app.rs`: add `ipc::people_to_name` to `generate_handler!`.

- [ ] **Step 11: TS mirror and mock** — `api.ts`: `PageFace` gains `personId: number | null` (doc: "the face's group: how a single face is named"); add `peopleToName: () => invoke<number>('people_to_name'),` beside `faceDataSummary`. `mock.js`: `people_to_name: () => 2,` beside `face_data_summary`, and every face the mock's `people_page` builds gains `personId` (its group's id). Fix every `PageFace` literal in UI tests that `npm run check` reports.

- [ ] **Step 12: Doc fixes from plan 1's re-review** (no tests; say so in the commit):
  - `face_embed/mod.rs`, the eight-lane dot product's comment: say that the changed summation order can move a cosine that sits on the threshold, or a near-tie between two groups, to the other side, and that this is accepted (a tie at that precision is not a decision the measurement supports either way).
  - `people.rs`, `UNGROUPED`'s comment: it says a face is placed "by a vector the embedder made for it"; after an `EMBEDDER_VERSION` bump an ungrouped face whose vector the *previous* model made still matches until it is embedded again. Say so plainly (the face is placed by its old vector during the step; correcting it would mean comparing versions in `UNGROUPED`, which is not worth a query term for a bump that has never happened).
  - `CLAUDE.md`, the paragraph that says the People page and its sidebar count "refetch on `data_changed`": leave as is (this plan makes it true), but if any sentence there names a function this task renamed or added, update it.

- [ ] **Step 13: Gates and commit** — Rust gate, `npm run check`, `npm test`.

```bash
git add -A crates ui
git commit -m "feat(people): the sidebar's count, a face's group on the page, error kinds; merging a gone group is refused"
```

---

### Task 2: The face-crop route

**Files:**
- Create: `crates/photon-core/src/thumbs/face_crop.rs`
- Modify: `crates/photon-core/src/thumbs/mod.rs` (declare the module), `thumbs/cache.rs` (a `face_crop` method; make `encode_webp` reachable from the new module if needed), `thumbs/service.rs` (passthrough)
- Modify: `crates/photon-core/src/library/detected_faces.rs` (`face_crop_source`)
- Modify: `crates/photon-app/src/protocol.rs` (route, module doc, tests)
- Modify: `crates/xtask/src/screenshots.rs` (`respond` answers `/face/`, test)
- Modify: `ui/src/lib/people.ts` is Task 3's; this task adds nothing to the UI.

**Interfaces:**
- Produces: `photon_core::thumbs::face_crop::square(rect: &Rect, width: u32, height: u32) -> Option<(u32, u32, u32)>` (x, y, side in pixels).
- Produces: `photon_core::thumbs::face_crop::CROP_PX: u32 = 96`, `WIDEN: f64 = 0.30`.
- Produces: `ThumbCache::face_crop(&self, key: u64, rect: &Rect) -> Result<Vec<u8>>` (WebP bytes) and `ThumbService::face_crop(&self, key: u64, rect: &Rect) -> Result<Vec<u8>>`.
- Produces: `Library::face_crop_source(&self, face: i64) -> Result<Option<(Rect, u64)>>` — the face's rectangle and its photo's *current* thumbnail key.
- Produces: route `photon://localhost/face/<face id>/<thumb key>` → `image/webp`, `public, max-age=31536000, immutable`; 400 for an id or key that does not parse; 404 for a face that is gone, a key that is not the photo's current key, or a preview that is not cached.

- [ ] **Step 1: Failing tests — the geometry** (`face_crop.rs`):

```rust
//! A face's crop for the People page: a square around the face, cut from the photo's cached
//! preview. The rectangle is widened by `WIDEN` a side, so a crop shows the head and not
//! only the eyes-to-chin box YuNet draws, made square on its longer side, and kept inside
//! the picture by moving it, then by shrinking it to the picture's shorter side.

use crate::face_detect::Rect;

/// The crop's side in pixels, as served: the page draws it at 48 CSS px, twice that for a
/// high-density screen.
pub const CROP_PX: u32 = 96;
/// How much of the face's width (and height) is added on each side.
pub const WIDEN: f64 = 0.30;

/// The square to cut, as (x, y, side) in pixels of a `width` × `height` picture, or `None`
/// for a picture with no pixels or a rectangle that is not numbers.
pub fn square(rect: &Rect, width: u32, height: u32) -> Option<(u32, u32, u32)> { todo!() }

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect { left, top, right, bottom }
    }

    /// 1000 × 500, a face 200 × 100 px in the middle: widened to 320 × 160, square on 320,
    /// centred on the face.
    #[test]
    fn a_face_in_the_middle_is_widened_and_squared_on_its_longer_side() {
        assert_eq!(square(&rect(0.4, 0.4, 0.6, 0.6), 1000, 500), Some((340, 90, 320)));
    }

    /// At the left edge the square is moved in, not cut: the face stays whole, off centre.
    #[test]
    fn a_face_at_the_edge_is_moved_inside_the_picture() {
        assert_eq!(square(&rect(0.0, 0.4, 0.1, 0.6), 1000, 500), Some((0, 170, 160)));
    }

    /// Wider than the picture is tall: the square shrinks to the shorter side.
    #[test]
    fn a_face_larger_than_the_picture_is_cut_to_its_shorter_side() {
        assert_eq!(square(&rect(0.0, 0.0, 1.0, 1.0), 1000, 500), Some((250, 0, 500)));
    }

    #[test]
    fn nothing_to_cut_is_none() {
        assert_eq!(square(&rect(0.4, 0.4, 0.6, 0.6), 0, 500), None);
        assert_eq!(square(&rect(f64::NAN, 0.4, 0.6, 0.6), 1000, 500), None);
    }
}
```

Work the expected numbers through yourself before trusting them (face 0.4-0.6 of 1000 is 400-600, widened by 60 each side to 340-660, 320 wide; 0.4-0.6 of 500 is 200-300, widened by 30 to 170-330, 160 tall; side 320, centre (500, 250), so x = 340, y = 90). If a number above is wrong, the test is wrong: fix the test, and say so in the report.

- [ ] **Step 2: Run, expect FAIL** (`todo!()` panics).

- [ ] **Step 3: Implement `square`**

```rust
pub fn square(rect: &Rect, width: u32, height: u32) -> Option<(u32, u32, u32)> {
    let numbers = [rect.left, rect.top, rect.right, rect.bottom];
    if width == 0 || height == 0 || !numbers.iter().all(|n| n.is_finite()) {
        return None;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let (fw, fh) = ((rect.right - rect.left) * w, (rect.bottom - rect.top) * h);
    let (cx, cy) = ((rect.left + rect.right) / 2.0 * w, (rect.top + rect.bottom) / 2.0 * h);
    let side = (fw.max(fh) * (1.0 + 2.0 * WIDEN)).round().clamp(1.0, w.min(h));
    let x = (cx - side / 2.0).round().clamp(0.0, w - side);
    let y = (cy - side / 2.0).round().clamp(0.0, h - side);
    // Every value is inside 0..=u32::MAX by the clamps above.
    Some((x as u32, y as u32, side as u32))
}
```

(Note: the square takes the *longer widened side*: widening first and then taking the longer side is the same as this for a positive `WIDEN`.)

- [ ] **Step 4: Run, expect PASS.** Probe: drop the `.clamp(0.0, w - side)` on `x` → the edge test fails; drop `.clamp(1.0, w.min(h))` → the larger-than test fails.

- [ ] **Step 5: Failing test — the render** (`face_crop.rs` or `cache.rs` tests, wherever a temp `ThumbCache` is easiest to build — `cache.rs`'s tests already make one): write a 400 × 200 preview under some key into a temp cache, a mid-grey picture with a solid red 40 × 40 square at x 180-220, y 80-120; a face rect over that square (0.45, 0.4, 0.55, 0.6). `cache.face_crop(key, &rect)` decodes (with `webp::Decoder`) to 96 × 96 and its centre pixel is red (within a tolerance for lossy WebP: R > 180, G < 80, B < 80) while a corner pixel is grey. Name it `a_face_crop_is_cut_around_the_face_from_the_cached_preview`.

  And `a_face_crop_is_cut_only_from_the_cached_preview`: with nothing cached under the key, `face_crop` errs with an I/O `NotFound` (assert the kind: the route maps exactly that to 404).

- [ ] **Step 6: Implement `face_crop`** in `cache.rs`:

```rust
/// A face's crop, as WebP: the square `face_crop::square` gives, cut from the cached
/// preview and scaled to `CROP_PX`. Only the cache is read - never the photo, and never a
/// render: a preview that is not cached is an I/O `NotFound`, which the route answers with
/// a 404 and the page with a placeholder.
pub fn face_crop(&self, key: u64, rect: &Rect) -> Result<Vec<u8>> {
    let preview = self.read(key, ThumbSize::Preview)?.to_rgb8();
    let (x, y, side) = face_crop::square(rect, preview.width(), preview.height())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no face to crop"))?;
    let cut = image::imageops::crop_imm(&preview, x, y, side, side).to_image();
    let small = image::imageops::resize(&cut, CROP_PX, CROP_PX, image::imageops::FilterType::Triangle);
    let encoded = encode_webp(
        &webp::Encoder::from_rgb(small.as_raw(), CROP_PX, CROP_PX),
        ThumbSize::Grid.webp_method(),
    )?;
    Ok(encoded.to_vec())
}
```

Check `self.read`'s error for a missing file is the `io::Error` from `fs::read` (it is: `fs::read(...)?`), so `NotFound` survives as `Error::Io` with that kind. `ThumbService::face_crop` delegates to `self.cache.face_crop`.

- [ ] **Step 7: Run, expect PASS; probe** the crop by swapping `x`/`y` in `crop_imm` (the centre pixel stops being red on the non-square preview).

- [ ] **Step 8: Failing test — `face_crop_source`** (`detected_faces.rs` tests): a photo with one detected face; `face_crop_source(face)` is `Some((rect, key))` with `key` equal to the photo's current thumbnail key (`Item::thumb_key()` read through `lib.item(id)`); after `set_item_edit` with a quarter turn the face is gone (`update_items`/`set_item_edit` delete detections) and the answer is `None`; an unknown id is `None`.

- [ ] **Step 9: Implement**

```rust
/// A face's rectangle and its photo's current thumbnail key: what the face's crop is cut
/// from. `None` when the face is gone.
pub fn face_crop_source(&self, face: i64) -> Result<Option<(Rect, u64)>> {
    let conn = self.reader()?;
    let found = conn
        .prepare_cached(
            "SELECT f.left, f.top, f.right, f.bottom,
                    i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
             FROM detected_faces f JOIN items i ON i.id = f.item_id
             WHERE f.id = ?1",
        )?
        .query_row(params![face], |r| {
            let path: String = r.get(4)?;
            let edit = edit_from_db(r.get(7)?, r.get(8)?);
            Ok((
                Rect { left: r.get(0)?, top: r.get(1)?, right: r.get(2)?, bottom: r.get(3)? },
                edit.thumb_key(fingerprint(&path, r.get(5)?, r.get(6)?)),
            ))
        })
        .optional()?;
    Ok(found)
}
```

Use the same `edit_from_db` and `fingerprint` that `people.rs`'s `face_row` uses (import them the way it does).

- [ ] **Step 10: Failing tests — the route** (`protocol.rs` tests; follow `serves_thumbnails_with_immutable_caching` and `a_thumbnail_is_cached_forever_only_under_its_own_key` for how a fixture with a cached preview and a detected face is made — the face pass's tests in `engine.rs` write detections with `write_face_batch`; a test here may insert a `detected_faces` row directly through the library's writer if that is simpler, saying why in a comment):
  - `serves_a_face_crop_as_immutable_webp`: 200, `image/webp`, `cache-control` is the `FOREVER` string, the body decodes to 96 × 96.
  - `a_face_that_is_gone_is_not_found`: an unknown face id → 404.
  - `a_face_crop_under_another_key_is_not_found`: the right face id with a key that is not the photo's current one (e.g. `hex_key(current ^ 1)`) → 404.
  - `a_face_crop_with_no_preview_is_not_found_and_renders_nothing`: the preview file removed from the cache → 404, and the preview file is still absent afterwards (nothing rendered it).
  - `a_face_crop_url_that_does_not_parse_is_a_bad_request`: `/face/x/abc` and `/face/1/+1` → 400.

- [ ] **Step 11: Implement the route** — in `handle`'s match, before `["image", _]`:

```rust
["face", id, key] => return face(engine, id, key).await,
```

and

```rust
/// A face's crop for the People page. The face id and the photo's thumbnail key name one
/// picture (a face is deleted when its photo's picture changes, and its id is never reused),
/// so the answer is `immutable`. Only the cached preview is read: no photo file and no
/// render, so a page of crops never queues a burst of renders, and a crop that cannot be
/// cut is a 404 the page draws a placeholder for.
async fn face(engine: Arc<Engine>, id: &str, key: &str) -> Response<Vec<u8>> {
    let (Ok(id), Some(key)) = (id.parse::<i64>(), parse_key(key)) else {
        return text(StatusCode::BAD_REQUEST, "bad face");
    };
    off_thread(move || match engine.lib.face_crop_source(id) {
        Ok(Some((rect, current))) if current == key => match engine.thumbs.face_crop(key, &rect) {
            Ok(bytes) => ok(bytes, "image/webp", FOREVER),
            Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                text(StatusCode::NOT_FOUND, "not found")
            }
            Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
        Ok(_) => text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    })
    .await
    .unwrap_or_else(|response| *response)
}
```

Match the real names (`Error::Io` variant spelling, `ok`, `text`, `FOREVER`) to the file. Add the route to the module doc's list at the top of `protocol.rs`.

- [ ] **Step 12: Probes** — remove the `current == key` guard (the other-key test fails); map `NotFound` to 500 (the no-preview test fails); serve with `no-store` (the immutable test fails).

- [ ] **Step 13: The screenshot server** — in `screenshots.rs`'s `respond`, answer `/face/<id>/<key>` with `media(id, photos)` (the mock's face *n* sits on photo *n*). Extend `media_is_a_gradient_that_follows_the_item_id` with `respond("/face/12/k11", ...)` equal to `/image/12`'s answer. Probe by removing the prefix.

- [ ] **Step 14: Gates and commit**

```bash
git commit -m "feat(people): a face's crop, cut from the cached preview"
```

---

### Task 3: The page's behaviour

All the page's logic, in a pure module and a `.svelte.ts` factory, tested under vitest's node project (the factory uses `$state` and plain getters, no `$derived` and no effects, so the server runtime runs it; see CLAUDE.md "There is no component test harness").

**Files:**
- Create: `ui/src/lib/people.ts`, `ui/src/lib/people.test.ts`
- Create: `ui/src/lib/people-page.svelte.ts`, `ui/src/lib/people-page.svelte.test.ts`

**Interfaces:**
- Consumes: `api.ts` types `PeoplePage`, `PageGroup`, `PageFace` (with `personId` from Task 1), `FaceFilter`; `mediaUrl` from `url.ts`.
- Produces (`people.ts`):
  - `type NameChoice = { kind: 'empty' } | { kind: 'same' } | { kind: 'new'; name: string } | { kind: 'merge'; id: number; name: string }`
  - `nameChoice(typed: string, people: readonly { id: number; name: string | null }[], self?: number): NameChoice`
  - `switchOffWarning(named: number): string`
  - `faceUrl(faceId: number, thumbKey: string, windows?: boolean): string`
  - `interface OpenFaceDeps { offsetOf(itemId: number): Promise<number | null>; cancelSearch(): void; showAll(): Promise<void>; open(offset: number): void; notify(message: string): void }`
  - `openFacePhoto(itemId: number, deps: OpenFaceDeps): Promise<void>`
- Produces (`people-page.svelte.ts`):
  - `STRIP = 12`, `MORE = 100`
  - `type StripKey = string`; `stripKey(section: Section, id: number): StripKey`; `SINGLE: StripKey = 'single'`; `IGNORED_FACES: StripKey = 'ignored-faces'`
  - `type Section = 'unnamed' | 'suggestion' | 'person' | 'ignored'`
  - `type FaceAction = 'confirm' | 'reject' | 'ignore' | 'unignore'`
  - `interface PeoplePageDeps` (below)
  - `createPeoplePage(deps: PeoplePageDeps)` returning the object below; `type PeoplePageModel = ReturnType<typeof createPeoplePage>`

- [ ] **Step 1: Failing tests — `people.ts`** (`people.test.ts`):

```ts
import { describe, expect, it, vi } from 'vitest';
import { faceUrl, nameChoice, openFacePhoto, switchOffWarning } from './people';

const people = [
  { id: 3, name: 'Anna' },
  { id: 5, name: 'Émile' },
  { id: 9, name: null },
];

describe('nameChoice', () => {
  it('is empty for nothing but spaces', () => {
    expect(nameChoice('   ', people)).toEqual({ kind: 'empty' });
  });
  it('is a new person for a name nobody has, trimmed', () => {
    expect(nameChoice('  Ben ', people)).toEqual({ kind: 'new', name: 'Ben' });
  });
  it('merges into a person whose name matches without case', () => {
    expect(nameChoice('anna', people)).toEqual({ kind: 'merge', id: 3, name: 'Anna' });
  });
  it('compares case beyond ASCII, as the backend does in Rust', () => {
    expect(nameChoice('ÉMILE', people)).toEqual({ kind: 'merge', id: 5, name: 'Émile' });
  });
  it('is the same person when renaming to their own name in another case', () => {
    expect(nameChoice('ANNA', people, 3)).toEqual({ kind: 'same' });
  });
  it('never matches an unnamed group', () => {
    expect(nameChoice('null', people)).toEqual({ kind: 'new', name: 'null' });
  });
});

describe('switchOffWarning', () => {
  it('counts the people', () => {
    expect(switchOffWarning(1)).toBe('This deletes 1 person you named and everything photon found.');
    expect(switchOffWarning(4)).toBe('This deletes 4 people you named and everything photon found.');
  });
});

describe('faceUrl', () => {
  it('names the face and its picture', () => {
    expect(faceUrl(7, '00ab', false)).toBe('photon://localhost/face/7/00ab');
    expect(faceUrl(7, '00ab', true)).toBe('http://photon.localhost/face/7/00ab');
  });
});

describe('openFacePhoto', () => {
  const deps = (offsets: (number | null)[]) => {
    const queue = [...offsets];
    return {
      offsetOf: vi.fn(async () => queue.shift() ?? null),
      cancelSearch: vi.fn(),
      showAll: vi.fn(async () => {}),
      open: vi.fn(),
      notify: vi.fn(),
    };
  };
  it('opens the photo where the grid already has it', async () => {
    const d = deps([4]);
    await openFacePhoto(11, d);
    expect(d.open).toHaveBeenCalledWith(4);
    expect(d.showAll).not.toHaveBeenCalled();
  });
  it('switches to All photos when the current view does not hold it', async () => {
    const d = deps([null, 9]);
    await openFacePhoto(11, d);
    expect(d.cancelSearch).toHaveBeenCalled();
    expect(d.showAll).toHaveBeenCalled();
    expect(d.open).toHaveBeenCalledWith(9);
  });
  it('says so when the photo is in no view', async () => {
    const d = deps([null, null]);
    await openFacePhoto(11, d);
    expect(d.open).not.toHaveBeenCalled();
    expect(d.notify).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run, expect FAIL** (`npm test -w ui -- src/lib/people.test.ts`).

- [ ] **Step 3: Implement `people.ts`**

```ts
/** Pure pieces of the People page: what a typed name will do, the switch-off warning, a
 *  face's crop URL, and opening a face's photo. */

import { mediaUrl } from './url';

export type NameChoice =
  | { kind: 'empty' }
  | { kind: 'same' }
  | { kind: 'new'; name: string }
  | { kind: 'merge'; id: number; name: string };

/** What committing `typed` will do, by the backend's own rule (`library/people.rs`,
 *  `clean` and `same_name`): trimmed, compared without case - `toLowerCase` is Unicode's
 *  default mapping, as Rust's `to_lowercase` is, so "ÉMILE" is Émile on both sides. `self`
 *  is the person being renamed, whom their own name does not merge into. */
export function nameChoice(
  typed: string,
  people: readonly { id: number; name: string | null }[],
  self?: number,
): NameChoice {
  const name = typed.trim();
  if (!name) return { kind: 'empty' };
  const key = name.toLowerCase();
  const match = people.find((p) => p.name !== null && p.name.toLowerCase() === key);
  if (!match) return { kind: 'new', name };
  if (match.id === self) return { kind: 'same' };
  return { kind: 'merge', id: match.id, name: match.name as string };
}

/** The confirmation before switching "Find faces" off deletes the names. */
export function switchOffWarning(named: number): string {
  return `This deletes ${named} ${named === 1 ? 'person' : 'people'} you named and everything photon found.`;
}

/** A face's crop, served by `protocol.rs` from the photo's cached preview. */
export function faceUrl(faceId: number, thumbKey: string, windows?: boolean): string {
  return mediaUrl(`face/${faceId}/${thumbKey}`, windows);
}

export interface OpenFaceDeps {
  offsetOf(itemId: number): Promise<number | null>;
  cancelSearch(): void;
  showAll(): Promise<void>;
  open(offset: number): void;
  notify(message: string): void;
}

/** Opens a face's photo in the viewer: where the grid already holds it, or in All photos.
 *  The viewer addresses a photo by grid offset, so the photo has to be in the grid's view;
 *  the People page shows no hidden photo's face, so All holds every one it can show unless
 *  the photo has gone since the page was read. */
export async function openFacePhoto(itemId: number, deps: OpenFaceDeps): Promise<void> {
  let at = await deps.offsetOf(itemId);
  if (at === null) {
    deps.cancelSearch();
    await deps.showAll();
    at = await deps.offsetOf(itemId);
  }
  if (at === null) {
    deps.notify('That photo is no longer in the library.');
    return;
  }
  deps.open(at);
}
```

Check `mediaUrl`'s signature in `url.ts` (`mediaUrl(path, windows = isWindows())`) — passing `undefined` must use the default; if it does not, pass `windows ?? isWindows()`.

- [ ] **Step 4: Run, expect PASS. Probe** the `self` check, the `.trim()`, and the second `offsetOf`.

- [ ] **Step 5: Failing tests — the factory** (`people-page.svelte.test.ts`). Build a fake `deps` whose functions are `vi.fn`s returning promises the test controls (a small `deferred<T>()` helper: `{ promise, resolve, reject }`), and a `page()` builder:

```ts
const face = (id: number, personId: number | null = null, confirmed = false): PageFace =>
  ({ id, itemId: id, thumbKey: 'k' + id, confirmed, personId });
const group = (id: number, name: string | null, faces: PageFace[], faceCount = faces.length): PageGroup =>
  ({ id, name, faceCount, faces, offer: null });
```

The tests, each its own `it`:
1. `loads the sections` — after `load()`, `page` is the answer; `faces(stripKey('unnamed', 1))` is group 1's faces; `count(...)` is its `faceCount`.
2. `selects within one strip at a time` — `toggle(a, 1)`, `toggle(a, 2)` → `selected(a)` is `[1, 2]`; `toggle(b, 7)` → `selected(a)` is `[]`, `selected(b)` is `[7]`; `toggle(b, 7)` again → nothing selected.
3. `offers the actions each section allows` — `actionsFor`: unnamed `['reject','ignore']`, suggestion `['confirm','reject','ignore']`, person `['reject','ignore']`, `SINGLE` `['ignore']`, `IGNORED_FACES` `['unignore']`, ignored group `[]`.
4. `an action hides its faces at once and reloads after` — select 1 and 2 in an unnamed strip, `act(key, 'reject')`: before `deps.reject` resolves, `faces(key)` lacks 1 and 2, `count(key)` is two less, nothing is selected, `deps.reject` was called with `[1, 2]`; after it resolves, `deps.load` was called again.
5. `a reload started before an action ends does not bring its faces back` — start `act`, start a `load()` while `reject` is pending and answer it with the faces still present: they stay hidden.
6. `a reload started after it shows what the backend says` — after `act` resolves, a `load()` answered with face 1 still present shows face 1 (the backend is the truth once it has seen the write).
7. `a failed action puts its faces back and reports` — `reject` rejects: the faces are back, `deps.reportError` was called with the error.
8. `confirm all confirms the faces on screen` — a suggestion strip with `faceCount` 30 and 12 faces: `confirmAll(personId)` calls `deps.confirm` with those 12 ids, not 30; `confirmAllLabel(personId)` is `'Confirm these 12'`; with `faceCount` equal to the faces shown it is `'Confirm all'`.
9. `show more pages through the group with the section's filter` — a person strip with 12 of 40 faces: `showMore(key)` calls `deps.more(id, 'confirmed', 12, MORE)`; a suggestion strip asks `'unconfirmed'`; unnamed and ignored groups `'all'`; the answer is appended to `faces(key)`; `showFewer(key)` drops it.
10. `a reload keeps what show more loaded` — after `showMore` loaded 28 extra faces, a reload asks `deps.more(id, filter, 12, 28)` again and the strip still holds 40.
11. `naming a group hides it from Unnamed and reloads` — `nameGroup(1, ' Ben ')` calls `deps.name(1, 'Ben')`; `unnamed` (the visible list) lacks group 1 while it is pending; a `nameGroup(1, '  ')` calls nothing.
12. `a refused name reloads the page and reports the error` — `deps.name` rejects with `{ kind: 'notAPerson', message: '...' }`: group 1 is visible again, `reportError` was called, `deps.load` was called again.
13. `naming single faces names the first group and merges the rest into it` — select single faces with `personId` 21, 22, 23; `nameSingles('Ben')`: `deps.name(21, 'Ben')` resolving 21, then `deps.merge(22, 21)` and `deps.merge(23, 21)`, in that order.
14. `merge and delete ask first, and do nothing when declined` — `deps.ask` resolving `false`: `merge(3, 5)` and `remove(3)` call neither `deps.merge` nor `deps.remove`; resolving `true`, they do, then reload. The question names both people (`Merge “Anna” into “Ben”?` — assert the names appear).
15. `renaming to the same name does nothing` — `rename(3, 'ANNA')` for Anna calls nothing.
16. `a selection loses faces a reload no longer shows` — select 1 and 2; reload without face 2 → `selected(key)` is `[1]`.

- [ ] **Step 6: Run, expect FAIL.**

- [ ] **Step 7: Implement the factory**

```ts
/** The People page's behaviour, apart from its markup so it can be tested: the sections as
 *  the backend last answered, the faces "Show all" has loaded on top, the selection, and
 *  the faces and groups hidden optimistically while an action is in flight. */

import type { FaceFilter, PageFace, PageGroup, PeoplePage } from './api';
import { nameChoice, type NameChoice } from './people';
import { singleFlight } from './single-flight';

/** Faces a strip shows before "Show all". */
export const STRIP = 12;
/** Faces one "Show more" asks for. */
export const MORE = 100;

export type Section = 'unnamed' | 'suggestion' | 'person' | 'ignored';
export type StripKey = string;
export type FaceAction = 'confirm' | 'reject' | 'ignore' | 'unignore';
export const SINGLE: StripKey = 'single';
export const IGNORED_FACES: StripKey = 'ignored-faces';
export const stripKey = (section: Section, id: number): StripKey => `${section}:${id}`;

export interface PeoplePageDeps {
  load(strip: number): Promise<PeoplePage>;
  more(person: number, which: FaceFilter, offset: number, limit: number): Promise<PageFace[]>;
  name(group: number, name: string): Promise<number>;
  rename(person: number, name: string): Promise<number>;
  confirm(faces: number[]): Promise<void>;
  reject(faces: number[]): Promise<void>;
  merge(from: number, into: number): Promise<void>;
  ignoreGroup(group: number, ignored: boolean): Promise<void>;
  ignoreFaces(faces: number[], ignored: boolean): Promise<void>;
  remove(person: number): Promise<void>;
  ask(message: string, title: string): Promise<boolean>;
  reportError(e: unknown): void;
}

const FILTER: Record<Section, FaceFilter> = {
  unnamed: 'all',
  suggestion: 'unconfirmed',
  person: 'confirmed',
  ignored: 'all',
};

const ACTIONS: Record<Section | 'single' | 'ignored-faces', FaceAction[]> = {
  unnamed: ['reject', 'ignore'],
  suggestion: ['confirm', 'reject', 'ignore'],
  person: ['reject', 'ignore'],
  ignored: [],
  single: ['ignore'],
  'ignored-faces': ['unignore'],
};

export function createPeoplePage(deps: PeoplePageDeps) {
  let page = $state.raw<PeoplePage | null>(null);
  /** Faces "Show all" loaded beyond each strip's first `STRIP`. */
  let extra = $state<Record<StripKey, PageFace[]>>({});
  let selection = $state<{ strip: StripKey; ids: number[] } | null>(null);
  /** Faces and groups hidden while an action on them is in flight, and after it until a
   *  reload that started after it lands: `null` while in flight, then the tick it ended at.
   *  A reload that started earlier may have read the backend before the write, and would
   *  otherwise put back what the user just removed. */
  let hiddenFaces = $state<Map<number, number | null>>(new Map());
  let hiddenGroups = $state<Map<number, number | null>>(new Map());
  let tick = 0;

  const parse = (key: StripKey): { section: Section | 'single' | 'ignored-faces'; id: number } => {
    const [section, id] = key.split(':');
    return { section: section as Section | 'single' | 'ignored-faces', id: Number(id) };
  };

  function groupOf(key: StripKey): PageGroup | undefined {
    if (!page) return undefined;
    const { section, id } = parse(key);
    const list =
      section === 'unnamed' ? page.unnamed
      : section === 'suggestion' ? page.suggestions
      : section === 'person' ? page.people
      : section === 'ignored' ? page.ignoredGroups
      : [];
    return list.find((g) => g.id === id);
  }

  function baseFaces(key: StripKey): PageFace[] {
    if (!page) return [];
    if (key === SINGLE) return page.singleFaces;
    if (key === IGNORED_FACES) return page.ignoredFaces;
    return groupOf(key)?.faces ?? [];
  }

  const shown = (f: PageFace) => !hiddenFaces.has(f.id);

  function faces(key: StripKey): PageFace[] {
    return [...baseFaces(key), ...(extra[key] ?? [])].filter(shown);
  }

  function count(key: StripKey): number {
    const all = [...baseFaces(key), ...(extra[key] ?? [])];
    const gone = all.length - all.filter(shown).length;
    if (!page) return 0;
    if (key === SINGLE) return page.singleCount - gone;
    if (key === IGNORED_FACES) return page.ignoredFaces.length - gone;
    return (groupOf(key)?.faceCount ?? 0) - gone;
  }

  /** Drops what the reload that started at `started` has seen land, and the parts of the
   *  selection and of "Show all" that no longer exist. */
  function settle(started: number) {
    for (const map of [hiddenFaces, hiddenGroups])
      for (const [id, ended] of map) if (ended !== null && ended < started) map.delete(id);
    hiddenFaces = new Map(hiddenFaces);
    hiddenGroups = new Map(hiddenGroups);
    if (selection) {
      const present = new Set(faces(selection.strip).map((f) => f.id));
      const ids = selection.ids.filter((id) => present.has(id));
      selection = ids.length ? { strip: selection.strip, ids } : null;
    }
  }

  async function fetch(): Promise<void> {
    const started = ++tick;
    const next = await deps.load(STRIP);
    // "Show all" survives a reload: an action reloads the page, and a strip the user is
    // working through must not fold up under them.
    const kept: Record<StripKey, PageFace[]> = {};
    page = next;
    await Promise.all(
      Object.entries(extra).map(async ([key, loaded]) => {
        const group = groupOf(key);
        const { section } = parse(key);
        if (!group || !loaded.length) return;
        kept[key] = await deps.more(group.id, FILTER[section as Section], group.faces.length, loaded.length);
      }),
    );
    extra = kept;
    settle(started);
  }

  const load = singleFlight(fetch);

  function hide(map: Map<number, number | null>, ids: number[]) {
    for (const id of ids) map.set(id, null);
  }
  function ended(map: Map<number, number | null>, ids: number[]) {
    const at = ++tick;
    for (const id of ids) if (map.has(id)) map.set(id, at);
  }
  function unhide(map: Map<number, number | null>, ids: number[]) {
    for (const id of ids) map.delete(id);
  }

  /** Runs `write` with `ids` hidden from `map`; puts them back if it fails. Reloads either
   *  way: a failure can mean the page is stale (a group a grouping run deleted). */
  async function optimistic(map: Map<number, number | null>, ids: number[], write: () => Promise<unknown>) {
    hide(map, ids);
    hiddenFaces = new Map(hiddenFaces);
    hiddenGroups = new Map(hiddenGroups);
    try {
      await write();
      ended(map, ids);
    } catch (e) {
      unhide(map, ids);
      deps.reportError(e);
    }
    hiddenFaces = new Map(hiddenFaces);
    hiddenGroups = new Map(hiddenGroups);
    await load().catch(deps.reportError);
  }

  function nameOf(id: number): string {
    return page?.people.find((p) => p.id === id)?.name ?? '';
  }

  return {
    get page() { return page; },
    get unnamed() { return (page?.unnamed ?? []).filter((g) => !hiddenGroups.has(g.id)); },
    get suggestions() { return page?.suggestions ?? []; },
    get people() { return page?.people ?? []; },
    get ignoredGroups() { return (page?.ignoredGroups ?? []).filter((g) => !hiddenGroups.has(g.id)); },
    load,
    faces,
    count,
    canShowMore: (key: StripKey) => key !== SINGLE && key !== IGNORED_FACES && faces(key).length < count(key),
    isExpanded: (key: StripKey) => (extra[key]?.length ?? 0) > 0,
    async showMore(key: StripKey) {
      const group = groupOf(key);
      if (!group) return;
      const { section } = parse(key);
      const offset = group.faces.length + (extra[key]?.length ?? 0);
      try {
        const more = await deps.more(group.id, FILTER[section as Section], offset, MORE);
        extra = { ...extra, [key]: [...(extra[key] ?? []), ...more] };
      } catch (e) {
        deps.reportError(e);
      }
    },
    showFewer(key: StripKey) {
      const { [key]: _, ...rest } = extra;
      extra = rest;
    },
    selected: (key: StripKey) => (selection?.strip === key ? selection.ids : []),
    isSelected: (key: StripKey, id: number) => selection?.strip === key && selection.ids.includes(id),
    toggle(key: StripKey, id: number) {
      if (selection?.strip !== key) selection = { strip: key, ids: [id] };
      else if (selection.ids.includes(id)) {
        const ids = selection.ids.filter((x) => x !== id);
        selection = ids.length ? { strip: key, ids } : null;
      } else selection = { strip: key, ids: [...selection.ids, id] };
    },
    clearSelection() { selection = null; },
    actionsFor: (key: StripKey) => ACTIONS[parse(key).section],
    async act(key: StripKey, action: FaceAction) {
      const ids = selection?.strip === key ? selection.ids : [];
      if (!ids.length) return;
      selection = null;
      const write =
        action === 'confirm' ? () => deps.confirm(ids)
        : action === 'reject' ? () => deps.reject(ids)
        : action === 'ignore' ? () => deps.ignoreFaces(ids, true)
        : () => deps.ignoreFaces(ids, false);
      await optimistic(hiddenFaces, ids, write);
    },
    confirmAllLabel(person: number): string {
      const key = stripKey('suggestion', person);
      const n = faces(key).length;
      return n < count(key) ? `Confirm these ${n}` : 'Confirm all';
    },
    async confirmAll(person: number) {
      const ids = faces(stripKey('suggestion', person)).map((f) => f.id);
      if (ids.length) await optimistic(hiddenFaces, ids, () => deps.confirm(ids));
    },
    choice: (typed: string, self?: number): NameChoice => nameChoice(typed, page?.people ?? [], self),
    async nameGroup(group: number, typed: string) {
      const name = typed.trim();
      if (!name) return;
      await optimistic(hiddenGroups, [group], () => deps.name(group, name));
    },
    async nameSingles(typed: string) {
      const name = typed.trim();
      const chosen = faces(SINGLE).filter((f) => selection?.strip === SINGLE && selection.ids.includes(f.id));
      const groups = chosen.map((f) => f.personId).filter((g): g is number => g !== null);
      if (!name || !groups.length) return;
      selection = null;
      await optimistic(hiddenFaces, chosen.map((f) => f.id), async () => {
        const person = await deps.name(groups[0], name);
        for (const g of groups.slice(1)) await deps.merge(g, person);
      });
    },
    async rename(person: number, typed: string) {
      const choice = nameChoice(typed, page?.people ?? [], person);
      if (choice.kind === 'empty' || choice.kind === 'same') return;
      try {
        await deps.rename(person, typed.trim());
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async merge(from: number, into: number) {
      const ok = await deps.ask(
        `Merge “${nameOf(from)}” into “${nameOf(into)}”? Their faces become ${nameOf(into)}'s. This cannot be undone.`,
        'Merge people',
      );
      if (!ok) return;
      try {
        await deps.merge(from, into);
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async remove(person: number) {
      const ok = await deps.ask(
        `Delete “${nameOf(person)}”? Their faces stay together as a group with no name.`,
        'Delete person',
      );
      if (!ok) return;
      try {
        await deps.remove(person);
      } catch (e) {
        deps.reportError(e);
      }
      await load().catch(deps.reportError);
    },
    async ignoreGroup(group: number, ignored: boolean) {
      await optimistic(hiddenGroups, [group], () => deps.ignoreGroup(group, ignored));
    },
  };
}

export type PeoplePageModel = ReturnType<typeof createPeoplePage>;
```

This code is a starting point, not a transcription target: it must satisfy the tests in Step 5, `npm run check` (0 warnings — e.g. the unused `_` in `showFewer` may need `delete` on a copy instead), and the repository's style. Where a test and this code disagree, the test (and the decision it encodes, in "Decisions made in planning") wins. Note that `$state` holding a `Map` is not deeply reactive: the code reassigns a new `Map` after each change, which is what makes a component re-read it; keep that pattern, or switch to `SvelteMap` from `svelte/reactivity` and say why.

- [ ] **Step 8: Run, expect PASS.** Probes (each must turn at least one test red): `settle`'s `ended < started` → `ended <= started + 1000` (test 5 or 6); remove `unhide` in the catch (test 7); `confirmAll` using `count` ids instead of the faces shown (test 8); `FILTER.suggestion` → `'all'` (test 9); drop the reload re-fetch of `extra` (test 10); `toggle` not resetting on another strip (test 2).

- [ ] **Step 9: Gates and commit**

```bash
git commit -m "feat(people): the People page's behaviour, as a tested factory"
```

---

### Task 4: The page's markup

Three components. No unit test is possible (CLAUDE.md: no component harness); verification is `npm run check` with 0 warnings, the `no-literals`/`tokens` tests, and the screenshots in Task 6. Say so in the commit.

**Files:**
- Create: `ui/src/components/FaceStrip.svelte`, `ui/src/components/NameBox.svelte`, `ui/src/components/PeoplePage.svelte`

**Interfaces:**
- Consumes: `createPeoplePage`, `stripKey`, `SINGLE`, `IGNORED_FACES`, `PeoplePageModel`, `FaceAction` (Task 3); `faceUrl`, `nameChoice` (Task 3); `faceStatus` (`lib/status.ts`); `library` (`lib/library.svelte.ts`: `dataVersion`, `faces`, `reportError`); `api` (Task 1's `peoplePage`, `personFaces`, `namePerson`, `renamePerson`, `confirmFaces`, `rejectFaces`, `mergePeople`, `ignorePerson`, `ignoreFaces`, `deletePerson`, `faceDetection`); `ask` from `@tauri-apps/plugin-dialog`; `fitMenu` from `lib/menu-place.ts`; `Icon.svelte`.
- Produces: `<PeoplePage onopen={(itemId: number) => void} onopensettings={() => void} />` with an exported `focus(): void` (focuses the page's own container, which has `tabindex="-1"` and `class="focus-container"`).

- [ ] **Step 1: `NameBox.svelte`** — a text field with a line under it that says what committing does, from `model.choice(text, self)`:

```svelte
<script lang="ts">
  import type { NameChoice } from '../lib/people';

  let {
    choose,
    commit,
    placeholder = 'Add a name',
    label,
    initial = '',
  }: {
    /** What committing `text` would do: `PeoplePageModel.choice`, bound to the person
     *  being renamed if any. */
    choose: (text: string) => NameChoice;
    commit: (text: string) => void | Promise<void>;
    placeholder?: string;
    label: string;
    initial?: string;
  } = $props();

  let text = $state(initial);
  const choice = $derived(choose(text));

  /** Said before the name is committed, because a taken name merges (spec "The People
   *  page"): the user must see "Add to Anna" before pressing Enter. */
  const hint = $derived(
    choice.kind === 'merge' ? `Add to ${choice.name}` : choice.kind === 'new' ? `New person “${choice.name}”` : '',
  );

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (choice.kind === 'new' || choice.kind === 'merge')) {
      e.preventDefault();
      const committed = text;
      text = '';
      await commit(committed);
    } else if (e.key === 'Escape') {
      text = initial;
    }
  }
</script>

<div class="namebox">
  <input class="field" bind:value={text} {placeholder} aria-label={label} {onkeydown} />
  {#if hint}<span class="hint" class:merge={choice.kind === 'merge'}>{hint} · Enter</span>{/if}
</div>
```

Style with tokens only (`--field`, `--line`, `--text-dim`, `--accent`, `--r-2`, `--s-*`, `--t-*`), matching `FolderTree.svelte`'s `.editor` field look.

- [ ] **Step 2: `FaceStrip.svelte`** — props: `model: PeoplePageModel`, `key: StripKey`, `suggestion = false` (dashed faces), `onopen: (itemId: number) => void`. Renders:
  - a `role="group"` with `aria-label` from a `label` prop;
  - each face of `model.faces(key)` as a `<button class="face" aria-pressed={model.isSelected(key, f.id)}>` holding `<img src={faceUrl(f.id, f.thumbKey)} alt="" width="48" height="48" draggable="false" loading="lazy">`. On `error` the image is replaced by `<Icon name="user" />` (a per-face `failed` set in the component; no retry — the route answered 404 for a reason that does not pass). `onclick` → `model.toggle(key, f.id)`; `ondblclick` → `onopen(f.itemId)`; `onkeydown` Enter → `onopen(f.itemId)` (Space keeps the button's own click, which toggles). Selected faces draw the selection ring as `Tile.svelte` does (`box-shadow: inset 0 0 0 2px var(--accent), inset 0 0 0 3px var(--surface)` on `::after`), inside the face's box; a suggestion face draws a dashed `--accent` outline inside its box.
  - after the faces: `Show all {count}` when `model.canShowMore(key)` and not expanded, `Show more ({count - shown} left)` when expanded and more remain, `Show fewer` when expanded; text buttons.
  - when `model.selected(key).length > 0`, an action bar: `{n} selected`, a button per `model.actionsFor(key)` (`confirm` → "Confirm", `reject` → "Not this person", `ignore` → "Ignore these faces", `unignore` → "Stop ignoring"), each calling `model.act(key, action)`; a "Clear" button; and the hint "Double-click a face to open its photo". For `key === SINGLE` the bar also holds a `NameBox` whose `commit` is `model.nameSingles` (label "Name the selected faces").

- [ ] **Step 3: `PeoplePage.svelte`** — the page:

```svelte
<script lang="ts">
  import { ask } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { fitMenu } from '../lib/menu-place';
  import { createPeoplePage, IGNORED_FACES, SINGLE, stripKey } from '../lib/people-page.svelte';
  import { faceStatus } from '../lib/status';
  import FaceStrip from './FaceStrip.svelte';
  import Icon from './Icon.svelte';
  import NameBox from './NameBox.svelte';

  let { onopen, onopensettings }: { onopen: (itemId: number) => void; onopensettings: () => void } = $props();

  const model = createPeoplePage({
    load: (strip) => api.peoplePage(strip),
    more: (person, which, offset, limit) => api.personFaces(person, which, offset, limit),
    name: (group, name) => api.namePerson(group, name),
    rename: (person, name) => api.renamePerson(person, name),
    confirm: (faces) => api.confirmFaces(faces),
    reject: (faces) => api.rejectFaces(faces),
    merge: (from, into) => api.mergePeople(from, into),
    ignoreGroup: (group, ignored) => api.ignorePerson(group, ignored),
    ignoreFaces: (faces, ignored) => api.ignoreFaces(faces, ignored),
    remove: (person) => api.deletePerson(person),
    ask: (message, title) => ask(message, { title, kind: 'warning' }),
    reportError: library.reportError,
  });

  /** Null until read: the page must not say "switched off" before it knows. */
  let enabled = $state<boolean | null>(null);
  let root: HTMLElement | undefined = $state();
  let renaming = $state<number | null>(null);
  let mergeMenu = $state<{ x: number; y: number; from: number } | null>(null);
  const progress = $derived(faceStatus(library.faces));

  onMount(() => {
    api.faceDetection().then((on) => (enabled = on)).catch(library.reportError);
  });

  // The page refetches on every library change that carries `data_changed` (spec
  // "Loading"): the face pass's grouping, a scan, and the page's own writes all announce
  // one. `dataVersion` moves exactly then; reading it here is what subscribes.
  $effect(() => {
    void library.dataVersion;
    void model.load().catch(library.reportError);
  });

  export function focus() {
    root?.focus();
  }
</script>
```

Markup, in order (headings are `<h2>`; each section is a `<section aria-labelledby>`):
  1. A header: "People", and when `progress` is non-null its label in `.hint` (`role="status"`).
  2. `enabled === false`: a notice "photon finds and groups faces only while Find faces is on." with a button "Open Settings" → `onopensettings()`; nothing else is drawn.
  3. `enabled && model.page` with every section empty: "No faces grouped yet." plus, while `progress` is null, "photon groups faces once it has found them; this page fills in as it goes."
  4. **Unnamed** (when `model.unnamed.length || model.page.singleCount`): heading "Unnamed" with the dim note "{n} groups, largest first". For each group: a `.row` holding `FaceStrip` (`key = stripKey('unnamed', g.id)`), then a line: when `g.offer` — a primary button "Yes, this is {g.offer.name}" → `model.nameGroup(g.id, g.offer.name)` and a dim line "Offered because {g.offer.faces} of these faces are ones Picasa named {g.offer.name}."; always — a `NameBox` (`choose = (t) => model.choice(t)`, `commit = (t) => model.nameGroup(g.id, t)`, label "Name this group"), and a button "Ignore" → `model.ignoreGroup(g.id, true)`. Then, when `singleCount > 0`, a `<details>` (closed by default) whose `<summary>` reads "{singleCount} single faces" holding a `FaceStrip` with `key = SINGLE`, and when `singleCount > page.singleFaces.length` a dim line "Showing the first {n}. Name or ignore some to see the rest."
  5. **Suggestions** (when `model.suggestions.length`): heading "Suggestions" with the note "faces photon thinks are someone you named". For each: the name in bold, "{count} to check", a `FaceStrip` with `suggestion` set (`key = stripKey('suggestion', p.id)`), and a primary button with `model.confirmAllLabel(p.id)` → `model.confirmAll(p.id)`.
  6. **People** (when `model.people.length`): heading "People · {n}". For each person: the name in bold (or, while `renaming === p.id`, a `NameBox` with `initial = p.name`, `choose = (t) => model.choice(t, p.id)`, `commit = async (t) => { await model.rename(p.id, t); renaming = null; }`, label "Rename {name}"), "{faceCount} faces" (and "No faces shown" when 0), a `FaceStrip` (`key = stripKey('person', p.id)`), and buttons "Rename" → `renaming = p.id`, "Merge into…" → opens `mergeMenu` at the button (`getBoundingClientRect()` bottom-left), "Delete" (class `danger`) → `model.remove(p.id)`.
  7. **Ignored** — a `<details>` (closed by default), summary "Ignored · {groups} groups, {faces} faces": each ignored group a `FaceStrip` (`key = stripKey('ignored', g.id)`) with "Stop ignoring" → `model.ignoreGroup(g.id, false)`; then the ignored faces as a `FaceStrip` with `key = IGNORED_FACES`.
  8. The merge menu, outside the scroll container, as `FolderTree.svelte`'s menus are: `<div class="menu focus-container" role="menu" tabindex="-1" use:fitMenu={mergeMenu}>` with a `role="menuitem"` button per other named person (by name), each → `model.merge(mergeMenu.from, p.id)` then `mergeMenu = null`; Escape and a click outside close it (copy `FolderTree.svelte`'s focus-on-open `$effect` and `onMenuKeydown`).

  The root is `<div class="people focus-container" tabindex="-1" bind:this={root}>`, a scroll container filling `<main>` (`height: 100%; overflow: auto`), padded `var(--s-4)`. Rows use `--surface`/`--line` borders and `--r-3`; primary buttons use `--accent`/`--on-accent`; dim text `--text-dim`. Copy button and field styles from `Settings.svelte`/`FolderTree.svelte` rather than inventing new ones. Every colour is a token; every icon an `Icon` name from `icons.ts`.

- [ ] **Step 4: Check** — `npm run check` (0 errors, 0 warnings) and `npm test` (`no-literals.test.ts` and `tokens.test.ts` included). The page is not mounted anywhere yet; that is Task 5.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(people): the People page's markup

No component test is possible here (vitest has no component harness); the behaviour is
createPeoplePage's, tested in Task 3. Checked by svelte-check and the screenshots."
```

---

### Task 5: Wiring — the main area, the sidebar, Settings

**Files:**
- Create: `ui/src/lib/main-page.svelte.ts`
- Modify: `ui/src/App.svelte`, `ui/src/components/FolderTree.svelte`, `ui/src/components/SearchBar.svelte`, `ui/src/components/Grid.svelte`, `ui/src/components/Settings.svelte`, `ui/src/lib/library.svelte.ts`, `ui/src/lib/library.test.ts` (and any test whose API mock lacks `peopleToName`)
- Test: `ui/src/lib/library.test.ts` (the collections load the count)

**Interfaces:**
- Consumes: `PeoplePage.svelte` (Task 4), `openFacePhoto`, `switchOffWarning` (Task 3), `api.peopleToName` (Task 1).
- Produces: `mainPage` (`current: 'grid' | 'people'`, `showGrid()`, `showPeople()`); `library.toName: number`.

- [ ] **Step 1: `main-page.svelte.ts`**

```ts
/** What the main area shows: the grid, or the People page. The People page is not a grid
 *  view - it has no rows - so it is not `GridView` and does not travel the view chain; the
 *  grid keeps whatever view it had, and is unmounted while the page is up. */
export type MainPage = 'grid' | 'people';

class MainPageState {
  current = $state<MainPage>('grid');
  showGrid() {
    this.current = 'grid';
  }
  showPeople() {
    this.current = 'people';
  }
}

export const mainPage = new MainPageState();
```

No test: it holds no logic.

- [ ] **Step 2: Failing test — the count** — in `library.test.ts`, wherever `refreshCollections` is tested with a mocked `api`, add `peopleToName` to the mock (returning 4) and assert `library.toName` is 4 after `refreshCollections()`. Run: FAIL.

- [ ] **Step 3: Implement** — `library.svelte.ts`: `toName = $state(0);` beside `people`, doc "Unnamed groups of two or more faces: the sidebar's 'N to name'"; `loadCollections` fetches `api.peopleToName()` in its `Promise.all` and assigns it. Run: PASS. Probe by not assigning.

- [ ] **Step 4: `App.svelte`** —
  - import `mainPage`, `PeoplePage`, `openFacePhoto`;
  - `let peoplePage: ReturnType<typeof PeoplePage> | undefined = $state();`
  - in `<main>`: `{#if mainPage.current === 'people'}<PeoplePage bind:this={peoplePage} onopen={openFace} onopensettings={() => openSettings('people')} />{:else}<Grid ... />{/if}` (the Grid's props unchanged);
  - `openFace`:

```ts
/** A face double-clicked on the People page: its photo in the viewer, over the page. The
 *  grid behind is switched to All photos when its view does not hold the photo, as "Locate
 *  in photon" does; the page stays, so closing the viewer lands back on it. */
function openFace(itemId: number) {
  void openFacePhoto(itemId, {
    offsetOf: (id) => api.gridOffsetOfItem(id).catch(() => null),
    cancelSearch: () => searchBox.cancel(),
    showAll: () => library.setView('all'),
    open,
    notify: library.notify,
  });
}
```

  (Check `library.notify`'s name and signature; `folderDrop` uses it.)
  - `closeViewer`: after `viewerAt = null` and `library.closeViewerOn(...)`, when `mainPage.current === 'people'`, `await tick(); peoplePage?.focus();` instead of the grid's scroll and focus (make the function `async`).
  - `mainPage.showGrid()` at the start of `locate`, `showCopiesOf`, `searchFrom`, `searchFromSettings` and `jump`. Each is the user asking for the grid.

- [ ] **Step 5: `FolderTree.svelte`** —
  - `show(switchView)` calls `mainPage.showGrid()` first: every sidebar view goes through it. Grep the component for any other `library.set*View(` or `onjump(` call that does not go through `show` and give it the same.
  - Every `class:active={...}` on a view row gains `mainPage.current === 'grid' &&`, so no grid view looks selected while the People page is up (a `const onGrid = $derived(mainPage.current === 'grid')` keeps the conditions short).
  - The People group header becomes two buttons in one row: a chevron button (`aria-expanded={open.people}`, `aria-label={open.people ? 'Hide people' : 'Show people'}`) toggling `open.people`, and the label button (`class="group"`, `class:active={mainPage.current === 'people'}`) that opens the page: `searchBox.cancel(); mainPage.showPeople(); open.people = true;`. The label button shows `<Icon name="user" />`, "People", and either `<span class="count to-name" title="Groups of faces waiting for a name">{counted.format(library.toName)} to name</span>` when `library.toName > 0` or the existing count of people otherwise. The row must look like the other group rows (same height, margin and type); the chevron keeps the 12 px `.chevron` slot it has today.
  - The empty-list text becomes "No named people yet. Name the faces photon found on the People page; names Picasa recorded are listed here too."

- [ ] **Step 6: `SearchBar.svelte`** — `oninput` calls `mainPage.showGrid()` before `searchBox.run(...)`: typing a search is asking for results.

- [ ] **Step 7: `Grid.svelte`** — the Person view's empty notice: `No photos of {library.personName(library.info.person) || 'this person'}.` (a person deleted while shown left "No photos of .").

- [ ] **Step 8: `Settings.svelte`** — the switch:

```ts
/** The box shows what is stored: put back if the store fails, or if the user keeps their
 *  names. Switching off deletes the people the user named (spec "Switching off"), so when
 *  there are any it asks first; with none it switches off as before. */
async function saveFindFaces(e: Event & { currentTarget: HTMLInputElement }) {
  const field = e.currentTarget;
  const next = field.checked;
  if (!next) {
    let named: number;
    try {
      named = (await api.faceDataSummary()).namedPeople;
    } catch (err) {
      field.checked = findFaces ?? true;
      library.reportError(err);
      return;
    }
    if (named > 0 && !(await ask(switchOffWarning(named), { title: 'Stop finding faces', kind: 'warning' }))) {
      field.checked = findFaces ?? true;
      return;
    }
  }
  api
    .setFaceDetection(next)
    .then(() => (findFaces = next))
    .catch((err) => {
      field.checked = findFaces ?? false;
      library.reportError(err);
    });
}
```

  and the text under "Find faces":

```svelte
<p class="hint">
  photon looks for faces in your photos and sorts them into groups, one per person as far as it
  can tell. Name them on the People page, in the sidebar, and find them with person:, has:face or
  faces:2+; the viewer's info panel shows them. It works in the background and can take hours on a
  large library; you can quit and it carries on next time. Until it has finished, a search for
  photos without faces also finds photos it has not reached yet.
</p>
<p class="hint">
  Everything stays on this computer. Switching this off deletes what photon found and the names you
  gave; faces named in Picasa are not affected.
</p>
```

  `settings.rs`'s comment "(the UI asks first)" is now true; leave it.

- [ ] **Step 9: Gates** — `npm run check` (0/0), `npm test`, and the Rust gate is untouched but run `cargo test -p xtask` (the mock test reads `api.ts`).

- [ ] **Step 10: Commit**

```bash
git commit -m "feat(people): the People page in the main area, the sidebar's People row, a question before switching off

The wiring (the main area's switch, leaving the page from the sidebar and the search box,
focus after the viewer, the switch-off question) is effect and markup wiring with no test
seam; the count the sidebar shows is tested in library.test.ts. On the smoke checklist."
```

---

### Task 6: Screenshots and documentation

**Files:**
- Modify: `crates/xtask/src/screenshots.rs` (two `SHOTS`), `crates/xtask/screenshots/mock.js` (a richer `people_page`, a `peoplepage` action)
- Modify: `CLAUDE.md`, `docs/superpowers/specs/2026-10-03-photon-people-design.md` (As built 15-16), `docs/smoke-checklist.md`, `README.md`

- [ ] **Step 1: Mock** — `people_page` returns, using the mock's existing `face`/`group` helpers extended with `personId`: two unnamed groups (5 and 3 faces), the first with `offer: { name: 'Jonas', contact: 'b', faces: 4 }`; `singleFaces` of 3 with `singleCount: 3`; one suggestion strip for Anna (2 faces); People: Anna (6 confirmed of `faceCount` 18, so "Show all 18" is drawn) and Ben (2); one ignored group of 2. Face ids are distinct and each `itemId` equals its id. Add an action:

```js
// The People page, opened from the sidebar's People row, with a face selected so the
// action bar is drawn.
peoplepage: () => {
  click('aside .group.people');
  later(300, () => document.querySelector('.people .face')?.click());
},
```

  (Give the label button a `people` class in Task 5's markup if it has none, or select it by its text as `foldermenu` does; match whatever Task 5 produced.)

- [ ] **Step 2: Shots** — two entries in `SHOTS` after `settings-people-light`: `people-light` (`theme=light&do=peoplepage`, `dark: false`) and `people-dark` (`theme=dark&do=peoplepage`, `dark: true`). Update every "twenty-nine" for the count in `CLAUDE.md` (two places) and anywhere `screenshots.rs` states it.

- [ ] **Step 3: Run** `cargo run -p xtask -- screenshots --only people-light` and `--only people-dark` (needs Chromium; if none is on `PATH`, say so in the report and skip — not a failure). Look at both PNGs with the Read tool: four sections drawn, crops drawn (gradients), no colour obviously wrong in dark, the action bar on the selected strip, nothing clipped at the right edge. Fix what is wrong in Task 4's components and say what in the report.

- [ ] **Step 4: Documentation**
  - **CLAUDE.md** — in "Recognising people": a paragraph on the page (it is `mainPage`, not a `GridView`; the grid unmounts while it is up; what leaves it; `createPeoplePage` holds the behaviour and is tested; optimistic hiding is settled by a reload that *started* after the write; "Show all" survives a reload; the sidebar's `people_to_name` equals the Unnamed section and is fetched with the collections on `data_changed`); the face-crop route in `protocol.rs`'s list (cut only from the cached preview, 404 otherwise, never a render, `immutable` because a face id never names another picture); confirmations are the native `ask`. Keep the existing sentence about the People page refetching on `data_changed` — now true.
  - **Spec** — As built 15: the switch-off question is the native dialog, from Settings (Decision 1). As built 16: "Confirm all" confirms the faces on screen (Decision 2). Also note Decisions 4-6 in one line each under As built 17.
  - **`docs/smoke-checklist.md`** — replace or extend the "People (backend)" section with "People" end to end on a real library: switch on, wait for "Recognising people", open People from the sidebar; name a group; type an existing name in another case and see "Add to …" then the merge; Not this person; Ignore and Stop ignoring; confirm suggestions; Show all on a large group, act on a face, the strip stays open; double-click a face (viewer opens over the page; closing returns to it; arrows browse the grid's view); rename to a taken name merges; Merge into…; Delete (the group returns unnamed); the sidebar's "N to name" moves; the Person view and `person:` show only confirmed faces; switch off with named people — the question names the count, Cancel keeps everything, OK deletes; a face whose photo is hidden is nowhere on the page.
  - **`README.md`** — the features list gains people: faces found and grouped on the computer, named by the user, searchable with `person:`; nothing leaves the machine. Read the README's existing feature lines and match their voice and length.

- [ ] **Step 5: Gates** — Rust gate, `npm run check`, `npm test`.

- [ ] **Step 6: Commit**

```bash
git commit -m "docs(people): the People page in CLAUDE.md, the spec, the smoke checklist and the README; two screenshots"
```

---

## Self-review

**Spec coverage** (spec section → task):
- The People page, four sections, strips, "Show all N", name box with "Yes, this is Anna" and why, singles closed under "N single faces", Suggestions dashed with Confirm all, People with Rename / Merge into… / Delete, Ignored closed with Stop ignoring → Tasks 3-4.
- Selection and the action bar ("Not this person", "Ignore these faces", "Confirm"), double-click opens the photo → Tasks 3-5.
- "Add to Anna" before committing → Task 3 (`nameChoice`), Task 4 (`NameBox`).
- Sidebar People row with the number of unnamed groups; a person's name opens the Person view as today → Tasks 1, 5.
- Loading: sections with first faces, "Show all" pages, refetch on `data_changed` → Tasks 3-4.
- Face crops `/face/<id>/<key>`, widened 30%, clamped, 96 px, from the cached preview, `immutable`, no photo file read → Task 2.
- Logic in a `.svelte.ts` factory, markup in the component → Tasks 3-4.
- Switching off asks with the count from `face_data_summary` → Task 5 (native dialog: Decision 1, recorded in Task 6).
- IPC: all commands exist from plan 1; `people_to_name` added → Task 1.
- Documentation: CLAUDE.md, README, smoke checklist; the website is not changed by this plan (its screenshots are regenerated at release with `--photos`, per CLAUDE.md, and nothing there lists features one by one — check `site/index.html` in Task 6 and add a line only if it does).
- Testing: the page's factory (selection, action bar states, paging, create-or-merge, removal) → Task 3; the markup and the dialog by `svelte-check`, screenshots and the smoke checklist → Tasks 4-6.

**Placeholder scan:** Task 1 Step 4 and Task 2 Step 10 leave the fixture's exact construction to the implementer, naming the helpers to read and what the test must hold; every other step has its code. Task 3 Step 7's factory is complete code with an explicit precedence rule (tests win).

**Type consistency:** `PageFace.personId` (Task 1) is read by `nameSingles` (Task 3); `StripKey`/`stripKey`/`SINGLE`/`IGNORED_FACES` (Task 3) are what `FaceStrip`/`PeoplePage` use (Task 4); `PeoplePage.focus()` (Task 4) is what `App.closeViewer` calls (Task 5); `library.toName` (Task 5) is what `FolderTree` reads (Task 5); the mock's `people_to_name` (Task 1) and `peoplepage` action (Task 6) match the command and the markup.

**Review Focus:** five conditions, each with its test in the owning task, or (5) the reason it has none.
