# Show a Photo's Duplicates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Right-click one photo → "Show N duplicates" switches the grid to a Copies view holding that photo and its identical and look-alike copies.

**Architecture:** A new parameterised `GridView::Copies` whose argument (the anchor photo's id) lives in `ViewState.arg`, like `Album`. Its membership is a grid filter in `library/duplicates.rs` applied through `entries_filtered`, so it reaches both the folder-order driver and the outer `WHERE`. The menu learns whether to offer the item from a new `copy_count` command that shares the viewer info panel's own copies list, so the two cannot disagree.

**Tech Stack:** Rust (rusqlite, Tauri 2), Svelte 5 runes + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-22-photon-show-copies-design.md`

## Global Constraints

- The Rust gate before every commit: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- The UI gate before every UI commit: `npm run check` (0 errors **and** 0 warnings) and `npm test`.
- **Every new test is shown to fail with its change reverted**, by an exact replacement (not a loose `sed`), and the revert is undone before committing. A compile error is not proof. Each task names its probe.
- A Rust field added to `GridInfo` is added to `api.ts` and every `GridInfo` literal in the UI tests **in the same commit**.
- An IPC command is three files (`commands.rs`, `ipc.rs`, `app.rs`'s `generate_handler!`) plus an answer in `crates/xtask/screenshots/mock.js` once it is in `api.ts`.
- No schema change. photon never writes, moves or deletes a photo file; nothing here does.
- Comments carry the reasoning, not the mechanics.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Work on branch `feat/show-copies` (already created; the spec is its first commit).

**One deliberate departure from the spec:** the spec puts `copy_count` on `Library`. The viewer's copies list is assembled in `photon-app`'s `viewer_item` (identical first, look-alikes minus any already listed), so the count is built from *that same code*, extracted as `item_copies` in `commands.rs`. A second implementation in core is exactly the drift the spec wants to rule out.

## File Map

| File | Change |
|---|---|
| `crates/photon-core/src/grid.rs` | `GridView::Copies`; `takes_argument` includes it |
| `crates/photon-core/src/library/duplicates.rs` | `COPIES_FILTER` + tests |
| `crates/photon-core/src/library/items.rs` | `entries_for` arm |
| `crates/photon-app/src/engine.rs` | `set_copies_view` + test |
| `crates/photon-app/src/commands.rs` | `CopiesOf`, `GridInfo.copies_of`, `item_copies`, `copy_count`, `set_copies_view` + tests |
| `crates/photon-app/src/ipc.rs`, `app.rs` | two commands |
| `ui/src/lib/api.ts` | `'copies'`, `copiesOf`, `copyCount`, `setCopiesView` |
| `ui/src/lib/copies.ts` (new) + `copies.test.ts` | `showCopies`, `keepCopiesName`, `showCopiesLabel` |
| `ui/src/lib/library.svelte.ts` | default `copiesOf`, `setCopiesView`, keep the name on refresh |
| `ui/src/lib/library.test.ts` | `copiesOf: null` in both `GridInfo` literals |
| `crates/xtask/screenshots/mock.js` | `copy_count` canned, `set_copies_view` silent, `copiesOf: null` |
| `ui/src/components/Grid.svelte` | menu item, stale-answer guard, empty line |
| `ui/src/App.svelte` | `onshowcopies` wiring |
| `ui/src/components/FolderTree.svelte` | "Copies of …" sub-row |
| `README.md` | Duplicates paragraph + smoke checklist |

---

### Task 1: The Copies view in core

**Files:**
- Modify: `crates/photon-core/src/grid.rs` (enum at ~line 29, `takes_argument` at ~line 36)
- Modify: `crates/photon-core/src/library/duplicates.rs` (new const after `DUPLICATE_FILTER`, ~line 101; tests in its `mod tests`)
- Modify: `crates/photon-core/src/library/items.rs` (`entries_for` match, ~line 707)

**Interfaces:**
- Produces: `GridView::Copies` (serde `"copies"`), `GridView::takes_argument()` true for it; `Library::entries_for(GridView::Copies, "<item id>")` returns the anchor plus its live identical and look-alike copies in grid order; `pub(crate) const COPIES_FILTER: &str` binding `?1` = anchor id.

- [ ] **Step 1: Write the failing tests** in `library/duplicates.rs`'s `mod tests`:

```rust
    /// Hashes a row as the pass would: `new_item` fixes size/mtime at 100/1000, so the
    /// candidate names that fingerprint.
    fn hash(lib: &Library, id: i64, path: &str, hash: u8) {
        let candidate = HashCandidate {
            id,
            path: path.to_string(),
            size: 100,
            mtime_ms: 1_000,
        };
        assert!(lib.set_content_hash(&candidate, &[hash; 16]).unwrap());
    }

    fn copies_view(lib: &Library, anchor: i64) -> Vec<i64> {
        let mut ids: Vec<i64> = lib
            .entries_for(GridView::Copies, &anchor.to_string())
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        ids.sort();
        ids
    }

    /// The view is the photo plus exactly what its info panel lists: the same bytes, the
    /// same look-alike group, live rows only. The unhashed pair is the input that tells
    /// `=` from `IS`: two NULL hashes are "equal" under `IS`, and every unhashed photo in
    /// the library would be shown as a copy of every other.
    #[test]
    fn the_copies_view_holds_the_photo_its_twins_and_its_look_alikes() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let paths = [
            "/p/anchor.jpg",
            "/p/twin.jpg",
            "/p/alike.jpg",
            "/p/other-twin-pair-a.jpg",
            "/p/missing-twin.jpg",
            "/p/unhashed-a.jpg",
            "/p/unhashed-b.jpg",
        ];
        let ids = lib
            .insert_items(
                &paths
                    .iter()
                    .enumerate()
                    .map(|(n, p)| new_item(folder, p, n as i64))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let [anchor, twin, alike, other, missing, unhashed_a, _unhashed_b] = ids[..] else {
            unreachable!()
        };
        hash(&lib, anchor, paths[0], 1);
        hash(&lib, twin, paths[1], 1);
        hash(&lib, missing, paths[4], 1);
        // A different hash: identical to nothing here, so not a copy of the anchor.
        hash(&lib, other, paths[3], 2);
        lib.set_similar_groups(&[(anchor, anchor), (alike, anchor)])
            .unwrap();
        lib.mark_missing(&[missing], 5_000).unwrap();

        assert_eq!(copies_view(&lib, anchor), vec![anchor, twin, alike]);
        assert_eq!(
            copies_view(&lib, unhashed_a),
            vec![unhashed_a],
            "an unhashed photo is a copy of nothing, least of all every other unhashed photo"
        );
    }

    /// An argument that names no photo gives an empty grid, not an error: an error rolls the
    /// view back (`rebuild_or_restore`), and a stale id is an ordinary thing to hold.
    #[test]
    fn a_copies_argument_that_is_not_an_id_shows_nothing() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        assert!(lib.entries_for(GridView::Copies, "").unwrap().is_empty());
        assert!(lib.entries_for(GridView::Copies, "x").unwrap().is_empty());
    }

    /// The Copies analogue of `the_duplicates_view_places_a_folder_by_its_oldest_matching_photo`:
    /// the filter has to reach `folder_order`'s copy as well as the outer `WHERE`.
    #[test]
    fn the_copies_view_places_a_folder_by_its_oldest_matching_photo() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let alpha = lib
            .upsert_folder(watched, Some(root), "/p/alpha", 1)
            .unwrap();
        let zulu = lib
            .upsert_folder(watched, Some(root), "/p/zulu", 1)
            .unwrap();
        let ids = lib
            .insert_items(&[
                new_item(alpha, "/p/alpha/old-unrelated.jpg", 1),
                new_item(alpha, "/p/alpha/anchor.jpg", 9),
                new_item(zulu, "/p/zulu/alike.jpg", 5),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[1], ids[1]), (ids[2], ids[1])])
            .unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Copies, &ids[1].to_string())
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(
            folders,
            [alpha, zulu],
            "alpha's oldest *matching* photo (9) is newer than zulu's (5), so alpha leads; \
             by its oldest photo overall (1) it would trail"
        );
    }

    #[test]
    fn the_copies_view_reaches_both_halves_through_their_indexes() {
        let (_dir, lib) = temp_library();
        let plan = plan(&lib, &grid_query(GRID_COLUMNS, COPIES_FILTER), &[&1i64]);
        for index in ["items_content_hash", "items_similar_group"] {
            assert!(
                plan.iter().any(|step| step.contains(index)),
                "expected {index}, got {plan:?}"
            );
        }
    }
