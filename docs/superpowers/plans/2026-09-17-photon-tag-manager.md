# Tag Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tags section in Settings that lists every tag and renames (merging on collision), removes and restores them, without touching photo files.

**Architecture:** A `tag_rules(tag, target)` table holds renames (`target` set) and removals (`target` NULL). `item_tags` keeps exactly what the files say; every reader applies the rules through one SQL fragment (`EFFECTIVE_TAGS`) or the Tag view's index-friendly `TAG_FILTER`. Rule changes are one write plus a grid rebuild through `Engine::tags_changed`, which also moves an open Tag view onto a renamed tag.

**Tech Stack:** Rust (rusqlite), Tauri 2, Svelte 5 runes + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-17-photon-tag-manager-design.md`

## Global Constraints

- photon never writes photo files; keywords stay read only. No change to `write_tags`, the scanner or `keywords.rs`.
- No native dependencies; case-insensitive sorting and matching are done in Rust/TS, never SQL `lower()` or `COLLATE NOCASE`.
- Every new Rust test must be shown to fail on an assertion with its change reverted (a compile error does not count). Each test step below says how.
- The Rust gate (`cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`) and the UI gate (`npm run check`, `npm test`) pass before every commit.
- Rust field changes and their TS mirror in `ui/src/lib/api.ts` land in the same commit.
- Never launch the GUI to verify; anything visual goes on the README smoke checklist.
- Commits end with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.

## Deviation from the spec

Spec §4 writes the Tag view filter as `tag IN (…) OR (tag = ?1 AND NOT EXISTS …)`. SQLite's OR optimisation is a cost decision and may choose a covering scan of the primary-key index instead. This plan writes the same set as `IN (… UNION ALL …)`, whose two arms are each a plain equality probe on `item_tags_tag`. Task 2 amends the spec's snippet to match.

---

### Task 1: `tag_rules` table and the rule mutations

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (append migration 6; add a migration test)
- Modify: `crates/photon-core/src/library/mod.rs` (register `tags` module, re-exports, version 5→6, table count 9→10)
- Modify: `crates/photon-core/src/error.rs` (add `EmptyTagName`)
- Create: `crates/photon-core/src/library/tags.rs`

**Interfaces:**
- Produces:
  - `photon_core::library::TagRule { pub tag: String, pub target: Option<String> }` (Serialize, camelCase)
  - `Library::rename_tag(&self, from: &str, to: &str) -> Result<String>` — returns the stored (trimmed) name
  - `Library::hide_tag(&self, tag: &str) -> Result<()>`
  - `Library::restore_tag_rule(&self, tag: &str) -> Result<()>`
  - `Library::tag_rules(&self) -> Result<Vec<TagRule>>`
  - `photon_core::Error::EmptyTagName`

- [ ] **Step 1: Add the migration**

Append to `MIGRATIONS` in `schema.rs`, after the fifth entry's closing `"#,`:

```rust
    r#"
-- A keyword the user renamed (target set) or removed (target NULL). Keywords are read from
-- the photo and never written, so a rule is applied wherever tags are read
-- (`library/tags.rs`), never folded into item_tags: a rescan rewrites that table from the
-- file. Every reader of item_tags goes through EFFECTIVE_TAGS or TAG_FILTER; one that does
-- not shows keywords the user renamed or removed.
CREATE TABLE tag_rules (
    tag    TEXT PRIMARY KEY,
    target TEXT
);
CREATE INDEX tag_rules_target ON tag_rules(target);
"#,
```

- [ ] **Step 2: Update the schema tripwires**

In `library/mod.rs`, `open_creates_schema_and_is_idempotent`: change `assert_eq!(version, 5);` to `assert_eq!(version, 6);`, add `'tag_rules'` to the `name IN (…)` list, and change `assert_eq!(tables, 9);` to `assert_eq!(tables, 10);`.

Add to `schema.rs`'s test module:

```rust
    /// The rules table arriving in a library that already has keywords. Nothing existing
    /// may change: the keywords are what the rules are applied to.
    #[test]
    fn the_sixth_migration_adds_tag_rules_and_keeps_keywords() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5i64).unwrap();
        conn.execute("INSERT INTO watched_folders (id, path) VALUES (1, '/p')", [])
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
        conn.execute("INSERT INTO item_tags (item_id, tag) VALUES (1, 'beach')", [])
            .unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 6);
        let rules: i64 = conn
            .query_row("SELECT count(*) FROM tag_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rules, 0, "an upgraded library starts with no rules");
        let tag: String = conn
            .query_row("SELECT tag FROM item_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "beach");
    }
```

- [ ] **Step 3: Run the schema tests; confirm the fail-without-change**

Run: `cargo test -p photon-core --lib schema library::tests::open_creates`
Expected: PASS. Then temporarily delete the new `MIGRATIONS` entry and re-run: both tests fail on assertions (`left: 5, right: 6`; the rules query fails with "no such table" inside `unwrap`, a panic, not a compile error). Restore the entry.

- [ ] **Step 4: Add the error**

In `crates/photon-core/src/error.rs`, after `EmptyAlbumName,`:

```rust
    #[error("a tag needs a name")]
    EmptyTagName,
```

- [ ] **Step 5: Write `tags.rs` with the failing tests**

Create `crates/photon-core/src/library/tags.rs`:

