# Never-reused library ids - design

Date: 2026-11-13. Status: draft for the user's approval.

## Why

A library id is a number the window holds and acts on later: the selection, a dialog's
subject, a menu's row, a write on the queue, `last_folder` in Settings. Five tables hand
a freed number to the next insert, because their key is `INTEGER PRIMARY KEY` without
`AUTOINCREMENT`, and SQLite then gives a new row `max(id) + 1`. So if the row with the
highest id is deleted, the next new row gets that same number back. The window has grown
guards around this, one bug at a time (`App::deleting`, `FolderRow::going`,
`App::listed`), and one case is still open by its own record
(`docs/architecture/ui-grid.md`, "By number, which is not for ever the same thing"). If a
keyword picker is left open while its photo is purged, and another photo is indexed under
the freed number, the keyword is written to the new photo.

`people` and `detected_faces` already never reuse a number (migration 25, for the same
reason). This design does the same for every other table, so that "an id names one row,
for ever" holds across the whole library, and the reasoning the code gives for it is
corrected to match.

## Goals

- `watched_folders`, `folders`, `items`, `albums` and `saved_searches` become
  `INTEGER PRIMARY KEY AUTOINCREMENT`. Once a number has named a row in one of them, no
  other row of that table ever gets it.
- Every existing row keeps its id, its columns and values, and every row that references
  it (album membership, keywords, user keywords, Picasa faces, detected faces, child
  folders, items).
- Every comment, architecture note, test and CLAUDE.md line that says ids are handed out
  again is corrected.

## Non-goals

- No change to `thumb_key()` or to the crash guard's keying (`thumbs/inflight.rs`,
  `thumbs/service.rs`). Both also guard against a changed file, a fresh edit, and a
  library deleted and rebuilt from scratch (which starts its ids at 1 again). Only their
  comments change: they stop citing id reuse as a live cause, and keep citing the rebuilt
  library.
- No change to the offset rules. An offset is still valid against one index version
  only, and anything held across a rebuild is still held by id.
- No ids for tables whose rows the window never names: `item_tags`, `item_user_tags`,
  `contacts`, `faces` (Picasa's), `album_items`, `tag_rules`, `person_contacts`,
  `face_rejections`, `settings`. Their implicit rowids are not touched. (`faces` is
  read `ORDER BY rowid`; the migration does not rebuild it, so that order is unchanged.)
- No UI for the upgrade (no progress bar, no message).

## Anti-goals

- **Losing a single dependent row.** The tables to rebuild are referenced with
  `ON DELETE CASCADE`, and photon opens every connection with `PRAGMA foreign_keys = ON`.
  Dropping the old `items` table with foreign keys on would delete every album
  membership, keyword and face in the library, and the upgrade would look like it
  succeeded.
- **Slowing a count.** Without `ANALYZE`, partial indexes that tie are chosen by creation
  order: the last one created wins (CLAUDE.md, schema rules;
  `the_starred_and_video_counts_are_served_by_their_indexes`). A rebuild that recreates
  `items`' indexes in another order can change the plan of a sidebar count.

## The user's decisions

- **Every table, and the guards removed** (2026-11-13). Removed in the sense of every
  statement that ids are reused. See the next point for the behaviour that stays.
- **Keep the guards' behaviour** (2026-11-13), with their reasons rewritten:
  - `App::deleting` and `FolderRow::going` keep taking the menu away from a row whose
    delete or removal is on its way. The new reason: a second Delete…/Remove… of the
    same row would ask again and then answer with the library's "not found".
  - `App::listed` keeps closing a confirm question whose album or saved search is no
    longer listed under the name it was asked about. The new reason: a question about
    something that has gone or been renamed is no longer the question the user is
    answering. This is the dialog rule "a dialog acts on what it took when it opened"
    (ui-grid.md), applied to the words it showed.

## Design

### Migration 27

One new entry in `MIGRATIONS`, which takes the schema from 26 to 27. It rebuilds the five
tables as SQLite's documented twelve-step procedure (ALTER TABLE, "Making Other Kinds Of
Table Schema Changes") prescribes. For each table:

1. Create `<table>_new` with exactly today's columns: the same names, order, types,
   `NOT NULL`, `DEFAULT`s, `UNIQUE` and `REFERENCES … ON DELETE …` clauses, with
   `AUTOINCREMENT` added to `id`. Today's columns are those of the first migration plus
   every `ALTER TABLE … ADD COLUMN` since, in migration order. `folders.parent_id` keeps
   referring to `folders`.
2. `INSERT INTO <table>_new (<every column>) SELECT <every column> FROM <table>`, with
   the columns spelled out (no `SELECT *`).
3. `DROP TABLE <table>`, then `ALTER TABLE <table>_new RENAME TO <table>`.
4. Recreate each of the table's indexes under its own name and definition, in the
   **order in which they stand in `sqlite_master` of a library upgraded through migration
   26**. For `items` that order is `items_folder`, `items_pending`, `items_recent`,
   `items_size`, `items_content_hash`, `items_similar_group`, `items_hidden`,
   `items_starred` (as a later migration dropped and recreated it), `items_videos`, `items_moved`. The
   builder reads the real order off an upgraded library rather than trusting this list.
   The `UNIQUE` constraints' automatic indexes come back with the tables.

Inserting explicit ids into an `AUTOINCREMENT` table raises `sqlite_sequence` to the
highest one inserted, so new ids continue above the highest *surviving* id.

**Foreign keys off around this one migration.** `PRAGMA foreign_keys` cannot change
inside a transaction, and `migrate` wraps every migration in one. So `migrate` gains a
way to run a migration with foreign keys off. The shape is the builder's choice (say, a
list of the migration numbers that rebuild tables, or a marker in the SQL). It works like
this:

- Turn foreign keys off before the transaction begins.
- In the transaction: the migration's SQL, then `PRAGMA foreign_key_check`. Any row it
  returns fails the migration, which rolls back and leaves the library at version 26
  (the open fails with an error; nothing is half-done).
- Commit, then turn foreign keys back on. **Foreign keys are switched back on whether
  the migration succeeded or failed**, because the connection is used afterwards.

The comment on migration 25 ("the drop, run with foreign keys on, cascades nowhere") is
the same trap seen from the other side. The comment on migration 27 says why foreign keys
are off and what would happen if they were on.

**A freed number from before the upgrade.** A row deleted before the upgrade whose id
was then the highest leaves no trace, so after the upgrade that one number can still be
handed out once. In memory nothing holds an id across the upgrade (it happens before the
window exists). In storage there is `settings.last_folder`. **My call:** the migration
raises the `folders` sequence to at least the stored `last_folder` (if a number is
stored), so a remembered folder that has since gone can never name a new one.
`last_folder` is the only id `settings` stores (`settings.rs`), and it is stored as text,
so the migration reads it with a guarded cast and raises the sequence only when the value
is a number above the current highest folder id. Outside the database, the window-layout
file holds no ids. The crash guard's marker files are named by item id but checked
against `thumb_key` (`inflight.rs`), so a gap id given out once does no harm there.

### What changes elsewhere

