# photon Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Typing in a sidebar box shows the photos whose file name or folder name contains the query, as a third grid view.

**Architecture:** `GridView` gains a `Search` variant; the query string lives beside it on `Engine`. One `Library::entries_for(view, query)` answers all three views, mapping rows through a single shared function. Search matching happens in Rust, not SQL, so case folding is Unicode-aware and `%`/`_` are literal.

**Tech Stack:** Rust (rusqlite, Tauri 2.11), Svelte 5 runes, TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-13-photon-search-design.md`

## Global Constraints

- **photon never writes to, moves or deletes files inside watched folders.** This feature performs no file I/O at all.
- **No new native library dependencies.** Nothing wrapping a C/C++ SDK, and no SQLite ICU extension. This is why matching is done in Rust (spec §3).
- **No schema change.** No column, no index, no migration. `user_version` stays at 2.
- **Never launch the GUI.** No agent on this project runs the app; UI verification is `svelte-check` plus unit tests, and anything needing eyes goes on the README's manual checklist.
- **The Rust gate is four commands, all of which must pass before any commit:**
  `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace` · `cargo bench -p photon-core --bench grid --no-run`
- **The UI gate:** `npm run check` in `ui/` must report **0 errors and 0 warnings** (it runs with `--fail-on-warnings`), plus `npm test`.
- **Every new test must be demonstrated to fail with its change reverted.** Revert the change, run the test, paste the failure, restore. A test that passes both ways proves nothing and must be replaced with one that discriminates — say so in your report rather than presenting it as proof.
- **TypeScript mirrors do not validate.** A Rust struct field added without updating its `ui/src/lib/api.ts` interface is silently `undefined` at runtime and no tool warns. Rust and mirror change in the same task.

---

### Task 1: The matcher in `photon-core`

**Files:**
- Modify: `crates/photon-core/src/grid.rs` (the `GridView` enum, ~line 8)
- Modify: `crates/photon-core/src/library/items.rs` (`grid_entries`, `grid_entries_for`, ~lines 306–338)
- Test: `crates/photon-core/src/library/items.rs` (the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `GridView::Search` variant; `Library::entries_for(&self, view: GridView, query: &str) -> Result<Vec<GridEntry>>`; `Library::grid_entries(&self) -> Result<Vec<GridEntry>>` unchanged in signature.
- Consumes: nothing from other tasks.

**Context you need:** `GridView` is `Copy` and serialises `rename_all = "camelCase"`, so `Search` appears on the wire as `"search"`. It must stay `Copy` — the query string is NOT added to the enum (spec §4).

The current code is:

```rust
pub(crate) const GRID_ORDER: &str = "ORDER BY f.sort_key, f.path, i.taken_at, i.file_name";

pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
    self.grid_entries_for(GridView::All)
}

