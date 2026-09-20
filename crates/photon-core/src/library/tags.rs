//! The user's changes to keywords: renames, merges and removals, held in `tag_rules`.
//!
//! Keywords come from the photo and are never written back, so a change cannot be made to
//! `item_tags` either: the next time the scanner re-reads a photo it rewrites that photo's
//! rows from the file. A rule is instead applied wherever tags are read. The rules are
//! kept flat — no rule's target is itself a ruled tag — so a tag's name is one lookup.

use super::Library;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};
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

/// A keyword and how many live photos carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

/// Each effective keyword row with the rules applied: `(item_id, tag, src, seq)`. A
/// renamed keyword appears under its new name, a removed one is absent, and a tag the user
/// added to one photo joins the file's own keywords. `src` is 0 for a file keyword and 1
/// for a user addition; `seq` is the source table's rowid, so `ORDER BY src, seq` is the
/// file's own keyword order followed by the order the user added tags in. One photo can
/// list two keywords that now share a name, so a reader that needs each once must
/// de-duplicate.
pub(super) const EFFECTIVE_TAGS: &str =
    "SELECT t.item_id, coalesce(r.target, t.tag) AS tag, t.src, t.seq
     FROM (SELECT it.item_id, it.tag, 0 AS src, it.rowid AS seq FROM item_tags it
            WHERE NOT EXISTS (SELECT 1 FROM item_user_tags u
                              WHERE u.item_id = it.item_id AND u.tag = it.tag AND u.added = 0)
           UNION ALL
           SELECT u.item_id, u.tag, 1 AS src, u.rowid AS seq
             FROM item_user_tags u WHERE u.added = 1) t
     LEFT JOIN tag_rules r ON r.tag = t.tag
     WHERE r.tag IS NULL OR r.target IS NOT NULL";

/// The Tag view's filter for the name bound to `?1`: every keyword renamed to it, plus the
/// keyword itself unless it is ruled away, plus every tag the user added under that name —
/// applying the same rules to the overlay's own tags that `EFFECTIVE_TAGS` applies to them,
/// so a tag added and later renamed or hidden moves or drops the same way it does in the
/// panel and the sidebar count. Each keyword arm also subtracts the photos that suppressed
/// their own copy of the keyword. Not written through `EFFECTIVE_TAGS`, whose `coalesce` no
/// index can serve: this form is equality probes on `item_tags_tag` and
/// `item_user_tags_tag`, with the suppression checks reaching `item_user_tags` through its
/// primary key, and `the_tag_view_is_served_by_its_index` holds it to that. `UNION ALL`
/// rather than `OR` because SQLite may answer an OR with a scan.
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
         SELECT item_id FROM item_user_tags
          WHERE added = 1 AND tag IN (SELECT tag FROM tag_rules WHERE target = ?1)
         UNION ALL
         SELECT item_id FROM item_user_tags
          WHERE added = 1 AND tag = ?1 AND NOT EXISTS (SELECT 1 FROM tag_rules WHERE tag = ?1)
     )";

impl Library {
    /// One photo's tags as the user now names them: the file's own keywords in the order
    /// the file lists them, followed by the tags the user added, each once.
    pub fn item_tags(&self, item_id: i64) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT tag FROM ({EFFECTIVE_TAGS}) WHERE item_id = ?1 ORDER BY src, seq"
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

    /// Renames `from` to `to`, merging the two when `to` already exists. Returns the name
    /// stored, which is `to` trimmed.
    ///
    /// Tags already merged into `from` follow it, which keeps the rules flat. `to` loses
    /// any rule of its own: it is the name the user just typed, so it must be a live name,
    /// and a rule `to → x` left behind would be a chain. That includes a rule `tag_rules`
    /// no longer lists because no photo carries `to`: if such photos come back, they show
    /// under the name the user chose to keep live, merged with `from`. That delete is also what makes
    /// renaming a tag back to its original name a restore: the first update turned the
    /// original's rule into `to → to`.
    ///
    /// `from` gets a rule only if some photo carries it as a keyword or as a tag the user
    /// added. A name that exists only as another rule's target (`vacation` after
    /// `holiday → vacation`) has nothing to rename, and a rule for it would show in Settings
    /// as a change no photo reflects.
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<String> {
        self.rename_tag_with(from, to, |_| ())
    }

