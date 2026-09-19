# Grid Multi-Select Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user select several photos in the grid with Ctrl/Cmd+click and Shift+click, and star, unstar or move all of them between albums in one action.

**Architecture:** The selection is a `Set` of **photo ids** in `LibraryStore`, beside the existing single "lead" offset, so no grid rebuild can invalidate it; an `anchor` offset drives Shift+click, and a range's ids come from `api.gridRows` in `MAX_ROWS` chunks. Bulk starring gets one folder-level INI writer (`picasa::set_stars`) that `set_star` then delegates to, and one `Engine::set_stars` that groups by directory, writes each INI once and refreshes the grid once.

**Tech Stack:** Rust (photon-core, photon-app/Tauri 2), Svelte 5 runes + TypeScript, vitest, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-09-19-photon-grid-multi-select-design.md`

## Global Constraints

- **photon never writes to, moves or deletes photo files.** The only file written inside a watched folder is Picasa's `.picasa.ini`/`Picasa.ini`, and only its `star=` lines. Every other byte is preserved.
- **The reader and the writer in `picasa.rs` share one line classifier (`classify`).** A second writer with its own header/key logic is a defect, not a shortcut.
- **INI first, database second.** A rating written before its INI write succeeds shows a star the next scan silently clears.
- **The Rust gate, all four, before any commit:** `cargo fmt --all` (run it, not just `--check`), then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate, from the repo root:** `npm run check` (0 errors AND 0 warnings) and `npm test`.
- **A new command means five files:** `commands.rs`, `ipc.rs`, `app.rs`'s `generate_handler!`, the hand-written mirror in `ui/src/lib/api.ts`, and an answer in `crates/xtask/screenshots/mock.js`. Forgetting `app.rs` compiles and fails at runtime; forgetting `mock.js` fails `screenshots.rs`'s test.
- **`ui/tsconfig.json` typechecks test files too**, so a changed interface breaks the literals in `library.test.ts` and `npm run check` fails on it.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not proof. Revert with an exact replacement, never a loose `sed`.
- **Never launch the GUI to verify a change.** The gates plus the README's smoke checklist are the verification.
- **`MAX_ROWS` is 1000** (`crates/photon-app/src/commands.rs:23`); `clamp_count` silently truncates a larger `grid_rows` ask.

---

### Task 1: `picasa::set_stars` — one rewrite per folder

**Files:**
- Modify: `crates/photon-core/src/picasa.rs` (`set_star` at :86, module doc at :1-13)
- Test: `crates/photon-core/src/picasa.rs` (the `#[cfg(test)] mod tests` at the bottom)

**Interfaces:**
- Consumes: the private `rewrite(bytes, file_name, starred) -> Option<Vec<u8>>` (:302), `ini_path`, `read_capped`, `write_atomically`, `NEW_INI`.
- Produces: `pub fn set_stars(dir: &Path, changes: &[(&str, bool)]) -> io::Result<bool>` — applies every change to one folder's INI in a single atomic write, returning whether the file changed. `pub fn set_star(dir: &Path, file_name: &str, starred: bool) -> io::Result<bool>` keeps its exact signature and behaviour, now as the one-element case.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `crates/photon-core/src/picasa.rs`:

```rust
#[test]
fn set_stars_writes_every_change_in_one_pass() {
    // The whole file is asserted, not just "both are starred": a per-photo loop would
    // also leave both starred, and the point of this writer is the single rewrite.
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nstar=yes\n[c.jpg]\nbackuphash=7\n");
    assert!(
        set_stars(
            dir.path(),
            &[("a.jpg", false), ("b.jpg", true), ("c.jpg", true)]
        )
        .unwrap()
    );
    assert_eq!(
        ini(dir.path(), ".picasa.ini"),
        b"[a.jpg]\n[c.jpg]\nstar=yes\nbackuphash=7\n[b.jpg]\nstar=yes\n",
        "one rewrite: a unstarred in place, c starred under its own header, b appended"
    );
    assert_eq!(entries(dir.path()), vec![".picasa.ini"], "no temporary left behind");
    assert_eq!(stars(dir.path()), vec!["b.jpg", "c.jpg"]);
}

#[test]
fn set_stars_that_changes_nothing_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let before: &[u8] = b"[a.jpg]\nstar=yes\n";
    write_file(dir.path(), ".picasa.ini", before);
    assert!(!set_stars(dir.path(), &[("a.jpg", true), ("b.jpg", false)]).unwrap());
    assert_eq!(ini(dir.path(), ".picasa.ini"), before);
}

#[test]
fn set_stars_creates_no_ini_when_nothing_is_being_starred() {
    // Same promise `unstarring_in_a_folder_without_an_ini_creates_nothing` makes: a folder
    // photon has never starred in must not grow a file because a selection was unstarred.
    let dir = tempfile::tempdir().unwrap();
    assert!(!set_stars(dir.path(), &[("a.jpg", false), ("b.jpg", false)]).unwrap());
    assert!(entries(dir.path()).is_empty());
}

#[test]
fn set_stars_edits_the_undotted_ini_in_place() {
    // Creating `.picasa.ini` beside an existing `Picasa.ini` makes the reader prefer the
    // new file and silently drop every star in the old one.
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "Picasa.ini", b"[z.jpg]\nstar=yes\n");
    assert!(set_stars(dir.path(), &[("a.jpg", true), ("b.jpg", true)]).unwrap());
    assert_eq!(entries(dir.path()), vec!["Picasa.ini"]);
    assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg", "z.jpg"]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core set_stars`
