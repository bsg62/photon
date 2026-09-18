# Setting Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user add a tag to one photo and remove a tag from one photo, from the viewer's info panel, without ever writing to the photo file.

**Architecture:** A new `item_user_tags` overlay table records per-photo decisions — a tag the user added (`added = 1`) or one of the photo's own file keywords hidden on it (`added = 0`). `item_tags` keeps holding exactly what the file says, because the scanner rewrites it from the file on every re-read. The overlay is applied inside `EFFECTIVE_TAGS` and `TAG_FILTER`, the two constants every tag reader already goes through, so the info panel, the sidebar, the tag manager's counts, the Tag view and text search all pick it up from one edit each.

**Tech Stack:** Rust (rusqlite/SQLite, Tauri 2), Svelte 5 runes + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-18-photon-set-tags-design.md`

## Global Constraints

- **The Rust gate, all four, before any commit:** `cargo fmt --all` (run it, not just `--check`), then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate, from the repo root:** `npm run check` (svelte-check, 0 errors AND 0 warnings) and `npm test`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not proof. Every task below has an explicit "verify it fails" step; do not skip it.
- **Never launch the GUI to verify a change.** Verification is the test suites plus `svelte-check`; anything needing eyes goes on the README's `## Manual smoke checklist`.
- **photon never writes to, moves or deletes photo files.** Nothing in this plan writes any file inside a watched folder — not the photo, not `.picasa.ini`.
- **No native library dependencies.** This plan adds no crate dependencies at all.
- **The TypeScript mirror in `ui/src/lib/api.ts` is hand-written and unchecked.** A Rust field or command added without its TS counterpart is silently `undefined` at runtime. Change both in the same commit.
- **IPC is three files per command**, in this order: `commands.rs` (a plain `pub fn` taking `&Engine`), `ipc.rs` (a `#[tauri::command(async)]` wrapper that only delegates), `app.rs` (an entry in `tauri::generate_handler![…]`). Forgetting the third compiles fine and fails at runtime.
- **Case-insensitive matching is done in Rust/TypeScript, never in SQL.** There is no `COLLATE NOCASE` and `lower()` is ASCII-only without ICU.
- **Comments carry the reasoning, not the mechanics.** The comments given in this plan's code blocks are part of the deliverable — copy them.

---

### Task 1: Migration 7 — the `item_user_tags` table

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (append to `MIGRATIONS`, add a migration test)
- Modify: `crates/photon-core/src/library/mod.rs:144`, `:149`, `:153`, `:170` (the version and table-count tripwires)

**Interfaces:**
- Consumes: nothing.
- Produces: the table `item_user_tags(item_id INTEGER, tag TEXT, added INTEGER, PRIMARY KEY (item_id, tag))` and the partial index `item_user_tags_tag ON item_user_tags(tag) WHERE added = 1`. Schema version becomes 7.

A schema bump is *meant* to break three tests in `library/mod.rs`. Update the numbers; never loosen them to `MIGRATIONS.len()` — the hardcoding is the tripwire.

- [ ] **Step 1: Write the failing migration test**

Append to the `mod tests` block in `crates/photon-core/src/library/schema.rs`:

```rust
    /// The overlay table arriving in a library that already has keywords and a rule.
    /// Nothing existing may change: the keywords and the rule are what the overlay is
    /// applied on top of.
    #[test]
    fn the_seventh_migration_adds_item_user_tags_and_keeps_keywords_and_rules() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..6] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 6i64).unwrap();
        conn.execute(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key) \
             VALUES (1, 1, NULL, '/p', 'p', 'p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width, \
             height, orientation, taken_at) \
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO item_tags (item_id, tag) VALUES (1, 'beach')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tag_rules (tag, target) VALUES ('beach', 'seaside')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 7);
        let overlay: i64 = conn
            .query_row("SELECT count(*) FROM item_user_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(overlay, 0, "an upgraded library starts with no per-photo changes");
        let tag: String = conn
            .query_row("SELECT tag FROM item_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "beach");
        let target: String = conn
            .query_row("SELECT target FROM tag_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(target, "seaside");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p photon-core the_seventh_migration`
Expected: FAIL — `no such table: item_user_tags`.

- [ ] **Step 3: Add migration 7**

Append a seventh entry to `MIGRATIONS` in `crates/photon-core/src/library/schema.rs`, after the `tag_rules` entry:

```rust
    r#"
-- The user's per-photo tag changes: a tag added to one photo (added = 1), or one of that
-- photo's own file keywords hidden on it (added = 0). Separate from item_tags for the same
-- reason tag_rules is: a rescan rewrites item_tags from the file, and the user's change
-- must outlive that. Suppressions name the *raw* keyword; additions name it as tag_rules
-- resolves it. The two namespaces meet only in remove_item_tag.
CREATE TABLE item_user_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    added   INTEGER NOT NULL,
    PRIMARY KEY (item_id, tag)
);
-- Partial: the Tag view's third arm asks only for additions, and suppressions are reached
-- through the primary key, correlated to one item.
CREATE INDEX item_user_tags_tag ON item_user_tags(tag) WHERE added = 1;
"#,
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p photon-core the_seventh_migration`
Expected: PASS.

- [ ] **Step 5: Update the three tripwires the bump breaks**

In `crates/photon-core/src/library/mod.rs`, in `open_creates_schema_and_is_idempotent`: change `assert_eq!(version, 6)` to `7`; add `'item_user_tags'` to the table-name list in the `sqlite_master` query; change `assert_eq!(tables, 10)` to `11`. In `refuses_newer_schema`: change `supported: 6` to `supported: 7`.

- [ ] **Step 6: Run the whole core suite**