```rust
//! The user's changes to keywords: renames, merges and removals, held in `tag_rules`.
//!
//! Keywords come from the photo and are never written back, so a change cannot be made to
//! `item_tags` either: the next time the scanner re-reads a photo it rewrites that photo's
//! rows from the file. A rule is instead applied wherever tags are read. The rules are
//! kept flat — no rule's target is itself a ruled tag — so a tag's name is one lookup.

use super::Library;
use crate::{Error, Result};
use rusqlite::params;
use serde::Serialize;

/// One change the user made. `target` is the new name, or `None` for a removed tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRule {
    pub tag: String,
    pub target: Option<String>,
}

/// A trimmed, non-empty tag name, or the error the UI shows.
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptyTagName);
    }
    Ok(name)
}

impl Library {
    /// Renames `from` to `to`, merging the two when `to` already exists. Returns the name
    /// stored, which is `to` trimmed.
    ///
    /// Tags already merged into `from` follow it, which keeps the rules flat. `to` loses
    /// any rule of its own: it is the name the user just typed, so it must be a live name,
    /// and a rule `to → x` left behind would be a chain. That delete is also what makes
    /// renaming a tag back to its original name a restore: the first update turned the
    /// original's rule into `to → to`.
    ///
    /// `from` gets a rule only if some photo carries it as a keyword. A name that exists
    /// only as another rule's target (`vacation` after `holiday → vacation`) has nothing to
    /// rename, and a rule for it would show in Settings as a change no photo reflects.
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<String> {
        let to = valid_name(to)?;
        if from == to {
            return Ok(to.to_string());
        }
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE tag_rules SET target = ?2 WHERE target = ?1",
            params![from, to],
        )?;
        tx.execute(
            "INSERT INTO tag_rules (tag, target)
             SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM item_tags WHERE tag = ?1)
             ON CONFLICT (tag) DO UPDATE SET target = excluded.target",
            params![from, to],
        )?;
        tx.execute("DELETE FROM tag_rules WHERE tag = ?1", params![to])?;
        tx.commit()?;
        Ok(to.to_string())
    }

    /// Removes `tag` and everything merged into it. Each merged tag keeps its own rule, now
    /// a removal, so restoring one brings it back under its original name.
    pub fn hide_tag(&self, tag: &str) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE tag_rules SET target = NULL WHERE target = ?1",
            params![tag],
        )?;
        tx.execute(
            "INSERT INTO tag_rules (tag, target)
             SELECT ?1, NULL WHERE EXISTS (SELECT 1 FROM item_tags WHERE tag = ?1)
             ON CONFLICT (tag) DO UPDATE SET target = NULL",
            params![tag],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Drops one rule, so the keyword shows under its own name again. Removing a rule
    /// cannot create a chain, so nothing else needs rewriting. Restoring a rule that is
    /// already gone is not an error: two clicks on a stale list both mean the same thing.
    pub fn restore_tag_rule(&self, tag: &str) -> Result<()> {
        self.writer()
            .execute("DELETE FROM tag_rules WHERE tag = ?1", params![tag])?;
        Ok(())
    }

    /// Every rule, sorted by tag case-insensitively in Rust (`lower()` is ASCII-only
    /// without ICU).
    pub fn tag_rules(&self) -> Result<Vec<TagRule>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare("SELECT tag, target FROM tag_rules")?;
        let mut rules = stmt
            .query_map([], |r| {
                Ok(TagRule {
                    tag: r.get(0)?,
                    target: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rules.sort_by_cached_key(|r| (r.tag.to_lowercase(), r.tag.clone()));
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;
    use tempfile::TempDir;

    /// A library with one photo per entry, each carrying those keywords.
    fn library_with(photos: &[&[&str]]) -> (TempDir, Library, Vec<i64>) {
        let (dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let items: Vec<NewItem> = photos
            .iter()
            .enumerate()
            .map(|(n, tags)| NewItem {
                tags: tags.iter().map(|t| t.to_string()).collect(),
                ..new_item(folder, &format!("/p/{n}.jpg"), n as i64)
            })
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        (dir, lib, ids)
    }

    fn rule(tag: &str, target: Option<&str>) -> TagRule {
        TagRule {
            tag: tag.into(),
            target: target.map(Into::into),
        }
    }

    #[test]
    fn a_rename_is_recorded_under_the_trimmed_name() {
        let (_dir, lib, _) = library_with(&[&["holiday"]]);
        assert_eq!(lib.rename_tag("holiday", "  vacation ").unwrap(), "vacation");
        assert_eq!(
            lib.tag_rules().unwrap(),
            [rule("holiday", Some("vacation"))]
        );
    }

    #[test]
    fn a_blank_rename_is_refused() {
        let (_dir, lib, _) = library_with(&[&["holiday"]]);
        assert!(matches!(
            lib.rename_tag("holiday", "  "),
            Err(Error::EmptyTagName)
        ));
        assert_eq!(lib.tag_rules().unwrap(), []);
    }

    /// Flatness: `vacation` is itself a keyword here, so both it and what was merged into
    /// it end up pointing straight at `trip`.
    #[test]
    fn renaming_a_merged_tag_moves_everything_merged_into_it() {
        let (_dir, lib, _) = library_with(&[&["holiday"], &["vacation"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        lib.rename_tag("vacation", "trip").unwrap();
        assert_eq!(
            lib.tag_rules().unwrap(),
            [rule("holiday", Some("trip")), rule("vacation", Some("trip"))]
        );
    }

    /// `vacation` is carried by no photo, so it gets no rule, and the original's rule is
    /// the identity that the rename deletes.
    #[test]
    fn renaming_a_tag_back_to_its_original_name_removes_the_rule() {
        let (_dir, lib, _) = library_with(&[&["holiday"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        lib.rename_tag("vacation", "holiday").unwrap();
        assert_eq!(lib.tag_rules().unwrap(), []);
    }

    #[test]
    fn renaming_onto_a_renamed_name_revives_it() {
        let (_dir, lib, _) = library_with(&[&["a"], &["b"]]);
        lib.rename_tag("b", "c").unwrap();
        lib.rename_tag("a", "b").unwrap();
        assert_eq!(lib.tag_rules().unwrap(), [rule("a", Some("b"))]);
    }

    #[test]
    fn hiding_a_merged_tag_hides_everything_merged_into_it() {
        let (_dir, lib, _) = library_with(&[&["holiday"], &["vacation"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        lib.hide_tag("vacation").unwrap();
        assert_eq!(
            lib.tag_rules().unwrap(),
            [rule("holiday", None), rule("vacation", None)]
        );
        lib.restore_tag_rule("holiday").unwrap();
        assert_eq!(lib.tag_rules().unwrap(), [rule("vacation", None)]);
        lib.restore_tag_rule("holiday").unwrap();
    }

    #[test]
    fn rules_are_listed_case_insensitively() {
        let (_dir, lib, _) = library_with(&[&["b", "A", "c"]]);
        lib.hide_tag("c").unwrap();
        lib.hide_tag("b").unwrap();
        lib.hide_tag("A").unwrap();
        let tags: Vec<String> = lib.tag_rules().unwrap().into_iter().map(|r| r.tag).collect();
        assert_eq!(tags, ["A", "b", "c"]);
    }
}
```