```

Why the folder-order expectation holds: with the filter reaching the driver, alpha's oldest *matching* photo is 9 and zulu's is 5. `GRID_ORDER` is the folder's oldest photo **descending**, so alpha (9) leads zulu (5); by alpha's oldest overall (1) it would trail. This mirrors the existing Duplicates test exactly.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-core --lib copies`
Expected: compile error (`GridView::Copies`, `COPIES_FILTER` missing). That is not the proof; the probes in Step 5 are.

- [ ] **Step 3: Implement.** In `grid.rs`, after `Duplicates`:

```rust
    /// One photo and its copies - the same bytes or the same picture - as its info panel
    /// lists them. The photo's id is the view argument.
    Copies,
```

and `takes_argument`:

```rust
        matches!(
            self,
            Self::Search | Self::Person | Self::Album | Self::Tag | Self::Copies
        )
```

In `library/duplicates.rs`, after `DUPLICATE_FILTER`:

```rust
/// One photo and its copies, as a grid filter: `?1` is the photo's id. The same two
/// relations `copies_of` and `similar_of` list for the info panel, so the view and the panel
/// cannot disagree about what a copy is.
///
/// `=` in the joins, never `IS`: a NULL hash names no group, and under `IS` every unhashed
/// photo would be a copy of every other. `UNION ALL` inside an `IN` for the reason
/// `DUPLICATE_FILTER` gives - an `OR` of the three plans as a scan of every live row.
/// Missing rows are dropped by `grid_query`'s own `missing_since IS NULL`.
pub(crate) const COPIES_FILTER: &str = "AND i.id IN (
    SELECT ?1
    UNION ALL
    SELECT o.id FROM items a JOIN items o ON o.content_hash = a.content_hash
    WHERE a.id = ?1
    UNION ALL
    SELECT o.id FROM items a JOIN items o ON o.similar_group = a.similar_group
    WHERE a.id = ?1)";
```

