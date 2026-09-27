# Folder Aliases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user name a folder in photon. The alias replaces the directory name in the sidebar and the grid header, orders the sidebar's name sort, and is found by search. Nothing on disk changes.

**Architecture:** A nullable `folders.alias` column (schema 21) holds the alias. It is written only by `Library::set_folder_alias`, which normalises the input, and is carried to the UI on `Folder` by `list_folders`. The engine rebuilds the grid after a change, because search reads the alias as a haystack. In the UI, one helper (`folderLabel`) decides what is shown, and the sidebar's inline editor gains a mode where an empty field clears the alias.

**Tech Stack:** Rust (rusqlite), Tauri 2, Svelte 5 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-27-photon-folder-alias-design.md`. Read it first.

## Global Constraints

- Work on branch `feat/folder-alias`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **photon never renames a directory.** The alias lives only in `library.db`. No task touches the filesystem or the Picasa INI.
- The TypeScript mirror (`ui/src/lib/api.ts`) changes in the same commit as the Rust struct it mirrors. `tsconfig` typechecks the tests, so every `Folder` literal in `ui/src/**/*.test.ts` and in `mock.js` gains `alias: null` in that commit too.
- The schema bump updates the hardcoded `20` literals to `21`. Do not change them to `MIGRATIONS.len()`. There are ten: `library/mod.rs` lines 166 and 192, plus eight `assert_eq!(version, 20)` in `library/schema.rs`. The table count stays at 12, since this adds a column, not a table.
- Case-insensitive matching stays in Rust. The search haystack is compared by `search::Query`, as the folder name already is.
- **A new test must be demonstrated to fail with its change reverted**, by an exact replacement, and the probe is recorded in the commit message. A compile error is not proof. Each task names its probe.
- The Rust gate runs before every commit: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. Commits that touch `ui/` also run the UI gate: `npm run check` (0 errors, 0 warnings) and `npm test`.
- Never launch the GUI to verify a change.

## Review Focus

1. **A rescan must not wipe an alias.** `upsert_folder`'s `ON CONFLICT` sets `parent_id` and `seen_scan` only. Task 1 pins this, both directly and through a scan plus a subtree scan.
2. **A search for the alias finds the folder's photos while a Search view is open.** This needs the engine's rebuild, because the alias changed no item row. Task 2 pins the haystack and Task 3 pins the rebuild.
3. **An empty field clears the alias instead of cancelling**, but only for the folder editor. Albums and saved searches keep "a blank commit is a cancel". Task 5 pins both halves.
4. **The alias is shown only where the spec says.** Settings and the viewer's info panel keep showing paths. Task 4 only touches `folderRows` and the grid header.

---

### Task 1: Schema 21, `Folder.alias`, `Library::set_folder_alias`

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs`: migration 21, a migration test, and the version literals
- Modify: `crates/photon-core/src/library/mod.rs`: the version literals
- Modify: `crates/photon-core/src/library/folders.rs`: `Folder.alias`, the `folders()` select, `set_folder_alias`, and tests
- Modify: `crates/photon-core/src/scanner.rs`: one end-to-end test
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/folders.test.ts`, `crates/xtask/screenshots/mock.js`: `alias` on `Folder` and on every literal

**Interfaces:**
- Produces: `Folder { …, pub alias: Option<String> }`, serialised as `alias`, plus `pub const MAX_FOLDER_ALIAS_CHARS: usize = 255;` and `pub fn set_folder_alias(&self, folder_id: i64, alias: Option<&str>) -> Result<bool>`. The function returns whether the stored value changed. An unknown id is `Error::NotFound(folder_id)`. Task 3 consumes it.

- [ ] **Step 1: Migration.** Append this to `MIGRATIONS`:

```rust
    r#"
