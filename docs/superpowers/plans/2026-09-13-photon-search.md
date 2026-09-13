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
- **Every task must leave the whole workspace compiling.** `--workspace` is in the gate: a change to `photon-core` that breaks `photon-app` fails the task that made it, not the task that would have fixed it.
- **The UI gate:** `npm run check` in `ui/` must report **0 errors and 0 warnings** (it runs with `--fail-on-warnings`), plus `npm test`. Note that `ui/tsconfig.json` includes `src/**/*.ts`, so **test files are typechecked too** — a struct field added to `GridInfo` breaks every `GridInfo` literal in `library.test.ts`.
- **Every new test must be demonstrated to fail with its change reverted.** Revert the change, run the test, paste the failure, restore. A test that passes both ways proves nothing and must be replaced with one that discriminates — say so in your report rather than presenting it as proof.
- **TypeScript mirrors do not validate.** A Rust struct field added without updating its `ui/src/lib/api.ts` interface is silently `undefined` at runtime and no tool warns. Rust and mirror change in the same task.

---

### Task 1: The matcher in `photon-core`

**Files:**
- Modify: `crates/photon-core/src/grid.rs` (the `GridView` enum, ~line 6)
- Modify: `crates/photon-core/src/library/items.rs` (`grid_entries` / `grid_entries_for`, ~lines 305–343; `GRID_ORDER` is far above at ~line 61)
- Modify: `crates/photon-app/src/engine.rs` (~line 123, the one call site in the other crate — see Step 6)
- Test: `crates/photon-core/src/library/items.rs` (the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `GridView::Search` variant; `Library::entries_for(&self, view: GridView, query: &str) -> Result<Vec<GridEntry>>`; `Library::grid_entries(&self) -> Result<Vec<GridEntry>>` unchanged in signature.
- Consumes: nothing from other tasks.

**Context you need:** `GridView` is `Copy` and serialises `rename_all = "camelCase"`, so `Search` appears on the wire as `"search"`. It must stay `Copy` — the query string is NOT added to the enum (spec §4). It already derives `PartialEq, Eq`; no derive change is needed.

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

**`grid_entries_for` has three call sites, not two** — two starred tests in `items.rs`, and one in `crates/photon-app/src/engine.rs` (`refresh_grid`). All three move to `entries_for` in this task. Find them with `rg 'grid_entries_for'` rather than trusting this count.

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

Run `cargo check --workspace`. It will FAIL on the non-exhaustive match in `grid_entries_for`. That failure is expected and Step 5 resolves it.

- [ ] **Step 2: Write the failing tests**

Add to the `mod tests` block in `items.rs`. Match the house style: long sentence-like names, and a message on any assertion whose expectation is not self-evident. The helpers `temp_library()`, `seed_folder(&lib, Path::new(...))` (returns `(watched_id, folder_id)`) and `new_item(folder, path, taken_at)` already exist and are imported. `new_item` derives `file_name` from the path and folder insertion derives `name` from the path's last segment, so `/München/Straße.jpg` really does store `file_name = "Straße.jpg"` and `name = "München"`.

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
    assert_eq!(
        hits, ids,
        "the folder's name matches even though the file's does not"
    );
}

#[test]
fn a_query_matching_neither_name_returns_nothing() {
    let (_dir, lib) = temp_library();
    let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
    lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();

    assert!(
        lib.entries_for(GridView::Search, "zzz").unwrap().is_empty(),
        "no match is an empty result, not the whole library"
    );
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
    // `mark_missing` takes a timestamp as its second argument; the existing tests in
    // this file call it as `mark_missing(&ids, 99)`.
    lib.mark_missing(&[ids[0]], 99).unwrap();

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
        NewItem {
            rating: Some(3),
            ..new_item(folder, "/p/b.jpg", 2)
        },
    )])
    .unwrap();

    let all: Vec<i64> = lib
        .entries_for(GridView::All, "")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    let starred: Vec<i64> = lib
        .entries_for(GridView::Starred, "")
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(all, ids);
    assert_eq!(starred, vec![ids[1]]);
    assert_eq!(
        lib.grid_entries().unwrap().len(),
        2,
        "the convenience wrapper still means All"
    );
}
```

**The `ß` and `München` behaviours above were verified against the real toolchain, not assumed:** `"Straße".to_lowercase()` is `straße`, `"STRASSE".to_lowercase()` is `strasse`, and both `"München"` and `"MÜNCHEN"` lowercase to `münchen`. If your implementation disagrees with any of these, trust what you observe over this plan and say so in your report.

Check every helper signature against the file before use (`mark_missing`, `update_items`, `NewItem`, `insert_items`) and adapt these calls if they differ. The test bodies matter more than their exact helper spelling.

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test -p photon-core search` — expect a compile failure (`entries_for` does not exist). That is the failure you want at this step.