Run: `cargo test -p photon-core`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/photon-core/src/library/schema.rs crates/photon-core/src/library/mod.rs
git commit -m "feat(tags): schema for per-photo tag changes"
```

---

### Task 2: `add_item_tag` and the addition arm of `EFFECTIVE_TAGS`

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (`EFFECTIVE_TAGS`, `item_tags`, new `add_item_tag`, tests)

**Interfaces:**
- Consumes: the `item_user_tags` table from Task 1.
- Produces: `Library::add_item_tag(&self, item_id: i64, tag: &str) -> Result<String>`, returning the stored name. `EFFECTIVE_TAGS` gains two columns, `src` (0 for a file keyword, 1 for a user addition) and `seq`; readers order by `src, seq`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `crates/photon-core/src/library/tags.rs`:

```rust
    #[test]
    fn a_tag_the_user_adds_shows_after_the_file_keywords() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        assert_eq!(lib.add_item_tag(ids[0], "  sunset ").unwrap(), "sunset");
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach", "sunset"]);
    }

    #[test]
    fn a_blank_tag_is_refused() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        assert!(matches!(
            lib.add_item_tag(ids[0], "   "),
            Err(Error::EmptyTagName)
        ));
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }

    #[test]
    fn adding_a_tag_the_file_already_carries_shows_it_once() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "beach").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }

    /// The reason the overlay is a second table: the scanner rewrites a photo's item_tags
    /// rows from the file whenever it re-reads it, and the user's tag must outlive that.
    #[test]
    fn a_user_tag_survives_rereading_the_keywords() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "sunset").unwrap();
        let row = lib.item(ids[0]).unwrap().unwrap();
        let reread = NewItem {
            tags: vec!["beach".into()],
            ..new_item(row.folder_id, &row.path, row.taken_at)
        };
        lib.update_item_meta(&[(ids[0], reread)]).unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach", "sunset"]);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core --lib tags`
Expected: FAIL to compile — `no method named add_item_tag`. Once Step 3 adds the method but before the `EFFECTIVE_TAGS` change, they fail on the assertion instead; that assertion failure is the proof, so keep going and re-check in Step 5.

- [ ] **Step 3: Rewrite `EFFECTIVE_TAGS` and `item_tags`**

Replace the `EFFECTIVE_TAGS` constant and its doc comment in `crates/photon-core/src/library/tags.rs`:

```rust
/// Each effective keyword row with the rules applied: `(item_id, tag, src, seq)`. A
/// renamed keyword appears under its new name, a removed one is absent, and a tag the user
/// added to one photo joins the file's own keywords. `src` is 0 for a file keyword and 1
/// for a user addition; `seq` is the source table's rowid, so `ORDER BY src, seq` is the
/// file's own keyword order followed by the order the user added tags in. One photo can
/// list two keywords that now share a name, so a reader that needs each once must
/// de-duplicate.
pub(super) const EFFECTIVE_TAGS: &str = "SELECT t.item_id, coalesce(r.target, t.tag) AS tag, t.src, t.seq
     FROM (SELECT it.item_id, it.tag, 0 AS src, it.rowid AS seq FROM item_tags it
           UNION ALL
           SELECT u.item_id, u.tag, 1 AS src, u.rowid AS seq
             FROM item_user_tags u WHERE u.added = 1) t
     LEFT JOIN tag_rules r ON r.tag = t.tag
     WHERE r.tag IS NULL OR r.target IS NOT NULL";
```

In `item_tags`, change the `ORDER BY`:

```rust
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT tag FROM ({EFFECTIVE_TAGS}) WHERE item_id = ?1 ORDER BY src, seq"
        ))?;
```

- [ ] **Step 4: Add `add_item_tag`**

Add to the `impl Library` block in `crates/photon-core/src/library/tags.rs`:

```rust
    /// Adds `tag` to one photo, returning the name stored. Adding a name the photo already
    /// carries is not an error: it leaves the photo with the tag, which is what was asked.
    pub fn add_item_tag(&self, item_id: i64, tag: &str) -> Result<String> {
        let tag = valid_name(tag)?;
        self.writer().execute(
            "INSERT INTO item_user_tags (item_id, tag, added) VALUES (?1, ?2, 1)
             ON CONFLICT (item_id, tag) DO UPDATE SET added = 1",
            params![item_id, tag],
        )?;
        Ok(tag.to_string())
    }
```

- [ ] **Step 5: Run the tests to verify they pass, then prove they discriminate**

Run: `cargo test -p photon-core --lib tags`
Expected: PASS.

Now revert only the `EFFECTIVE_TAGS` change (keep `add_item_tag`) and re-run: `a_tag_the_user_adds_shows_after_the_file_keywords` and `a_user_tag_survives_rereading_the_keywords` must FAIL on their assertions. Restore the change.

- [ ] **Step 6: Run the whole core suite**

Run: `cargo test -p photon-core`
Expected: PASS. (`tags_with_counts` and search read through `EFFECTIVE_TAGS`, so their existing tests are the check that the rewrite did not change behaviour for file keywords.)

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/photon-core/src/library/tags.rs
git commit -m "feat(tags): add a tag to one photo"
```

---

### Task 3: `remove_item_tag` and the suppression arm

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (`EFFECTIVE_TAGS`, new `remove_item_tag`, tests)

**Interfaces:**
- Consumes: `add_item_tag` and the `EFFECTIVE_TAGS` shape from Task 2.
- Produces: `Library::remove_item_tag(&self, item_id: i64, tag: &str) -> Result<()>`. This task handles exact names only; Task 4 generalises it to rule-resolved names.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/photon-core/src/library/tags.rs`:

```rust
    #[test]
    fn removing_a_file_keyword_hides_it_on_that_photo_only() {
        let (_dir, lib, ids) = library_with(&[&["beach", "sunset"], &["beach"]]);
        lib.remove_item_tag(ids[0], "beach").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["sunset"]);
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["beach"]);
    }

    /// The scanner rewrites item_tags from the file; a suppression that did not outlive
    /// that would bring the keyword back at the next rescan.
    #[test]
    fn a_suppressed_keyword_stays_hidden_across_a_reread() {
        let (_dir, lib, ids) = library_with(&[&["beach", "sunset"]]);
        lib.remove_item_tag(ids[0], "beach").unwrap();
        let row = lib.item(ids[0]).unwrap().unwrap();
        let reread = NewItem {
            tags: vec!["beach".into(), "sunset".into()],
            ..new_item(row.folder_id, &row.path, row.taken_at)
        };
        lib.update_item_meta(&[(ids[0], reread)]).unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["sunset"]);
    }

    #[test]
    fn removing_a_tag_the_user_added_takes_it_away_again() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "sunset").unwrap();
        lib.remove_item_tag(ids[0], "sunset").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }

    /// A name that is both a file keyword and a user addition is one row, so the states
    /// have to degenerate correctly: removing then re-adding leaves the photo carrying it.
    #[test]
    fn re_adding_a_removed_file_keyword_brings_it_back() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "beach").unwrap();
        lib.remove_item_tag(ids[0], "beach").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), Vec::<String>::new());
        lib.add_item_tag(ids[0], "beach").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core --lib tags`