-- A name the user gave the folder in photon, shown in place of its directory name. NULL
-- for none. Never read from or written to the disk: the directory keeps its name, and a
-- folder whose row is pruned (its directory gone, renamed or moved) loses the alias with it.
ALTER TABLE folders ADD COLUMN alias TEXT;
"#,
```

  Move the ten version literals to 21. Add `migration_21_leaves_existing_folders_without_an_alias`, modelled on `migration_17_keeps_existing_albums_as_photons_own`. It seeds `MIGRATIONS[..20]`, sets `user_version` to 20, inserts a watched folder and a folder, opens the library, and asserts that `alias` is NULL and the version is 21.

- [ ] **Step 2: `Folder.alias`.** Add the field with this doc comment:

  ```rust
  /// The user's name for the folder in photon, shown in place of `name`; `None` for none.
  ```

  Select `alias` in `folders()`. Update the two `Folder` literals in `folders.rs`'s tests with `alias: None`.

- [ ] **Step 3: `set_folder_alias`.** Write it in one transaction:

```rust
    /// Names a folder in photon, or clears the name with `None`. The input is trimmed and
    /// cut to `MAX_FOLDER_ALIAS_CHARS` on a character boundary. An empty result, or one equal
    /// to the directory's own name, is stored as NULL: "renaming it back" must not leave an
    /// alias that merely repeats the name, which would go on winning the sort's ties and
    /// matching search as a second copy. Returns whether the stored value changed, so the
    /// engine rebuilds only when something did; `NotFound` for a folder that does not exist.
    pub fn set_folder_alias(&self, folder_id: i64, alias: Option<&str>) -> Result<bool> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let (name, current): (String, Option<String>) = tx
            .query_row(
                "SELECT name, alias FROM folders WHERE id = ?1",
                params![folder_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(crate::Error::NotFound(folder_id))?;
        let alias = alias
            .map(|a| a.trim().chars().take(MAX_FOLDER_ALIAS_CHARS).collect::<String>())
            .map(|a| a.trim_end().to_string())
            .filter(|a| !a.is_empty() && *a != name);
        if alias == current {
            return Ok(false);
        }
        tx.execute(
            "UPDATE folders SET alias = ?2 WHERE id = ?1",
            params![folder_id, alias],
        )?;
        tx.commit()?;
        Ok(true)
    }
```

  The second `trim_end` is there because a cut can land just after a space. `.optional()` needs `rusqlite::OptionalExtension` in scope; check `folders.rs`'s imports.

- [ ] **Step 4: Tests** in `folders.rs`:
  - `a_folder_alias_is_trimmed_and_cleared_by_empty_or_the_real_name`:
    - `"  Easter  "` is stored as `Some("Easter")` and returns `true`.
    - Setting `"Easter"` again returns `false`.
    - `"   "` clears it.
    - Setting the directory's own name clears it.
    - `None` clears it.
    - An unknown id is `NotFound`.
  - `a_long_folder_alias_is_cut_on_a_character_boundary`: 300 × `"é"` is stored as 255 characters.
  - `an_alias_survives_upsert_folder`: set an alias, call `upsert_folder` again with a later `scan_id`, and the alias is unchanged.

  In `scanner.rs`, add `a_folder_alias_survives_a_scan_and_a_subtree_scan`, shaped like `a_file_scanned_into_a_hidden_folder_arrives_hidden`: scan, alias `sub`, write a file, `scan`, write another, `scan_sub`, then assert that the alias is still `Some`.

  Probes:
  - Survival: add `, alias = NULL` to `upsert_folder`'s `ON CONFLICT … DO UPDATE SET`. Both survival tests fail.
  - Normalisation: replace `.filter(|a| !a.is_empty() && *a != name)` with `.filter(|a| !a.is_empty())`. The first test fails on the "real name" assertion.
  - Cut: replace `.take(MAX_FOLDER_ALIAS_CHARS)` with `.take(usize::MAX)`.

- [ ] **Step 5: The TS mirror.**
  - In `api.ts`, `Folder` gains `alias: string | null`, with a comment that it is shown in place of `name` (see `folderLabel`).
  - Add `alias: null` to the four literals in `folders.test.ts` and the five in `mock.js`.
  - Give `mock.js` folder 5 the alias `'Lisbon with the Silvas'` so the screenshots show one. Its path still ends in `Lisbon`.

- [ ] **Step 6: Gates, then commit** with the message `feat(core): folder aliases in the library (schema 21)`.

### Task 2: Search matches the alias

**Files:**
- Modify: `crates/photon-core/src/library/items.rs`: `search_entries` and one test

- [ ] **Step 1:** Append `, f.alias` after `i.caption` in `search_entries`' select, so no existing index shifts. Read it at `base + 10`, and push it as a haystack when it is `Some`. Extend the doc comment's list of haystacks to name "the folder's alias".

- [ ] **Step 2: Test** `search_matches_a_folders_alias_and_still_its_name`. It seeds `/dcim-0412` with one photo, then calls `set_folder_alias(folder, Some("Easter"))`. A search for `easter` finds the photo, and a search for `dcim` still finds it.

  Probe: delete the `push` of the alias. The `easter` assertion fails.

- [ ] **Step 3: Gates, then commit** with the message `feat(search): a folder's alias is a haystack`.

### Task 3: Engine and IPC

**Files:**
- Modify: `crates/photon-app/src/engine.rs`: `set_folder_alias` and a test
- Modify: `crates/photon-app/src/commands.rs`, `ipc.rs`, `app.rs`: the command in all three places
- Modify: `ui/src/lib/api.ts`: `setFolderAlias`
- Modify: `crates/xtask/screenshots/mock.js`: a canned answer

- [ ] **Step 1: Engine.**

```rust
    /// Names a folder in photon (`Library::set_folder_alias`). The grid rebuilds when the name
    /// changed: search reads the alias as a haystack, so an open Search view has to follow,
    /// and no item row moved to make the refresh chain run on its own.
    pub fn set_folder_alias(&self, folder_id: i64, alias: Option<&str>) -> Result<bool> {
        let changed = self.lib.set_folder_alias(folder_id, alias)?;
        if changed {
            self.refresh_grid()?;
        }
        Ok(changed)
    }
```

- [ ] **Step 2: Command, in three files, in this order:**
  1. `commands.rs`: `pub fn set_folder_alias(engine: &Engine, folder_id: i64, alias: Option<String>) -> CmdResult<bool>`, which delegates with `alias.as_deref()`.
  2. `ipc.rs`: the `#[tauri::command(async)]` wrapper, next to `set_folder_hidden`.
  3. `app.rs`: `ipc::set_folder_alias` in `generate_handler!`, next to `ipc::set_folder_hidden`.

- [ ] **Step 3: UI and mock.**
  - `api.ts`: `setFolderAlias: (folderId: number, alias: string | null) => invoke<boolean>('set_folder_alias', { folderId, alias })`.
  - `mock.js` canned: `set_folder_alias: () => true`. The screenshots test in `screenshots.rs` fails without it.

- [ ] **Step 4: Test** `aliasing_a_folder_rebuilds_the_grid_and_search_follows`, modelled on `hiding_a_folder_rebuilds_the_grid_and_reports_its_flag`:
  - Put the engine in `GridView::Search` with the query `easter`. It has 0 photos.
  - Alias the folder `Easter`. The call returns `true`, the grid version rose, `len` is the folder's count, and `list_folders` reports the alias.
  - Aliasing it `Easter` again returns `false` and leaves the version unchanged.

  Probe: replace `if changed {` with `if false {`. The rebuild assertion fails.

- [ ] **Step 5: Gates, then commit** with the message `feat(app): set_folder_alias command`.

### Task 4: Show the alias

**Files:**
- Modify: `ui/src/lib/folders.ts`: `folderLabel`, and its use in `folderRows`
- Modify: `ui/src/lib/folders.test.ts`
- Modify: `ui/src/components/Grid.svelte`: the header

- [ ] **Step 1:**

```ts
/** What photon calls a folder: the user's alias, else its directory name. The one place that
 *  decides it, so the sidebar, its name sort and the grid header cannot disagree. The path,
 *  shown beside the header and as the sidebar row's tooltip, keeps the real name visible. */
export function folderLabel(f: Pick<Folder, 'name' | 'alias'>): string {
  return f.alias ?? f.name;
}
```

  In `folderRows`, build `names` from `folderLabel(f)`. `FOLDER_ORDER.name` already compares `row.name`, so the name sort follows without a change.

- [ ] **Step 2:** In `Grid.svelte`'s header, replace `{folder?.name ?? ''}` with `{folder ? folderLabel(folder) : ''}`.

- [ ] **Step 3: Tests** in `folders.test.ts`:
  - `folderRows` shows an alias in place of the name.
  - `arrangeFolders` by name orders an aliased folder by its alias: `oslo` aliased as `Aarhus trip` sorts before `old`.

  Probe: return `f.name` from `folderLabel`. Both tests fail.

- [ ] **Step 4: UI gate, then commit** with the message `feat(ui): a folder's alias replaces its name in the sidebar and grid`.

### Task 5: Rename in photon…

**Files:**
- Modify: `ui/src/lib/album-editor.svelte.ts` and its test: the `blankClears` option
- Modify: `ui/src/lib/library.svelte.ts`: `setFolderAlias`
- Modify: `ui/src/components/FolderTree.svelte`: the menu items and the inline field
- Modify: `README.md`: a user paragraph beside Hide folder's, and a smoke-checklist item

- [ ] **Step 1: The editor.** Add `blankClears?: boolean` to `createAlbumEditor`'s deps and document it on the doc comment: "for a name the backend can clear (a folder's alias), a blank commit in rename mode sends `''` instead of cancelling". In `commit()`, take the cancel branch only when `!name && !(deps.blankClears && current.kind === 'rename')`.

  Tests:
  - `with blankClears, a blank rename sends an empty name`: `rename` is called with `(4, '')`, and `commit` resolves `true`.
  - The existing `treats a blank commit as a cancel, and sends nothing` still passes. It is the other half of Review Focus 3.

  Probe: delete `&& !(deps.blankClears && current.kind === 'rename')`. The new test fails.

