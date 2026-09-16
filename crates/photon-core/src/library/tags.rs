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
        let (_dir, lib, _) = library_with(&[&["b", "A", "c"]]);
        lib.hide_tag("c").unwrap();
        lib.hide_tag("b").unwrap();
        lib.hide_tag("A").unwrap();
        let tags: Vec<String> = lib
            .tag_rules()
            .unwrap()
            .into_iter()
            .map(|r| r.tag)
            .collect();
        assert_eq!(tags, ["A", "b", "c"]);
    }
}