Expected: FAIL — `cannot find function 'set_stars' in this scope`. (A compile error is only the starting gun here; Step 4 is where the behaviour is proven, and Step 6 is where the discrimination is proven.)

- [ ] **Step 3: Write the implementation**

Replace the body of `set_star` and add `set_stars` above it in `crates/photon-core/src/picasa.rs`:

```rust
/// Sets or clears `file_name`'s star in `dir`'s Picasa INI, and returns whether the file
/// changed.
///
/// [`set_stars`] with one change; see it for what is and is not written.
pub fn set_star(dir: &Path, file_name: &str, starred: bool) -> io::Result<bool> {
    set_stars(dir, &[(file_name, starred)])
}

/// Applies every `(file_name, starred)` change to `dir`'s Picasa INI in **one** rewrite,
/// and returns whether the file changed.
///
/// One rewrite, not one per photo: starring a selection of two hundred photos in a folder
/// through `set_star` would read, rewrite and rename the same file two hundred times. The
/// changes are folded in memory through the same `rewrite` the single-photo case uses, so
/// the writer keeps sharing its line classifier with the reader.
///
/// Edits the same file `read_stars` would read. That matters when a folder carries only an
/// old `Picasa.ini`: creating `.picasa.ini` beside it would make the reader prefer the new
/// file and silently drop every other star in the folder. A folder with no INI gets a
/// `.picasa.ini` when something is being starred, and nothing at all when every change is
/// an unstar — there is no star to clear, so there is nothing to write.
///
/// Every byte outside the affected `star=` lines is preserved, including bytes that are not
/// UTF-8 and the file's own line endings: the INI carries Picasa's face, crop and edit
/// records, and photon has no business rewriting them. Changes that ask for what the file
/// already says leave it alone, so its modification time does not move. The write is atomic
/// (a temporary file renamed over the original), so a crash cannot leave a truncated INI.
pub fn set_stars(dir: &Path, changes: &[(&str, bool)]) -> io::Result<bool> {
    let (path, mut bytes, template) = match ini_path(dir)? {
        Some(path) => {
            let bytes = read_capped(&path)?;
            let meta = fs::metadata(&path)?;
            (path, bytes, Some(meta))
        }
        None if changes.iter().any(|&(_, starred)| starred) => {
            (dir.join(NEW_INI), Vec::new(), None)
        }
        None => return Ok(false),
    };
    let mut changed = false;
    for &(file_name, starred) in changes {
        if let Some(rewritten) = rewrite(&bytes, file_name, starred) {
            bytes = rewritten;
            changed = true;
        }
    }
    if !changed {
        return Ok(false);
    }
    write_atomically(&path, &bytes, template.as_ref())?;
    Ok(true)
}
```

Update the module doc at the top of the file: `set_star` → "`set_star` and `set_stars` are the one place photon writes inside a watched folder: they set or clear `star=` lines and leave every other byte of the file as they found it."

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core picasa`
Expected: PASS — the four new tests and every existing `set_star` test, which now exercise the new writer.

- [ ] **Step 5: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

- [ ] **Step 6: Demonstrate the first test discriminates**

Temporarily replace the `for` loop body in `set_stars` with a version that writes the file per change (the behaviour this task removes):

```rust
    for &(file_name, starred) in changes {
        if let Some(rewritten) = rewrite(&bytes, file_name, starred) {
            bytes = rewritten;
            changed = true;
            write_atomically(&path, &bytes, template.as_ref())?;
        }
    }
```

Run: `cargo test -p photon-core set_stars`
Expected: `set_stars_writes_every_change_in_one_pass` still passes — which proves the *file contents* assertion alone does not discriminate. So also revert `set_star` to its original standalone body (Step 3's diff, reversed) and confirm `set_stars` fails to compile — then restore Step 3 exactly. Record in the commit message that the single-rewrite property is enforced by construction (one `write_atomically` call, after the loop) rather than by a test, because a temporary file's absence is all an observer can see and `entries()` already asserts that.

- [ ] **Step 7: Commit**

```bash
git add crates/photon-core/src/picasa.rs
git commit -m "feat(core): picasa::set_stars applies a folder's stars in one rewrite

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `Engine::set_stars` and the `set_stars` command

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (`set_star` at :463)
- Modify: `crates/photon-app/src/commands.rs` (`set_star` at :478)
- Modify: `crates/photon-app/src/ipc.rs`
- Modify: `crates/photon-app/src/app.rs` (`tauri::generate_handler![...]`)
- Modify: `ui/src/lib/api.ts` (`setStar` at :163)
- Modify: `crates/xtask/screenshots/mock.js` (`canned`, :73)
- Test: `crates/photon-app/src/engine.rs` tests

**Interfaces:**
- Consumes: `photon_core::picasa::set_stars` (Task 1); `self.lib.item`, `self.lib.set_ratings(&[(i64, u8)])`, `self.refresh_grid()`, the `ini_write` mutex.
- Produces:
  - `Engine::set_stars(&self, ids: &[i64], starred: bool) -> Result<usize>` — the number of photos whose star actually landed.
  - `commands::set_stars(engine: &Engine, ids: &[i64], starred: bool) -> CmdResult<usize>`.
  - `api.setStars(ids: number[], starred: boolean): Promise<number>` in `ui/src/lib/api.ts`.

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `crates/photon-app/src/engine.rs`:

```rust
#[test]
fn set_stars_writes_one_ini_per_folder_and_refreshes_once() {
    let img = jpeg(16, 16);
    let f = fixture(&[
        ("a.jpg", &img),
        ("b.jpg", &img),
        ("sub/c.jpg", &img),
    ]);
    f.add_photos();
    let ids = f.ids();
    let version = f.engine.grid().0;

    assert_eq!(f.engine.set_stars(&ids, true).unwrap(), 3);

    assert_eq!(
        std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
        b"[a.jpg]\r\nstar=yes\r\n[b.jpg]\r\nstar=yes\r\n",
        "both of this folder's photos in one file"
    );
    assert_eq!(
        std::fs::read(f.photos.join("sub").join(".picasa.ini")).unwrap(),
        b"[c.jpg]\r\nstar=yes\r\n"
    );
    let info = crate::commands::grid_info(&f.engine);
    assert_eq!(info.starred_count, 3);
    assert_eq!(
        f.engine.grid().0,
        version + 1,
        "one refresh for the whole batch, not one per photo"
    );
}

#[test]
fn set_stars_skips_a_folder_it_cannot_write_and_reports_the_count() {
    // An oversized INI is unreadable (MAX_INI), which is how the single-photo test
    // arranges a failing write. The other folder must still be starred: a read-only
    // folder in a selection cannot cost the user every other photo in it.
    let img = jpeg(16, 16);
    let f = fixture(&[("a.jpg", &img), ("sub/c.jpg", &img)]);
    f.add_photos();
    let ids = f.ids();
    std::fs::write(
        f.photos.join(".picasa.ini"),
        vec![b' '; photon_core::picasa::MAX_INI as usize + 1],
    )
    .unwrap();

    assert_eq!(f.engine.set_stars(&ids, true).unwrap(), 1);

    assert_eq!(
        std::fs::read(f.photos.join("sub").join(".picasa.ini")).unwrap(),
        b"[c.jpg]\r\nstar=yes\r\n"
    );
    let starred: Vec<i64> = f
        .engine
        .grid()
        .1
        .rows(0, 2)
        .iter()
        .filter(|e| e.starred)
        .map(|e| e.id)
        .collect();
    assert_eq!(starred.len(), 1, "only the folder that could be written");
    assert_ne!(
        f.engine.lib.item(ids[0]).unwrap().unwrap().rating,
        Some(1),
        "a folder whose INI write failed gets no rating either"
    );
}

#[test]
fn set_stars_fails_when_no_folder_could_be_written() {
    let img = jpeg(16, 16);
    let f = fixture(&[("a.jpg", &img)]);
    f.add_photos();
    let ids = f.ids();
    let version = f.engine.grid().0;
    std::fs::write(
        f.photos.join(".picasa.ini"),
        vec![b' '; photon_core::picasa::MAX_INI as usize + 1],
    )
    .unwrap();

    let err: crate::error::AppError = f.engine.set_stars(&ids, true).unwrap_err().into();

    assert_eq!(err.kind, "iniWrite");
    assert_eq!(f.engine.grid().0, version, "nothing landed, nothing to refresh");
}

#[test]
fn set_stars_ignores_unknown_and_missing_photos() {
    let img = jpeg(16, 16);
    let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
    f.add_photos();
    let ids = f.ids();
    f.engine.lib.mark_missing(&[ids[0]], 1).unwrap();

    assert_eq!(
        f.engine.set_stars(&[ids[0], ids[1], ids[1] + 1000], true).unwrap(),
        1,
        "the missing one and the unknown one are skipped, not fatal"
    );
    assert_eq!(
        std::fs::read(f.photos.join(".picasa.ini")).unwrap(),
        b"[b.jpg]\r\nstar=yes\r\n"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-app set_stars`
Expected: FAIL — no method `set_stars` on `Engine`.

- [ ] **Step 3: Write the engine method**

In `crates/photon-app/src/engine.rs`, leave `set_star` as it is and add beside it:

```rust
    /// Stars or unstars several photos, returning how many landed.
    ///
    /// Grouped by directory so each folder's INI is rewritten once (`picasa::set_stars`),
    /// with one rating write and one `refresh_grid` for the whole batch: the per-photo
    /// route would rewrite the same file once per photo and rebuild the grid as many times.
    ///
    /// A folder whose INI cannot be written is **skipped**, not fatal - one read-only
    /// folder in a selection must not cost the user every other photo in it. The count tells
    /// the caller how many landed so it can say so. When no folder at all could be written
    /// the first error is returned instead, so the user sees the reason rather than a zero.
    ///
    /// Unknown and missing ids are skipped for the same reason: a selection can outlive the
    /// photos in it (a scan purges one between the right-click and the click), and that is
    /// not a failure of the other eleven. `set_star`, which acts on the photo the user is
    /// looking at, still refuses them - see `live_item` for why the two differ.
    pub fn set_stars(&self, ids: &[i64], starred: bool) -> Result<usize> {
        let _serialised = self.ini_write.lock();
        let mut by_dir: BTreeMap<PathBuf, Vec<(i64, String)>> = BTreeMap::new();
        for &id in ids {
            let Some(item) = self.lib.item(id)? else {
                continue;
            };
            if item.missing_since.is_some() {
                continue;
            }
            let path = PathBuf::from(&item.path);
            let (Some(dir), Some(file_name)) = (path.parent(), path.file_name()) else {
                continue;
            };
            by_dir
                .entry(dir.to_path_buf())
                .or_default()
                .push((id, file_name.to_string_lossy().into_owned()));
        }

        let mut ratings: Vec<(i64, u8)> = Vec::new();
        let mut first_error: Option<Error> = None;
        for (dir, files) in &by_dir {
            let changes: Vec<(&str, bool)> =
                files.iter().map(|(_, name)| (name.as_str(), starred)).collect();
            match picasa::set_stars(dir, &changes) {
                Ok(_) => ratings.extend(files.iter().map(|&(id, _)| (id, u8::from(starred)))),
                Err(source) => {
                    // The file first, then the database, per folder: a rating written for a
                    // folder whose INI never took it is shown to the user and then silently
                    // cleared by the next scan.
                    first_error.get_or_insert(Error::IniWrite {
                        path: dir.join(picasa::ini_name(dir)),
                        source,
                    });
                }
            }
        }

        if ratings.is_empty() {
            return match first_error {
                Some(err) => Err(err),
                None => Ok(0),
            };
        }
        self.lib.set_ratings(&ratings)?;
        self.refresh_grid()?;
        Ok(ratings.len())
    }
```