- [ ] **Step 2: Store.**

```ts
  /** Names a folder in photon, or clears the name with `null` or `''`. The grid follows the
   *  backend's rebuild; the folder list is refetched here, as after `setFolderHidden`,
   *  because a library change refetches collections and not folders. */
  async setFolderAlias(folderId: number, alias: string | null): Promise<void> {
    await api.setFolderAlias(folderId, alias || null);
    await this.refreshFolders();
  }
```

- [ ] **Step 3: FolderTree.** Add a third editor instance, `folderEditor = createAlbumEditor({ create: async () => {}, rename: (id, name) => library.setFolderAlias(id, name), blankClears: true })`. The comment on `searchEditor` already explains why each list keeps its own editor: the ids of different lists collide.
  - The folder menu gains **Rename in photon…** after "Reveal in file manager". It closes the menu, calls `folderEditor.startRename(folder.id, folderLabel(folder))`, awaits `tick()`, then focuses and selects the field.
  - It also gains **Use folder name**, shown only when `folder.alias` is set, which calls `library.setFolderAlias(folder.id, null)`.
  - In the folder row loop, `{#if folderEditor.editing(row.folderId)}` renders the same `<input class="editor">` as the album rows, with `aria-label="Folder name in photon"`, its own `bind:this`, and Enter, Escape and blur wired as `onEditorKeydown` and `commitEditor` do for albums. Otherwise it renders the existing button.
  - Errors go to `library.reportError`.

  There is no automated test: this is effect wiring, which `svelte-check` verifies, plus the smoke item below.