Expected: FAIL to compile — `no method named remove_item_tag`.

- [ ] **Step 3: Add the suppression filter to `EFFECTIVE_TAGS`**

In `crates/photon-core/src/library/tags.rs`, the file-keyword arm of the union gains a `NOT EXISTS`. Replace that one line:

```rust
     FROM (SELECT it.item_id, it.tag, 0 AS src, it.rowid AS seq FROM item_tags it
            WHERE NOT EXISTS (SELECT 1 FROM item_user_tags u
                              WHERE u.item_id = it.item_id AND u.tag = it.tag AND u.added = 0)
```

Note the filter names the **raw** keyword, before `coalesce` resolves it — a suppression is of the file's row, not of the display name.

- [ ] **Step 4: Add `remove_item_tag`**

Add to the `impl Library` block, below `add_item_tag`:

```rust
    /// Removes `tag` from one photo: a tag the user added is deleted, one of the photo's
    /// own keywords is suppressed. Both in one transaction, deletion first, because a name
    /// that is both ends as the single suppression row the primary key allows.
    pub fn remove_item_tag(&self, item_id: i64, tag: &str) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM item_user_tags WHERE item_id = ?1 AND tag = ?2 AND added = 1",
            params![item_id, tag],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO item_user_tags (item_id, tag, added)
             SELECT item_id, tag, 0 FROM item_tags WHERE item_id = ?1 AND tag = ?2",
            params![item_id, tag],
        )?;
        tx.commit()?;
        Ok(())
    }
```

- [ ] **Step 5: Run the tests to verify they pass, then prove they discriminate**

Run: `cargo test -p photon-core --lib tags`
Expected: PASS.

Revert only the `NOT EXISTS` line from Step 3 and re-run: `removing_a_file_keyword_hides_it_on_that_photo_only` and `a_suppressed_keyword_stays_hidden_across_a_reread` must FAIL. Restore it.

- [ ] **Step 6: Run the whole core suite**

Run: `cargo test -p photon-core`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/photon-core/src/library/tags.rs
git commit -m "feat(tags): remove a tag from one photo"
```

---

### Task 4: The rules and the overlay meet

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (`add_item_tag`, `remove_item_tag`, imports, tests)

**Interfaces:**
- Consumes: `add_item_tag` and `remove_item_tag` from Tasks 2 and 3.
- Produces: the same two signatures. `add_item_tag` now stores a rename rule's target instead of what was typed, and deletes a removal rule for the typed name. `remove_item_tag` now takes the **displayed** name and resolves it to every raw keyword behind it.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/photon-core/src/library/tags.rs`:

```rust
    /// The user gets the tag they see: under `holiday → vacation`, typing either name
    /// stores `vacation`.
    #[test]
    fn adding_a_renamed_name_stores_its_target() {
        let (_dir, lib, ids) = library_with(&[&["holiday"], &[]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        assert_eq!(lib.add_item_tag(ids[1], "holiday").unwrap(), "vacation");
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["vacation"]);
    }

    /// The one global effect a per-photo action has, and the reason for it: a name the user
    /// has just typed must not come back hidden.
    #[test]
    fn adding_a_removed_name_brings_the_tag_back_everywhere() {
        let (_dir, lib, ids) = library_with(&[&["junk"], &[]]);
        lib.hide_tag("junk").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), Vec::<String>::new());

        assert_eq!(lib.add_item_tag(ids[1], "junk").unwrap(), "junk");
        assert_eq!(lib.tag_rules().unwrap(), []);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["junk"]);
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["junk"]);
    }

    /// One displayed name can stand for several raw keywords after a merge. Missing one
    /// would leave the tag on the photo after the user removed it.
    #[test]
    fn removing_a_merged_name_suppresses_every_keyword_behind_it() {
        let (_dir, lib, ids) = library_with(&[&["holiday", "vacation"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["vacation"]);
        lib.remove_item_tag(ids[0], "vacation").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), Vec::<String>::new());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core --lib tags`
Expected: FAIL — `adding_a_renamed_name_stores_its_target` gets `"holiday"` where `"vacation"` is expected, `adding_a_removed_name_brings_the_tag_back_everywhere` finds the rule still present, `removing_a_merged_name_suppresses_every_keyword_behind_it` still shows `["vacation"]`.

- [ ] **Step 3: Resolve rules in `add_item_tag`**

Change the import at the top of `crates/photon-core/src/library/tags.rs`:

```rust
use rusqlite::{OptionalExtension, params};
```

Replace `add_item_tag`'s body:

```rust
    /// Adds `tag` to one photo, returning the name stored. Adding a name the photo already
    /// carries is not an error: it leaves the photo with the tag, which is what was asked.
    ///
    /// A rename rule's target is stored instead of what was typed, so the user gets the tag
    /// they see. A removal rule for the typed name is dropped, which brings that tag back
    /// everywhere — the one global effect a per-photo action has. The alternatives were
    /// refusing a name the user has just typed, or storing it literally and watching it
    /// vanish from the panel on the next read, which reads as a bug.
    pub fn add_item_tag(&self, item_id: i64, tag: &str) -> Result<String> {
        let tag = valid_name(tag)?;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let name = tx
            .query_row(
                "SELECT target FROM tag_rules WHERE tag = ?1",
                params![tag],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or_else(|| tag.to_string());
        tx.execute(
            "DELETE FROM tag_rules WHERE tag = ?1 AND target IS NULL",
            params![tag],
        )?;
        tx.execute(
            "INSERT INTO item_user_tags (item_id, tag, added) VALUES (?1, ?2, 1)
             ON CONFLICT (item_id, tag) DO UPDATE SET added = 1",
            params![item_id, &name],
        )?;
        tx.commit()?;
        Ok(name)
    }
```