Add `use std::collections::BTreeMap;` to the file's imports if it is not already there (`BTreeMap`, not `HashMap`, so the folder order — and therefore which error is "first" — is deterministic).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-app set_stars`
Expected: PASS, all four.

- [ ] **Step 5: Add the command, the wrapper, the handler, the mirror and the mock**

`crates/photon-app/src/commands.rs`, beside `set_star`:

```rust
/// Stars or unstars several photos, returning how many landed. See `Engine::set_stars` for
/// why a folder that cannot be written is skipped rather than fatal.
pub fn set_stars(engine: &Engine, ids: &[i64], starred: bool) -> CmdResult<usize> {
    Ok(engine.set_stars(ids, starred)?)
}
```

`crates/photon-app/src/ipc.rs`, in the same shape as the neighbouring wrappers:

```rust
#[tauri::command(async)]
pub fn set_stars(state: State<'_, Arc<Engine>>, ids: Vec<i64>, starred: bool) -> CmdResult<usize> {
    commands::set_stars(&state, &ids, starred)
}
```

(Match the surrounding wrappers exactly — if they take `state: State<'_, Arc<Engine>>`, so does this one; if they use a different alias, follow it.)

`crates/photon-app/src/app.rs`: add `ipc::set_stars` to `tauri::generate_handler![...]`, next to `ipc::set_star`.

`ui/src/lib/api.ts`, under `setStar`:

```ts
  /** Stars or unstars several photos at once, answering how many landed: a folder whose
   *  `.picasa.ini` cannot be written is skipped, and the caller says so. */
  setStars: (ids: number[], starred: boolean) => invoke<number>('set_stars', { ids, starred }),
```

