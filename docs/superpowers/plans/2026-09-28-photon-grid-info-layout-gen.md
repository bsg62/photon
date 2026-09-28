# grid_info Layout Generation and Counts Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `grid_info` leaves the sections and folders out when the UI already holds them, and stops re-running its four counts when nothing they read has changed.

**Architecture:** The engine publishes a `layout_gen` beside the grid version, bumped only when a new index's sections or folders differ from the one it replaces. `grid_info(known_layout)` returns the layout only when the caller's generation is stale. The four library-wide counts are cached under a `counts_epoch` that data rebuilds and the hashing pass bump. The UI store keeps its `info` shape and merges answers without a layout.

**Tech Stack:** Rust (photon-app engine, Tauri IPC), Svelte 5 / TypeScript store, vitest.

**Spec:** `docs/superpowers/specs/2026-09-28-photon-grid-info-layout-gen-design.md`

## Global Constraints

- One call per grid version, as today; the layout travels in `GridInfo.layout: Option<GridLayout { gen, sections, folders }>`, `None` exactly when `known_layout == Some(layout_gen)`.
- `layout_gen` is read in the same read of `Engine.grid` as the version, the index and the failure.
- `layout_gen` changes exactly when the new index's `sections()` or `folders()` differ (`!=`) from the replaced index's; the `NOT_BUILT` grid has generation 0.
- Counts: epoch read *before* the queries; stored under that epoch; a failed count query is logged, read as 0 and the result not cached. Bumped by `data_snapshot` and by `hash_after_scan` before its duplicate-pass rebuild; not by a poster frame's rebuild nor by view/sort rebuilds.
- The TS mirror (`ui/src/lib/api.ts`) changes in the same commit as the Rust structs (CLAUDE.md: nothing validates it).
- `LibraryStore.info` keeps `sections` and `folders`; components do not change.
- Every new test is shown to fail with its rule reverted by an exact replacement (CLAUDE.md).
- Rust gate: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. UI gate: `npm run check` (0 errors AND 0 warnings), `npm test`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **A star in a view the star changes** (Starred): the star moves a photo in or out of the view, so the layout *does* change and must be sent. Pinned in Task 1 (`starring_in_the_starred_view_moves_the_layout_generation`).
2. **A reload of the webview** with the engine still running: the store starts at `layoutGen: null` and must get the layout. Pinned in Task 3 (`the first fetch asks for the layout`).
3. **A view switch whose new view happens to have identical sections** (two views over the same photos, e.g. All → Search matching everything): omitting the layout is then correct, and the store must keep sections that are right for the new view. Pinned in Task 3 (`an answer without a layout keeps the sections and takes everything else`), which checks `view` moves while sections stay.
4. **The same-version answer** `loadGrid` applies on purpose (view/counts moved without a publish): with no layout it must still take the new counts. Pinned in Task 3 by the same test (counts change, version equal).
5. **Counts after a scan purges hidden photos**: a purge is a data write through `refresh_grid`, so `hidden_count` must drop. Pinned in Task 2 (`a hide changes the hidden count on the next read`, the same bump path).

---

## File Structure

- `crates/photon-app/src/engine.rs`: `layout_gen` in the published grid and its bump in `publish`; the accessor `published()`; `Counts`, `counts_epoch`, the cache and `Engine::counts()`; bumps in `data_snapshot` and `hash_after_scan`; tests.
- `crates/photon-app/src/commands.rs`: `GridLayout`; `GridInfo` without `sections`/`folders`, with `layout`; `grid_info(engine, known_layout)`; test call sites.
- `crates/photon-app/src/ipc.rs`: the `grid_info` wrapper takes `known_layout: Option<u64>`.
- `ui/src/lib/api.ts`: `GridLayout`, `GridInfo.layout`, `gridInfo(knownLayout)`.
- `ui/src/lib/library.svelte.ts`: `GridState`, the merge in `loadGrid`, `lastPhoto`.
- `ui/src/lib/library.test.ts`: the answer literals and two new tests.
- `crates/xtask/screenshots/mock.js`: `grid_info` answers with a layout.
- `CLAUDE.md`: the counts-epoch rule beside the `data_dirty` rule.