In `library/mod.rs`: add `mod tags;` after `mod settings;`, and `pub use tags::TagRule;` after the `items` re-export.

- [ ] **Step 6: Run the tests; confirm each discriminates**

Run: `cargo test -p photon-core --lib library::tags`
Expected: all 7 PASS.

Then demonstrate each fails on an assertion, reverting one line at a time and restoring after each:
- delete `let to = valid_name(to)?;` and add `let to = to;` → `a_rename_is_recorded…` and `a_blank_rename…` fail.
- delete the `UPDATE tag_rules SET target = ?2 …` execute → `renaming_a_merged_tag…` fails (`holiday → vacation` left).
- delete the `DELETE FROM tag_rules WHERE tag = ?1` execute → `renaming_a_tag_back…` and `renaming_onto…` fail.
- replace `WHERE EXISTS (SELECT 1 FROM item_tags WHERE tag = ?1)` in `rename_tag` with `WHERE true` → `renaming_a_tag_back…` fails (a `vacation → holiday` rule remains).
- delete the `UPDATE tag_rules SET target = NULL …` execute → `hiding_a_merged_tag…` fails.
- remove the `sort_by_cached_key` line → `rules_are_listed…` fails.

- [ ] **Step 7: Run the Rust gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core
git commit -m "feat(tags): tag_rules table and rename/merge/remove/restore rules

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Every tag reader applies the rules

**Files:**
- Modify: `crates/photon-core/src/library/tags.rs` (move `TagCount`, `item_tags`, `tags_with_counts` here; add `EFFECTIVE_TAGS`, `TAG_FILTER`; tests)
- Modify: `crates/photon-core/src/library/items.rs` (remove the moved items; Tag view and search use the fragments; plan test)
- Modify: `crates/photon-core/src/library/mod.rs` (re-export `TagCount` from `tags`)
- Modify: `docs/superpowers/specs/2026-09-17-photon-tag-manager-design.md` (§4 Tag view snippet)

**Interfaces:**
- Consumes: Task 1's `rename_tag`, `hide_tag`, `restore_tag_rule`, and the `library_with` helper in `tags.rs`'s test module.
- Produces: `pub(super) const EFFECTIVE_TAGS: &str`, `pub(super) const TAG_FILTER: &str`; `Library::item_tags` and `Library::tags_with_counts` with unchanged signatures; `photon_core::library::TagCount` path unchanged.

- [ ] **Step 1: Write the failing reader tests**

Add to `tags.rs`'s test module:

```rust
    use crate::grid::GridView;

    fn tag_view(lib: &Library, tag: &str) -> Vec<i64> {
        lib.entries_for(GridView::Tag, tag)
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect()
    }

    fn search(lib: &Library, query: &str) -> Vec<i64> {
        lib.entries_for(GridView::Search, query)
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect()
    }

    fn listed(lib: &Library) -> Vec<(String, i64)> {
        lib.tags_with_counts()
            .unwrap()
            .into_iter()
            .map(|t| (t.tag, t.count))
            .collect()
    }

    #[test]
    fn a_renamed_tag_is_listed_viewed_searched_and_shown_by_its_new_name() {
        let (_dir, lib, ids) = library_with(&[&["holiday"], &[]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        assert_eq!(listed(&lib), [("vacation".to_string(), 1)]);
        assert_eq!(tag_view(&lib, "vacation"), [ids[0]]);
        assert_eq!(tag_view(&lib, "holiday"), Vec::<i64>::new());
        assert_eq!(search(&lib, "vacation"), [ids[0]]);
        assert_eq!(search(&lib, "holiday"), Vec::<i64>::new());
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["vacation"]);
    }

    #[test]
    fn merging_two_tags_counts_each_photo_once() {
        let (_dir, lib, ids) = library_with(&[&["holiday", "vacation"], &["vacation"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        assert_eq!(listed(&lib), [("vacation".to_string(), 2)]);
        assert_eq!(tag_view(&lib, "vacation"), [ids[0], ids[1]]);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["vacation"]);
    }

    #[test]
    fn a_removed_tag_is_gone_from_every_reader_until_restored() {
        let (_dir, lib, ids) = library_with(&[&["beach", "junk"]]);
        lib.hide_tag("junk").unwrap();
        assert_eq!(listed(&lib), [("beach".to_string(), 1)]);
        assert_eq!(tag_view(&lib, "junk"), Vec::<i64>::new());
        assert_eq!(search(&lib, "junk"), Vec::<i64>::new());
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);

        lib.restore_tag_rule("junk").unwrap();
        assert_eq!(
            listed(&lib),
            [("beach".to_string(), 1), ("junk".to_string(), 1)]
        );
        assert_eq!(tag_view(&lib, "junk"), [ids[0]]);
    }

    /// The reason rules are applied on read: the scanner rewrites a photo's keywords from
    /// the file whenever it re-reads it, and the user's rename must outlive that.
    #[test]
    fn a_rule_survives_rereading_the_keywords() {
        let (_dir, lib, ids) = library_with(&[&["holiday"]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        let row = lib.item(ids[0]).unwrap().unwrap();
        let reread = NewItem {
            tags: vec!["holiday".into()],
            ..new_item(row.folder_id, &row.path, row.taken_at)
        };
        lib.update_item_meta(&[(ids[0], reread)]).unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["vacation"]);
        assert_eq!(tag_view(&lib, "vacation"), [ids[0]]);
    }
```