`crates/xtask/screenshots/mock.js`, in `canned` (keys at four spaces' indent, alphabetical among its neighbours) — a canned answer rather than `SILENT`, because the caller reads the number:

```js
    set_stars: (args) => (args.ids || []).length,
```

- [ ] **Step 6: Run both gates**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
```

Expected: all pass, including `screenshots.rs`'s test that every `api.ts` command has an answer in `mock.js`.

- [ ] **Step 7: Demonstrate the tests discriminate**

One at a time, restore each line afterwards exactly:

1. In `set_stars`, move `self.refresh_grid()?;` inside the `for (dir, files)` loop. Run `cargo test -p photon-app set_stars_writes_one_ini_per_folder_and_refreshes_once` — expected FAIL on the version assertion (`version + 2`).
2. Change the `Err(source)` arm to `return Err(...)` instead of recording it. Run `cargo test -p photon-app set_stars_skips_a_folder` — expected FAIL: the other folder never gets starred.
3. Change `if ratings.is_empty()` to `if false`. Run `cargo test -p photon-app set_stars_fails_when_no_folder` — expected FAIL: `Ok(0)` where an error was expected.
4. Replace the `continue` for `missing_since` with nothing (star it anyway). Run `cargo test -p photon-app set_stars_ignores_unknown` — expected FAIL on the INI contents.

- [ ] **Step 8: Commit**

```bash
git add crates/photon-app/src/engine.rs crates/photon-app/src/commands.rs \
        crates/photon-app/src/ipc.rs crates/photon-app/src/app.rs \
        ui/src/lib/api.ts crates/xtask/screenshots/mock.js
git commit -m "feat(app): set_stars stars a selection, one INI write per folder

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The selection set in `LibraryStore`

**Files:**
- Modify: `ui/src/lib/library.svelte.ts` (`selectedOffset` at :48, `selected` at :53, `selectItem` at :67, `switchView` at :241, `setSearchQuery` at :318)
- Test: `ui/src/lib/library.test.ts`

**Interfaces:**
- Consumes: `api.gridRows(offset, count)`, `this.pages.get(offset)`, `this.info.version`, `this.info.len`.
- Produces, on `LibraryStore`:
  - `get selected(): number | null` / `set selected(offset)` — the lead, unchanged signature; the setter now also replaces the selection with that one id and moves the anchor.
  - `selectItem(offset: number, id: number): void` — unchanged signature; replaces the selection unless `id` is already the lead's id.
  - `toggleSelected(offset: number): void`
  - `extendSelection(offset: number): Promise<void>`
  - `clearSelection(): void`
  - `isSelected(id: number): boolean`
  - `get selectionCount(): number`
  - `get selectedItemIds(): number[]`

- [ ] **Step 1: Write the failing tests**

Add a `describe` block to `ui/src/lib/library.test.ts`. The file already mocks `./api`
wholesale and builds a store per test with `new LibraryStore()` + `await store.init()`; it
has no shared fixture, so these declare their own, in the shape the existing tests use
(`gridRows` answers `{ version, rows }`, and `GridEntry` needs all eight fields or
`npm run check` fails on the literal).

```ts
  describe('multi-selection', () => {
    /** Photo ids are offset + 100, so a wrong offset is visible in the failure message. */
    const idAt = (offset: number) => offset + 100;
    const entryAt = (offset: number) => ({
      id: idAt(offset),
      folderId: 1,
      takenAt: 0,
      aspect: 1,
      kind: 'image' as const,
      thumbKey: '0',
      starred: false,
    });

    /** A store over `len` photos, with every page answerable. */
    async function storeOf(len: number) {
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 1,
        len,
        sections: [],
        starredCount: 0,
        duplicateCount: 0,
        view: 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 1,
        rows: Array.from({ length: Math.min(count, len - offset) }, (_, i) => entryAt(offset + i)),
      }));
      const store = new LibraryStore();
      await store.init();
      await store.ensure(0, Math.min(len, 50));
      return store;
    }

    it('ctrl+click toggles a photo in and out, moving the lead each time', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      expect(store.selectedItemIds).toEqual([idAt(2)]);

      store.toggleSelected(5);
      expect([...store.selectedItemIds].sort()).toEqual([idAt(2), idAt(5)].sort());
      expect(store.selected).toBe(5);

      store.toggleSelected(5);
      expect(store.selectedItemIds).toEqual([idAt(2)]);
      expect(store.selected).toBe(5);
    });

    it('a plain selection replaces the whole set', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(5);
      store.selected = 7;
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selectionCount).toBe(1);
    });

    it('shift+click selects the range from the anchor, chunking past MAX_ROWS', async () => {
      // 1301 > MAX_ROWS (1000): one un-chunked grid_rows call is silently truncated by
      // clamp_count, and the range's last three hundred photos would go unselected with
      // no error anywhere.
      const store = await storeOf(1500);
      store.selected = 100;
      vi.mocked(api.gridRows).mockClear(); // only this call's fetches count below

      await store.extendSelection(1400);

      expect(store.selectionCount).toBe(1301);
      expect(store.isSelected(idAt(1400))).toBe(true);
      expect(store.isSelected(idAt(99))).toBe(false);
      expect(vi.mocked(api.gridRows).mock.calls.filter(([, count]) => count > 1000)).toEqual([]);
      expect(store.selected).toBe(1400);
    });

    it('shift+click twice re-ranges from the same anchor', async () => {
      const store = await storeOf(20);
      store.selected = 5;
      await store.extendSelection(10);
      await store.extendSelection(7);
      expect(store.selectionCount).toBe(3);
      expect(store.isSelected(idAt(10))).toBe(false);
      expect(store.isSelected(idAt(5))).toBe(true);
    });

    it('extends backwards from the anchor too', async () => {
      const store = await storeOf(20);
      store.selected = 10;
      await store.extendSelection(6);
      expect(store.selectionCount).toBe(5);
      expect(store.isSelected(idAt(6))).toBe(true);
      expect(store.isSelected(idAt(11))).toBe(false);
    });

    it('keeps the selection across a refresh that shifts every offset', async () => {
      // The test that discriminates ids from offsets. A scan indexing a photo into an
      // earlier folder moves every later offset by one; an offset-based selection would
      // silently ring - and star - the neighbours of what the user picked.
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);
      const picked = [...store.selectedItemIds].sort();

      // One photo appears ahead of them all, so every old offset is now one later.
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 2,
        len: 11,
        sections: [],
        starredCount: 0,
        duplicateCount: 0,
        view: 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(5);
      await store.refresh();

      expect([...store.selectedItemIds].sort()).toEqual(picked);
      expect(store.selected).toBe(5);
    });

    it('a view switch clears the selection', async () => {
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);
      vi.mocked(api.setGridView).mockResolvedValue(undefined);

      await store.setView('starred');

      expect(store.selectionCount).toBe(0);
      expect(store.selected).toBeNull();
    });

    it('selectItem collapses only when it names a different photo', async () => {
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);

      store.selectItem(4, idAt(4)); // the viewer closing on the photo it opened with
      expect(store.selectionCount).toBe(2);

      store.selectItem(8, idAt(8)); // the viewer navigated away and closed there
      expect(store.selectedItemIds).toEqual([idAt(8)]);
    });
  });
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npm test -w ui -- src/lib/library.test.ts`
Expected: FAIL — `store.toggleSelected is not a function`.

- [ ] **Step 3: Write the implementation**

In `ui/src/lib/library.svelte.ts`, beside `selectedOffset`:

```ts
  /** The photo ids in the selection, as a set replaced wholesale on every change — Svelte's
   *  runes do not track mutation of a plain Set.
   *
   *  Ids, not offsets. An offset only means "this photo" against one version of the index,
   *  and a scan that indexes a photo into an earlier folder shifts every later one: an
   *  offset-based selection would silently ring, and star, the neighbours of what the user
   *  picked. Ids survive every rebuild untouched, which is also why there is no multi-photo
   *  counterpart to `rebindSelection`.
   *
   *  A photo purged by a scan leaves its id behind here. Nothing is drawn wrong — there is
   *  no tile left to ring — but `selectionCount` over-reports until the next plain click.
   *  Pruning it needs a "which of these ids are still live" round trip, which is not worth
   *  an IPC surface for a count one click from correct. Do not "fix" this with offsets. */
  private selection = $state<Set<number>>(new Set());
  /** The grid offset a Shift+click extends from: the last plain click or Ctrl+click. Plain,
   *  not `$state` — nothing renders from it. */
  private anchor: number | null = null;
```

The lead setter, replacing the existing one:

```ts
  set selected(offset: number | null) {
    this.selectedOffset = offset;
    // Which photo that offset meant, remembered now while the page holding it is loaded -
    // by the time the grid is rebuilt the pages are gone.
    const id = offset === null ? null : (this.pages.get(offset)?.id ?? null);
    this.selectedId = id;
    // Every caller of the plain setter is a collapse: an arrow key, a plain click, a
    // right-click outside the selection. Keeping that rule here rather than at each call
    // site is what stops a new caller silently leaving a stale multi-selection behind.
    this.selection = id === null ? new Set() : new Set([id]);
    this.anchor = offset;
  }
```

`selectItem`, replacing the existing one:

```ts
  selectItem(offset: number, id: number): void {
    // Closing the viewer on the photo it was opened with keeps the selection; navigating
    // away inside the viewer and closing there collapses to the photo on screen.
    const collapse = id !== this.selectedId;
    this.selectedOffset = offset;
    this.selectedId = id;
    if (collapse) this.selection = new Set([id]);
    this.anchor = offset;
  }
```

And the new methods:

```ts
  /** Ctrl/Cmd+click: adds or removes one photo. The lead and the anchor move to it either
   *  way, so the next Shift+click extends from where the user last clicked. */
  toggleSelected(offset: number): void {
    const id = this.pages.get(offset)?.id;
    if (id === undefined) return;
    const next = new Set(this.selection);
    if (!next.delete(id)) next.add(id);
    this.selection = next;
    this.selectedOffset = next.size === 0 ? null : offset;
    this.selectedId = next.size === 0 ? null : id;
    this.anchor = offset;
  }

  /** Shift+click: replaces the selection with the range between the anchor and `offset`.
   *
   *  The ids come from the backend rather than from the loaded pages: a range can span
   *  thousands of photos the grid has never rendered. `MAX_ROWS` in `commands.rs` clamps a
   *  `grid_rows` ask to 1000 *silently*, so a single call for a wider range would select its
   *  first thousand photos and drop the rest without an error anywhere. */
  async extendSelection(offset: number): Promise<void> {
    const from = this.anchor ?? this.selectedOffset ?? 0;
    const start = Math.max(0, Math.min(from, offset));
    const end = Math.min(this.info.len - 1, Math.max(from, offset));
    if (end < start) return;
    const version = this.info.version;
    const ids = new Set<number>();
    for (let at = start; at <= end; at += GRID_ROWS_CHUNK) {
      const count = Math.min(GRID_ROWS_CHUNK, end - at + 1);
      const rows = await api.gridRows(at, count);
      // A refresh has landed while this was in flight; its own selection is the current one.
      if (version !== this.info.version) return;
      for (const entry of rows.entries) ids.add(entry.id);
    }
    this.selection = ids;
    this.selectedOffset = offset;
    this.selectedId = this.pages.get(offset)?.id ?? null;
    // The anchor stays put, so dragging the far end back and forth re-ranges from the
    // same start rather than walking away from it.
  }

  clearSelection(): void {
    this.selection = new Set();
    this.selectedOffset = null;
    this.selectedId = null;
    this.anchor = null;
  }

  isSelected(id: number): boolean {
    return this.selection.has(id);
  }

  get selectionCount(): number {
    return this.selection.size;
  }

  get selectedItemIds(): number[] {
    return [...this.selection];
  }
```

With, near the top of the file:

```ts
/** How many rows one `gridRows` call may ask for: `MAX_ROWS` in `commands.rs`, which
 *  `clamp_count` applies without telling the caller it truncated. */
const GRID_ROWS_CHUNK = 1000;
```

Then clear the selection on every view switch — in `switchView`, after the refresh, and in `setSearchQuery`'s `try` after its refresh:

```ts
      await command();
      this.clearSelection();
      await this.refresh();
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm test -w ui -- src/lib/library.test.ts`
Expected: PASS.

- [ ] **Step 5: Run the UI gate**

```bash
npm run check
npm test
```

- [ ] **Step 6: Demonstrate the tests discriminate**

Restore each line exactly afterwards:

1. In `extendSelection`, replace the chunking loop with one `await api.gridRows(start, end - start + 1)`. Run `npm test -w ui -- src/lib/library.test.ts -t 'chunking'` — expected FAIL: 1000 selected, not 1301.
2. Make `selection` hold offsets instead of ids (`next.add(offset)` in `toggleSelected`, `new Set([offset])` in the setter) and `isSelected` compare offsets. Run the suite — expected FAIL on "keeps the selection across a refresh that shifts every offset".
3. Drop the `collapse` check from `selectItem` (always replace). Run `-t 'selectItem collapses'` — expected FAIL: the count is 1, not 2.
4. Remove `this.clearSelection()` from `switchView`. Run `-t 'a view switch clears'` — expected FAIL.

- [ ] **Step 7: Commit**

```bash
git add ui/src/lib/library.svelte.ts ui/src/lib/library.test.ts
git commit -m "feat(ui): the grid selection is a set of photo ids

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The gestures and the context menu

**Files:**
- Modify: `ui/src/components/Tile.svelte` (the `onselect` prop at :19, the `<button>` at :84)
- Modify: `ui/src/components/Grid.svelte` (`onkeydown` at :141, `tileMenu` at :172, `withEntry` at :186, the `<Tile>` at :234, the menu markup)
- Modify: `ui/src/App.svelte` (`open` at :78, `closeViewer` at :83)

**Interfaces:**
- Consumes: Task 3's `library.toggleSelected`, `extendSelection`, `isSelected`, `selectionCount`, `selectedItemIds`; Task 2's `api.setStars`.
- Produces: `Tile`'s `onselect: (e: MouseEvent) => void` (was `() => void`) and `selected` now meaning "my id is in the selection".

There is no component test harness (`vitest` runs in `environment: 'node'`), so this task's verification is `svelte-check`, the tests from Tasks 2 and 3 that it wires together, and the README smoke checklist added in Task 5. Say so in the commit message.

- [ ] **Step 1: Give the tile's click its event**

In `ui/src/components/Tile.svelte`, change the prop type and the handler:

```ts
    /** The click that selects. The event travels because the grid, not the tile, decides
     *  what Ctrl and Shift mean. */
    onselect: (e: MouseEvent) => void;
```

and in the markup, `onclick={onselect}` stays as it is — the handler now receives the event it was already being passed.

- [ ] **Step 2: Route the click in the grid**

In `ui/src/components/Grid.svelte`, add beside `tileMenu`:

```ts
  /** Shift extends, Ctrl/Cmd toggles, a plain click collapses to one. Shift wins when both
   *  are held, which is what every file manager does. */
  function tileClick(e: MouseEvent, offset: number) {
    if (e.shiftKey) {
      void library.extendSelection(offset).catch(library.reportError);
    } else if (e.ctrlKey || e.metaKey) {
      library.toggleSelected(offset);
    } else {
      library.selected = offset;
    }
  }
```

and change the `<Tile>` call:

```svelte
              <Tile
                {entry}
                selected={!!entry && library.isSelected(entry.id)}
                dimmed={!!entry && !library.isOnline(entry.folderId)}
                onselect={(e) => tileClick(e, offset)}
                onopen={() => onopen(offset)}
                onmenu={(e) => tileMenu(e, offset)}
              />
```

- [ ] **Step 3: Keep a right-click inside the selection**

Replace `tileMenu` in `ui/src/components/Grid.svelte`:

```ts
  /** Right-clicking outside the selection selects that tile first, so what the menu acts on
   *  is always what is outlined. Inside it, the whole selection stands. */
  function tileMenu(e: MouseEvent, offset: number) {
    e.preventDefault();
    const entry = library.entry(offset);
    if (!entry) return;
    if (!library.isSelected(entry.id)) library.selected = offset;
    menu = { x: e.clientX, y: e.clientY, entry };
  }
```

- [ ] **Step 4: Act on the selection in the menu**

Replace `withEntry` with a helper that acts on the ids, and add the star items. In the `<script>`:

```ts
  const count = $derived(library.selectionCount);
  /** "photo" / "12 photos", for menu items that name what they will act on. */
  const subject = $derived(count === 1 ? 'photo' : `${count.toLocaleString()} photos`);

  function withSelection(action: (ids: number[]) => Promise<unknown>) {
    const ids = library.selectedItemIds;
    menu = null;
    if (ids.length) action(ids).catch(library.reportError);
    focus();
  }

  /** Stars or unstars everything selected. The backend skips a folder whose `.picasa.ini`
   *  it cannot write and answers with how many landed, so a read-only folder in the
   *  selection costs the user a toast rather than the other eleven photos. */
  async function star(ids: number[], starred: boolean) {
    const done = await api.setStars(ids, starred);
    if (done < ids.length) {
      throw new Error(
        `${(ids.length - done).toLocaleString()} of ${ids.length.toLocaleString()} photos could not be ${starred ? 'starred' : 'unstarred'}`,
      );
    }
  }
```

and in the menu markup, replacing the "Reveal in file manager" line and the album buttons:

```svelte
    {#if count === 1}
      <button role="menuitem" onclick={() => withSelection((ids) => api.revealInFileManager(ids[0]))}>
        Reveal in file manager
      </button>
    {/if}
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, true))}>Star {subject}</button>
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, false))}>Unstar {subject}</button>
    {#if albumId !== null}
      <button role="menuitem" onclick={() => withSelection((ids) => library.removeFromAlbum(albumId, ids))}>
        Remove {subject} from “{library.albumName(albumId)}”
      </button>
    {/if}
    <div class="heading">Add to album</div>
    {#each library.albums as album (album.id)}
      <!-- Adding is idempotent, so the album the photos are already in is not filtered out
           here: the grid rows do not know their memberships, and asking per photo for a
           menu would be a round trip for nothing. -->
      <button role="menuitem" class="album" onclick={() => withSelection((ids) => library.addToAlbum(album.id, ids))}>
        {album.name}
      </button>
    {:else}
      <div class="none">No albums yet — create one in the sidebar.</div>
    {/each}
```

Delete `withEntry`: nothing calls it now. Its `entry` field goes with it — the menu state
becomes `let menu = $state<{ x: number; y: number } | null>(null)`, since `tileMenu` reads
the entry only to decide whether to collapse, and the items act on `library.selectedItemIds`.
Drop `GridEntry` from the `../lib/api` import unless something else in the file still uses
it: `svelte-check` warns on an unused import and `npm run check` fails on warnings.

Also fix `onkeydown`'s Ctrl+Shift+R, which must stay single-photo:

```ts
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      // One path is all a file manager takes, so this acts on the lead alone.
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) api.revealInFileManager(entry.id).catch(library.reportError);
      return;
    }