- [ ] **Step 4: Resolve the displayed name in `remove_item_tag`**

Replace the two statements in `remove_item_tag` so both match on the rule-resolved name:

```rust
    /// Removes the displayed name `tag` from one photo: every tag the user added that shows
    /// under that name is deleted, and every one of the photo's own keywords that shows
    /// under it is suppressed. A merge means one displayed name can stand for several raw
    /// keywords, and all of them have to go or the tag reappears.
    ///
    /// Deletion runs before suppression because a name that is both ends as the single
    /// suppression row the primary key allows.
    pub fn remove_item_tag(&self, item_id: i64, tag: &str) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM item_user_tags WHERE item_id = ?1 AND added = 1
               AND coalesce((SELECT target FROM tag_rules WHERE tag = item_user_tags.tag),
                            item_user_tags.tag) = ?2",
            params![item_id, tag],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO item_user_tags (item_id, tag, added)
             SELECT item_id, tag, 0 FROM item_tags
              WHERE item_id = ?1
                AND coalesce((SELECT target FROM tag_rules WHERE tag = item_tags.tag),
                             item_tags.tag) = ?2",
            params![item_id, tag],
        )?;
        tx.commit()?;
        Ok(())
    }
```

A keyword under a *removal* rule resolves through `coalesce` to its own name and so could be suppressed by this, but it is hidden from every reader already, so the row changes nothing the user can see.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-core --lib tags`
Expected: PASS.

- [ ] **Step 6: Prove the tests discriminate**

Revert the `DELETE FROM tag_rules … target IS NULL` statement alone: `adding_a_removed_name_brings_the_tag_back_everywhere` must FAIL. Restore it. Revert the `coalesce` in the suppression `INSERT` back to `tag = ?2`: `removing_a_merged_name_suppresses_every_keyword_behind_it` must FAIL. Restore it.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-core/src/library/tags.rs
git commit -m "feat(tags): resolve the user's global rules when a photo's tags change"
```

---

### Task 5: The Tag view, the counts and search

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (`TAG_FILTER`, tests)
- Modify: `crates/photon-core/src/library/items.rs:1336-1363` (the plan test's comment and assertions)

**Interfaces:**
- Consumes: the overlay and both mutations from Tasks 2–4.
- Produces: `TAG_FILTER` matching user additions and skipping suppressed keywords, still served by indexes.

`tags_with_counts` and `search_entries` read through `EFFECTIVE_TAGS` and so already follow; this task pins that with tests rather than changing them.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/photon-core/src/library/tags.rs`:

```rust
    #[test]
    fn a_user_tag_is_viewed_counted_and_searched_like_a_keyword() {
        let (_dir, lib, ids) = library_with(&[&["beach"], &[]]);
        lib.add_item_tag(ids[1], "sunset").unwrap();
        assert_eq!(tag_view(&lib, "sunset"), [ids[1]]);
        assert_eq!(search(&lib, "sunset"), [ids[1]]);
        assert_eq!(
            listed(&lib),
            [("beach".to_string(), 1), ("sunset".to_string(), 1)]
        );
    }

    #[test]
    fn a_photo_leaves_the_tag_view_when_its_keyword_is_removed_there() {
        let (_dir, lib, ids) = library_with(&[&["beach"], &["beach"]]);
        lib.remove_item_tag(ids[0], "beach").unwrap();
        assert_eq!(tag_view(&lib, "beach"), [ids[1]]);
        assert_eq!(search(&lib, "beach"), [ids[1]]);
        assert_eq!(listed(&lib), [("beach".to_string(), 1)]);
    }

    /// The photo keeps one keyword that answers to the name, so it stays in the view: the
    /// suppression is of a row, not of the photo.
    #[test]
    fn suppressing_one_of_two_merged_keywords_keeps_the_photo_in_the_view() {
        let (_dir, lib, ids) = library_with(&[&["holiday", "vacation"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        lib.writer()
            .execute(
                "INSERT INTO item_user_tags (item_id, tag, added) VALUES (?1, 'holiday', 0)",
                params![ids[0]],
            )
            .unwrap();
        assert_eq!(tag_view(&lib, "vacation"), [ids[0]]);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core --lib tags`
Expected: FAIL — `a_user_tag_is_viewed_counted_and_searched_like_a_keyword` returns no rows from `tag_view`, and `a_photo_leaves_the_tag_view_when_its_keyword_is_removed_there` still returns both ids.

- [ ] **Step 3: Rewrite `TAG_FILTER`**

Replace the constant and its doc comment in `crates/photon-core/src/library/tags.rs`:

```rust
/// The Tag view's filter for the name bound to `?1`: every keyword renamed to it, plus the
/// keyword itself unless it is ruled away, plus every tag the user added under that name —
/// each minus the photos that suppressed their own copy of the keyword. Not written
/// through `EFFECTIVE_TAGS`, whose `coalesce` no index can serve: this form is equality
/// probes on `item_tags_tag` and `item_user_tags_tag`, with the suppression checks reaching
/// `item_user_tags` through its primary key, and
/// `the_tag_view_is_served_by_its_index` holds it to that. `UNION ALL` rather than `OR`
/// because SQLite may answer an OR with a scan.
pub(super) const TAG_FILTER: &str = "AND i.id IN (
         SELECT item_id FROM item_tags
         WHERE tag IN (SELECT tag FROM tag_rules WHERE target = ?1)
           AND NOT EXISTS (SELECT 1 FROM item_user_tags u
                           WHERE u.item_id = item_tags.item_id AND u.tag = item_tags.tag
                             AND u.added = 0)
         UNION ALL
         SELECT item_id FROM item_tags
         WHERE tag = ?1 AND NOT EXISTS (SELECT 1 FROM tag_rules WHERE tag = ?1)
           AND NOT EXISTS (SELECT 1 FROM item_user_tags u
                           WHERE u.item_id = item_tags.item_id AND u.tag = item_tags.tag
                             AND u.added = 0)
         UNION ALL
         SELECT item_id FROM item_user_tags WHERE tag = ?1 AND added = 1
     )";
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core --lib tags`
Expected: PASS.

- [ ] **Step 5: Extend the plan test**

In `crates/photon-core/src/library/items.rs`, in `the_tag_view_is_served_by_its_index`, add the second index assertion after the existing `item_tags_tag` one:

```rust
        assert!(
            plan.iter().any(|step| step.contains("item_user_tags_tag")),
            "expected the user tags arm to probe its index, got {plan:?}"
        );
```

The existing "no step starts with SCAN" assertion already covers the suppression subqueries, which reach `item_user_tags` through its primary key.

- [ ] **Step 6: Run the plan test, and prove it discriminates**

Run: `cargo test -p photon-core the_tag_view_is_served_by_its_index`
Expected: PASS.

Now delete the whole `CREATE INDEX item_user_tags_tag` statement from migration 7 and re-run: the user-tag arm falls back to `SCAN item_user_tags` and the test must FAIL. Restore the statement.

Dropping only the `WHERE added = 1` does **not** discriminate, and is not the check to run: SQLite still probes the now-full index for `tag = ?1` and filters `added = 1` itself, so the plan is unchanged. The predicate earns its place by keeping suppression rows out of the index, not by changing this plan.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/photon-core/src/library/tags.rs crates/photon-core/src/library/items.rs
git commit -m "feat(tags): the Tag view, counts and search follow per-photo changes"
```

---

### Task 6: The tag manager stops going blind

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (`rename_tag_with`, `hide_tag`, `tag_rules`, tests)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces: no signature changes. `rename_tag`, `hide_tag` and `tag_rules()` now treat a tag that exists only in the overlay as existing.

All three decide whether a tag exists with `EXISTS (SELECT 1 FROM item_tags WHERE tag = ?)`. A tag the user created by hand fails that test, so today it cannot be renamed, cannot be removed, and its rule would never be listed in Settings.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/photon-core/src/library/tags.rs`:

```rust
    /// A tag that exists only because the user added it is still the user's tag: the tag
    /// manager has to be able to rename it, remove it, and list what it did.
    #[test]
    fn a_tag_that_exists_only_as_a_user_tag_can_be_managed() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "sunset").unwrap();

        lib.rename_tag("sunset", "dusk").unwrap();
        assert_eq!(lib.tag_rules().unwrap(), [rule("sunset", Some("dusk"))]);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach", "dusk"]);

        lib.hide_tag("dusk").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
        assert_eq!(listed(&lib), [("beach".to_string(), 1)]);
    }

    /// A rename after the fact carries the user's own tags with it, because the overlay is
    /// read through the same rules as the file's keywords.
    #[test]
    fn renaming_a_tag_later_moves_the_users_own_tags_too() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "sunset").unwrap();
        lib.rename_tag("sunset", "dusk").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach", "dusk"]);
        lib.remove_item_tag(ids[0], "dusk").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core --lib tags`
Expected: FAIL — `a_tag_that_exists_only_as_a_user_tag_can_be_managed` finds `tag_rules()` empty, because no rule was written and none would be listed.

- [ ] **Step 3: Widen the three existence checks**

In `rename_tag_with`, the second statement:

```rust
        tx.execute(
            "INSERT INTO tag_rules (tag, target)
             SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM item_tags WHERE tag = ?1)
                              OR EXISTS (SELECT 1 FROM item_user_tags
                                         WHERE tag = ?1 AND added = 1)
             ON CONFLICT (tag) DO UPDATE SET target = excluded.target",
            params![from, to],
        )?;