- Version literals: 26 becomes 27 in `Library::open`'s tests (`mod.rs`) and in
  `migration_25_…`/`migration_26_…` in schema.rs. The table-count literal (16) does not
  change (`sqlite_sequence` already exists, and the list names photon's tables only).
  Per CLAUDE.md, the numbers are updated, never loosened.
- Old migration tests build their fixtures on a plain `rusqlite::Connection`, where
  foreign keys are off. A fixture with a dangling reference will now fail migration 27's
  check: fix the fixture, never loosen the check. Real libraries have had foreign keys on
  since the first commit (`configure()`).
- Comments, rewritten so they no longer say ids are reused:
  - `app.rs` (`deleting`, `show_dialog`)
  - `watched_list.rs` (`going`)
  - `thumbs/inflight.rs` (module doc, the comment by line 155, the assertion message
    near line 497)
  - `thumbs/service.rs` (near line 894)
  - schema.rs's migration-25 comment, which explains `AUTOINCREMENT` for two tables, and
    the comment over `albums` ("membership is by item id … a photo renamed on disk …
    leaves its albums when the old row is purged") stays true and stays.
- Notes:
  - `docs/architecture/ui-grid.md`: "By number, which is not for ever the same thing" and
    the `App::deleting` paragraph.
  - `docs/architecture/ui-pages.md`: the `FolderRow::going` paragraph.
  - `docs/architecture/library.md`: a short rule under schema and migrations, saying every
    table whose id the window holds is `AUTOINCREMENT`, a new such table is created with
    it, a rebuild runs with foreign keys off, and indexes are recreated in their old order.
- CLAUDE.md, cross-cutting rules: "…held by id, never by offset - and library ids are
  reused once freed." becomes "…held by id, never by offset. Library ids are never
  reused (`AUTOINCREMENT`, migration 27); a new table whose ids the window holds is
  created with it."

### Tests

Each new test must fail with its change reverted (CLAUDE.md).

**photon-core, schema.rs:**

- One test per table that the highest id, deleted, is not given to the next insert, like
  `a_deleted_persons_id_is_never_reused` (reuse `next_id_after_deleting_the_highest`).
  That's five tests. Without migration 27 each of them fails.
- **The upgrade keeps everything.** Build a version-26 library with rows in every table
  and every reference between them: two watched folders, nested folders, items in them,
  album membership, item tags, user tags, Picasa faces, detected faces with a person, a
  saved search, and `last_folder` set. Delete the highest item first, so a gap exists.
  Then migrate. Assert:
  - every row of every table is unchanged, with the same ids;
  - `PRAGMA foreign_key_check` is empty;
  - foreign keys are on afterwards (a cascade still cascades: delete a watched folder
    and its items and their album rows go);
  - the next item id is above the highest surviving one.
  Revert check: the same migration run with foreign keys on loses the dependent rows.
- **The tables' shape is unchanged.** `PRAGMA table_info` and `PRAGMA foreign_key_list`
  of each of the five tables are equal before and after the upgrade.
- **The indexes are unchanged, in order.** The `(name, sql)` list of indexes on the five
  tables, in `sqlite_master` order, is equal before and after. Revert check: swap two
  recreated `items` indexes and the test fails.
- **The plan tests that exist still pass.** A fresh library runs every migration from
  0, so `temp_library()` already goes through migration 27 and the existing plan tests
  already exercise the rebuilt `items`. What would catch a reordering on a real upgraded
  library is the index-order test above.
- **A failing check rolls back.** A version-26 library with a dangling reference (made
  with foreign keys off) fails migration 27, stays at 26 with its rows, and its
  connection has foreign keys on. This runs `schema::migrate` on a raw connection,
  because `Library::open` drops its connection when it fails.
- **`last_folder` is respected.** A stored `last_folder` above every surviving folder id
  is never handed to a new folder.

**photon-ui:**

- The tests that assert `"the number is handed out again"` (`tests/dialogs.rs` near
  lines 561-720, and `tests/settings_lists.rs`'s
  `a_folder_whose_removal_is_on_its_way_is_not_removed_again_under_its_reused_id`) set
  up a race that only mattered because of reuse. They keep their scenario and their
  assertions about the behaviour that stays (no menu on a going row; a stale question
  closes; the album made meanwhile survives). They lose the reuse assertion, and their
  names and comments are rewritten.
- **The open case closes.** A keyword picker is open on a photo, the photo is purged
  and a new photo is indexed, then the picker is answered. Assert the new photo does not
  get the keyword. `add_items_tag`/`remove_items_tag` already skip ids that are no longer
  live photos, so the answer writes nothing and needs no new error path. The builder decides whether the window's test seams can produce this
  order of events. If they can't, the core-level per-table tests carry the proof and the
  commit message says so.

### Performance

The upgrade copies `items` and rebuilds its indexes once, before the window opens
(`Engine::open` → `Library::open`). The builder measures it on a library of 100,000
photos (`cargo run -p xtask -- fixture-library` or a direct fill) in a release build and
records the time in the PR. If it is over about 3 seconds, that goes into the release
notes' Upgrading section. No progress UI is planned. If the time is unacceptable, come
back before adding one.

## Release

Schema 26 → 27 makes the release a minor one (0.57.0). Upgrading section: an older
photon refuses the library once it has been opened by 0.57.0, and the first launch after
the upgrade may take a few seconds longer on a large library. Nothing is visible to the
user otherwise. Releasing is not part of the build: it waits for the user's word.

## Left to the builder

- How `migrate` marks a migration that runs with foreign keys off.
- Whether the five rebuilds are one migration (my call: one, so the library is never at
  a version where some tables reuse ids and others don't).
- The exact wording of the rewritten comments and notes, held to "Comments carry the
  reasoning; a wrong justification is a defect".
- Whether the keyword-picker test is feasible (see above).