Run: `cargo test -p photon-core --lib library::tags`
Expected: the four new tests FAIL on assertions (e.g. `left: [("holiday", 1)]`), since no reader applies rules yet. `a_rule_survives…` fails at its first `item_tags` assertion.

- [ ] **Step 2: Move the readers into `tags.rs` and apply the rules**

In `items.rs`, delete the `TagCount` struct (around line 62–67) and the `item_tags` and `tags_with_counts` methods (lines ~404–435), keeping everything else. Update `library/mod.rs`: remove `TagCount` from the `items` re-export and change the tags line to `pub use tags::{TagCount, TagRule};`. Copy `TagCount`'s exact derive and doc comment from `items.rs` into `tags.rs`.

Add to `tags.rs`, above `impl Library`:

```rust
/// Each keyword row with the rules applied: `(item_id, tag, seq)`, a renamed keyword under
/// its new name, a removed one absent. `seq` is `item_tags`' rowid, the order the file
/// listed the keywords in. One photo can list two keywords that now share a name, so a
/// reader that needs each once must de-duplicate.
pub(super) const EFFECTIVE_TAGS: &str = "SELECT t.item_id, coalesce(r.target, t.tag) AS tag, t.rowid AS seq
     FROM item_tags t LEFT JOIN tag_rules r ON r.tag = t.tag
     WHERE r.tag IS NULL OR r.target IS NOT NULL";

/// The Tag view's filter for the name bound to `?1`: every keyword renamed to it, plus the
/// keyword itself unless it is ruled away. Not written through `EFFECTIVE_TAGS`, whose
/// `coalesce` no index can serve: this form is two equality probes on `item_tags_tag`,
/// and `the_tag_view_is_served_by_its_index` holds it to that. `UNION ALL` rather than
/// `OR` because SQLite may answer an OR with a scan.
pub(super) const TAG_FILTER: &str = "AND i.id IN (
         SELECT item_id FROM item_tags
         WHERE tag IN (SELECT tag FROM tag_rules WHERE target = ?1)
         UNION ALL
         SELECT item_id FROM item_tags
         WHERE tag = ?1 AND NOT EXISTS (SELECT 1 FROM tag_rules WHERE tag = ?1)
     )";
```

Add inside `impl Library` in `tags.rs`:

```rust
    /// One photo's tags as the user now names them, in the order the file lists them,
    /// each once.
    pub fn item_tags(&self, item_id: i64) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT tag FROM ({EFFECTIVE_TAGS}) WHERE item_id = ?1 ORDER BY seq"
        ))?;
        let mut tags: Vec<String> = Vec::new();
        for tag in stmt.query_map(params![item_id], |r| r.get::<_, String>(0))? {
            let tag = tag?;
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        Ok(tags)
    }

    /// Every tag carried by at least one live photo, with its photo count. `DISTINCT`
    /// because a photo carrying both halves of a merge has two rows under one name.
    /// Sorted in Rust: the ordering is case-insensitive and `lower()` is ASCII-only
    /// without ICU.
    pub fn tags_with_counts(&self) -> Result<Vec<TagCount>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT e.tag, count(DISTINCT e.item_id)
             FROM ({EFFECTIVE_TAGS}) e JOIN items i ON i.id = e.item_id
             WHERE i.missing_since IS NULL
             GROUP BY e.tag"
        ))?;
        let mut tags = stmt
            .query_map([], |r| {
                Ok(TagCount {
                    tag: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tags.sort_by_cached_key(|t| (t.tag.to_lowercase(), t.tag.clone()));
        Ok(tags)
    }
```

In `items.rs`, add `use super::tags::{EFFECTIVE_TAGS, TAG_FILTER};` to the imports, then:

Replace the `GridView::Tag` arm:

```rust
            GridView::Tag => self.entries_filtered(TAG_FILTER, &[&arg]),
```