- [ ] **Step 4: Implement**

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
    /// SQLite folds case for ASCII only, so `MÜNCHEN` would not find `München`; and
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
                // No `Ok(…?)` wrapper here: the closure already returns this type, and
                // wrapping it trips `clippy::needless_question_mark`, which the gate
                // treats as an error.
                hit.then(|| map_grid_row(r)).transpose()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(rows)
    }
}
```

Keep `starred_count` exactly as it is. Passing the free function `map_grid_row` directly to `query_map` is fine and has precedent in this codebase (`row_to_watched` in `folders.rs`).

- [ ] **Step 5: Update the two test call sites in `items.rs`**

The starred tests call `grid_entries_for(view)`; change them to `entries_for(view, "")`.

- [ ] **Step 6: Keep the workspace compiling**

`crates/photon-app/src/engine.rs` (~line 123, in `refresh_grid`) is the third call site:

```rust
// before
let index = Arc::new(GridIndex::build(self.lib.grid_entries_for(*self.view.read())?));
// after — Task 2 replaces the empty string with the engine's real query
let index = Arc::new(GridIndex::build(
    self.lib.entries_for(*self.view.read(), "")?,
));
```

This one line belongs to this task, not Task 2: `cargo clippy --workspace` and `cargo test --workspace` are in the gate, so leaving it broken would make this task's own gate unpassable.

- [ ] **Step 7: Run the tests, then prove they discriminate**

`cargo test --workspace` — all green, including the pre-existing starred tests.

Then, for `search_folds_case_for_non_ascii_text` and `search_treats_sql_wildcards_as_literal_characters`, temporarily replace the Rust match with the SQL form the spec rejected:

```rust
// TEMPORARY — revert after observing the failure
"WHERE i.missing_since IS NULL AND (i.file_name LIKE '%' || ?1 || '%' OR f.name LIKE '%' || ?1 || '%')"
```

Run both tests and paste the failures into your report. Restore the Rust implementation. If either test passes against the `LIKE` version, it does not pin the design — say so and fix the test.

- [ ] **Step 8: Run the full Rust gate and commit**

All four commands from Global Constraints, then:

```bash
git add crates/photon-core crates/photon-app/src/engine.rs
git commit -m "feat(search): match photos by file and folder name in photon-core"
```

---

### Task 2: Engine state, IPC and the TypeScript mirror

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (struct ~line 47, `Engine::open` ~line 93, `refresh_grid` ~line 120, `set_view` ~line 142)
- Modify: `crates/photon-app/src/commands.rs` (`GridInfo` ~line 30, `grid_info` ~line 93, beside `set_grid_view` ~line 106)
- Modify: `crates/photon-app/src/ipc.rs` (~line 66)
- Modify: `crates/photon-app/src/app.rs` (the `generate_handler!` list, ~lines 118–132)
- Modify: `ui/src/lib/api.ts` (~lines 16–17 and ~line 51)
- Modify: `ui/src/lib/library.svelte.ts` (the `info` state default, ~line 19)
- Modify: `ui/src/lib/library.test.ts` (**four** `GridInfo` literals — see Step 5)
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

`refresh_grid`'s index line becomes (Task 1 left it with an empty string):

```rust
let index = Arc::new(GridIndex::build(
    self.lib.entries_for(*self.view.read(), &self.search_query.read())?,
));
```

**Watch the lock order.** `refresh_grid` holds `self.refresh` and then takes read locks on `view` and `search_query`; `set_search_query` must release both write guards before calling it. Each `*self.x.write() = …` is a statement-scoped temporary and so is already fine — but if you introduce a `let` binding for a guard, drop it explicitly before the `refresh_grid` call.

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

- [ ] **Step 5: Update the TypeScript mirror — including the test file**

`ui/src/lib/api.ts`:

```ts
export type GridView = 'all' | 'starred' | 'search';
export interface GridInfo { version: number; len: number; sections: Section[]; starredCount: number; view: GridView; searchQuery: string }
```

and beside `setGridView`:

```ts
setSearchQuery: (query: string) => invoke<void>('set_search_query', { query }),
```

`ui/src/lib/library.svelte.ts` (~line 19): add `searchQuery: ''` to the `info` state default.

**`ui/src/lib/library.test.ts` builds four `GridInfo` literals, and `tsconfig.json` typechecks test files.** Every one of them needs `searchQuery: ''` or `npm run check` fails: the `mockResolvedValue({… view: 'all' })` near line 57, the `deferred<{…}>` type argument around lines 152–158, and the two `resolve(…)` calls around lines 172 and 201. Find them all with `rg "view: 'all'" ui/src`. Also add `setSearchQuery` to the `vi.mock('./api', …)` factory, or any later test that exercises it will fail on an undefined mock.

This step is where the previous feature's defect lived: a Rust field added without its TypeScript mirror is silently `undefined` at runtime and nothing warns.

- [ ] **Step 6: Run the gates and prove the tests discriminate**

Full Rust gate, plus `npm run check` (0 errors AND 0 warnings) and `npm test` in `ui/`. Then revert the `if view != GridView::Search { … clear() }` line in `set_view`, confirm `switching_to_another_view_clears_the_search_query` fails, and restore it. Paste the failure into your report.

- [ ] **Step 7: Commit**

```bash
git add crates/photon-app ui/src/lib
git commit -m "feat(search): add the search view to the engine and IPC surface"
```

---

### Task 3: The search box

**Files:**
- Create: `ui/src/lib/search.ts` (the debounce and the results-changed predicate)
- Create: a test file for it, named to match the existing convention in `ui/src/lib/` (check whether they are `*.test.ts` beside the module)
- Modify: `ui/src/lib/library.svelte.ts` (add `setSearchQuery` beside the existing `setView`, ~line 92)
- Modify: `ui/src/components/FolderTree.svelte` (the toolbar, and `jumpToFolder` ~line 115)
- Modify: `ui/src/App.svelte` (the scroll-reset `$effect`, ~lines 22–29)
- Modify: `ui/src/components/Grid.svelte` (the **existing** empty state at ~lines 104–112, which already branches on `view === 'starred'`)
- Modify: `README.md` (the "## Manual smoke checklist" section, ~line 69)

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

`jumpToFolder` in `FolderTree.svelte`, which already carries a load-bearing await and a comment explaining it:

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

- [ ] **Step 2: Write the failing tests for `search.ts`**

Both pieces of logic go in `ui/src/lib/search.ts` as pure functions, following the pattern of `ui/src/lib/nav.ts`. **Logic left inline in a `.svelte` file cannot be unit-tested in this project** — that is the whole reason for extracting them, so do not leave either one in the component.

```ts
export const SEARCH_DEBOUNCE_MS = 150;