pub fn grid_entries_for(&self, view: GridView) -> Result<Vec<GridEntry>> {
    let filter = match view {
        GridView::All => "",
        GridView::Starred => "AND i.rating >= 1",
    };
    let conn = self.reader();
    let mut stmt = conn.prepare(&format!(
        "SELECT i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating
         FROM items i JOIN folders f ON f.id = i.folder_id
         WHERE i.missing_since IS NULL {filter} {GRID_ORDER}"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            let (w, h) = oriented_dims(r.get(3)?, r.get(4)?, r.get(5)?);
            Ok(GridEntry {
                id: r.get(0)?,
                folder_id: r.get(1)?,
                taken_at: r.get(2)?,
                aspect: if w == 0 || h == 0 { 1.0 } else { w as f32 / h as f32 },
                kind: MediaKind::from_db(r.get(6)?).unwrap_or(MediaKind::Image),
                starred: r.get::<_, Option<i64>>(10)?.unwrap_or(0) >= 1,
                thumb_key: fingerprint(&r.get::<_, String>(7)?, r.get(8)?, r.get(9)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
```

- [ ] **Step 1: Add the `Search` variant**

In `crates/photon-core/src/grid.rs`:

```rust
pub enum GridView {
    #[default]
    All,
    Starred,
    /// Photos whose file or folder name contains the engine's current search query.
    /// The query itself lives on the engine, not here: this enum is `Copy` and is
    /// mirrored in TypeScript as a union of plain strings.
    Search,
}
```

Run `cargo check --workspace`. It will FAIL on the non-exhaustive match in `grid_entries_for`. That failure is expected and Step 2 resolves it.

- [ ] **Step 2: Write the failing tests**

Add to the `mod tests` block in `items.rs`. Match the house style: long sentence-like names, and a message on any assertion whose expectation is not self-evident. Helpers `temp_library()`, `seed_folder(&lib, Path::new(...))`, `new_item(folder, path, taken_at)` already exist and are already imported.

```rust
#[test]
fn search_matches_a_substring_of_the_file_name() {
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    let ids = lib
        .insert_items(&[
            new_item(folder, "/p/sunset-beach.jpg", 1),
            new_item(folder, "/p/mountain.jpg", 2),
        ])
        .unwrap();

    let hits: Vec<i64> = lib
        .entries_for(GridView::Search, "beach")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(hits, vec![ids[0]]);
}

#[test]
fn search_matches_a_substring_of_the_folder_name() {
    let (_dir, lib) = temp_library();
    let (_watched, holiday) = seed_folder(&lib, Path::new("/holiday-2024"));
    let ids = lib
        .insert_items(&[new_item(holiday, "/holiday-2024/a.jpg", 1)])
        .unwrap();

    let hits: Vec<i64> = lib
        .entries_for(GridView::Search, "holiday")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(hits, ids, "the folder's name matches even though the file's does not");
}

#[test]
fn search_folds_case_for_non_ascii_text() {
    // This is the test that pins the whole "match in Rust, not in SQL" decision
    // (spec §3): SQLite's LIKE and lower() fold ASCII only, so a `LIKE`-based
    // implementation passes the ASCII cases above and fails this one. Deleting it
    // removes the only evidence for the design.
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/München"));
    lib.insert_items(&[new_item(folder, "/München/Straße.jpg", 1)])
        .unwrap();

    for query in ["münchen", "MÜNCHEN", "München"] {
        assert_eq!(
            lib.entries_for(GridView::Search, query).unwrap().len(),
            1,
            "{query} must find the folder München regardless of case"
        );
    }
    assert_eq!(
        lib.entries_for(GridView::Search, "straße").unwrap().len(),
        1,
        "the file Straße.jpg is found by its own name"
    );
}

#[test]
fn search_does_not_treat_ss_and_eszett_as_the_same_letter() {
    // A documented limit, not an aspiration. Rust's `to_lowercase` maps "Straße" to
    // "straße" and "STRASSE" to "strasse", so the two spellings never meet. Someone
    // who types `strasse` looking for `Straße.jpg` finds nothing.
    //
    // Left as-is deliberately: fixing it means full Unicode case-folding (ß → ss),
    // which needs a dependency or a hand-rolled table, and this is a simple search.
    // The test exists so the behaviour is a decision on record rather than a surprise.
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    lib.insert_items(&[new_item(folder, "/p/Straße.jpg", 1)])
        .unwrap();

    assert!(
        lib.entries_for(GridView::Search, "strasse").unwrap().is_empty(),
        "ß does not case-fold to ss"
    );
}

#[test]
fn search_treats_sql_wildcards_as_literal_characters() {
    // A LIKE-based implementation would return both rows for "%", since an
    // unescaped % matches everything (spec §3).
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    let ids = lib
        .insert_items(&[
            new_item(folder, "/p/50% grey.jpg", 1),
            new_item(folder, "/p/plain.jpg", 2),
        ])
        .unwrap();

    let hits: Vec<i64> = lib
        .entries_for(GridView::Search, "%")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(hits, vec![ids[0]], "% matches the character, not every row");
}

#[test]
fn search_keeps_grid_order_and_excludes_missing_items() {
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/trip"));
    let ids = lib
        .insert_items(&[
            new_item(folder, "/trip/b.jpg", 2),
            new_item(folder, "/trip/a.jpg", 1),
        ])
        .unwrap();
    lib.mark_missing(&[ids[0]]).unwrap();

    let hits: Vec<i64> = lib
        .entries_for(GridView::Search, "trip")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(hits, vec![ids[1]], "a missing item is not a search result");
}

#[test]
fn an_empty_search_query_matches_nothing_rather_than_everything() {
    // The engine turns an empty query back into the All view (Task 2); this is the
    // safety net under that, so a bug there shows as an empty grid rather than as a
    // "search" indistinguishable from the full library.
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();

    assert!(lib.entries_for(GridView::Search, "").unwrap().is_empty());
    assert!(lib.entries_for(GridView::Search, "   ").unwrap().is_empty());
}

#[test]
fn the_all_and_starred_views_are_unchanged_by_the_new_entry_point() {
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    let ids = lib
        .insert_items(&[
            new_item(folder, "/p/a.jpg", 1),
            new_item(folder, "/p/b.jpg", 2),
        ])
        .unwrap();
    lib.update_items(&[(
        ids[1],
        NewItem { rating: Some(3), ..new_item(folder, "/p/b.jpg", 2) },
    )])
    .unwrap();

    let all: Vec<i64> = lib.entries_for(GridView::All, "").unwrap().iter().map(|e| e.id).collect();
    let starred: Vec<i64> = lib.entries_for(GridView::Starred, "").unwrap().iter().map(|e| e.id).collect();
    assert_eq!(all, ids);
    assert_eq!(starred, vec![ids[1]]);
    assert_eq!(lib.grid_entries().unwrap().len(), 2, "the convenience wrapper still means All");
}
```

**The `ß` behaviour above was verified, not assumed.** `"Straße".to_lowercase()` is `straße` and `"STRASSE".to_lowercase()` is `strasse`; `"München"` and `"MÜNCHEN"` both lowercase to `münchen`. Those are the assertions written. If your implementation disagrees with any of them, trust the observed behaviour over this plan and say so in your report.

`mark_missing` and `update_items`/`NewItem` are existing APIs — check their exact signatures in `items.rs` before use and adapt these calls if they differ. The test bodies matter more than their exact helper spelling.

- [ ] **Step 4: Run the tests and watch them fail**

Run: `cargo test -p photon-core search` — expect compile failure (`entries_for` does not exist). That is the failure you want at this step.

- [ ] **Step 5: Implement**

In `items.rs`, replace `grid_entries_for` with the shared-mapping version. **The row mapping must exist once.** Both query paths call it, so they cannot drift:

```rust
/// The grid's columns, in the order `map_grid_row` reads them. Both query paths select
/// this same prefix so one mapping serves both.
const GRID_COLUMNS: &str =
    "i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating";

fn map_grid_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<GridEntry> {
    let (w, h) = oriented_dims(r.get(3)?, r.get(4)?, r.get(5)?);
    Ok(GridEntry {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        taken_at: r.get(2)?,
        aspect: if w == 0 || h == 0 { 1.0 } else { w as f32 / h as f32 },
        kind: MediaKind::from_db(r.get(6)?).unwrap_or(MediaKind::Image),
        starred: r.get::<_, Option<i64>>(10)?.unwrap_or(0) >= 1,
        thumb_key: fingerprint(&r.get::<_, String>(7)?, r.get(8)?, r.get(9)?),
    })
}

impl Library {
    /// Every visible item in grid order: folder tree order, then capture time, then name.
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        self.entries_for(GridView::All, "")
    }

    /// The grid's rows for one view. `query` is used only by `Search`; the other views
    /// ignore it. One entry point rather than two, because `GridView` is matched
    /// exhaustively and a `Search` arm that could not see the query would have to lie.
    pub fn entries_for(&self, view: GridView, query: &str) -> Result<Vec<GridEntry>> {
        match view {
            GridView::All => self.entries_filtered(""),
            GridView::Starred => self.entries_filtered("AND i.rating >= 1"),
            GridView::Search => self.search_entries(query),
        }
    }

    fn entries_filtered(&self, filter: &str) -> Result<Vec<GridEntry>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT {GRID_COLUMNS}
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {filter} {GRID_ORDER}"
        ))?;
        let rows = stmt
            .query_map([], map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Photos whose file name or folder name contains `query`, case-insensitively.
    ///
    /// The match runs in Rust rather than as SQL `LIKE` for two reasons (spec §3):
    /// SQLite folds case for ASCII only, so `münchen` would not find `München`; and
    /// `LIKE` would read `%` and `_` in the user's query as wildcards. This is one pass
    /// over the same rows an index rebuild already reads, with two short string compares
    /// added per row.
    fn search_entries(&self, query: &str) -> Result<Vec<GridEntry>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT {GRID_COLUMNS}, i.file_name, f.name
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {GRID_ORDER}"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                let file_name: String = r.get(11)?;
                let folder_name: String = r.get(12)?;
                let hit = file_name.to_lowercase().contains(&needle)
                    || folder_name.to_lowercase().contains(&needle);
                Ok(hit.then(|| map_grid_row(r)).transpose()?)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(rows)
    }
}
```

Keep `starred_count` exactly as it is. Update the two existing call sites of `grid_entries_for` (the starred tests in this file) to `entries_for(view, "")`.

- [ ] **Step 6: Run the tests**

`cargo test -p photon-core` — all green, including the pre-existing starred tests.

- [ ] **Step 7: Prove the tests discriminate**

For `search_folds_case_for_non_ascii_text` and `search_treats_sql_wildcards_as_literal_characters`, temporarily replace the Rust match with the SQL form the spec rejected:

```rust
// TEMPORARY — revert after observing the failure
"WHERE i.missing_since IS NULL AND (i.file_name LIKE '%' || ?1 || '%' OR f.name LIKE '%' || ?1 || '%')"
```

Run both tests and paste the failures into your report. Restore the Rust implementation. If either test passes against the `LIKE` version, it does not pin the design — say so and fix the test.

- [ ] **Step 8: Run the full Rust gate and commit**

All four commands from Global Constraints, then:

```bash
git add crates/photon-core
git commit -m "feat(search): match photos by file and folder name in photon-core"
```

---

### Task 2: Engine state, IPC and the TypeScript mirror

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (the `Engine` struct ~line 47, `Engine::open` ~line 93, `refresh_grid` ~line 117, `view()` ~line 135, `set_view` ~line 142)
- Modify: `crates/photon-app/src/commands.rs` (`GridInfo` ~line 34, `grid_info` ~line 93, `set_grid_view` ~line 106)
- Modify: `crates/photon-app/src/ipc.rs` (~line 67)
- Modify: `crates/photon-app/src/app.rs` (the `generate_handler!` list, ~lines 118–132)
- Modify: `ui/src/lib/api.ts` (~lines 15–18 and ~line 51)
- Test: `crates/photon-app/src/engine.rs` or the existing app test module — follow whatever the starred view's engine tests already do.

**Interfaces:**
- Consumes: `Library::entries_for(view, query)` and `GridView::Search` from Task 1.
- Produces: `Engine::set_search_query(&str) -> Result<()>`; the `set_search_query` Tauri command; `GridInfo.search_query` / TS `GridInfo.searchQuery`; TS `GridView` union gains `'search'`.

**Context you need.** The existing plumbing, verbatim:

```rust
// engine.rs
view: RwLock<GridView>,                       // struct field
view: RwLock::new(GridView::All),             // in Engine::open

pub fn view(&self) -> GridView { *self.view.read() }

pub fn set_view(&self, view: GridView) -> Result<()> {
    *self.view.write() = view;
    self.refresh_grid()
}

pub fn refresh_grid(&self) -> Result<()> {
    let _serialize = self.refresh.lock();
    let index = Arc::new(GridIndex::build(self.lib.grid_entries_for(*self.view.read())?));
    let (version, len) = {
        let mut grid = self.grid.write();
        grid.0 += 1;
        grid.1 = index;
        (grid.0, grid.1.len())
    };
    self.events.library_changed(LibraryChanged { version, len });
    Ok(())
}
```

```rust
// commands.rs
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridInfo {
    pub version: u64,
    pub len: usize,
    pub sections: Vec<Section>,
    pub starred_count: usize,
    pub view: GridView,
}
```

```rust
// ipc.rs
#[tauri::command(async)]
pub fn set_grid_view(engine: Eng<'_>, view: photon_core::grid::GridView) -> Result<(), AppError> {
    commands::set_grid_view(&engine, view)
}
```

```ts
// ui/src/lib/api.ts
export type GridView = 'all' | 'starred';
export interface GridInfo { version: number; len: number; sections: Section[]; starredCount: number; view: GridView }
setGridView: (view: GridView) => invoke<void>('set_grid_view', { view }),
```

- [ ] **Step 1: Write the failing tests**

Follow the file's existing engine-test conventions (find the starred view's engine test and mirror its setup). Three behaviours:

```rust
#[test]
fn setting_a_search_query_switches_to_the_search_view_and_filters_the_grid() {
    // ...seed a library with two photos whose names differ...
    engine.set_search_query("beach").unwrap();
    let info = commands::grid_info(&engine);
    assert_eq!(info.view, GridView::Search);
    assert_eq!(info.search_query, "beach");
    assert_eq!(info.len, 1);
}

#[test]
fn an_empty_search_query_returns_to_the_all_view() {
    // Clearing the box must restore the library, not leave a "search" view that is
    // indistinguishable from All but labelled differently (spec §4).
    engine.set_search_query("beach").unwrap();
    engine.set_search_query("   ").unwrap();
    let info = commands::grid_info(&engine);
    assert_eq!(info.view, GridView::All);
    assert_eq!(info.search_query, "");
    assert_eq!(info.len, 2, "the whole library is back");
}

#[test]
fn switching_to_another_view_clears_the_search_query() {
    // Otherwise a stale query rides along and reappears the next time Search is entered.
    engine.set_search_query("beach").unwrap();
    engine.set_view(GridView::Starred).unwrap();
    assert_eq!(commands::grid_info(&engine).search_query, "");
}
```

- [ ] **Step 2: Run them and watch them fail**

`cargo test -p photon-app search` — compile failure, `set_search_query` does not exist.

- [ ] **Step 3: Implement the engine state**

```rust
// struct Engine, beside `view`
/// The active search query. Beside the view rather than inside it: `GridView` is `Copy`
/// and is mirrored in TypeScript as plain strings (spec §4).
search_query: RwLock<String>,
```

`Engine::open` gains `search_query: RwLock::new(String::new()),`.

```rust
pub fn search_query(&self) -> String {
    self.search_query.read().clone()
}

/// Searches for `query`, or returns to the full library when it is blank.
///
/// An empty query is not a search: matching nothing would show an empty grid, and
/// matching everything would be the All view under a different name (spec §4).
pub fn set_search_query(&self, query: &str) -> Result<()> {
    if query.trim().is_empty() {
        return self.set_view(GridView::All);
    }
    *self.search_query.write() = query.to_string();
    *self.view.write() = GridView::Search;
    self.refresh_grid()
}

pub fn set_view(&self, view: GridView) -> Result<()> {
    // A query left behind would reappear the next time Search is entered.
    if view != GridView::Search {
        self.search_query.write().clear();
    }
    *self.view.write() = view;
    self.refresh_grid()
}
```

`refresh_grid`'s index line becomes:

```rust
let index = Arc::new(GridIndex::build(
    self.lib.entries_for(*self.view.read(), &self.search_query.read())?,
));
```

**Watch the lock order.** `refresh_grid` holds `self.refresh` and then takes read locks on `view` and `search_query`; `set_search_query` releases both write guards before calling it. Do not hold a write guard across the `refresh_grid` call — check that each `*self.x.write() = …` statement ends before the next line, and if you introduce a `let` binding for a guard, drop it explicitly.

- [ ] **Step 4: Thread it through IPC**

`commands.rs`:

```rust
pub struct GridInfo {
    // ...existing fields...
    pub search_query: String,
}

// in grid_info()
search_query: engine.search_query(),

// beside set_grid_view
pub fn set_search_query(engine: &Engine, query: &str) -> CmdResult<()> {
    engine.set_search_query(query)?;
    Ok(())
}
```

`ipc.rs`:

```rust
#[tauri::command(async)]
pub fn set_search_query(engine: Eng<'_>, query: String) -> Result<(), AppError> {
    commands::set_search_query(&engine, &query)
}
```

`app.rs`: add `ipc::set_search_query,` to the `generate_handler!` list, after `ipc::set_grid_view,`.

- [ ] **Step 5: Update the TypeScript mirror — same task, not later**

`ui/src/lib/api.ts`:

```ts
export type GridView = 'all' | 'starred' | 'search';
export interface GridInfo { version: number; len: number; sections: Section[]; starredCount: number; view: GridView; searchQuery: string }
```

and beside `setGridView`:

```ts
setSearchQuery: (query: string) => invoke<void>('set_search_query', { query }),
```

Then update the `info` state default in `ui/src/lib/library.svelte.ts` (~line 19) to include `searchQuery: ''`, or `svelte-check` will fail on the missing property.

- [ ] **Step 6: Run the gates and prove the tests discriminate**

Full Rust gate, plus `npm run check` and `npm test` in `ui/`. Then revert the `if view != GridView::Search { … clear() }` line in `set_view` and confirm `switching_to_another_view_clears_the_search_query` fails; restore it. Paste the failure into your report.

- [ ] **Step 7: Commit**

```bash
git add crates/photon-app ui/src/lib
git commit -m "feat(search): add the search view to the engine and IPC surface"
```

---

### Task 3: The search box

**Files:**
- Modify: `ui/src/lib/library.svelte.ts` (add `setSearchQuery` beside the existing `setView`, ~line 92)
- Modify: `ui/src/components/FolderTree.svelte` (the toolbar, and `jumpToFolder` ~line 111)
- Modify: `ui/src/App.svelte` (the scroll-reset `$effect`, ~lines 19–27)
- Modify: `ui/src/components/Grid.svelte` (the empty state)
- Test: the existing UI test files — follow their conventions

**Interfaces:**
- Consumes: `api.setSearchQuery`, `GridInfo.searchQuery`, `GridView` including `'search'` from Task 2.
- Produces: nothing later tasks depend on.

**Context you need.** `library.setView` today:

```ts
async setView(view: GridView): Promise<void> {
  try {
    await api.setGridView(view);
    await this.refresh();
  } catch (e) {
    this.reportError(e);
  }
}
```

`jumpToFolder` in `FolderTree.svelte`, which already carries the load-bearing await:

```svelte
async function jumpToFolder(folderId: number) {
  if (library.info.view === 'starred') await library.setView('all');
  onjump(folderId);
}
```

The scroll-reset effect in `App.svelte`:

```svelte
let lastView = library.info.view;
$effect(() => {
  const view = library.info.view;
  if (view !== lastView) { lastView = view; grid?.scrollToOffset(0, 'start'); }
});
```

- [ ] **Step 1: Add the store method**

```ts
/** Searches for `query`. A blank query returns the backend to the All view. */
async setSearchQuery(query: string): Promise<void> {
  try {
    await api.setSearchQuery(query);
    await this.refresh();
  } catch (e) {
    this.reportError(e);
  }
}
```

- [ ] **Step 2: Write the failing tests**

Put the debounce in a testable function in `ui/src/lib/` rather than inline in the component — the component cannot be unit-tested here, and a debounce asserted only by eye is not asserted. Follow the existing pattern of `ui/src/lib/nav.ts` (pure functions, tested directly).

```ts
// ui/src/lib/search.ts
export const SEARCH_DEBOUNCE_MS = 150;
export function debounce<T extends (...args: never[]) => void>(fn: T, ms: number): (...args: Parameters<T>) => void
```

Tests: several calls inside the window invoke `fn` once, with the LAST arguments; a call after the window invokes it again. Use the test runner's fake timers — check how the existing UI tests handle time before writing this.

- [ ] **Step 3: Run them and watch them fail**

`npm test` in `ui/` — the module does not exist yet.

- [ ] **Step 4: Implement the debounce, then the box**

In `FolderTree.svelte`'s toolbar, above the Starred row:

```svelte
<input
  class="search"
  type="search"
  placeholder="Search"
  aria-label="Search photos by file or folder name"
  bind:value={query}
  oninput={() => runSearch(query)}
  onkeydown={(e) => { if (e.key === 'Escape') { query = ''; runSearch.flush?.(); library.setSearchQuery(''); } }}
/>
```

with

```svelte
let query = $state(library.info.searchQuery);
const runSearch = debounce((q: string) => library.setSearchQuery(q), SEARCH_DEBOUNCE_MS);
```

Escape must take effect immediately rather than after the debounce — decide how (cancel the pending call, or call the store directly as sketched) and make the two paths agree so a pending debounced call cannot land after the clear and re-enter the search view. State in your report which you chose and why.

Extend `jumpToFolder` so it leaves Search too:

```svelte
async function jumpToFolder(folderId: number) {
  if (library.info.view !== 'all') await library.setView('all');
  onjump(folderId);
}
```

The await is load-bearing — see the comment already above that function; keep it, and widen its wording to cover both views.

- [ ] **Step 5: Reset scroll when the results change, not just the view**

In `App.svelte`, the effect must key on the query as well:

```svelte
let lastView = library.info.view;
let lastQuery = library.info.searchQuery;
$effect(() => {
  const view = library.info.view;
  const query = library.info.searchQuery;
  if (view !== lastView || query !== lastQuery) {
    lastView = view;
    lastQuery = query;
    grid?.scrollToOffset(0, 'start');
  }
});
```

Without the query in the condition, refining a query keeps the scroll offset from the previous, larger result set (spec §5).

- [ ] **Step 6: The empty state**

In `Grid.svelte`, when `library.info.len === 0` and `library.info.view === 'search'`, render `No photos match "<query>"` rather than an empty area. Match the component's existing empty-state markup and styling if one exists; if none does, keep it plain and consistent with the sidebar's type.

- [ ] **Step 7: Run the UI gate**

`npm run check` (0 errors AND 0 warnings — it runs `--fail-on-warnings`) and `npm test`.

- [ ] **Step 8: Prove the debounce test discriminates**

Replace `debounce` with a pass-through that calls `fn` immediately, confirm the "invokes once with the last arguments" test fails, and restore. Paste the failure into your report.

- [ ] **Step 9: Full gate and commit**

Both Rust and UI gates, then:

```bash
git add ui
git commit -m "feat(search): add the sidebar search box"
```

---

## Coverage against the spec

| Spec section | Task |
|---|---|
| §1 scope — file and folder name only | 1 |
| §2 no schema change | 1 (assert none is added) |
| §3 match in Rust, Unicode case folding, literal `%`/`_` | 1 (Steps 5, 7) |
| §4 `Search` variant, query beside it, one entry point, shared row mapping | 1 (variant, entry point), 2 (engine state) |
| §4 empty query returns to All | 2 (policy), 1 (safety net) |
| §5 search box, debounce, Escape clears | 3 |
| §5 folder click leaves Search, awaited | 3 (Step 4) |
| §5 scroll resets when the query changes | 3 (Step 5) |
| §5 empty state | 3 (Step 6) |
| §5 `GridInfo.search_query` and its TS mirror | 2 (Step 5) |
| §6 a failed query does not blank the window | 2 (`reportError` path), 3 |
| §7 testing | 1, 2, 3 |

Every §5 clause has a frontend task owning it. This table is checked against the spec deliberately: on the starred work, §5's scroll-to-top was mapped to a backend-only task and consequently nothing implemented it.