In `items.rs`, import it beside `DUPLICATE_FILTER` (`use super::duplicates::{COPIES_FILTER, DUPLICATE_FILTER};`) and add the arm after `Duplicates`:

```rust
            GridView::Copies => {
                // Like Album: an argument that is not an id names nothing, and an empty grid
                // says so rather than an error that would roll the view back.
                let anchor: i64 = arg.parse().unwrap_or(-1);
                self.entries_filtered(COPIES_FILTER, &[&anchor])
            }
```

If `grid_query` binds the driver's filter and the outer filter as two separate `?1` occurrences, that still works: SQLite binds every `?1` to the one parameter.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-core --lib copies`
Expected: 4 passed. If the plan test fails for one index, print the plan, and restructure only that half (for example `SELECT id FROM items WHERE content_hash = (SELECT content_hash FROM items WHERE id = ?1)`), keeping the membership tests green.

- [ ] **Step 5: Probes** — each exact replacement, test run, then revert:
  1. `ON o.content_hash = a.content_hash` → `ON o.content_hash IS a.content_hash` : `the_copies_view_holds_…` must fail (the unhashed assertion).
  2. Drop the look-alike half: replace the last three lines of `COPIES_FILTER` (`UNION ALL` / `SELECT o.id … ON o.similar_group = a.similar_group` / `WHERE a.id = ?1)`) with a lone `)`. The membership test and the folder-order test must fail.
  3. `GridView::Copies => {` arm body `self.entries_filtered(COPIES_FILTER, &[&anchor])` → `self.entries_filtered("AND i.id = ?1", &[&anchor])` : membership fails.
  4. Folder placement: this is only discriminated if the driver sees the filter. `entries_filtered` always passes it to both, so there is no revert at this layer that leaves the rest intact; record in the commit message that the test pins the route through `entries_filtered` against a future hand-rolled query.

- [ ] **Step 6: Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core
git commit -m "feat(core): a Copies view of one photo and its duplicates

<probe notes from Step 5>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

`cargo clippy` may flag a non-exhaustive `match` on `GridView` elsewhere in the workspace; any such match is a real place the new view needs an answer — give it the same one `Album` gets.

---

### Task 2: Engine, commands and IPC

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (after `set_album_view`, ~line 396; test beside `the_album_person_and_tag_views_take_their_argument_from_the_setter`, ~line 2945)
- Modify: `crates/photon-app/src/commands.rs` (`GridInfo` ~line 60, `grid_info` ~line 227, `viewer_item` ~line 490, new fns after `set_tag_view`; test beside `the_viewer_lists_identical_copies_before_look_alikes`, ~line 853)
- Modify: `crates/photon-app/src/ipc.rs` (after `set_tag_view`), `crates/photon-app/src/app.rs` (`generate_handler!`, after `ipc::set_tag_view`)

**Interfaces:**
- Consumes: `GridView::Copies` (Task 1).
- Produces: `Engine::set_copies_view(&self, item_id: i64) -> Result<()>`; `commands::copy_count(&Engine, i64) -> CmdResult<usize>`; `commands::set_copies_view(&Engine, i64) -> CmdResult<()>`; IPC `copy_count { id }` → number, `set_copies_view { id }` → void; `GridInfo.copies_of: Option<CopiesOf>` serialised as `copiesOf: { id, fileName } | null`.

