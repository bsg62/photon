# Never-reused library ids Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `watched_folders`, `folders`, `items`, `albums` and `saved_searches` never hand a
freed id to a new row, so an id the window holds names one row for ever.

**Architecture:** Migration 27 rebuilds the five tables as `INTEGER PRIMARY KEY
AUTOINCREMENT`, using SQLite's twelve-step procedure. The migration runner gains a way to
run a migration with foreign keys off, then checks `PRAGMA foreign_key_check` before it
commits. Comments, notes, tests and CLAUDE.md stop saying that ids are reused. The
window's guards (`App::deleting`, `FolderRow::going`, `App::listed`) keep their behaviour
with new reasons.

**Tech Stack:** Rust, rusqlite 0.40.2 (bundled SQLite), egui_kittest tests through
`tests/common`'s `Driver`.

**Spec:** `docs/superpowers/specs/2026-11-13-photon-never-reused-ids-design.md`

## Global Constraints

- The gate, all five, before every commit: `cargo fmt --all`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`,
  `cargo bench -p photon-core --bench grid --no-run`.
- Work on a branch `never-reused-ids` in its own worktree with its own `CARGO_TARGET_DIR`
  under the repo's `target/` (never shared with the main checkout).
- Every new test must be shown to fail with its change reverted. A compile error is not
  proof. Revert with an exact replacement, then restore and `touch` the file. Each task's
  report says which revert made which test fail.
- Schema version 26 → 27. Literal version tests are updated, never loosened; the
  table-count literal (16) stays.
- A fixture with a dangling reference is fixed, never the check loosened.
- Never `cargo run -p photon-ui`. Never launch the GUI.
- Comments carry the reasoning. A wrong justification is a defect.
- No release in this plan: releasing waits for the user's word.

## Review Focus

1. **An old library with a dangling reference.** The open must fail with an error, leave
   the library at version 26 with every row, and leave the connection's foreign keys on,
   so an older photon still opens the library. Pinned in Task 1
   (`a_failing_foreign_key_check_rolls_back_and_leaves_foreign_keys_on`).
2. **A library from long before 26.** One that starts at version 2 or 9 runs every
   migration through the new runner path in one open. The existing `the_third_migration_…`
   and `the_tenth_migration_…` tests now run through 27. Their version literals are
   updated to 27, and Task 2 adds an assertion to one of them that its rows' ids survived.
3. **`last_folder` that isn't a number** (garbage text, negative, or below the highest
   folder). The migration must not fail and must not lower the sequence. Pinned in Task 2
   (`migration_27_ignores_a_last_folder_that_is_not_above_every_folder`).
4. **Uniqueness after the rebuild.** A second watched folder, folder or item with an
   existing path must still be refused, and a second album with the same
   `picasa_token` too. Pinned in Task 2 by the index-order test (the automatic indexes
   are in the list) and by `migration_27_keeps_the_unique_paths`.
5. **A large library's first launch.** The upgrade runs before the window exists. It is
   measured in Task 2 (100,000 items, release build) and the time goes in the PR.

---

### Task 1: The migration runner can run a migration with foreign keys off

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (`migrate`, near line 432; tests module)

**Interfaces:**
- Produces: `const WITHOUT_FOREIGN_KEYS: &[usize]`, the 1-based schema versions whose
  migration runs with foreign keys off. Empty in this task; Task 2 adds `27`.
- Produces: `fn run(conn: &Connection, migrations: &[&str], without_foreign_keys: &[usize]) -> Result<()>`,
  private to `schema.rs`. `migrate(conn)` becomes `run(conn, MIGRATIONS, WITHOUT_FOREIGN_KEYS)`.
  The `SchemaTooNew` check stays in `run`, against `migrations.len()`.
- Produces: `Error::ForeignKeyCheck { version: i64, violations: usize }` in
  photon-core's `Error` (find it with `grep -n "SchemaTooNew" crates/photon-core/src`).
  Its message says the library's references don't hold together and the upgrade to
  `version` was not made.

- [ ] **Step 1: Write the failing tests** (in `schema.rs`'s tests, on a raw
  `rusqlite::Connection` opened on a temp file, with `PRAGMA foreign_keys = ON` set as
  `configure()` does). A toy schema serves as migrations 1 and 2:
  1. `CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL); CREATE TABLE child (parent_id INTEGER NOT NULL REFERENCES parent(id) ON DELETE CASCADE);`
  2. A rebuild of `parent`: `CREATE TABLE parent_new (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL); INSERT INTO parent_new (id, name) SELECT id, name FROM parent; DROP TABLE parent; ALTER TABLE parent_new RENAME TO parent;`

  - `a_rebuild_run_without_foreign_keys_keeps_the_rows_that_reference_it`: run migration
    1, insert `parent(1,'a')` and `child(1)`, run both with `without_foreign_keys = &[2]`.
    Assert `child` still has 1 row, `user_version` = 2,
    `PRAGMA foreign_keys` = 1 afterwards, and deleting parent 1 then cascades (`child` is
    empty).
  - `a_failing_foreign_key_check_rolls_back_and_leaves_foreign_keys_on`: run migration
    1, then with `PRAGMA foreign_keys = OFF` insert a `child(99)` that has no parent, and
    switch foreign keys back on. Running both must return
    `Err(Error::ForeignKeyCheck { version: 2, violations: 1 })`. Afterwards
    `user_version` = 1, the `parent` table's sql has no `AUTOINCREMENT`, `child` still has
    its row, and `PRAGMA foreign_keys` = 1.

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo nextest run -p photon-core a_rebuild_run_without_foreign_keys a_failing_foreign_key_check`
  Expected: compile error (no `run`). Then, with `run` stubbed as today's loop, the
  first test fails with `child` at 0 rows (the drop cascaded), and the second with `Ok`.
  That is the revert proof: record it.