```

In `hide_tag`, the second statement:

```rust
        tx.execute(
            "INSERT INTO tag_rules (tag, target)
             SELECT ?1, NULL WHERE EXISTS (SELECT 1 FROM item_tags WHERE tag = ?1)
                               OR EXISTS (SELECT 1 FROM item_user_tags
                                          WHERE tag = ?1 AND added = 1)
             ON CONFLICT (tag) DO UPDATE SET target = NULL",
            params![tag],
        )?;
```

In `tag_rules`, the listing query:

```rust
        let mut stmt = conn.prepare(
            "SELECT r.tag, r.target FROM tag_rules r
             WHERE EXISTS (SELECT 1 FROM item_tags t WHERE t.tag = r.tag)
                OR EXISTS (SELECT 1 FROM item_user_tags u
                           WHERE u.tag = r.tag AND u.added = 1)",
        )?;
```

Update `rename_tag_with`'s and `tag_rules`'s doc comments, which both say a tag gets a rule only if "some photo carries it as a keyword": it is now "carries it, as a keyword or as a tag the user added".

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib tags`
Expected: PASS.

- [ ] **Step 5: Prove the test discriminates**

Revert the `OR EXISTS` in `tag_rules`'s listing query alone and re-run: `a_tag_that_exists_only_as_a_user_tag_can_be_managed` must FAIL. Restore it. Do the same for `hide_tag`'s: the `listed(&lib)` assertion must FAIL. Restore it.

- [ ] **Step 6: Run the whole core suite**

Run: `cargo test -p photon-core`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/photon-core/src/library/tags.rs
git commit -m "fix(tags): the tag manager sees tags that exist only in the overlay"
```

---

### Task 7: Engine, IPC and the TypeScript mirror

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (two methods beside `set_star`, a test)
- Modify: `crates/photon-app/src/commands.rs` (two commands, beside `set_star` at :358)
- Modify: `crates/photon-app/src/ipc.rs` (two wrappers)
- Modify: `crates/photon-app/src/app.rs:155-193` (two handler entries)
- Modify: `ui/src/lib/api.ts` (two functions)

**Interfaces:**
- Consumes: `Library::add_item_tag(i64, &str) -> Result<String>` and `Library::remove_item_tag(i64, &str) -> Result<()>`.
- Produces: `Engine::add_item_tag(&self, id: i64, tag: &str) -> Result<String>`, `Engine::remove_item_tag(&self, id: i64, tag: &str) -> Result<()>`; Tauri commands `add_item_tag` / `remove_item_tag`; TS `api.addItemTag(id, tag): Promise<string>` and `api.removeItemTag(id, tag): Promise<void>`.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/photon-app/src/engine.rs`, beside `set_star_writes_the_ini_and_the_grid_follows`:

```rust
    /// A tag change moves Tag-view membership and what search matches, so it has to travel
    /// the refresh chain. Without it the database moves while the grid shows stale rows.
    #[test]
    fn setting_a_tag_refreshes_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let version = f.engine.grid().0;

        assert_eq!(f.engine.add_item_tag(ids[0], "sunset").unwrap(), "sunset");
        assert!(
            f.engine.grid().0 > version,
            "the grid version must move with the tag"
        );

        f.engine.set_tag_view("sunset").unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);

        f.engine.remove_item_tag(ids[0], "sunset").unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
    }
```

`fixture` and `jpeg` are already imported by this `mod tests` block from `crate::testutil`; `set_tag_view` is `Engine`'s own method, the one `commands::set_tag_view` delegates to, so no new imports are needed.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p photon-app setting_a_tag_refreshes_the_grid`
Expected: FAIL to compile — `no method named add_item_tag` on `Engine`.

- [ ] **Step 3: Add the Engine methods**

In `crates/photon-app/src/engine.rs`, below `set_star`:

```rust
    /// Adds `tag` to one photo, returning the name stored — a rename rule can make that
    /// differ from what was typed, and the caller shows the stored name.
    ///
    /// The refresh is not optional: a tag change moves Tag-view membership and the text
    /// search matches, so a change that skipped the refresh chain would update the database
    /// while the grid showed stale rows.
    pub fn add_item_tag(&self, id: i64, tag: &str) -> Result<String> {
        self.live_item(id)?;
        let name = self.lib.add_item_tag(id, tag)?;
        self.refresh_grid()?;
        Ok(name)
    }

    /// Removes the displayed name `tag` from one photo. Refreshes for the same reason.
    pub fn remove_item_tag(&self, id: i64, tag: &str) -> Result<()> {
        self.live_item(id)?;
        self.lib.remove_item_tag(id, tag)?;
        self.refresh_grid()
    }

    /// `NotFound` for an id that has been purged or marked missing, so a stale viewer gets
    /// the same answer here as it does from `set_star`.
    fn live_item(&self, id: i64) -> Result<()> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p photon-app setting_a_tag_refreshes_the_grid`
Expected: PASS.

Then delete the `self.refresh_grid()?;` line from `add_item_tag` and re-run: the version assertion must FAIL. Restore it.

- [ ] **Step 5: Add the three IPC files**

`crates/photon-app/src/commands.rs`, below `set_star`:

```rust
pub fn add_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<String> {
    Ok(engine.add_item_tag(id, tag)?)
}

pub fn remove_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<()> {
    engine.remove_item_tag(id, tag)?;
    Ok(())
}
```

`crates/photon-app/src/ipc.rs`, following the shape of the wrappers around it (copy the `#[tauri::command(async)]` attribute exactly as its neighbours spell it):

```rust
#[tauri::command(async)]
pub fn add_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<String, AppError> {
    commands::add_item_tag(&engine, id, &tag)
}

#[tauri::command(async)]
pub fn remove_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<(), AppError> {
    commands::remove_item_tag(&engine, id, &tag)
}
```

`crates/photon-app/src/app.rs`, in `tauri::generate_handler![…]`, after `ipc::set_star`:

```rust
            ipc::add_item_tag,
            ipc::remove_item_tag,
```

Forgetting this last entry compiles fine and fails at runtime inside the webview.

- [ ] **Step 6: Add the TypeScript mirror**

In `ui/src/lib/api.ts`, beside the other tag functions:

```ts
  /** Adds a tag to one photo. Resolves to the name stored, which a rename rule can make
   *  different from what was typed. */
  addItemTag: (id: number, tag: string) => invoke<string>('add_item_tag', { id, tag }),
  removeItemTag: (id: number, tag: string) => invoke<void>('remove_item_tag', { id, tag }),
```

`ViewerItem.tags` already exists on both sides, so nothing else in the mirror changes.