```

(unchanged — confirm it still reads the lead and is not routed through `withSelection`.)

- [ ] **Step 5: Keep the selection across the viewer**

In `ui/src/App.svelte`:

```ts
  function open(offset: number) {
    // Only when it is not already the lead: assigning collapses a multi-selection, and
    // Enter on a selection of twelve should open one photo without throwing the other
    // eleven away. (A double-click collapses anyway — the click lands first.)
    if (library.selected !== offset) library.selected = offset;
    viewerAt = offset;
  }

  function closeViewer(at: number) {
    viewerAt = null;
    // Same rule, the other way round: closing on the photo the viewer was opened with
    // leaves the selection alone; closing after navigating collapses to what is on screen.
    if (library.selected !== at) library.selected = at;
    grid?.scrollToOffset(at, 'nearest');
    grid?.focus();
  }
```

- [ ] **Step 6: Run the UI gate**

```bash
npm run check
npm test
```

Expected: 0 errors, 0 warnings; the vitest suite green (Task 3's tests cover the store this wires to).

- [ ] **Step 7: Commit**

```bash
git add ui/src/components/Tile.svelte ui/src/components/Grid.svelte ui/src/App.svelte
git commit -m "feat(ui): ctrl+click and shift+click select several photos

The menu's items act on the whole selection and name what they will act on;
Reveal stays single-photo, since a file manager takes one path.