- [ ] **Step 3: Implement `run`**

  For each pending migration whose version (index + 1) is in `without_foreign_keys`, do
  the following in order:
  - Run `PRAGMA foreign_keys = OFF` **before** the transaction. Inside a transaction it
    is a silent no-op.
  - Begin the transaction and execute the SQL.
  - Run `PRAGMA foreign_key_check` and count its rows. If there are any, drop the
    transaction (rollback) and produce `ForeignKeyCheck`.
  - Otherwise set `user_version` and commit.
  - Restore `PRAGMA foreign_keys = ON` on every path: success, `foreign_key_check`
    failure, and a SQL error. Use a guard or an explicit match, not `?` before the
    restore.

  Other migrations run exactly as today. A comment says why foreign keys are off: dropping
  a table that others reference with `ON DELETE CASCADE` deletes their rows when foreign
  keys are on. It also says why the check runs inside the transaction: so a failure
  leaves the library at the old version, which the previous photon still opens.

- [ ] **Step 4: Run the tests and the whole crate**

  Run: `cargo nextest run -p photon-core`
  Expected: all pass (migration 27 does not exist yet; `WITHOUT_FOREIGN_KEYS` is empty).

- [ ] **Step 5: Gate and commit**

  ```bash
  git commit -m "feat(core): migrations can rebuild a referenced table with foreign keys off"
  ```