- [ ] **Step 7: Run both gates and commit**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
git add crates/photon-app/src ui/src/lib/api.ts
git commit -m "feat(tags): IPC for setting a photo's tags"
```

---

### Task 8: The tag editor's logic

**Files:**
- Create: `ui/src/lib/tag-editor.svelte.ts`
- Create: `ui/src/lib/tag-editor.svelte.test.ts`

**Interfaces:**
- Consumes: `api.addItemTag(id, tag): Promise<string>`, `api.removeItemTag(id, tag): Promise<void>` from Task 7.
- Produces: `createTagEditor(deps: { add: (itemId: number, tag: string) => Promise<string>; remove: (itemId: number, tag: string) => Promise<void> })` with `bind(itemId, tags)`, `get list(): string[]`, `busy(tag): boolean`, `suggestions(all: string[]): string[]`, `add(tag): Promise<void>`, `remove(tag): Promise<void>`, and the type `TagEditor`.

There is no component test harness — vitest runs with `environment: 'node'`, so a `.svelte` file cannot be rendered. The logic lives here and is tested here; Task 9 leaves only effect wiring in the component.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/lib/tag-editor.svelte.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest';
import { createTagEditor } from './tag-editor.svelte';

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createTagEditor', () => {
  it('shows a tag optimistically and calls add for the bound photo', async () => {
    const add = vi.fn(async (_id: number, tag: string) => tag);
    const editor = createTagEditor({ add, remove: async () => {} });
    editor.bind(7, ['beach']);

    const p = editor.add('  sunset ');
    expect(editor.list).toEqual(['beach', 'sunset']);
    expect(editor.busy('sunset')).toBe(true);
    await p;
    expect(add).toHaveBeenCalledWith(7, 'sunset');
    expect(editor.busy('sunset')).toBe(false);
  });

  it('shows the name the backend stored when a rule renamed it', async () => {
    const editor = createTagEditor({
      add: async () => 'vacation',
      remove: async () => {},
    });
    editor.bind(7, []);
    await editor.add('holiday');
    expect(editor.list).toEqual(['vacation']);
  });

  it('drops a blank tag and one the photo already carries', async () => {
    const add = vi.fn(async (_id: number, tag: string) => tag);
    const editor = createTagEditor({ add, remove: async () => {} });
    editor.bind(7, ['beach']);
    await editor.add('   ');
    await editor.add('beach');
    expect(add).not.toHaveBeenCalled();
    expect(editor.list).toEqual(['beach']);
  });

  it('removes optimistically and puts the tag back where it was on failure', async () => {
    const editor = createTagEditor({
      add: async (_id, tag) => tag,
      remove: async () => {
        throw new Error('nope');
      },
    });
    editor.bind(7, ['beach', 'sunset', 'dusk']);
    const p = editor.remove('sunset');
    expect(editor.list).toEqual(['beach', 'dusk']);
    await expect(p).rejects.toThrow('nope');
    expect(editor.list).toEqual(['beach', 'sunset', 'dusk']);
  });

  it('reverts only if the same photo is still bound', async () => {
    const gate = deferred<string>();
    const editor = createTagEditor({ add: () => gate.promise, remove: async () => {} });
    editor.bind(7, []);
    const p = editor.add('sunset');
    expect(editor.list).toEqual(['sunset']);

    // The user moved on; the failure must not edit the next photo's tags.
    editor.bind(8, ['sunset']);
    gate.reject(new Error('nope'));
    await expect(p).rejects.toThrow('nope');
    expect(editor.list).toEqual(['sunset']);
  });

  it('drops a second click on a tag whose call is still in flight', async () => {
    const gate = deferred<void>();
    const remove = vi.fn(() => gate.promise);
    const editor = createTagEditor({ add: async (_id, tag) => tag, remove });
    editor.bind(7, ['beach']);
    const first = editor.remove('beach');
    await editor.remove('beach');
    expect(remove).toHaveBeenCalledTimes(1);
    gate.resolve();
    await first;
  });

  it('suggests tags the photo does not have, matched case-insensitively', () => {
    const editor = createTagEditor({ add: async (_id, tag) => tag, remove: async () => {} });
    editor.bind(7, ['Beach']);
    editor.draft = 'be';
    expect(editor.suggestions(['Beach', 'Bergen', 'sunset'])).toEqual(['Bergen']);
    editor.draft = '';
    expect(editor.suggestions(['Beach', 'sunset'])).toEqual(['sunset']);
  });
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npm test -w ui -- src/lib/tag-editor.svelte.test.ts`
Expected: FAIL — cannot resolve `./tag-editor.svelte`.

- [ ] **Step 3: Write the factory**

Create `ui/src/lib/tag-editor.svelte.ts`:

```ts
/** The info panel's tag editor, separated from the inputs that render it.
 *
 *  Optimistic like the album checkboxes and the star, for the same reason: a chip that lags
 *  its own click reads as broken. The revert on failure is guarded by the photo id, because
 *  the user can navigate while a call is in flight and a revert landing on the next photo
 *  would change a tag nobody touched.
 *
 *  `add` resolves to the name the backend stored, which a rename rule can make different
 *  from what was typed; the optimistic chip is replaced with that name.
 *
 *  `add` and `remove` are injected so the machine can be tested without the backend. */
export function createTagEditor(deps: {
  add: (itemId: number, tag: string) => Promise<string>;
  remove: (itemId: number, tag: string) => Promise<void>;
}) {
  let tags = $state<string[]>([]);
  /** Tags with a call in flight; a second click on one of them is dropped, not queued. */
  let pending = $state<Set<string>>(new Set());
  /** What the user is typing. A `let` with accessors, not a property: `$state` is only
   *  valid in a variable declaration or a class field, never in an object literal. */
  let draft = $state('');
  let bound: number | null = null;

  return {
    get draft(): string {
      return draft;
    },

    set draft(value: string) {
      draft = value;
    },

    get list(): string[] {
      return tags;
    },

    busy(tag: string): boolean {
      return pending.has(tag);
    },

    /** Called when the viewer loads a photo, with the tags it carries. */
    bind(itemId: number, current: string[]) {
      bound = itemId;
      tags = [...current];
      pending = new Set();
      draft = '';
    },

    /** Known tags the photo does not carry, matching the draft. Case-insensitive here
     *  rather than in SQL: there is no COLLATE NOCASE in the library and `lower()` is
     *  ASCII-only without ICU, so the whole app matches case in the language, not the
     *  query. */
    suggestions(all: string[]): string[] {
      const typed = draft.trim().toLowerCase();
      const have = new Set(tags.map((t) => t.toLowerCase()));
      return all.filter((t) => !have.has(t.toLowerCase()) && t.toLowerCase().includes(typed));
    },

    /** Adds `tag` to the bound photo. A blank name, a name already shown, or one already in
     *  flight is dropped: each would be a call whose result the user can already see. */
    async add(tag: string): Promise<void> {
      const name = tag.trim();
      if (bound === null || !name || pending.has(name)) return;
      if (tags.some((t) => t === name)) return;
      const id = bound;
      tags = [...tags, name];
      pending = new Set(pending).add(name);
      try {
        const stored = await deps.add(id, name);
        if (bound === id && stored !== name) {
          tags = tags.filter((t) => t !== name && t !== stored).concat(stored);
        }
      } catch (e) {
        if (bound === id) tags = tags.filter((t) => t !== name);
        throw e;
      } finally {
        if (bound === id) {
          const done = new Set(pending);
          done.delete(name);
          pending = done;
        }
      }
    },

    /** Removes `tag` from the bound photo, putting it back at its old position if the call
     *  fails — appending it would reorder the panel for no reason the user can see. */
    async remove(tag: string): Promise<void> {
      if (bound === null || pending.has(tag)) return;
      const id = bound;
      const at = tags.indexOf(tag);
      if (at < 0) return;
      tags = tags.filter((t) => t !== tag);
      pending = new Set(pending).add(tag);
      try {
        await deps.remove(id, tag);
      } catch (e) {
        if (bound === id) {
          const back = [...tags];
          back.splice(at, 0, tag);
          tags = back;
        }
        throw e;
      } finally {
        if (bound === id) {
          const done = new Set(pending);
          done.delete(tag);
          pending = done;
        }
      }
    },
  };
}

export type TagEditor = ReturnType<typeof createTagEditor>;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm test -w ui -- src/lib/tag-editor.svelte.test.ts`
Expected: PASS.

