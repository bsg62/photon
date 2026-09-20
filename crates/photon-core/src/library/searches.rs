//! Saved searches: a name over a query string, listed in the sidebar.
//!
//! Unlike an album, a saved search stores nothing about the photos. The query is re-run on
//! every visit, so a photo indexed tomorrow appears in it with nothing updated, and a photo
//! renamed on disk keeps its place - the limitation albums and edits carry (membership by
//! item id) does not apply here at all.
//!
//! What it deliberately does not carry is a count. Albums, People and Tags all show one in
//! the sidebar because theirs is a cheap join; a saved search's is not. `search_entries`
//! selects every live row in the library, builds each one's haystacks and filters them in
//! Rust, so one count per saved search would mean a full library pass per row on every
//! `library_changed`.

use super::Library;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub created_ms: i64,
}

/// A trimmed, non-empty name, or the error the UI shows.
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptySearchName);
    }
    Ok(name)
}

/// A trimmed, non-empty query. Only the surrounding whitespace goes: everything else is
/// kept exactly as typed, because `search::Query` is the only thing entitled to interpret
/// it and it parses on use, not on save.
fn valid_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if query.is_empty() {
        return Err(Error::EmptySearchQuery);
    }
    Ok(query)
}

impl Library {
    pub fn create_saved_search(&self, name: &str, query: &str, now_ms: i64) -> Result<SavedSearch> {
        let name = valid_name(name)?;
        let query = valid_query(query)?;
        let conn = self.writer();
        conn.execute(
            "INSERT INTO saved_searches (name, query, created_ms) VALUES (?1, ?2, ?3)",
            params![name, query, now_ms],
        )?;
        Ok(SavedSearch {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            query: query.to_string(),
            created_ms: now_ms,
        })
    }

    pub fn rename_saved_search(&self, id: i64, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        let changed = self.writer().execute(
            "UPDATE saved_searches SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    pub fn delete_saved_search(&self, id: i64) -> Result<()> {
        let changed = self
            .writer()
            .execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    pub fn saved_search(&self, id: i64) -> Result<Option<SavedSearch>> {
        let found = self
            .reader()?
            .query_row(
                "SELECT id, name, query, created_ms FROM saved_searches WHERE id = ?1",
                params![id],
                |r| {
                    Ok(SavedSearch {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        query: r.get(2)?,
                        created_ms: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Every saved search, sorted by name in Rust - case-insensitively, since `lower()` is
    /// ASCII-only without ICU and there is no `COLLATE NOCASE` anywhere in this schema.
    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare("SELECT id, name, query, created_ms FROM saved_searches")?;
        let mut found = stmt
            .query_map([], |r| {
                Ok(SavedSearch {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    query: r.get(2)?,
                    created_ms: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        found.sort_by_cached_key(|s| (s.name.to_lowercase(), s.name.clone(), s.id));
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_library;

    fn names(lib: &Library) -> Vec<String> {
        lib.saved_searches()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    #[test]
    fn create_trims_the_name_and_the_query() {
        let (_dir, lib) = temp_library();
        let saved = lib
            .create_saved_search("  Canon shots  ", "  camera:canon  ", 10)
            .unwrap();
        assert_eq!(
            (saved.name.as_str(), saved.query.as_str()),
            ("Canon shots", "camera:canon")
        );
        let stored = lib.saved_search(saved.id).unwrap().unwrap();
        assert_eq!((stored.name, stored.query), (saved.name, saved.query));
    }

    #[test]
    fn searches_are_listed_sorted_case_insensitively() {
        let (_dir, lib) = temp_library();
        // Ordered so that a plain byte sort puts the lowercase name last instead.
        lib.create_saved_search("Zurich", "zurich", 10).unwrap();
        lib.create_saved_search("alps", "alps", 11).unwrap();
        assert_eq!(
            names(&lib),
            ["alps", "Zurich"],
            "byte order would be Zurich, alps"
        );
    }

    #[test]
    fn renaming_changes_the_name_and_leaves_the_query() {
        let (_dir, lib) = temp_library();
        let saved = lib
            .create_saved_search("Canon", "camera:canon", 10)
            .unwrap();
        lib.rename_saved_search(saved.id, "  Dad's camera  ")
            .unwrap();
        let stored = lib.saved_search(saved.id).unwrap().unwrap();
        assert_eq!(stored.name, "Dad's camera", "renamed, and trimmed");
        assert_eq!(stored.query, "camera:canon", "the query is untouched");
        assert_eq!(stored.created_ms, 10, "and so is when it was saved");
    }

    #[test]
    fn deleting_removes_only_that_search() {
        let (_dir, lib) = temp_library();
        let canon = lib
            .create_saved_search("Canon", "camera:canon", 10)
            .unwrap();
        lib.create_saved_search("alps", "lake", 11).unwrap();
        lib.delete_saved_search(canon.id).unwrap();
        assert!(lib.saved_search(canon.id).unwrap().is_none());
        assert_eq!(names(&lib), ["alps"], "the other one survives");
    }

    #[test]
    fn renaming_or_deleting_an_unknown_search_is_not_found() {
        let (_dir, lib) = temp_library();
        assert!(matches!(
            lib.rename_saved_search(404, "gone"),
            Err(Error::NotFound(404))
        ));
        assert!(matches!(
            lib.delete_saved_search(404),
            Err(Error::NotFound(404))
        ));
    }

    #[test]
    fn a_saved_search_needs_both_a_name_and_a_query() {
        let (_dir, lib) = temp_library();
        assert!(matches!(
            lib.create_saved_search("   ", "camera:canon", 1),
            Err(Error::EmptySearchName)
        ));
        assert!(matches!(
            lib.create_saved_search("Canon", "   ", 1),
            Err(Error::EmptySearchQuery)
        ));
        assert!(matches!(
            lib.rename_saved_search(1, ""),
            Err(Error::EmptySearchName)
        ));
        assert!(
            lib.saved_searches().unwrap().is_empty(),
            "a refused create wrote nothing"
        );
    }

    /// The grammar belongs to `search::Query`, which parses on use. Storing anything other
    /// than what the user typed would freeze today's operators into old rows.
    #[test]
    fn the_query_is_stored_exactly_as_typed() {
        let (_dir, lib) = temp_library();
        let raw = r#"camera:canon AND "lake como" OR iso400 2019-06"#;
        let saved = lib.create_saved_search("Como", raw, 1).unwrap();
        assert_eq!(saved.query, raw);
        assert_eq!(lib.saved_search(saved.id).unwrap().unwrap().query, raw);
    }
}