Replace the search keyword column (inside `search_entries`'s `format!`):

```rust
                "{GRID_COLUMNS}, i.file_name, f.name, i.make, i.model, i.lens, i.focal_mm, i.aperture, i.iso,
                 (SELECT group_concat(e.tag, ' ') FROM ({EFFECTIVE_TAGS}) e WHERE e.item_id = i.id)"
```

and in `search_entries`' doc comment, change "a correlated subquery over `item_tags`" to "a correlated subquery over the rule-applied keywords (`EFFECTIVE_TAGS`)".

Finally, update the doc comment on `write_tags` in `items.rs` to add, at the end: "The rows are the file's keywords verbatim; the user's renames and removals are applied on read (`library/tags.rs`)."

- [ ] **Step 3: Run the reader tests**

Run: `cargo test -p photon-core --lib library`
Expected: all PASS, including the existing `search_finds_a_photo_by_its_camera_lens_keyword_and_date` and the scanner keyword tests (`cargo test -p photon-core keyword`).

- [ ] **Step 4: Add the plan test**

In `items.rs`'s test module, beside `the_recent_view_is_served_by_its_index`:

```rust
    /// The Tag view probes `item_tags_tag` for each keyword that answers to the name. A
    /// filter rewritten through `EFFECTIVE_TAGS` returns the same rows, but its `coalesce`
    /// cannot use the index, and every Tag view click would scan every keyword.
    #[test]
    fn the_tag_view_is_served_by_its_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                grid_query(GRID_COLUMNS, TAG_FILTER)
            ))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(["x"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("item_tags_tag")),
            "expected an index probe, got {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN item_tags")),
            "no keyword scan: {plan:?}"
        );
    }
```

Run: `cargo test -p photon-core --lib the_tag_view_is_served_by_its_index`
Expected: PASS. If `query_map(["x"], …)` errors with a parameter count mismatch, use `query_map([], …)` (EXPLAIN binds unbound parameters as NULL).

Demonstrate it discriminates: temporarily set the Tag arm to
`self.entries_filtered(&format!("AND i.id IN (SELECT item_id FROM ({EFFECTIVE_TAGS}) WHERE tag = ?1)"), &[&arg])` and the test's `TAG_FILTER` to the same string; the test must FAIL with a `SCAN item_tags` step (or no `item_tags_tag`). If it passes, SQLite found another index — replace the assertions with ones that pin what the plan printed for the good query, and re-check. Restore both.

Demonstrate the reader tests discriminate: temporarily restore each old reader SQL one at a time (`SELECT tag FROM item_tags WHERE item_id = ?1 ORDER BY rowid`; `count(*)` over `item_tags t JOIN items`; the old Tag filter; the old `group_concat` subquery) and confirm at least one of the four new tests fails on an assertion each time. Restore.

- [ ] **Step 5: Amend the spec**

In the spec's §4 Tag view bullet, replace the SQL block with the `TAG_FILTER` text above, and replace "Both arms are equality probes" with "`UNION ALL` rather than `OR`, which SQLite may answer with a scan; both arms are equality probes".

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core docs/superpowers/specs/2026-09-17-photon-tag-manager-design.md
git commit -m "feat(tags): apply tag rules in the tag list, Tag view, search and viewer

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: IPC commands and the Tag view following a rename

**Files:**
- Modify: `crates/photon-app/src/error.rs` (map `EmptyTagName`)
- Modify: `crates/photon-app/src/engine.rs` (`tags_changed`)
- Modify: `crates/photon-app/src/commands.rs` (four commands; test)
- Modify: `crates/photon-app/src/ipc.rs` (four wrappers)
- Modify: `crates/photon-app/src/app.rs` (register four handlers)

**Interfaces:**
- Consumes: Task 1/2 `Library` methods and `TagRule`.
- Produces: Tauri commands `list_tag_rules() -> Vec<TagRule>`, `rename_tag(from, to)`, `hide_tag(tag)`, `restore_tag_rule(tag)`; error kind `"emptyTagName"`; `Engine::tags_changed(&self, renamed: Option<(&str, &str)>) -> Result<()>`.

- [ ] **Step 1: Write the failing command test**

In `commands.rs`'s test module:

```rust
    /// Gives a scanned photo keywords the way the scanner's metadata backfill writes them.
    fn set_keywords(f: &crate::testutil::Fixture, id: i64, tags: &[&str]) {
        use photon_core::library::NewItem;
        use photon_core::media::MediaKind;
        let row = f.engine.lib.item(id).unwrap().unwrap();
        let described = NewItem {
            folder_id: row.folder_id,
            path: row.path.clone(),
            file_name: row.file_name.clone(),
            kind: MediaKind::Image,
            size: row.size,
            mtime_ms: row.mtime_ms,
            width: row.width,
            height: row.height,
            orientation: row.orientation,
            taken_at: row.taken_at,
            rating: None,
            camera: Default::default(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        };
        f.engine.lib.update_item_meta(&[(id, described)]).unwrap();
    }

    /// Renaming the tag on screen must carry the view with it: left on the old name, the
    /// view would show a keyword that now answers to nothing, and the grid would empty.
    #[test]
    fn renaming_the_viewed_tag_keeps_the_view_on_it() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        set_keywords(&f, ids[0], &["holiday"]);
        set_tag_view(&f.engine, "holiday").unwrap();
        assert_eq!(grid_info(&f.engine).len, 1);

        assert_eq!(
            rename_tag(&f.engine, "holiday", " ").unwrap_err().kind,
            "emptyTagName"
        );
        rename_tag(&f.engine, "holiday", " vacation").unwrap();
        let info = grid_info(&f.engine);
        assert_eq!(info.tag.as_deref(), Some("vacation"));
        assert_eq!(info.len, 1);
        assert_eq!(
            list_tag_rules(&f.engine).unwrap(),
            [photon_core::library::TagRule {
                tag: "holiday".into(),
                target: Some("vacation".into()),
            }]
        );

        hide_tag(&f.engine, "vacation").unwrap();
        assert_eq!(grid_info(&f.engine).len, 0, "the removed tag's view empties");
        assert!(list_tags(&f.engine).unwrap().is_empty());

        restore_tag_rule(&f.engine, "holiday").unwrap();
        assert_eq!(list_tags(&f.engine).unwrap()[0].tag, "holiday");
    }
```

Check `crate::testutil` for the fixture's type name (`grep -n "pub struct" crates/photon-app/src/testutil.rs`) and the `Item` field names used above (`grep -n "pub struct Item" -A25 crates/photon-core/src/library/items.rs`); adjust the helper's names to match. If `viewer_item_carries_camera_keywords_faces_and_albums` builds the same `NewItem`, change it to call `set_keywords` only if its fields are otherwise defaults — it sets camera values, so leave it alone.

Run: `cargo test -p photon-app renaming_the_viewed_tag`
Expected: compile failure (commands missing) — not yet proof; proof comes in Step 4.

- [ ] **Step 2: Implement**

`crates/photon-app/src/error.rs`, after the `EmptyAlbumName` arm:

```rust
            EmptyTagName => "emptyTagName",
```

`crates/photon-app/src/engine.rs`, after `albums_changed`:

```rust
    /// After a tag rule change: rebuilds the grid, since the Tag and Search views read the
    /// rules, and the rebuild's `library_changed` is what makes the sidebar refetch its tag
    /// list. A rename carries an open Tag view from the old name to the new one inside the
    /// same state change, so no rebuild can publish the view under a name that now answers
    /// to nothing.
    pub fn tags_changed(&self, renamed: Option<(&str, &str)>) -> Result<()> {
        self.rebuild_or_restore(|state| {
            if let Some((from, to)) = renamed
                && state.view == GridView::Tag
                && state.arg == from
            {
                state.arg = to.to_string();
            }
        })
    }
```

`crates/photon-app/src/commands.rs`: add `TagRule` to the `photon_core::library::{…}` import, and after `list_tags`:

```rust
/// The user's tag renames and removals, for Settings.
pub fn list_tag_rules(engine: &Engine) -> CmdResult<Vec<TagRule>> {
    Ok(engine.lib.tag_rules()?)
}

pub fn rename_tag(engine: &Engine, from: &str, to: &str) -> CmdResult<()> {
    let to = engine.lib.rename_tag(from, to)?;
    engine.tags_changed(Some((from, &to)))?;
    Ok(())
}

pub fn hide_tag(engine: &Engine, tag: &str) -> CmdResult<()> {
    engine.lib.hide_tag(tag)?;
    engine.tags_changed(None)?;
    Ok(())
}

pub fn restore_tag_rule(engine: &Engine, tag: &str) -> CmdResult<()> {
    engine.lib.restore_tag_rule(tag)?;
    engine.tags_changed(None)?;
    Ok(())
}
```

`crates/photon-app/src/ipc.rs`: add `TagRule` to its `photon_core::library` import (find it with `grep -n "TagCount" crates/photon-app/src/ipc.rs`), and after `list_tags`:

```rust
#[tauri::command(async)]
pub fn list_tag_rules(engine: Eng<'_>) -> Result<Vec<TagRule>, AppError> {
    commands::list_tag_rules(&engine)
}

#[tauri::command(async)]
pub fn rename_tag(engine: Eng<'_>, from: String, to: String) -> Result<(), AppError> {
    commands::rename_tag(&engine, &from, &to)
}

#[tauri::command(async)]
pub fn hide_tag(engine: Eng<'_>, tag: String) -> Result<(), AppError> {
    commands::hide_tag(&engine, &tag)
}

#[tauri::command(async)]
pub fn restore_tag_rule(engine: Eng<'_>, tag: String) -> Result<(), AppError> {
    commands::restore_tag_rule(&engine, &tag)
}
```

`crates/photon-app/src/app.rs`, after `ipc::list_tags,`:

```rust
            ipc::list_tag_rules,
            ipc::rename_tag,
            ipc::hide_tag,
            ipc::restore_tag_rule,
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p photon-app renaming_the_viewed_tag`
Expected: PASS.

- [ ] **Step 4: Demonstrate it discriminates**

Temporarily replace the body of `tags_changed` with `self.refresh_grid()`. Run the test: it must FAIL at `info.tag.as_deref() == Some("vacation")` (left `Some("holiday")`). Temporarily remove the `EmptyTagName` arm in `error.rs`: it must FAIL with `left: "internal"`. Restore both.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-app
git commit -m "feat(tags): rename, remove and restore tag commands

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The Tags section in Settings

**Files:**
- Modify: `ui/src/lib/api.ts` (`TagRule`, four functions)
- Create: `ui/src/lib/tags.ts`, `ui/src/lib/tags.test.ts`
- Modify: `ui/src/lib/library.svelte.ts` (three store methods)
- Modify: `ui/src/lib/library.test.ts` (mocks, test)
- Modify: `ui/src/lib/settings.ts` (`'tags'` section)
- Modify: `ui/src/components/Settings.svelte`
- Modify: `README.md` (smoke checklist)

**Interfaces:**
- Consumes: Task 3's commands (`list_tag_rules`, `rename_tag {from, to}`, `hide_tag {tag}`, `restore_tag_rule {tag}`).
- Produces: `api.listTagRules`, `api.renameTag`, `api.hideTag`, `api.restoreTagRule`; `library.renameTag/hideTag/restoreTagRule`; `renameCheck`, `filterTags`, `ruleLabel`.

- [ ] **Step 1: Mirror the API**

In `api.ts`, after `export interface TagCount …`:

```ts
/** A tag the user renamed (`target` set) or removed (`target` null). photon applies it
 *  when reading tags; the photo files keep their keywords. */
export interface TagRule { tag: string; target: string | null }
```

In the `api` object, after `listTags`:

```ts
  listTagRules: () => invoke<TagRule[]>('list_tag_rules'),
  /** Renames a tag, merging it into `to` if that already exists. */
  renameTag: (from: string, to: string) => invoke<void>('rename_tag', { from, to }),
  hideTag: (tag: string) => invoke<void>('hide_tag', { tag }),
  restoreTagRule: (tag: string) => invoke<void>('restore_tag_rule', { tag }),
```

- [ ] **Step 2: Write the failing helper tests**

Create `ui/src/lib/tags.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { TagCount } from './api';
import { filterTags, renameCheck, ruleLabel } from './tags';

const tags: TagCount[] = [
  { tag: 'Beach', count: 3 },
  { tag: 'holiday', count: 1 },
];

describe('renameCheck', () => {
  it.each([
    ['a blank name is refused', 'holiday', '   ', 'blank'],
    ['the same name is no change', 'holiday', ' holiday ', 'same'],
    ['an existing name merges', 'holiday', 'Beach', 'merge'],
    ['names match exactly, so a case change is a merge only onto that exact name', 'holiday', 'beach', 'ok'],
    ['a new name renames', 'holiday', 'vacation', 'ok'],
  ] as const)('%s', (_, from, to, expected) => {
    expect(renameCheck(from, to, tags)).toBe(expected);
  });
});

describe('filterTags', () => {
  it('matches a substring case-insensitively', () => {
    expect(filterTags(tags, 'EAC').map((t) => t.tag)).toEqual(['Beach']);
  });

  it('a blank filter keeps everything', () => {
    expect(filterTags(tags, '  ')).toEqual(tags);
  });
});

describe('ruleLabel', () => {
  it('shows a rename as an arrow', () => {
    expect(ruleLabel({ tag: 'holiday', target: 'vacation' })).toBe('holiday → vacation');
  });

  it('shows a removal', () => {
    expect(ruleLabel({ tag: 'junk', target: null })).toBe('junk — removed');
  });
});
```

Run: `npm test -w ui -- src/lib/tags.test.ts`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement the helpers**

Create `ui/src/lib/tags.ts`:

```ts
import type { TagCount, TagRule } from './api';

/** What committing a rename would do. `same` needs no call; `merge` asks first, because
 *  two tags become one and only restoring the rule separates them again. Names compare
 *  exactly, as the backend does. */
export type RenameCheck = 'blank' | 'same' | 'merge' | 'ok';

export function renameCheck(from: string, to: string, existing: readonly TagCount[]): RenameCheck {
  const name = to.trim();
  if (name === '') return 'blank';
  if (name === from) return 'same';
  return existing.some((t) => t.tag === name) ? 'merge' : 'ok';
}

/** The Settings list's filter: a case-insensitive substring. Done here rather than in the
 *  backend because the whole list is already on screen. */
export function filterTags(tags: readonly TagCount[], query: string): TagCount[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return [...tags];
  return tags.filter((t) => t.tag.toLowerCase().includes(needle));
}

export function ruleLabel(rule: TagRule): string {
  return rule.target === null ? `${rule.tag} — removed` : `${rule.tag} → ${rule.target}`;
}
```

Run: `npm test -w ui -- src/lib/tags.test.ts`
Expected: PASS. Demonstrate: change `name === from` to `to === from` → the `same` case fails; drop `.toLowerCase()` on `t.tag` → the filter case fails. Restore.

- [ ] **Step 4: Store methods, with a failing test**

In `library.test.ts`'s `vi.mock` `api` object, add `renameTag: vi.fn(), hideTag: vi.fn(), restoreTagRule: vi.fn(),`. Add a test after the collections test:

```ts
  it('tag rule changes refetch the collections', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'vacation', count: 1 }]);
    vi.mocked(api.renameTag).mockResolvedValue();
    await store.renameTag('holiday', 'vacation');
    expect(api.renameTag).toHaveBeenCalledWith('holiday', 'vacation');
    expect(store.tags).toEqual([{ tag: 'vacation', count: 1 }]);

    vi.mocked(api.listTags).mockResolvedValue([]);
    vi.mocked(api.hideTag).mockResolvedValue();
    await store.hideTag('vacation');
    expect(store.tags).toEqual([]);

    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'holiday', count: 1 }]);
    vi.mocked(api.restoreTagRule).mockResolvedValue();
    await store.restoreTagRule('holiday');
    expect(store.tags).toEqual([{ tag: 'holiday', count: 1 }]);
  });
```

Run: `npm test -w ui -- src/lib/library.test.ts` → FAIL (`store.renameTag is not a function`).

In `library.svelte.ts`, after `removeFromAlbum`:

```ts
  /** Tag rule changes. The backend's rebuild announces a library change, which refetches
   *  the collections too; refetching here as well means the caller's list is current when
   *  its await returns, not a round trip later. Errors are thrown to the caller. */
  async renameTag(from: string, to: string): Promise<void> {
    await api.renameTag(from, to);
    await this.refreshCollections();
  }

  async hideTag(tag: string): Promise<void> {
    await api.hideTag(tag);
    await this.refreshCollections();
  }

  async restoreTagRule(tag: string): Promise<void> {
    await api.restoreTagRule(tag);
    await this.refreshCollections();
  }
```

Run again → PASS. Demonstrate: remove `await this.refreshCollections();` from `renameTag` → the first `store.tags` assertion fails. Restore.

- [ ] **Step 5: The Settings section**

`ui/src/lib/settings.ts`: `export type SettingsSection = 'folders' | 'tags' | 'about';`

`ui/src/components/Settings.svelte`:

Script — change the imports and add state and handlers:

```ts
  import { onMount, tick } from 'svelte';
  import { api, type AppInfo, type TagCount, type TagRule, type WatchedFolder } from '../lib/api';
  import { filterTags, renameCheck, ruleLabel } from '../lib/tags';
```

```ts
  let rules = $state<TagRule[]>([]);
  let tagFilter = $state('');
  let renaming = $state<string | null>(null);
  let draft = $state('');
  let renameError = $state('');
  let renameInput = $state<HTMLInputElement | undefined>();
  const shownTags = $derived(filterTags(library.tags, tagFilter));

  // The rules change only through this dialog, but a rename's rebuild is what bumps the
  // version, so keying on it covers both our own changes and any later ones.
  $effect(() => {
    void library.info.version;
    let stale = false;
    api
      .listTagRules()
      .then((r) => {
        if (!stale) rules = r;
      })
      .catch(library.reportError);
    return () => {
      stale = true;
    };
  });

  /** The field appears a tick after `renaming` is set; focusing and selecting it then lets
   *  a small correction be a few keystrokes. */
  async function startRename(tag: TagCount) {
    renaming = tag.tag;
    draft = tag.tag;
    renameError = '';
    await tick();
    renameInput?.focus();
    renameInput?.select();
  }

  function cancelRename() {
    renaming = null;
    renameError = '';
  }

  async function commitRename(from: string) {
    const check = renameCheck(from, draft, library.tags);
    if (check === 'blank') {
      renameError = 'A tag needs a name.';
      return;
    }
    const to = draft.trim();
    // Closed before the confirm: the dialog takes focus, and the field's blur would
    // otherwise cancel underneath it.
    cancelRename();
    if (check === 'same') return;
    try {
      if (check === 'merge') {
        const confirmed = await ask(`Merge “${from}” into “${to}”? Photos tagged with either will show under “${to}”.`, {
          title: 'Merge tags',
          kind: 'warning',
        });
        if (!confirmed) return;
      }
      await library.renameTag(from, to);
    } catch (e) {
      library.reportError(e);
    }
  }

  function onRenameKeydown(e: KeyboardEvent, from: string) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commitRename(from);
    } else if (e.key === 'Escape') {
      // The dialog closes on Escape too; this one only closes the field.
      e.preventDefault();
      e.stopPropagation();
      cancelRename();
    }
  }

  async function removeTag(tag: TagCount) {
    try {
      const confirmed = await ask(
        `Remove “${tag.tag}” from photon? The keyword stays in your photo files, and you can restore it under Changes.`,
        { title: 'Remove tag', kind: 'warning' },
      );
      if (!confirmed) return;
      await library.hideTag(tag.tag);
    } catch (e) {
      library.reportError(e);
    }
  }

  async function restore(rule: TagRule) {
    try {
      await library.restoreTagRule(rule.tag);
    } catch (e) {
      library.reportError(e);
    }
  }
```

Note the `commitRename` ordering: `cancelRename()` runs before `renameError` could be shown only for `blank`, which returns first, so the error stays visible with the field open.

Nav — between the Folders and About buttons:

```svelte
        <button class:active={current === 'tags'} aria-current={current === 'tags'} onclick={() => (current = 'tags')}>
          Tags
        </button>
```

Section — change `{:else}` before `<h2>About</h2>` to `{:else if current === 'tags'}` followed by the block below, then `{:else}` before `<h2>About</h2>`:

```svelte
          <h2>Tags</h2>
          <p class="hint">Tags are the keywords in your photos. Renaming or removing one changes how photon shows it; your files keep their keywords.</p>
          {#if library.tags.length === 0}
            <p class="empty">No tags. Keywords saved in your photos appear here.</p>
          {:else}
            <input class="filter" type="search" placeholder="Filter tags" aria-label="Filter tags" bind:value={tagFilter} />
            <ul class="tags">
              {#each shownTags as tag (tag.tag)}
                <li>
                  {#if renaming === tag.tag}
                    <div class="meta">
                      <input
                        class="rename"
                        bind:this={renameInput}
                        bind:value={draft}
                        aria-label="New name for {tag.tag}"
                        aria-invalid={renameError !== ''}
                        onkeydown={(e) => onRenameKeydown(e, tag.tag)}
                        onblur={cancelRename}
                      />
                      {#if renameError}<span class="error">{renameError}</span>{/if}
                    </div>
                  {:else}
                    <div class="meta">
                      <span class="name">{tag.tag}</span>
                      <span class="details">{photoCountLabel(tag.count)}</span>
                    </div>
                    <div class="actions">
                      <button onclick={() => startRename(tag)}>Rename</button>
                      <button class="danger" onclick={() => removeTag(tag)}>Remove…</button>
                    </div>
                  {/if}
                </li>
              {:else}
                <li class="empty">No tag matches “{tagFilter}”.</li>
              {/each}
            </ul>
          {/if}
          {#if rules.length > 0}
            <h2>Changes</h2>
            <ul class="tags">
              {#each rules as rule (rule.tag)}
                <li>
                  <span class="meta name">{ruleLabel(rule)}</span>
                  <div class="actions">
                    <button onclick={() => restore(rule)}>Restore</button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
```

Styles — add:

```css
  .filter, .rename {
    width: 100%;
    padding: 4px 8px;
    border: 1px solid #fff2;
    border-radius: 4px;
    background: var(--panel-2);
    color: inherit;
    font: inherit;
  }
  .filter { margin-bottom: 8px; }
  .tags { margin: 0 0 16px; padding: 0; list-style: none; }
  .tags li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-bottom: 1px solid #ffffff0d;
  }
  .error { color: var(--danger); font-size: 12px; }
```

If `svelte-check` reports `TagCount` unused (it is used by `startRename`/`removeTag` parameter types, so it should not), or an a11y warning, fix it rather than suppressing — the gate is 0 warnings. Check `--danger`/`--panel-2` exist (`grep -rn "\-\-danger" ui/src`); they are already used in this file.

- [ ] **Step 6: README smoke checklist**

After the "Settings → About …" line in `README.md`'s `## Manual smoke checklist`, add:

```markdown
- [ ] Settings → Tags lists every tag with its photo count, and the filter box narrows it. Rename a tag: Enter saves, Escape or clicking away cancels, a blank name is refused. The sidebar's Tags list and an open Tag view follow the new name.
- [ ] Renaming a tag to another existing tag asks to merge, then shows one tag whose count is the photos carrying either. "Remove…" asks first and the tag disappears from the sidebar and from search.
- [ ] Each rename or removal appears under Changes; "Restore" brings the original tag back. Rescanning the folder does not undo a rename, and the photo files' keywords are unchanged (check in another app).
```

- [ ] **Step 7: Gates and commit**

```bash
npm run check && npm test
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add ui README.md
git commit -m "feat(tags): tag manager in Settings

The component is effect wiring and cannot be rendered under vitest's node
environment; its logic is in lib/tags.ts and the store, which are tested, and
the rest is on the smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Documentation touch-ups

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-09-17-photon-tag-manager-design.md` (Status line)

- [ ] **Step 1:** In `CLAUDE.md`'s Conventions bullet on the central promise, change "keywords are read from the photo and never written" to "keywords are read from the photo and never written (the user's renames and removals are `tag_rules` rows applied on read, `library/tags.rs`)". In the Scanning section's metadata-backfill paragraph, after "every writer of an item row goes through `write_tags`.", add: "Every *reader* of keywords goes through `EFFECTIVE_TAGS` or `TAG_FILTER` in `library/tags.rs`, which apply the user's rules; a reader of `item_tags` that bypasses them shows tags the user renamed or removed."
- [ ] **Step 2:** Set the spec's `**Status:**` to `Approved design, implemented`.
- [ ] **Step 3:** Commit: `docs: tag rules in CLAUDE.md; mark the tag manager spec implemented`.