No test: vitest runs in node and cannot render a .svelte file. The logic this
wires to is tested in library.test.ts; the wiring itself is covered by
svelte-check and the README's smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The count, the checklist and the whole-branch gate

**Files:**
- Modify: `ui/src/components/StatusBar.svelte` (the photo count at :31)
- Modify: `README.md` (`## Manual smoke checklist`)

**Interfaces:**
- Consumes: Task 3's `library.selectionCount`.
- Produces: nothing other tasks read.

- [ ] **Step 1: Show the count**

In `ui/src/components/StatusBar.svelte`, replace the count span:

```svelte
  <span>
    {#if library.selectionCount > 1}<span class="selected">{library.selectionCount.toLocaleString()} selected</span> · {/if}{library.info.len.toLocaleString()} photos
  </span>
```

and add to the `<style>` block:

```css
  /* Brighter than the footer's --text-dim: the count is the only thing here that changes
     under the user's hand, and it answers "what will the menu act on". */
  .selected { color: var(--text); }
```

Only above one selected photo: a single selection is the grid's resting state and "1 selected" beside every click is noise.

- [ ] **Step 2: Run the UI gate**

```bash
npm run check
npm test
```

Expected: 0 errors, 0 warnings. `no-literals.test.ts` stays green — `--text` is a declared token and there is no colour literal here.