### Task 2: Migration 27 makes five tables AUTOINCREMENT

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (`MIGRATIONS` gains entry 27,
  `WITHOUT_FOREIGN_KEYS = &[27]`, the migration-25 comment, the 25/26 tests'
  `user_version` literals, the 3rd/10th migration tests' literals, new tests)
- Modify: `crates/photon-core/src/library/mod.rs` (`open_creates_schema_and_is_idempotent`
  and any other literal 26: `grep -n "26" crates/photon-core/src/library/mod.rs`)
- Modify: `crates/photon-core/src/thumbs/inflight.rs` (module doc near lines 18-28, the
  comment near line 155, the message near line 497), `crates/photon-core/src/thumbs/service.rs` (near line 894)
- Modify: `docs/architecture/library.md` (`### Schema`), `CLAUDE.md` (the "An offset is valid…" rule)

**Interfaces:**
- Consumes: `WITHOUT_FOREIGN_KEYS` and `run` from Task 1.
- Produces: schema version 27. Every later task and test sees `AUTOINCREMENT` on the
  five tables.

- [ ] **Step 1: Write the failing tests** (schema.rs tests)

  - Five tests through `next_id_after_deleting_the_highest`, each asserting
    `next > deleted`:
    - `a_deleted_watched_folders_id_is_never_reused`:
      `INSERT INTO watched_folders (path) VALUES (hex(randomblob(8)))`
    - `a_deleted_folders_id_is_never_reused`: one watched folder made first, then
      `INSERT INTO folders (watched_id, path, name, sort_key) VALUES (<w>, hex(randomblob(8)), 'f', 'f')`
    - `a_deleted_items_id_is_never_reused`: a folder made first, then an items insert with
      every `NOT NULL` column and `hex(randomblob(8))` as the path
    - `a_deleted_albums_id_is_never_reused`:
      `INSERT INTO albums (name, created_ms) VALUES ('a', 1)`
    - `a_deleted_saved_searches_id_is_never_reused`:
      `INSERT INTO saved_searches (name, query, created_ms) VALUES ('s', 'q', 1)`

    Update the helper's doc comment, which today names only the People page.
  - `migration_27_keeps_every_row_and_reference`: build a version-26 library on a raw
    connection the way `migration_26_adds_the_move_index_…` does (`MIGRATIONS[..26]`,
    `user_version` 26). Fill it with:
    - two watched folders, and a root folder with one child folder under the first;
    - four items across both folders, with the highest-id item then deleted, so there is
      a gap;
    - an `album_items` row, an `item_tags` row, an `item_user_tags` row, a Picasa `faces`
      row, a `people` row, a `detected_faces` row on that person, and a `face_rejections`
      row;
    - an album with a `picasa_token`, a saved search, and `settings` `last_folder` set to
      the child folder's id.

    Snapshot each table as `SELECT * … ORDER BY rowid`, read into `Vec<Vec<rusqlite::types::Value>>`.
    Then `Library::open` it and assert:
    - every snapshot is equal;
    - `PRAGMA foreign_key_check` returns no rows and `user_version` is 27;
    - an item inserted next gets an id above the highest surviving one;
    - deleting the first watched folder through the writer cascades to its folders, its
      items and their `album_items`/`item_tags` rows.
  - `migration_27_keeps_the_tables_shape_and_their_indexes_in_order`: on the same kind of
    version-26 fixture, record, for each of the five tables:
    - `PRAGMA table_info(t)` and `PRAGMA foreign_key_list(t)` as value rows;
    - `SELECT name, sql FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1 ORDER BY rowid`.

    Open the library and assert all three are equal for each table. Also assert that each
    table's `sql` in `sqlite_master` contains `AUTOINCREMENT`.
  - `migration_27_keeps_the_unique_paths`: after the upgrade, inserting a second
    `watched_folders`, `folders` and `items` row with an existing path each fails with a
    constraint error, and so does an album with an existing `picasa_token`.
  - `migration_27_never_hands_out_the_remembered_folders_id`: a version-26 fixture with
    folders 1 and 2, `last_folder` = '2', then folder 2 deleted. After the upgrade, a new
    folder's id is above 2.
  - `migration_27_ignores_a_last_folder_that_is_not_above_every_folder`: three fixtures,
    with `last_folder` set to `'garbage'`, `'-5'` and `'1'`, and folders 1..=3 present.
    Each one upgrades without error, and a new folder gets id 4.
  - In the existing `the_third_migration_…` test (which starts at version 2), also assert
    that the rows it inserted keep their ids.

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo nextest run -p photon-core never_reused migration_27`
  Expected: the five `never_reused` tests fail (`next == deleted`), the `AUTOINCREMENT`
  assertion fails, and `user_version` is 26, not 27.

- [ ] **Step 3: Write migration 27 and add `27` to `WITHOUT_FOREIGN_KEYS`**

  One SQL entry, the five rebuilds in the order `watched_folders`, `folders`, `items`,
  `albums`, `saved_searches`. For each table:
  - Create `<t>_new` with today's columns. Take them from `PRAGMA table_info` and
    `sqlite_master.sql` of a library at version 26: the first migration's definition plus
    every `ADD COLUMN` in order, with the same `NOT NULL`/`DEFAULT`/`UNIQUE`/
    `REFERENCES … ON DELETE` clauses. Add `AUTOINCREMENT` on `id`. References name the
    final tables (`folders`, not `folders_new`).
  - `INSERT INTO <t>_new (cols…) SELECT cols… FROM <t>`.
  - `DROP TABLE <t>` and `ALTER TABLE <t>_new RENAME TO <t>`.
  - Recreate the table's named indexes with their exact current `sql`, in the
    `sqlite_master` rowid order of a version-26 library. Read that order with the
    query from the index-order test rather than trusting the spec's list.

  After the rebuilds, raise the folders sequence from `last_folder`:
  `UPDATE sqlite_sequence SET seq = <n> WHERE name = 'folders' AND <n> > seq`. Here `<n>`
  is `last_folder`'s value when it is all digits (`value GLOB '[0-9]*' AND value NOT GLOB '*[^0-9]*'`),
  cast to an integer. If `folders` is empty, `sqlite_sequence` has no folders row, so also
  insert one when it is missing and `<n>` is valid. `settings` is `(key TEXT PRIMARY KEY,
  value TEXT NOT NULL)`, and the row is `key = 'last_folder'`.

  The migration's comment says three things. Why the rebuild: AUTOINCREMENT can't be added
  to an existing table, and the window holds ids. Why foreign keys are off (the cascade).
  And why the indexes come back in their old order: without `ANALYZE`, tied partial
  indexes are chosen by creation order. Rewrite the migration-25 comment so it no longer
  implies the other tables reuse ids: it gave AUTOINCREMENT to two tables first, and
  migration 27 gives it to the rest.

- [ ] **Step 4: Fix version literals and fixtures, then run the crate**

  Run: `cargo nextest run -p photon-core`
  Expected: the new tests pass. Literal-26 failures are updated to 27. If an older
  migration test's fixture now fails `ForeignKeyCheck`, give that fixture the missing
  parent row; never relax the check.

- [ ] **Step 5: Show each new test fails reverted**
  - Take `27` out of `WITHOUT_FOREIGN_KEYS`: `migration_27_keeps_every_row_and_reference`
    loses dependent rows.
  - Swap two recreated `items` indexes: the index-order test fails.
  - Drop the `last_folder` statement: the remembered-folder test fails.
  - Drop `AUTOINCREMENT` from one table at a time: that table's `never_reused` test fails.

  Restore each change exactly and `touch schema.rs`.

- [ ] **Step 6: Measure the upgrade**

  This step is throwaway and not committed. In a release build
  (`cargo nextest run --release`, or a scratch `#[ignore]` test deleted afterwards),
  fill a version-26 library with 100,000 items in 50 folders, with realistic column
  values (paths of about 60 characters, half the items with `content_hash`), in one
  transaction. Time `Library::open`. Write the time, the machine and the row count in
  the PR body. If it takes over 3 seconds, stop and report before going on.

- [ ] **Step 7: Correct the core's comments and the notes**
  - `inflight.rs` and `service.rs`: `thumb_key()` keying stays. The comments stop
    calling id reuse a live cause. They keep the reasons that remain true: a changed
    file, a fresh edit, and a library deleted and built again, whose ids start at 1. The
    assertion message near line 497 is reworded the same way.
  - `docs/architecture/library.md`, `### Schema`, gains a rule: every table whose ids the
    window holds is `AUTOINCREMENT` (migrations 25 and 27). A new such table is created
    with it. A rebuild of a referenced table runs through `WITHOUT_FOREIGN_KEYS` and
    recreates its indexes in their old `sqlite_master` order.
  - CLAUDE.md: "or a round trip is held by id, never by offset - and library ids are
    reused once freed." becomes "or a round trip is held by id, never by offset. Library
    ids are never reused (`AUTOINCREMENT`, migration 27); a new table whose ids the
    window holds is created with it."

- [ ] **Step 8: Gate and commit**

  ```bash
  git commit -m "feat(core): migration 27 - library ids are never handed out again"
  ```

### Task 3: The window's tests and reasons follow

**Files:**
- Modify: `crates/photon-ui/src/app.rs` (`deleting` doc near line 169, `show_dialog`
  comment near line 2244)
- Modify: `crates/photon-ui/src/watched_list.rs` (`going` doc near line 81)
- Modify: `crates/photon-ui/tests/dialogs.rs` (the comment near line 561;
  `an_album_being_deleted_has_no_menu_until_it_has_left_the_list`,
  `a_saved_search_being_deleted_…`, `a_question_whose_album_has_gone_…`,
  `a_question_whose_saved_search_has_gone_…`; a new test)
- Modify: `crates/photon-ui/tests/settings_lists.rs`
  (`a_folder_whose_removal_is_on_its_way_is_not_removed_again_under_its_reused_id` and
  its comment)
- Modify: `docs/architecture/ui-grid.md` (near lines 305-315 and 365-370),
  `docs/architecture/ui-pages.md` (near line 419)

**Interfaces:**
- Consumes: schema 27 from Task 2. Nothing new is produced.

- [ ] **Step 1: Run the window's tests on Task 2's branch**

  Run: `cargo nextest run -p photon-ui --test all dialogs:: settings_lists::`
  Expected: every test asserting `"the number is handed out again"` now fails. That is
  the proof that the window sees the new ids.

- [ ] **Step 2: Turn those assertions around**
  - Each `assert_eq!(new, old, "the number is handed out again")` becomes
    `assert_ne!(new, old, "a number is never handed out again")`.
  - Each test keeps every assertion about the behaviour that stays: no menu on a going
    row, a stale question closing, the album or folder made meanwhile surviving.
  - The settings test is renamed
    `a_folder_whose_removal_is_on_its_way_offers_no_second_remove`. Its comment and the
    comment above the album tests say the new reason: a second Delete…/Remove… would ask
    again and then answer with the library's "not found".
  - `a_question_whose_album_has_gone_…` and its saved-search twin keep their names.
    Their comments say a question about something gone or renamed is no longer the
    question the user is answering.

- [ ] **Step 3: Write `a_keyword_picker_left_open_writes_nothing_to_a_photo_indexed_after_its_own_was_purged`**
  (tests/dialogs.rs):
  1. `library(dir, 900, 0, 0)` and `photo_ids`. Find the grid position of the photo with
     the highest id and `open_picker` on it alone with "Add keyword to photo…" (`tile_menu.rs`).
  2. Through a separate `Library::open(&library.dirs().db_path)`, `purge_items(&[highest])`,
     then `insert_items` one new item in the same folder with a fresh path. Assert its id
     differs from `highest`.
  3. Type `lisbon` and press Enter. Wait with `driver.until` for the write to be answered
     (the toast, or the dialog closed and the queue idle; use whichever the existing
     picker tests wait for).
  4. Assert `lib.item_tags(new_id)` is empty.

  If the selection or the grid drops the purged photo before Enter and the picker closes,
  that also passes. Still assert the new photo has no keyword.

- [ ] **Step 4: Show it fails reverted**

  On a scratch copy of the branch, take migration 27 out of `MIGRATIONS` and `WITHOUT_FOREIGN_KEYS` (and the
  version literals as needed to compile and open). Run the new test and expect the new
  item to get `highest`'s id, and the keyword with it. If the window's seams cannot
  produce this order of events, delete the test and say why in the commit message, per
  the spec.

- [ ] **Step 5: Rewrite the reasons in the window and its notes**
  - `app.rs` `deleting`: the row of something being deleted has no menu, because a
    second Delete… would ask again and end in "not found".
  - `app.rs` `show_dialog`: a question about something that has gone, or is no longer
    called what the question said, is no question. This is the rule that a dialog acts
    on what it showed.
  - `watched_list.rs` `going`: the same reason as `deleting`, for Remove….
  - `ui-grid.md`: "By number, which is not for ever the same thing" becomes a statement
    that a library id names one row for ever (migration 27), and the open keyword-picker
    case is recorded as closed. The `App::deleting` paragraph takes the new reason.
  - `ui-pages.md`: the `FolderRow::going` paragraph, the same.

  Check: `grep -rn -i "handed out again\|reused once\|id reuse" crates docs/architecture CLAUDE.md`
  returns only lines that say ids are *not* reused or that describe a library deleted
  and rebuilt.

- [ ] **Step 6: Gate and commit**

  ```bash
  git commit -m "test(ui): the window's guards keep their behaviour; ids are never handed out again"
  ```

### Task 4: Whole-branch review and PR

- [ ] **Step 1:** An independent review of the branch (CLAUDE.md: a large branch gets
  one). Point it at three things:
  - `run`'s foreign-key restore on every path;
  - migration 27's columns and index order against a version-26 library;
  - what the migration *arms* in old code, such as other code that reads
    `sqlite_master`, or `ORDER BY rowid` on a rebuilt table.

  Give the reviewer its own `CARGO_TARGET_DIR`.
- [ ] **Step 2:** Fix what it confirms, each fix with its test, and run the gate.
- [ ] **Step 3:** Push with gh's credential helper and open the PR. The body gives the
  measured upgrade time, the revert proofs, and "Schema 27: the next release is a minor".
  Watch `gh pr checks N --watch` until green on all platforms.