---

### Task 1: The layout generation in the engine

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (field `grid` ~line 184, its initialiser ~394, `grid_and_failure` ~440, `publish` ~664; tests module)
- Modify: `crates/photon-app/src/commands.rs:287` (the one caller of `grid_and_failure`)

**Interfaces:**
- Produces: `pub fn published(&self) -> (u64, Arc<GridIndex>, Option<String>, u64)` — version, index, failure, layout generation, from one read. Replaces `grid_and_failure` (its only caller is `commands::grid_info`).

- [ ] **Step 1: Write the failing tests** (in `engine.rs`'s `mod tests`, beside `a_poster_frame_rebuild_does_not_announce_a_data_change`)

```rust
    /// A star moves no photo between folders in the All view, so the sidebar's folders and
    /// the grid's sections are the ones the UI already holds: the generation stays, and
    /// `grid_info` can leave them out.
    #[test]
    fn a_star_leaves_the_layout_generation_alone() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (version, _, _, layout) = f.engine.published();
        assert_ne!(layout, 0, "the first built grid has a layout of its own");

        f.engine.set_star(f.ids()[0], true).unwrap();

        let (after, _, _, same) = f.engine.published();
        assert!(after > version, "the star rebuilt the grid");
        assert_eq!(same, layout);
    }

    #[test]
    fn a_hide_moves_the_layout_generation() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (_, _, _, layout) = f.engine.published();

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();

        assert_eq!(f.engine.published().3, layout + 1);
    }

    /// Under a flat sort the grid is one run, which a file growing leaves as it was; the
    /// sidebar's folder sizes move, and the sidebar orders by them under a size sort.
    #[test]
    fn a_change_to_the_folders_alone_moves_the_layout_generation() {
        use photon_core::sort::{Sort, SortKey};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        let watched = f.add_photos();
        f.engine
            .set_sort(Sort { key: SortKey::Size, reverse: false })
            .unwrap();
        let (_, before, _, layout) = f.engine.published();

        // More bytes after the end-of-image marker: the same picture, a bigger file.
        let mut bigger = img.clone();
        bigger.resize(img.len() + 4096, 0);
        std::fs::write(f.photos.join("a").join("one.jpg"), &bigger).unwrap();
        f.engine.start_scan(watched);
        f.settle();

        let (_, after, _, moved) = f.engine.published();
        assert_eq!(after.sections(), before.sections(), "one flat run, as before");
        assert_ne!(after.folders(), before.folders());
        assert_eq!(moved, layout + 1);
    }

    /// Review Focus 1: in Starred a star moves a photo into the view.
    #[test]
    fn starring_in_the_starred_view_moves_the_layout_generation() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        f.engine.set_view(GridView::Starred).unwrap();
        let (_, _, _, layout) = f.engine.published();

        f.engine.set_star(ids[0], true).unwrap();

        assert_eq!(f.engine.published().3, layout + 1);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-app --lib layout_generation`
Expected: FAIL to compile: `no method named published`.

- [ ] **Step 3: Implement**

The field: `grid: RwLock<(u64, Arc<GridIndex>, Option<String>, u64)>,` and extend its doc comment with one line: the fourth value is the layout generation - it moves only when a publish changes the sections or folders, which is what lets `grid_info` leave them out (`commands::grid_info`).

The initialiser gains a trailing `0,` after `None,`.

Replace `grid_and_failure` with:

```rust
    /// The published grid in one read: its version, the index, why it is empty when only the
    /// first build failed (`None` for every grid actually built), and its layout generation.
    /// One read so the layout `grid_info` sends or leaves out is the version's own.
    pub fn published(&self) -> (u64, Arc<GridIndex>, Option<String>, u64) {
        let grid = self.grid.read();
        (grid.0, grid.1.clone(), grid.2.clone(), grid.3)
    }
```

In `publish`, replace the block that writes the grid with:

```rust
        let (version, len) = {
            let mut grid = self.grid.write();
            // What the UI draws the grid and the sidebar from. A star, a keyword, an edit, a
            // poster frame or a hashing pass leaves both as they were, and then `grid_info`
            // need not send them again; compared here, once per publish, rather than on every
            // `grid_info`.
            if grid.1.sections() != index.sections() || grid.1.folders() != index.folders() {
                grid.3 += 1;
            }
            grid.0 += 1;
            grid.1 = index;
            grid.2 = failure;
            (grid.0, grid.1.len())
        };
```

In `commands.rs:287`: `let (version, grid, build_error, _layout_gen) = engine.published();` (Task 3 uses it).

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-app --lib layout_generation`
Expected: PASS (4 tests). (If `SortKey`'s size variant is named differently, use it; check `photon_core::sort`.)

- [ ] **Step 5: Revert probes** (exact replacement, run, restore from a copy, `touch` the file)

- `if grid.1.sections() != index.sections() || grid.1.folders() != index.folders() {` → `if true {` — expect `a_star_leaves_the_layout_generation_alone` to fail.
- same → `if false {` — expect the hide and Starred tests to fail.
- same → `if grid.1.sections() != index.sections() {` — expect `a_change_to_the_folders_alone_moves_the_layout_generation` to fail.

- [ ] **Step 6: Rust gate, commit**

```bash
git add crates/photon-app/src/engine.rs crates/photon-app/src/commands.rs
git commit -m "perf(engine): a layout generation published beside the grid version

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The counts cache

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (struct fields, `open`'s initialiser, `data_snapshot` ~591, `hash_after_scan` ~2070; tests)
- Modify: `crates/photon-app/src/commands.rs` (`grid_info` reads `engine.counts()`)

**Interfaces:**
- Produces: `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct Counts { pub starred: usize, pub duplicate: usize, pub hidden: usize, pub video: usize }` and `pub fn counts(&self) -> Counts`.

- [ ] **Step 1: Write the failing tests** (engine.rs tests)

```rust
    #[test]
    fn a_star_changes_the_starred_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        assert_eq!(f.engine.counts().starred, 0);

        f.engine.set_star(f.ids()[0], true).unwrap();

        assert_eq!(f.engine.counts().starred, 1);
    }

    /// Review Focus 5: a hide is a data write like any other.
    #[test]
    fn a_hide_changes_the_hidden_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        f.add_photos();
        assert_eq!(f.engine.counts().hidden, 0);

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();

        assert_eq!(f.engine.counts().hidden, 1);
    }

    /// The hashing pass writes `content_hash` without a data rebuild, so it bumps the epoch
    /// itself. The counts are read (and cached) while the pass is held off, then the pass
    /// runs.
    #[test]
    fn a_duplicate_pass_changes_the_duplicate_count_on_the_next_read() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        {
            // Held, every pass the scan requests finds `hashing` taken and leaves.
            let _held = f.engine.hashing.lock();
            f.engine.add_folder(&f.photos).unwrap();
            f.engine.wait_for_scans();
            f.engine.wait_for_similar_pass();
            assert_eq!(f.engine.counts().duplicate, 0);
        }
        f.engine.request_similar_pass();
        f.engine.wait_for_similar_pass();

        assert_eq!(f.engine.counts().duplicate, 2);
    }

    /// A frame moves a thumbnail, which no count reads; its rebuild - up to one a second -
    /// must not rerun the queries.
    #[test]
    fn a_poster_frame_rebuild_does_not_recount() {
        let f = fixture(&[("a/one.jpg", &jpeg(16, 16))]);
        f.add_photos();
        f.engine.counts();
        let computed = f.engine.counts_computed.load(Ordering::SeqCst);

        f.engine.frame_stored();
        f.engine.counts();

        assert_eq!(f.engine.counts_computed.load(Ordering::SeqCst), computed);
    }
```

(If `hashing`'s lock type is not `lock()`-able from the test — check its declaration — use the same accessor the existing hashing tests use to hold it.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-app --lib _count`
Expected: FAIL to compile: `no method named counts`.

- [ ] **Step 3: Implement**

Fields beside `data_dirty`:

```rust
    /// Moves whenever something a count reads may have changed: every data rebuild
    /// (`data_snapshot`) and the duplicate pass (`hash_after_scan`). The counts are
    /// library-wide, so a view or sort switch leaves it, and so does a poster frame.
    counts_epoch: AtomicU64,
    /// The four counts `grid_info` reports, and the epoch they were read at.
    counts: Mutex<Option<(u64, Counts)>>,
    #[cfg(test)]
    counts_computed: AtomicUsize,
```

initialised `counts_epoch: AtomicU64::new(0), counts: Mutex::new(None), #[cfg(test)] counts_computed: AtomicUsize::new(0),`.

The type, near the top of the file:

```rust
/// The sidebar's library-wide counts, as `grid_info` reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub starred: usize,
    pub duplicate: usize,
    pub hidden: usize,
    pub video: usize,
}
```

The method:

```rust
    /// The four counts, from the cache when nothing they read has moved since it was filled.
    ///
    /// The epoch is read before the queries and the result stored under it: a write that
    /// lands while they run bumps past it, so the next read queries again rather than
    /// keeping an answer from before the write. A query that fails reads as 0 - the
    /// sidebar then hides that row - and the answer is not kept, so the next read tries
    /// again.
    pub fn counts(&self) -> Counts {
        let epoch = self.counts_epoch.load(Ordering::SeqCst);
        if let Some((at, counts)) = *self.counts.lock()
            && at == epoch
        {
            return counts;
        }
        #[cfg(test)]
        self.counts_computed.fetch_add(1, Ordering::SeqCst);
        let mut failed = false;
        let mut read = |what: &str, count: Result<usize>| {
            count.unwrap_or_else(|err| {
                tracing::warn!(%err, "{what} count query failed");
                failed = true;
                0
            })
        };
        let counts = Counts {
            starred: read("starred", self.lib.starred_count()),
            duplicate: read("duplicate", self.lib.duplicate_count()),
            hidden: read("hidden", self.lib.hidden_count()),
            video: read("video", self.lib.video_count()),
        };
        if !failed {
            *self.counts.lock() = Some((epoch, counts));
        }
        counts
    }
```

In `data_snapshot`, after `self.data_dirty.store(true, Ordering::SeqCst);`:

```rust
        self.counts_epoch.fetch_add(1, Ordering::SeqCst);
```

and extend its doc comment: it also moves `counts_epoch`, for the same reason and at the same point.

In `hash_after_scan`, the duplicate branch becomes:

```rust
                    Ok(_) => {
                        // `content_hash` moved, which the Duplicates count reads, and this
                        // rebuild is a derived one that does not mark the data.
                        self.counts_epoch.fetch_add(1, Ordering::SeqCst);
                        if let Err(err) = self.refresh_grid_derived() {
                            tracing::warn!(%err, "grid refresh failed");
                        }
                    }
```

In `commands::grid_info`, replace the four `*_count: engine.lib.*_count().unwrap_or_else(...)` fields with:

```rust
    let counts = engine.counts();
    // ...
        starred_count: counts.starred,
        duplicate_count: counts.duplicate,
        hidden_count: counts.hidden,
        video_count: counts.video,
```

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-app --lib _count` then `cargo test -p photon-app`
Expected: PASS; the existing `a_duplicate_pass_rebuild_does_not_announce_a_data_change` (which reads `grid_info(...).duplicate_count`) still passes.

- [ ] **Step 5: Revert probes**

- `self.counts_epoch.fetch_add(1, Ordering::SeqCst);` in `data_snapshot` → `` — expect the star and hide tests to fail.
- the same line in `hash_after_scan` → `` — expect the duplicate test to fail.
- add `self.counts_epoch.fetch_add(1, Ordering::SeqCst);` as the first line of `refresh_grid_derived` — expect the poster-frame test to fail.
- `if !failed {` → `if true {` — no test drives a failing count query; record in the commit message that the not-caching of a failed read has no test (no seam fails a count query without failing the grid queries first).

- [ ] **Step 6: Rust gate, commit**

```bash
git add crates/photon-app/src/engine.rs crates/photon-app/src/commands.rs
git commit -m "perf(engine): grid_info's counts cached until something they read moves

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `grid_info` leaves out a layout the UI holds (Rust, TS mirror, store, mock)

One commit: the IPC contract, its TS mirror and its consumer change together (CLAUDE.md).

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (`GridInfo`, new `GridLayout`, `grid_info`, test call sites)
- Modify: `crates/photon-app/src/ipc.rs:83` (`grid_info` wrapper)
- Modify: `ui/src/lib/api.ts` (`GridLayout`, `GridInfo`, `gridInfo`)
- Modify: `ui/src/lib/library.svelte.ts` (`GridState`, `info`, `loadGrid`, `lastPhoto`)
- Modify: `ui/src/lib/library.test.ts` (answer literals; two new tests)
- Modify: `crates/xtask/screenshots/mock.js` (`grid_info`)

**Interfaces:**
- Consumes: `Engine::published()` (Task 1), `Engine::counts()` (Task 2).
- Produces: `commands::grid_info(engine: &Engine, known_layout: Option<u64>) -> GridInfo`; `GridInfo.layout: Option<GridLayout>`; TS `api.gridInfo(knownLayout: number | null)`; `LibraryStore.info: GridState` with `sections`, `folders`, `layoutGen`.

- [ ] **Step 1: Rust — write the failing tests** (`commands.rs` tests)

```rust
    #[test]
    fn grid_info_leaves_out_a_layout_the_caller_holds() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let first = grid_info(&f.engine, None);
        let layout = first.layout.expect("a first call always gets the layout");
        assert_eq!(layout.sections.len(), 2);

        f.engine.set_star(f.ids()[0], true).unwrap();
        let again = grid_info(&f.engine, Some(layout.gen));

        assert!(again.version > first.version);
        assert!(again.layout.is_none(), "the star moved no photo between folders");
        assert_eq!(again.starred_count, 1);
    }

    #[test]
    fn grid_info_sends_a_layout_that_moved() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let gen = grid_info(&f.engine, None).layout.unwrap().gen;

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();
        let after = grid_info(&f.engine, Some(gen)).layout.expect("the hide moved the layout");

        assert_eq!(after.sections.len(), 1);
        assert_eq!(after.gen, gen + 1);
    }
```

Update every existing `grid_info(&f.engine)` in the crate to `grid_info(&f.engine, None)` (about 50 sites; `grep -rn "grid_info(&" crates`), and `info.sections` / `info.folders` in the existing tests to `info.layout.as_ref().unwrap().sections` / `.folders` (bind `let layout = info.layout.unwrap();` where clearer).

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-app --lib grid_info`
Expected: FAIL to compile (`grid_info` takes one argument; no field `layout`).

- [ ] **Step 3: Rust — implement**

In `commands.rs`, `GridInfo` loses `sections` and `folders` and gains, after `len`:

```rust
    /// What the grid lays out and the sidebar lists, and its generation - `None` when the
    /// caller already holds this generation (`known_layout`), which is most versions: a
    /// star, a keyword, an edit, a poster frame or a hashing pass moves no photo between
    /// folders, and at 5,000 folders the two lists are about 1 MB of JSON.
    pub layout: Option<GridLayout>,
```

New struct beside it:

```rust
/// The grid's layout at one generation: the runs it lays out (one per folder, or one
/// headerless run in a flat view) and the folders its photos come from (the sidebar's list).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridLayout {
    pub gen: u64,
    pub sections: Vec<Section>,
    pub folders: Vec<FolderTally>,
}
```

`grid_info`:

```rust
pub fn grid_info(engine: &Engine, known_layout: Option<u64>) -> GridInfo {
    let (version, grid, build_error, layout_gen) = engine.published();
    // ... (view/arg/copies_of unchanged)
    let counts = engine.counts();
    GridInfo {
        version,
        len: grid.len(),
        layout: (known_layout != Some(layout_gen)).then(|| GridLayout {
            gen: layout_gen,
            sections: grid.sections().to_vec(),
            folders: grid.folders().to_vec(),
        }),
        starred_count: counts.starred,
        // ... rest unchanged
    }
}
```

`ipc.rs`:

```rust
#[tauri::command(async)]
pub fn grid_info(engine: Eng<'_>, known_layout: Option<u64>) -> commands::GridInfo {
    commands::grid_info(&engine, known_layout)
}
```

Run: `cargo test -p photon-app` — PASS.

- [ ] **Step 4: TS mirror**

`api.ts`: add above `GridInfo`

```ts
/** Mirrors `commands::GridLayout`: the runs the grid lays out and the folders the sidebar
 *  lists, at one generation. */
export interface GridLayout {
  gen: number;
  sections: Section[];
  /** The folders the view's photos come from - the sidebar's list. */
  folders: FolderTally[];
}
```

In `GridInfo` replace `sections` and `folders` with:

```ts
  /** Null when the call named this generation as the one it holds: the store keeps its own. */
  layout: GridLayout | null;
```

and the command: `gridInfo: (knownLayout: number | null) => invoke<GridInfo>('grid_info', { knownLayout }),`

- [ ] **Step 5: UI tests — write the failing tests** (`library.test.ts`)

Convert every `GridInfo` literal: `sections: X, folders: Y,` → `layout: { gen: 1, sections: X, folders: Y },` (27 literals; where a test's sequence of answers moves the sections, give each distinct layout its own `gen`). Then add, in `describe('LibraryStore')`:

```ts
  it('the first fetch asks for the layout', async () => {
    const store = new LibraryStore();
    await store.refresh();
    expect(vi.mocked(api.gridInfo)).toHaveBeenLastCalledWith(null);
  });

  it('an answer without a layout keeps the sections and takes everything else', async () => {
    const section: Section = { folderId: 3, offset: 0, count: 2, takenAtMin: 0 };
    const folder = { folderId: 3, count: 2, takenAtMin: 0, bytes: 10, modifiedMs: 0 };
    const answer = (version: number, over: Partial<GridInfo>): GridInfo => ({
      version,
      len: 2,
      layout: null,
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date', reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
      ...over,
    });
    const store = new LibraryStore();
    vi.mocked(api.gridInfo).mockResolvedValueOnce(answer(2, { layout: { gen: 5, sections: [section], folders: [folder] } }));
    await store.refresh();
    vi.mocked(api.gridInfo).mockResolvedValueOnce(answer(2, { starredCount: 4, view: 'search', searchQuery: 'x' }));
    await store.refresh();

    expect(vi.mocked(api.gridInfo)).toHaveBeenLastCalledWith(5);
    expect(store.info.sections).toEqual([section]);
    expect(store.info.folders).toEqual([folder]);
    expect(store.info.layoutGen).toBe(5);
    expect(store.info.starredCount).toBe(4);
    expect(store.info.view).toBe('search');
  });
```

(Adapt construction to the file's existing pattern if `new LibraryStore()` needs arguments or a disposal; the `FolderTally` fields must match `api.ts`.)

Run: `npm test -w ui -- src/lib/library.test.ts` — FAIL (`layoutGen` undefined; `gridInfo` called with no argument).

- [ ] **Step 6: Store — implement** (`library.svelte.ts`)

```ts
/** `GridInfo` as the store holds it: the layout of the last answer that carried one, and
 *  its generation, which the next fetch sends so an unchanged layout is not sent again. */
export type GridState = Omit<GridInfo, 'layout'> & {
  sections: Section[];
  folders: FolderTally[];
  layoutGen: number | null;
};
```

`info = $state.raw<GridState>({ ... sections: [], folders: [], layoutGen: null, ... })` (import `Section`, `FolderTally` types).

`loadGrid`:

```ts
    const info = await api.gridInfo(this.info.layoutGen);
    // ... unchanged guard and prefetch ...
    const { layout, ...rest } = info;
    // No layout means the one sent is the one held: `loadGrid` is the only writer of
    // `info` and fetches never overlap, so what was sent is still what is held here.
    this.info = {
      ...rest,
      sections: layout?.sections ?? this.info.sections,
      folders: layout?.folders ?? this.info.folders,
      layoutGen: layout?.gen ?? this.info.layoutGen,
      copiesOf: keepCopiesName(this.info.copiesOf, info.copiesOf),
    };
```

`lastPhoto`: `const info = await api.gridInfo(this.info.layoutGen);` (it reads only `len`).

Run: `npm test -w ui -- src/lib/library.test.ts`, then `npm run check` and `npm test` — PASS, 0 warnings.

- [ ] **Step 7: The mock** (`crates/xtask/screenshots/mock.js`)

In `grid_info`, replace `sections,` and `folders: sections.map(...)` with:

```js
      // Always sent: a backend may, and the mock's layout never changes anyway.
      layout: { gen: 1, sections, folders: sections.map(({ folderId, count, takenAtMin }) => ({ folderId, count, takenAtMin, bytes: count * 4_000_000, modifiedMs: takenAtMin * 1000 })) },
```

Run: `cargo test -p xtask` and `cargo run -p xtask -- scroll-probe` (5/5 ok) and `cargo run -p xtask -- screenshots --only main-light`; Read `target/screenshots/main-light.png` to confirm the grid and sidebar still render.

- [ ] **Step 8: Revert probes**

- Rust: `layout: (known_layout != Some(layout_gen)).then(` → `layout: Some(()).map(|()|` (always send) — expect `grid_info_leaves_out_a_layout_the_caller_holds` to fail.
- Store: `sections: layout?.sections ?? this.info.sections,` → `sections: layout?.sections ?? [],` — expect `an answer without a layout keeps…` to fail.
- Store: `api.gridInfo(this.info.layoutGen)` (in `loadGrid`) → `api.gridInfo(null)` — expect the `toHaveBeenLastCalledWith(5)` assertion to fail.

- [ ] **Step 9: Both gates, commit**

```bash
git add crates/photon-app/src/commands.rs crates/photon-app/src/ipc.rs ui/src/lib/api.ts ui/src/lib/library.svelte.ts ui/src/lib/library.test.ts crates/xtask/screenshots/mock.js
git commit -m "perf(grid): grid_info leaves out a layout the UI already holds

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: CLAUDE.md, the audit plan, and the measurement

**Files:**
- Modify: `CLAUDE.md` (the refresh-chain section, after the paragraph ending "…checked against those queries. A command that has committed its write rebuilds through `refresh_after_write`…")
- Modify: `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md` (section 7's `grid_info` item)

- [ ] **Step 1: CLAUDE.md**

Add after the `refresh_after_write` sentence:

```markdown
`grid_info` sends the sections and folders only when their `layout_gen` - published with the
grid, moved only when a publish changes them - is not the one the UI names, and reads its four
counts from a cache keyed by `counts_epoch`. `data_snapshot` moves the epoch beside
`data_dirty`, and `hash_after_scan` moves it before its duplicate rebuild; a new writer that
changes something a count reads without going through `refresh_grid` must move it too, or the
sidebar shows a stale count until the next data write.
```

- [ ] **Step 2: The audit plan**

Replace the `**`grid_info` sends every section and folder on every call**` item and its three sub-bullets with one line: `- ~~**`grid_info` sends every section and folder on every call.**~~ Done: layout generation and counts cache (docs/superpowers/specs/2026-09-28-photon-grid-info-layout-gen-design.md). Deltas during a scan were left out.`

- [ ] **Step 3: The measurement** (not committed)

In a scratch Rust test or example: build a `GridIndex` with 5,000 folders of 60 photos (the benches in `crates/photon-core/benches/grid.rs` show how to build entries), then time `serde_json::to_string` of a `GridInfo` with and without `layout` (release build), and report the byte sizes. Put the numbers in the commit message below and in the PR.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md docs/superpowers/plans/2026-09-28-performance-audit-open-items.md
git commit -m "docs: grid_info's layout generation and counts epoch

<the measurement: bytes and ms with and without the layout>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
