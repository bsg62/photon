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

/// A keyword and how many live photos carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

/// Each keyword row with the rules applied: `(item_id, tag, seq)`, a renamed keyword under
/// its new name, a removed one absent. `seq` is `item_tags`' rowid, the order the file
/// listed the keywords in. One photo can list two keywords that now share a name, so a
/// reader that needs each once must de-duplicate.
pub(super) const EFFECTIVE_TAGS: &str =
    "SELECT t.item_id, coalesce(r.target, t.tag) AS tag, t.rowid AS seq
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

impl Library {
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
}