- [ ] **Step 1: Write the failing tests.** In `commands.rs` tests, after `the_viewer_lists_identical_copies_before_look_alikes`:

```rust
    /// The menu's count is the info panel's list, counted: `identical.jpg` is both the same
    /// bytes and the same picture as `orig.jpg`, and is one copy, not two. Same fixture and
    /// the same reason for the second scan as the test above.
    #[test]
    fn the_copy_count_is_the_info_panels_list_counted_once() {
        use crate::testutil::jpeg_pattern;
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
            ("a/unrelated.jpg", &jpeg_pattern(64, 64)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.engine.wait_for_scans();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let id_of = |name: &str| {
            f.ids()
                .into_iter()
                .find(|&id| path_of(id).ends_with(name))
                .unwrap()
        };

        let orig = id_of("orig.jpg");
        assert_eq!(copy_count(&f.engine, orig).unwrap(), 2);
        assert_eq!(
            copy_count(&f.engine, orig).unwrap(),
            viewer_item(&f.engine, orig).unwrap().copies.len()
        );
        assert_eq!(copy_count(&f.engine, id_of("unrelated.jpg")).unwrap(), 0);
    }
```

Check first that `jpeg_pattern(64, 64)` is not a look-alike of the 180×120 pattern; if the similar pass groups it, use a solid `jpeg(64, 64)` from the same `testutil` instead, with a byte appended after the end-of-image marker if its size collides with another fixture.

In `engine.rs` tests, after `the_album_person_and_tag_views_take_their_argument_from_the_setter`:

```rust
    #[test]
    fn the_copies_view_takes_its_photo_from_the_setter_and_forgets_it_on_leaving() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img), ("a/three.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        f.engine
            .lib
            .set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();
        let name = |id: i64| {
            std::path::Path::new(&f.engine.lib.item(id).unwrap().unwrap().path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };

        f.engine.set_copies_view(ids[0]).unwrap();
        let info = crate::commands::grid_info(&f.engine);
        assert_eq!((info.view, info.len), (GridView::Copies, 2));
        let copies_of = info.copies_of.expect("reported while the view is open");
        assert_eq!((copies_of.id, copies_of.file_name), (ids[0], name(ids[0])));
        assert_eq!(info.album, None, "the argument is a photo id, not an album id");

        f.engine.set_view(GridView::All).unwrap();
        assert!(crate::commands::grid_info(&f.engine).copies_of.is_none());
        // Re-entering without the setter must not bring the old photo back.
        f.engine.set_view(GridView::Copies).unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
    }
```

`set_similar_groups` is `pub` on `Library` (`library/similar.rs:143`); if the engine's similar pass runs after `add_photos` and regroups, call `f.engine.wait_for_scans()` before setting the groups, and set them after, as above.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-app copies` and `cargo test -p photon-app copy_count`
Expected: compile errors (missing `set_copies_view`, `copy_count`, `copies_of`).

- [ ] **Step 3: Implement.** `engine.rs`, after `set_album_view`:

```rust
    /// Shows one photo and its copies. Rolls back on a failed refresh.
    pub fn set_copies_view(&self, item_id: i64) -> Result<()> {
        self.rebuild_or_restore(|state| {
            state.arg = item_id.to_string();
            state.view = GridView::Copies;
        })
    }
```

`commands.rs` — beside `GridInfo`:

```rust
/// The photo a Copies view is of. The name travels with the id because the sidebar labels
/// the view by it, and asking again per render would be a round trip for a constant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopiesOf {
    pub id: i64,
    /// Empty when the photo has left the library since the view opened; the UI keeps the
    /// name it already had.
    pub file_name: String,
}
```

and in `GridInfo` after `tag`:

```rust
    /// The photo while `view` is `Copies`.
    pub copies_of: Option<CopiesOf>,
```

In `grid_info`, before building the struct (the `tag` field consumes `arg`, so compute this first):

```rust
    let copies_of = (view == GridView::Copies)
        .then(|| arg.parse::<i64>().ok())
        .flatten()
        .map(|id| CopiesOf {
            id,
            file_name: engine
                .lib
                .item(id)
                .ok()
                .flatten()
                .and_then(|item| {
                    Path::new(&item.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .unwrap_or_default(),
        });
```

and `copies_of,` as the last field.

Extract the copies list out of `viewer_item` into a function both use — replace the `let identical = …; let identical_ids …; let copies = identical…collect();` block in `viewer_item` with `let copies = item_copies(engine, item.id)?;`, and add:

```rust
/// A photo's copies as its info panel lists them: byte-identical first, then look-alikes
/// that are not already listed. The menu's count is this list's length, so the two cannot
/// disagree about what a copy is.
fn item_copies(engine: &Engine, id: i64) -> CmdResult<Vec<ItemCopy>> {
    let identical = engine.lib.copies_of(id)?;
    let identical_ids: std::collections::HashSet<i64> = identical.iter().map(|c| c.id).collect();
    Ok(identical
        .into_iter()
        .map(|c| ItemCopy {
            id: c.id,
            path: c.path,
            kind: CopyKind::Identical,
            width: c.width,
            height: c.height,
        })
        // A look-alike that is also a byte-identical twin is already listed above; a
        // photo appearing twice in the info panel is a bug the user sees, not a detail.
        .chain(
            engine
                .lib
                .similar_of(id)?
                .into_iter()
                .filter(|c| !identical_ids.contains(&c.id))
                .map(|c| ItemCopy {
                    id: c.id,
                    path: c.path,
                    kind: CopyKind::Similar,
                    width: c.width,
                    height: c.height,
                }),
        )
        .collect())
}

/// How many copies the tile menu may offer to show; 0 hides the item.
pub fn copy_count(engine: &Engine, id: i64) -> CmdResult<usize> {
    Ok(item_copies(engine, id)?.len())
}

pub fn set_copies_view(engine: &Engine, id: i64) -> CmdResult<()> {
    engine.set_copies_view(id)?;
    Ok(())
}
```

`ipc.rs`, after `set_tag_view`:

```rust
#[tauri::command(async)]
pub fn copy_count(engine: Eng<'_>, id: i64) -> Result<usize, AppError> {
    commands::copy_count(&engine, id)
}

#[tauri::command(async)]
pub fn set_copies_view(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    commands::set_copies_view(&engine, id)
}
```

`app.rs`, in `generate_handler!` after `ipc::set_tag_view,`: `ipc::copy_count, ipc::set_copies_view,`. Forgetting this compiles and fails only at runtime.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-app copies` and `cargo test -p photon-app copy_count`
Expected: both pass; `the_viewer_lists_identical_copies_before_look_alikes` still passes.

- [ ] **Step 5: Probes** — exact replacement, run, revert:
  1. In `copy_count`, `Ok(item_copies(engine, id)?.len())` → `Ok(engine.lib.copies_of(id)?.len() + engine.lib.similar_of(id)?.len())` : `the_copy_count_is_…` must fail with 3 ≠ 2. If it *passes*, `identical.jpg` was not grouped as a look-alike of `orig.jpg` in the fixture and the test does not discriminate; fix the fixture rather than accepting it.
  2. In `set_copies_view`, `state.view = GridView::Copies;` → `state.view = GridView::Album;` : the engine test must fail.
  3. In `grid_info`, `(view == GridView::Copies)` → `(true)` : the engine test's `copies_of.is_none()` after leaving must fail.

- [ ] **Step 6: Rust gate, then commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-app
git commit -m "feat(app): copy_count and set_copies_view, and GridInfo.copiesOf

item_copies is the viewer's copies list, extracted, so the menu's count is
that list's length and cannot drift from it.

<probe notes>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

`cargo test --workspace` does not yet see the commands in `api.ts`, so `screenshots.rs` stays green until Task 3 adds them — which is why the mock answers belong to Task 3.

---

### Task 3: The TypeScript mirror, the store and `showCopies`

**Files:**
- Modify: `ui/src/lib/api.ts` (`GridView` line 21, `GridInfo` ~line 29, the `api` object near `setTagView`)
- Create: `ui/src/lib/copies.ts`, `ui/src/lib/copies.test.ts`
- Modify: `ui/src/lib/library.svelte.ts` (default `info` ~line 31, `refresh` ~line 412, after `setTagView` ~line 545)
- Modify: `ui/src/lib/library.test.ts` (the `GridInfo` literals at ~lines 86 and 331)
- Modify: `crates/xtask/screenshots/mock.js` (`canned` ~line 86, `grid_info`, `SILENT` ~line 151)

**Interfaces:**
- Consumes: IPC `copy_count { id }`, `set_copies_view { id }`, `GridInfo.copiesOf` (Task 2).
- Produces: `api.copyCount(id: number): Promise<number>`, `api.setCopiesView(id: number): Promise<void>`; `type GridView` includes `'copies'`; `GridInfo.copiesOf: CopiesOf | null` with `interface CopiesOf { id: number; fileName: string }`; `library.setCopiesView(id: number): Promise<void>`; from `copies.ts`: `showCopies(itemId, deps)`, `keepCopiesName(prev, next)`, `showCopiesLabel(n)`.

- [ ] **Step 1: Write the failing tests** — `ui/src/lib/copies.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { keepCopiesName, showCopies, showCopiesLabel } from './copies';