- [ ] **Step 3: Add the smoke checklist lines**

In `README.md`'s `## Manual smoke checklist`, in the style of its neighbours:

```markdown
- Ctrl+click (Cmd on macOS) three photos: each gets a ring, the status bar reads
  `3 selected`. Shift+click a fourth further down: the run between the last Ctrl+click and
  it is selected. A plain click anywhere collapses back to one.
- Right-click inside a selection: the menu reads `Star 4 photos`, and Reveal is absent.
  Right-click a photo outside it: the selection collapses to that one first.
- Star a selection spanning two folders, then check both `.picasa.ini` files: each holds a
  `star=yes` under every selected photo's header, and every other line it had is untouched.
- Select several, open one with Enter and close it again: the selection is still there.
  Arrow to another photo inside the viewer and close: only that photo is selected.
```

- [ ] **Step 4: Run the full gates, both languages**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
```

- [ ] **Step 5: See it without launching it**

```bash
cargo run -p xtask -- screenshots
```

Expected: ten PNGs in `target/screenshots/`, no `mock.js has no answer for` warning on the console. (Needs Chromium on `PATH` or in `CHROMIUM`; if it is not available, say so rather than skipping silently.)

- [ ] **Step 6: Commit**

```bash
git add ui/src/components/StatusBar.svelte README.md
git commit -m "feat(ui): the status bar says how many photos are selected

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: Independent review before merge**

Per the repo convention, a branch this size gets a read by someone other than its author before it merges. Point the reviewer at:

- the effect wiring in `Grid.svelte` and `App.svelte` — the parts no test can see;
- what this **arms in old code**: every existing caller of `library.selected` now clears a
  selection as a side effect, and `Tile`'s `selected` prop changed meaning;
- `Engine::set_stars`'s partial-failure path, which is the one place a user can lose half an
  action.