    /// `rename_tag`, with `guard` run on the stored name after the rename is written and
    /// before it commits. What `guard` returns is held until the commit has finished, so a
    /// lock it returns spans the moment the rename becomes visible to readers. Nothing is
    /// run for a refused or unchanged name. The write lock is already held when `guard`
    /// runs, so a lock it takes must never be held elsewhere while waiting for a write.
    pub fn rename_tag_with<G>(
        &self,
        from: &str,
        to: &str,
        guard: impl FnOnce(&str) -> G,
    ) -> Result<String> {
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
                              OR EXISTS (SELECT 1 FROM item_user_tags
                                         WHERE tag = ?1 AND added = 1)
             ON CONFLICT (tag) DO UPDATE SET target = excluded.target",
            params![from, to],
        )?;
        tx.execute("DELETE FROM tag_rules WHERE tag = ?1", params![to])?;
        let held = guard(to);
        tx.commit()?;
        drop(held);
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
                               OR EXISTS (SELECT 1 FROM item_user_tags
                                          WHERE tag = ?1 AND added = 1)
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

    /// Adds `tag` to one photo, returning the name stored. Adding a name the photo already
    /// carries is not an error: it leaves the photo with the tag, which is what was asked.
    ///
    /// A rename rule's target is stored instead of what was typed, so the user gets the tag
    /// they see. A removal rule for the typed name is dropped — not any rule merged into
    /// it, so this only ever undoes hiding that exact name, and a tag reached through a
    /// separate merged name (`trash → junk` still hides photos carrying `trash` after
    /// `hide_tag("junk")`) stays hidden. That is the one global effect a per-photo action
    /// has. The alternatives were
    /// refusing a name the user has just typed, or storing it literally and watching it
    /// vanish from the panel on the next read, which reads as a bug.
    pub fn add_item_tag(&self, item_id: i64, tag: &str) -> Result<String> {
        Ok(self.add_items_tag(&[item_id], tag)?.0)
    }

    /// `add_item_tag` for several photos at once, returning the name stored and how many
    /// rows were written. One writer for both, so the single-photo path (which is now its
    /// one-element case) cannot drift from it - the same reason `picasa::set_star` became
    /// `set_stars`' one-element case.
    ///
    /// The rename rule is resolved, and a removal rule dropped, **once for the batch**: per
    /// photo, a rule changing under a long write could split one click between two names.
    ///
    /// An id that is no longer a live photo - purged, or soft-deleted by a scan that found
    /// the file gone - writes nothing instead of failing on the foreign key: a selection can
    /// outlive the photos in it, and one such id must not cost the user the rest of the
    /// batch. The count is what landed, so the caller can say so.
    pub fn add_items_tag(&self, item_ids: &[i64], tag: &str) -> Result<(String, usize)> {
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
        let mut count = 0;
        {
            // The `WHERE EXISTS` is also what makes this an upsert SQLite will parse: a
            // SELECT-form INSERT needs a WHERE clause before ON CONFLICT to disambiguate.
            let mut stmt = tx.prepare_cached(
                "INSERT INTO item_user_tags (item_id, tag, added)
                 SELECT ?1, ?2, 1
                  WHERE EXISTS (SELECT 1 FROM items WHERE id = ?1 AND missing_since IS NULL)
                 ON CONFLICT (item_id, tag) DO UPDATE SET added = 1",
            )?;
            for &item_id in item_ids {
                count += stmt.execute(params![item_id, &name])?;
            }
        }
        tx.commit()?;
        Ok((name, count))
    }

    /// Removes the displayed name `tag` from one photo: every tag the user added that shows
    /// under that name is deleted, and every one of the photo's own keywords that shows
    /// under it is suppressed. A merge means one displayed name can stand for several raw
    /// keywords, and all of them have to go or the tag reappears.
    ///
    /// Deletion runs before suppression because a name that is both ends as the single
    /// suppression row the primary key allows.
    pub fn remove_item_tag(&self, item_id: i64, tag: &str) -> Result<()> {
        self.remove_items_tag(&[item_id], tag)?;
        Ok(())
    }