describe('showCopies', () => {
  function spyDeps(at: number | null = 3) {
    const order: string[] = [];
    return {
      order,
      deps: {
        cancelSearch: () => order.push('cancel'),
        setCopiesView: (id: number) => {
          order.push(`view:${id}`);
          return Promise.resolve();
        },
        offsetOfItem: (id: number) => {
          order.push(`find:${id}`);
          return Promise.resolve(at);
        },
        select: (offset: number, itemId: number) => order.push(`select:${offset}:${itemId}`),
      },
    };
  }

  it('switches to the view before looking the photo up, so the offset is against its index', async () => {
    const { order, deps } = spyDeps();
    await showCopies(42, deps);
    expect(order).toEqual(['cancel', 'view:42', 'find:42', 'select:3:42']);
  });

  it('selects nothing when the view does not hold the photo', async () => {
    const { order, deps } = spyDeps(null);
    await showCopies(42, deps);
    expect(order).toEqual(['cancel', 'view:42', 'find:42']);
  });
});

describe('keepCopiesName', () => {
  it('keeps the name it had when the photo has since left the library', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 7, fileName: '' })).toEqual({ id: 7, fileName: 'a.jpg' });
  });
  it('does not carry one photo’s name onto another', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 8, fileName: '' })).toEqual({ id: 8, fileName: '' });
  });
  it('takes a fresh name, and leaves no view as no view', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 7, fileName: 'b.jpg' })).toEqual({ id: 7, fileName: 'b.jpg' });
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, null)).toBeNull();
  });
});

describe('showCopiesLabel', () => {
  it('counts in the singular and the plural', () => {
    expect(showCopiesLabel(1)).toBe('Show 1 duplicate');
    expect(showCopiesLabel(3)).toBe('Show 3 duplicates');
    expect(showCopiesLabel(1200)).toBe(`Show ${(1200).toLocaleString()} duplicates`);
  });
});
```

- [ ] **Step 2: Run to see them fail**

Run: `npm test -w ui -- src/lib/copies.test.ts`
Expected: FAIL — cannot resolve `./copies`.

- [ ] **Step 3: Implement.** `api.ts`:

```ts
export type GridView = 'all' | 'starred' | 'recent' | 'search' | 'person' | 'album' | 'tag' | 'duplicates' | 'copies';
/** Mirrors `commands::CopiesOf`. `fileName` is empty once the photo has left the library. */
export interface CopiesOf { id: number; fileName: string }
```

Extend the `GridInfo` doc comment's list with `copiesOf`, and add after `tag`:

```ts
  /** The photo while `view` is 'copies'. */
  copiesOf: CopiesOf | null;
```

In the `api` object, after `setTagView`:

```ts
  copyCount: (id: number) => invoke<number>('copy_count', { id }),
  setCopiesView: (id: number) => invoke<void>('set_copies_view', { id }),
```

`ui/src/lib/copies.ts`:

```ts
import type { CopiesOf } from './api';

/** "Show duplicates" from the tile menu: lands the grid on the Copies view with the photo
 *  selected. The same order as `locateItem`, for the same reason: an offset only means
 *  something against the index it was looked up in, so the view switches first. */