- [ ] **Step 4: README.**
  - Add a short paragraph next to Hide folder's (around line 228) saying:
    - **Rename in photon…** gives a folder a name that photon shows instead of the directory's.
    - The directory itself is not renamed.
    - The grid header's path still shows the real name.
    - Search finds either name.
    - The name is lost if the directory is renamed or moved on disk.
  - Add this smoke item next to Hide folder's:

    `- [ ] Right-click a folder → **Rename in photon…**: the field opens pre-filled; type a name and press Enter. The sidebar and the grid header show it, the header's path still ends in the directory's name, and the directory on disk is unchanged. Sort by name: the folder sorts by its new name. Search for it: its photos are found. Rename it to an empty field: the directory name returns. **Use folder name** does the same.`

- [ ] **Step 5: UI gate, and `cargo run -p xtask -- screenshots`** if Chromium is available (look at the sidebar shot for folder 5's alias). Then commit with the message `feat(ui): Rename in photon… for folders`. The commit message says why Step 3 has no test.

### Task 6: Whole-branch review

- [ ] Get an independent read of the whole branch before merging. Point the reviewer at:
  - the FolderTree effect wiring, where three editors now share one sidebar and blur commits;
  - whether any display of a folder name still reads `f.name` directly (`grep -rn "\.name" ui/src/components` for folders);
  - what the new rebuild *arms*: `set_folder_alias` calls `refresh_grid()` although no item row changed, so check that nothing downstream (the viewer's `pictureChanged`, the selection's re-find) reacts to a version bump with identical rows.
- [ ] Open the PR with `gh pr create`, then `gh pr checks N --watch` once checks exist.