    /// `remove_item_tag` for several photos at once, returning how many photos it changed.
    /// One writer for both, and one transaction for the batch; see `add_items_tag` for why
    /// an id that is no longer there is skipped rather than fatal.
    pub fn remove_items_tag(&self, item_ids: &[i64], tag: &str) -> Result<usize> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut count = 0;
        {
            let mut delete = tx.prepare_cached(
                "DELETE FROM item_user_tags WHERE item_id = ?1 AND added = 1
                   AND coalesce((SELECT target FROM tag_rules WHERE tag = item_user_tags.tag),
                                item_user_tags.tag) = ?2
                   AND EXISTS (SELECT 1 FROM items
                                WHERE id = ?1 AND missing_since IS NULL)",
            )?;
            let mut suppress = tx.prepare_cached(
                "INSERT OR REPLACE INTO item_user_tags (item_id, tag, added)
                 SELECT item_id, tag, 0 FROM item_tags
                  WHERE item_id = ?1
                    AND coalesce((SELECT target FROM tag_rules WHERE tag = item_tags.tag),
                                 item_tags.tag) = ?2
                    AND EXISTS (SELECT 1 FROM items
                                 WHERE id = ?1 AND missing_since IS NULL)",
            )?;
            for &item_id in item_ids {
                // Deletion before suppression, per photo: a name that is both ends as the
                // single suppression row the primary key allows.
                let deleted = delete.execute(params![item_id, tag])?;
                let suppressed = suppress.execute(params![item_id, tag])?;
                if deleted + suppressed > 0 {
                    count += 1;
                }
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Every rule whose keyword some photo still carries as a keyword or as a tag the user
    /// added, sorted by tag case-insensitively in Rust (`lower()` is ASCII-only without ICU).
    ///
    /// A photo on an offline drive still counts: its rows stay until the scanner purges
    /// them. A rule whose keyword has gone entirely is kept but not listed. It applies to
    /// nothing, so there is nothing to restore; and deleting it instead would lose the
    /// user's change when a removed folder is added back.
    pub fn tag_rules(&self) -> Result<Vec<TagRule>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT r.tag, r.target FROM tag_rules r
             WHERE EXISTS (SELECT 1 FROM item_tags t WHERE t.tag = r.tag)
                OR EXISTS (SELECT 1 FROM item_user_tags u
                           WHERE u.tag = r.tag AND u.added = 1)",
        )?;
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
        assert_eq!(
            lib.rename_tag("holiday", "  vacation ").unwrap(),
            "vacation"
        );
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
            [
                rule("holiday", Some("trip")),
                rule("vacation", Some("trip"))
            ]
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

    /// A rule whose keyword has left the library is not listed, but kept: the keyword can
    /// come back (a folder removed and added again), and the user's change with it.
    #[test]
    fn a_rule_is_listed_only_while_some_photo_carries_its_keyword() {
        let (_dir, lib, ids) = library_with(&[&["junk"], &["holiday"]]);
        lib.hide_tag("junk").unwrap();
        lib.rename_tag("holiday", "vacation").unwrap();
        lib.purge_items(&[ids[0]]).unwrap();
        assert_eq!(
            lib.tag_rules().unwrap(),
            [rule("holiday", Some("vacation"))]
        );

        let folder = lib.item(ids[1]).unwrap().unwrap().folder_id;
        let back = NewItem {
            tags: vec!["junk".into()],
            ..new_item(folder, "/p/again.jpg", 5)
        };
        lib.insert_items(&[back]).unwrap();
        assert_eq!(
            lib.tag_rules().unwrap(),
            [rule("holiday", Some("vacation")), rule("junk", None)]
        );
        assert_eq!(listed(&lib), [("vacation".to_string(), 1)]);
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
        // Byte order would put `B` before `a`.
        let (_dir, lib, _) = library_with(&[&["c", "B", "a"]]);
        lib.hide_tag("c").unwrap();
        lib.hide_tag("B").unwrap();
        lib.hide_tag("a").unwrap();
        let tags: Vec<String> = lib
            .tag_rules()
            .unwrap()
            .into_iter()
            .map(|r| r.tag)
            .collect();
        assert_eq!(tags, ["a", "B", "c"]);
    }

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
        assert_eq!(tag_view(&lib, "dusk"), [ids[0]]);
        lib.remove_item_tag(ids[0], "dusk").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }

    /// A tag the user added, later hidden in Settings, must leave the Tag view the same way
    /// it leaves `item_tags` and the sidebar count — `TAG_FILTER`'s overlay arm has to apply
    /// `tag_rules` too, not just match the overlay's raw stored name.
    #[test]
    fn hiding_a_users_own_tag_removes_it_from_the_tag_view() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "sunset").unwrap();
        assert_eq!(tag_view(&lib, "sunset"), [ids[0]]);
        lib.hide_tag("sunset").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
        assert_eq!(tag_view(&lib, "sunset"), Vec::<i64>::new());
    }

    /// Hiding a tag the user added directly creates a rule, so the tag is removed and the
    /// rule is listed. The first UPDATE in hide_tag is a no-op when nothing targets the tag
    /// yet, so the INSERT's OR EXISTS is what writes the rule.
    #[test]
    fn hiding_a_freshly_added_overlay_tag_creates_its_rule() {
        let (_dir, lib, ids) = library_with(&[&["beach"]]);
        lib.add_item_tag(ids[0], "vacation").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach", "vacation"]);
        assert_eq!(
            listed(&lib),
            [("beach".to_string(), 1), ("vacation".to_string(), 1)]
        );

        lib.hide_tag("vacation").unwrap();
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
        assert_eq!(listed(&lib), [("beach".to_string(), 1)]);
        assert_eq!(lib.tag_rules().unwrap(), [rule("vacation", None)]);
    }

    #[test]
    fn a_bulk_add_writes_one_name_to_every_photo() {
        let (_dir, lib, ids) = library_with(&[&[], &["beach"], &[]]);
        let (name, count) = lib.add_items_tag(&ids, "beach").unwrap();
        assert_eq!((name.as_str(), count), ("beach", 3));
        for id in &ids {
            assert_eq!(lib.item_tags(*id).unwrap(), ["beach"]);
        }
    }

    /// The name is resolved once for the batch, not per photo, so one click can never split
    /// across two names - and the stored name is the one the user sees, as for one photo.
    #[test]
    fn a_bulk_add_of_a_renamed_away_name_stores_the_target() {
        let (_dir, lib, ids) = library_with(&[&["holiday"], &[]]);
        lib.rename_tag("holiday", "vacation").unwrap();
        let (name, count) = lib.add_items_tag(&ids, "holiday").unwrap();
        assert_eq!((name.as_str(), count), ("vacation", 2));
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["vacation"]);
    }

    #[test]
    fn a_bulk_remove_suppresses_every_photos_own_keyword() {
        let (_dir, lib, ids) = library_with(&[&["beach", "sun"], &["beach"]]);
        assert_eq!(lib.remove_items_tag(&ids, "beach").unwrap(), 2);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["sun"]);
        assert!(lib.item_tags(ids[1]).unwrap().is_empty());
    }

    /// A selection can outlive the photos in it; one id that has been purged must not cost
    /// the user every other photo in the batch.
    #[test]
    fn an_unknown_id_does_not_stop_the_rest_of_a_batch() {
        let (_dir, lib, ids) = library_with(&[&[], &[]]);
        let with_ghost = [ids[0], 9_999, ids[1]];
        assert_eq!(lib.add_items_tag(&with_ghost, "beach").unwrap().1, 2);
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["beach"]);
        assert_eq!(lib.remove_items_tag(&with_ghost, "beach").unwrap(), 2);
    }

    /// A photo the scanner has marked missing is still in `items` and still selectable in a
    /// grid built before the scan, but its file is gone; a keyword written to it would be a
    /// row the user cannot see and cannot undo.
    #[test]
    fn a_batch_skips_photos_whose_files_have_gone() {
        let (_dir, lib, ids) = library_with(&[&["beach"], &["beach"]]);
        lib.mark_missing(&[ids[0]], 1).unwrap();

        assert_eq!(lib.add_items_tag(&ids, "sun").unwrap().1, 1);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
        assert_eq!(lib.item_tags(ids[1]).unwrap(), ["beach", "sun"]);

        assert_eq!(lib.remove_items_tag(&ids, "beach").unwrap(), 1);
        assert_eq!(lib.item_tags(ids[0]).unwrap(), ["beach"]);
    }
}