export async function showCopies(
  itemId: number,
  deps: {
    cancelSearch: () => void;
    setCopiesView: (itemId: number) => Promise<void>;
    offsetOfItem: (itemId: number) => Promise<number | null>;
    select: (offset: number, itemId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  await deps.setCopiesView(itemId);
  const at = await deps.offsetOfItem(itemId);
  if (at === null) return;
  deps.select(at, itemId);
}

/** The backend reports an empty name once the photo has been purged; the sidebar row keeps
 *  the name it was opened with rather than going blank under the user. */
export function keepCopiesName(prev: CopiesOf | null, next: CopiesOf | null): CopiesOf | null {
  if (next && next.fileName === '' && prev && prev.id === next.id) return { ...next, fileName: prev.fileName };
  return next;
}

export function showCopiesLabel(n: number): string {
  return n === 1 ? 'Show 1 duplicate' : `Show ${n.toLocaleString()} duplicates`;
}
```

`library.svelte.ts`: add `copiesOf: null,` to the default `info` after `tag: null,`; import `keepCopiesName` from `./copies`; in `refresh` replace `this.info = info;` with

```ts
    this.info = { ...info, copiesOf: keepCopiesName(this.info.copiesOf, info.copiesOf) };
```

and after `setTagView`:

```ts
  /** Shows one photo and its copies. */
  async setCopiesView(itemId: number): Promise<void> {
    await this.switchView(() => api.setCopiesView(itemId));
  }
```

`library.test.ts`: add `copiesOf: null,` after `tag: null,` in both `GridInfo` literals.

`mock.js`: in `canned` add `    copy_count: () => 2,` (four spaces' indent, which the screenshots test parses), add `copiesOf: null,` after `tag: null,` in `grid_info`, and add `'set_copies_view'` to `SILENT`.

- [ ] **Step 4: Run to see them pass, and the gates**

Run: `npm test -w ui -- src/lib/copies.test.ts`, then `npm run check && npm test`, then `cargo test -p xtask`.
Expected: all green; svelte-check 0 errors 0 warnings.

- [ ] **Step 5: Probes** — exact replacement, run, revert:
  1. In `showCopies`, move `await deps.setCopiesView(itemId);` below the `offsetOfItem` line (lookup first): the first test must fail.
  2. In `keepCopiesName`, drop `&& prev.id === next.id`: the "does not carry" test must fail.
  3. Remove `copy_count` from `mock.js`'s `canned`: `cargo test -p xtask` must fail.

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib crates/xtask/screenshots/mock.js
git commit -m "feat(ui): mirror the Copies view and add showCopies

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The menu item, the sidebar row, the empty line, the docs

**Files:**
- Modify: `ui/src/components/Grid.svelte` (props ~line 12–21, menu state ~line 283, `tileMenu` ~line 489, `closeMenu` ~line 497, empty text ~line 572, menu markup ~line 641)
- Modify: `ui/src/App.svelte` (import, a `showCopiesOf` beside `locate` ~line 134, `<Grid …>` line 291)
- Modify: `ui/src/components/FolderTree.svelte` (Duplicates row ~line 246, `.active` style ~line 490)
- Modify: `README.md` (Duplicates paragraph ~line 145, smoke checklist after line 245)

**Interfaces:**
- Consumes: `api.copyCount`, `library.setCopiesView`, `showCopies`, `showCopiesLabel`, `GridInfo.copiesOf` (Task 3).
- Produces: `Grid` prop `onshowcopies: (id: number) => void`.

No unit test is possible here: vitest runs in `node` and cannot render a `.svelte` file. The logic worth testing is already in `copies.ts` (Task 3); what remains is effect wiring, verified by `svelte-check`, the screenshots and the smoke checklist. Say so in the commit message.

- [ ] **Step 1: Grid.svelte — the prop.** Add `onshowcopies` to the props destructuring and its type, beside `oncompare`:

```ts
    /** "Show duplicates" on one photo. App owns the view switch and the selection after it. */
    onshowcopies: (id: number) => void;
```

- [ ] **Step 2: Grid.svelte — the lookup.** Beside `let menu`:

```ts
  /** The copy count of the one photo the menu was opened on, once the backend answers.
   *  `menuSeq` makes a late answer harmless: one for a menu since closed, or reopened on a
   *  different photo, would otherwise offer that photo's copies under this one. */
  let menuCopies = $state<{ id: number; count: number } | null>(null);
  let menuSeq = 0;
```

In `tileMenu`, after `menu = { x: e.clientX, y: e.clientY };`:

```ts
    const seq = ++menuSeq;
    menuCopies = null;
    if (library.selectionCount === 1) {
      const id = entry.id;
      api
        .copyCount(id)
        .then((count) => {
          if (seq === menuSeq && menu) menuCopies = { id, count };
        })
        // No item is the right answer to a failed lookup: the menu's other verbs still work.
        .catch(() => {});
    }
```

In `closeMenu`, and everywhere else the file sets `menu = null` for good, the stale answer is already harmless because `menuSeq` only moves forward on a new menu; `menuCopies` is reset on the next open. Nothing more is needed there.

- [ ] **Step 3: Grid.svelte — the item.** After the `{#if count === 1}` Reveal block:

```svelte
    {#if count === 1 && menuCopies && menuCopies.count > 0}
      {@const id = menuCopies.id}
      <button
        role="menuitem"
        onclick={() => {
          menu = null;
          onshowcopies(id);
        }}>{showCopiesLabel(menuCopies.count)}</button
      >
    {/if}
```

and `import { showCopiesLabel } from '../lib/copies';`.

- [ ] **Step 4: Grid.svelte — the empty and lone-photo line.** In the `{#if library.info.len === 0}` block, before `{:else if library.info.view === 'tag'}`:

```svelte
        {:else if library.info.view === 'copies'}
          No other copies of {library.info.copiesOf?.fileName || 'this photo'} any more.
```

and for the case where only the photo itself is left, after that `{/if}` block's closing `</p>{/if}`:

```svelte
    {#if library.info.view === 'copies' && library.info.len === 1}
      <!-- The group has shrunk to the photo itself since the view opened (a copy was deleted
           and the rescan purged it). The photo stays on screen; this says why it is alone. -->
      <p class="empty">No other copies of {library.info.copiesOf?.fileName || 'this photo'} any more.</p>
    {/if}
```

Check with `cargo run -p xtask -- screenshots` whether `.empty` overlaps the lone tile; if it does, give it a class that places it below the canvas (reuse existing spacing tokens only; `no-literals.test.ts` fails on a colour literal).

- [ ] **Step 5: App.svelte.** `import { showCopies } from './lib/copies';` and, after `locate`:

```ts
  /** "Show duplicates" from the tile menu; the switch-then-lookup order is `showCopies`'s. */
  async function showCopiesOf(itemId: number) {
    await showCopies(itemId, {
      cancelSearch: () => searchBox.cancel(),
      setCopiesView: (id) => library.setCopiesView(id),
      offsetOfItem: (id) => api.gridOffsetOfItem(id).catch(() => null),
      select: (offset, id) => {
        library.selectItem(offset, id);
        grid?.scrollToOffset(offset, 'nearest');
        grid?.focus();
      },
    }).catch(library.reportError);
  }
```

and `onshowcopies={showCopiesOf}` on `<Grid …>`.

- [ ] **Step 6: FolderTree.svelte.** Change the Duplicates condition to `library.info.duplicateCount > 0 || library.info.view === 'duplicates' || library.info.view === 'copies'`, and after its `</button>` (inside the `{#if}`):

```svelte
    {#if library.info.view === 'copies'}
      <!-- Not a saved place: it exists while the view is open, and leaving removes it. -->
      <button class="root copies active" aria-current="true" title={library.info.copiesOf?.fileName}>
        <span class="name">Copies of {library.info.copiesOf?.fileName || 'a photo'}</span>
      </button>
    {/if}
```

Indent it the way `.menu .album` indents (a left padding from the spacing already used for nested rows in this file; read the `.node` depth padding and match it), and add `.copies.active` to the existing `.active` rule at ~line 490.

- [ ] **Step 7: README.** In `### Duplicates`, after "clicking one locates it.": `Right-click a photo that has copies and choose **Show N duplicates** to see just that photo and its copies in the grid.` Add to the smoke checklist after the look-alike line:

```markdown
- [ ] Right-click a photo with copies: "Show N duplicates" appears with the right count, and does not appear for a photo without copies or for a multi-photo selection. Choose it: the grid holds the photo and its copies, the clicked photo is selected, and the sidebar shows "Copies of <name>" under Duplicates. Right-click one photo then quickly another: the item names the second one's count. Delete a copy on disk and wait for the rescan: the group shrinks, and with none left the grid says "No other copies of <name> any more."
```

- [ ] **Step 8: Gates and a look.** Run `npm run check && npm test`, the full Rust gate, and `cargo run -p xtask -- screenshots --no-build` after a build (`cargo run -p xtask -- screenshots`) to confirm nothing else moved. Do **not** run `npm run dev`.

- [ ] **Step 9: Commit**

```bash
git add ui/src/components ui/src/App.svelte README.md
git commit -m "feat(ui): Show N duplicates in the tile menu

The menu lookup, its stale-answer guard, the sidebar row and the lone-photo
line are effect wiring: vitest runs in node and cannot render a component, so
they are covered by svelte-check and the smoke checklist, not a test. The
logic they call is tested in copies.test.ts.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Whole-branch review and PR

- [ ] **Step 1:** Per CLAUDE.md, an independent read of the whole branch before it merges. Point the reviewer at the effect wiring (the menu's `menuSeq`, the refresh's `keepCopiesName`) and at what the new view *arms* in old code: every `match` / `===` over `GridView`, `viewKey` in `search.ts` (a Copies view must not be saveable as a search), `albums_changed`, the rename-tag view carry, and `locateItem` (which leaves any non-All view for All — right for Copies too).
- [ ] **Step 2:** Push with gh's credential helper and open the PR; wait with `gh pr checks N --watch` only after checks exist.