- [ ] **Step 5: Prove the tests discriminate**

Change `if (bound === id) tags = tags.filter((t) => t !== name);` in `add`'s `catch` to an unguarded `tags = tags.filter((t) => t !== name);` and re-run: `reverts only if the same photo is still bound` must FAIL. Restore it. Change `back.splice(at, 0, tag)` in `remove` to `back.push(tag)` and re-run: `removes optimistically and puts the tag back where it was on failure` must FAIL. Restore it.

- [ ] **Step 6: Run the UI gate and commit**

```bash
npm run check
npm test
git add ui/src/lib/tag-editor.svelte.ts ui/src/lib/tag-editor.svelte.test.ts
git commit -m "feat(ui): tag editor logic for the info panel"
```

---

### Task 9: Wire the editor into the viewer

**Files:**
- Modify: `ui/src/components/Viewer.svelte:5` (import), `:83-92` (the membership block), `:304` (the bind on load), `:527-536` (the Keywords section), `:684` (styles)
- Modify: `README.md` (the `## Manual smoke checklist` section)

**Interfaces:**
- Consumes: `createTagEditor` from Task 8, `api.addItemTag` / `api.removeItemTag` from Task 7, and `library.tags` — the `TagCount[]` the library store already loads and refreshes alongside albums and people, which is where the suggestions come from. No new fetch.

What is left in the component is effect wiring, verified by `svelte-check` and the smoke checklist, not by a test: vitest's `node` environment cannot render a `.svelte` file. This is the case the conventions describe — say so in the commit message.

- [ ] **Step 1: Wire the factory into the component's script**

In `ui/src/components/Viewer.svelte`, beside the `createAlbumMembership` import:

```ts
  import { createTagEditor } from '../lib/tag-editor.svelte';
```

Beside the `membership` block (around line 83), matching the comment style that is already there:

```ts
  // The tag editor in the info panel. Bound per photo with the star and the album
  // checkboxes, and optimistic for the same reason.
  const tags = createTagEditor({
    add: (id, tag) => api.addItemTag(id, tag),
    remove: (id, tag) => api.removeItemTag(id, tag),
  });

  function addTag() {
    const draft = tags.draft;
    tags.draft = '';
    tags.add(draft).catch(library.reportError);
  }

  function removeTag(tag: string) {
    tags.remove(tag).catch(library.reportError);
  }
```

`api` is already imported at the top of this file, so the two calls above need no new import.

- [ ] **Step 2: Bind it when a photo loads**

At line ~304, beside `membership.bind(it.id, it.albums);`:

```ts
      tags.bind(it.id, it.tags);
```

- [ ] **Step 3: Replace the Keywords section's markup**

The `Keywords` section (around line 527) currently renders `item.tags` as plain chips. Replace it with the editor, keeping the heading and using the editor's list — `item.tags` is the snapshot from load, `tags.list` is what the user is changing:

```svelte
      <h3>Keywords</h3>
      {#if tags.list.length}
        <ul class="chips">
          {#each tags.list as tag (tag)}
            <li>
              {tag}
              <button
                type="button"
                class="chip-remove"
                aria-label="Remove {tag}"
                disabled={tags.busy(tag)}
                onclick={() => removeTag(tag)}>×</button
              >
            </li>
          {/each}
        </ul>
      {:else}
        <p class="info-muted">No keywords yet.</p>
      {/if}
      <input
        class="tag-input"
        list="tag-suggestions"
        placeholder="Add a keyword"
        bind:value={tags.draft}
        onkeydown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            addTag();
          }
        }}
      />
      <datalist id="tag-suggestions">
        {#each tags.suggestions(library.tags.map((t) => t.tag)) as name (name)}
          <option value={name}></option>
        {/each}
      </datalist>
```

The empty-state copy changes from "No keywords in the file." to "No keywords yet." — the old wording was true when keywords could only come from the file, and is now wrong.

- [ ] **Step 4: Add the styles**

Beside the `.albums` rules at the end of the `<style>` block, following the spacing and shorthand the file already uses:

```css
  .chip-remove {
    margin-left: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    line-height: 1;
    cursor: pointer;
    opacity: 0.6;
  }
  .chip-remove:hover:not(:disabled) { opacity: 1; }
  .chip-remove:disabled { cursor: default; opacity: 0.3; }
  .tag-input { width: 100%; margin-top: 6px; box-sizing: border-box; }
```

- [ ] **Step 5: Run the UI gate**

Run: `npm run check` — expected: 0 errors AND 0 warnings.
Run: `npm test` — expected: PASS.

- [ ] **Step 6: Add the smoke checks**

In `README.md`, under `## Manual smoke checklist`, add, in the style of the entries already there:

```markdown
- Open a photo, type a keyword in the info panel and press Enter: the chip appears at once,
  and the keyword shows in the sidebar's Tags list and finds the photo in search.
- Click the × on a keyword that came from the photo's own metadata: it goes from this photo
  only, and stays gone after the folder is rescanned.
- Rename that keyword in Settings → Tags: the photo's chip follows to the new name.
```

- [ ] **Step 7: Run both gates in full and commit**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
git add ui/src/components/Viewer.svelte README.md
git commit -m "$(cat <<'MSG'
feat(ui): set a photo's tags from the info panel

No test: what is left in the component after createTagEditor is effect
wiring, and vitest runs with environment: 'node', so a .svelte file cannot
be rendered or asserted on. Covered by svelte-check and the README's smoke
checklist instead.
MSG
)"
```

---

## Done when

- A tag typed in the viewer's info panel appears on the photo, in the sidebar's Tags list, in the Tag view and in search, and survives a rescan of its folder.
- A file keyword removed with its × goes from that photo only, and stays gone across a rescan.
- A tag that exists only because the user added it can be renamed and removed in Settings → Tags.
- Both gates pass: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`, `npm run check`, `npm test`.
- No photo file and no `.picasa.ini` is written anywhere in the change.