/** Calls `fn` once the caller stops calling for `ms`. `cancel` drops a pending call. */
export function debounce<T extends (...args: never[]) => void>(
  fn: T,
  ms: number,
): ((...args: Parameters<T>) => void) & { cancel(): void };

/** Whether the grid is showing a different set of photos than it was. */
export function resultsChanged(
  prev: { view: GridView; query: string },
  next: { view: GridView; query: string },
): boolean;
```

Tests:
- several `debounce` calls inside the window invoke `fn` **once**, with the **last** arguments;
- a call after the window invokes it again;
- `cancel()` stops a pending call from ever firing;
- `resultsChanged` is true when only the **view** differs, true when only the **query** differs, and false when neither does. The query case is the one that matters: it is the clause spec §5 requires and the one an implementation naturally forgets.

**No existing UI test uses fake timers** — `vi.useFakeTimers()` is available in Vitest and works, but there is no in-repo precedent to copy, so write it the way the Vitest docs do.

- [ ] **Step 3: Run them and watch them fail**

`npm test` in `ui/` — the module does not exist yet.

- [ ] **Step 4: Implement `search.ts`, then the box**

In `FolderTree.svelte`'s toolbar, above the Starred row:

```svelte
<input
  class="search"
  type="search"
  placeholder="Search"
  aria-label="Search photos by file or folder name"
  bind:value={query}
  oninput={() => runSearch(query)}
  onkeydown={(e) => { if (e.key === 'Escape') clearSearch(); }}
/>
```

with

```svelte
let query = $state(library.info.searchQuery);
const runSearch = debounce((q: string) => library.setSearchQuery(q), SEARCH_DEBOUNCE_MS);

function clearSearch() {
  // Cancel first: a pending debounced call would otherwise land after the clear and
  // put the backend straight back into the search view.
  runSearch.cancel();
  query = '';
  library.setSearchQuery('');
}

// The backend is the source of truth for the active query (spec §5): clicking Starred
// or a folder clears it server-side, and without this the box would keep displaying
// text that no longer filters anything.
$effect(() => {
  const backend = library.info.searchQuery;
  if (backend !== query) query = backend;
});
```

**Do not use `flush`.** Firing the pending call and then clearing sends two un-ordered async IPC calls and re-enters the search view — the exact race the awaited `setView` in `jumpToFolder` exists to prevent. `cancel` is the correct primitive.

The `$effect` above needs care: it writes `query`, which it also reads, so confirm it settles rather than looping. If Svelte 5 warns about that, restructure it (for example, track the last backend value seen in a separate variable and compare against that). Report what you did.

Extend `jumpToFolder` so it leaves Search too:

```svelte
async function jumpToFolder(folderId: number) {
  if (library.info.view !== 'all') await library.setView('all');
  onjump(folderId);
}
```

The await is load-bearing — keep the existing comment above that function and widen its wording to cover both non-All views.

- [ ] **Step 5: Reset scroll when the results change, not just the view**

In `App.svelte`, use the extracted predicate so the condition is the tested one:

```svelte
let last = { view: library.info.view, query: library.info.searchQuery };
$effect(() => {
  const next = { view: library.info.view, query: library.info.searchQuery };
  if (resultsChanged(last, next)) {
    last = next;
    grid?.scrollToOffset(0, 'start');
  }
});
```

Without the query in the comparison, refining a query keeps the scroll offset from the previous, larger result set (spec §5).

- [ ] **Step 6: Extend the existing empty state**

`Grid.svelte` already has an empty state that branches on `view === 'starred'` (~lines 104–112). Add a `search` branch to that same chain — `No photos match "<query>"` — rather than writing new markup. Use `library.info.searchQuery` for the text, not a local variable.

- [ ] **Step 7: Add the manual checklist items**

The three items in spec §7 need eyes and no agent here runs the GUI. Append them to `README.md`'s "## Manual smoke checklist", matching its existing bullet style: typing part of a folder's name finds its photos; clearing the box restores the full library; clicking a folder while a search is active leaves search and lands on that folder.

- [ ] **Step 8: Run the UI gate**

`npm run check` (0 errors AND 0 warnings — it runs `--fail-on-warnings`) and `npm test`.

- [ ] **Step 9: Prove the tests discriminate**

Two reverts, both pasted into your report:
- Replace `debounce` with a pass-through that calls `fn` immediately; confirm the "invokes once with the last arguments" test fails; restore.
- Change `resultsChanged` to compare only `view`; confirm the query-only test fails; restore.

- [ ] **Step 10: Full gate and commit**

Both Rust and UI gates, then:

```bash
git add ui README.md
git commit -m "feat(search): add the sidebar search box"
```

---

## Coverage against the spec

| Spec section | Task |
|---|---|
| §1 scope — file and folder name only | 1 |
| §2 no schema change | 1 (assert none is added) |
| §3 match in Rust, Unicode case folding, literal `%`/`_` | 1 (Steps 4, 7) |
| §3 `ß` does not fold to `ss` — recorded, not fixed | 1 (Step 2) |
| §4 `Search` variant, query beside it, one entry point, shared row mapping | 1 (variant, entry point), 2 (engine state) |
| §4 empty query returns to All | 2 (policy), 1 (safety net) |
| §5 search box, debounce, Escape clears via `cancel` | 3 (Steps 2, 4) |
| §5 folder click leaves Search, awaited | 3 (Step 4) |
| §5 the box renders the backend's query, not local state | 3 (Step 4, the `$effect`) |
| §5 scroll resets when the query changes | 3 (Step 5), tested via `resultsChanged` in 3 (Step 2) |
| §5 empty state | 3 (Step 6) |
| §5 `GridInfo.search_query` and its TS mirror | 2 (Step 5), including the test file's four literals |
| §6 a failed query surfaces as an error and leaves the previous results on screen | 2 (`?` propagation), 3 (`reportError` in the store) |
| §7 matcher tests, including "matches neither" | 1 (Step 2) |
| §7 UI tests — debounce, scroll reset | 3 (Step 2) |
| §7 manual checklist | 3 (Step 7) |

**This table was verified by a pre-flight scan against the spec and the real code, not merely written.** That scan found §6 mapped to a `reportError` path that did not implement what §6 then said — the identical failure mode that left §5's scroll-to-top unimplemented on the starred work. The spec's §6 was amended as a result: a failed query propagates and is surfaced, rather than silently falling back to an empty result, because an empty grid is indistinguishable from "no matches". A coverage table is worth nothing unless something checks it.
