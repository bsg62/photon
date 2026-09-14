//! Small pieces of UI state that have to outlive the process.
//!
//! Only one key so far — the folder the grid was last showing — but the table is generic
//! because a column per setting would mean a migration per setting.

use super::Library;
use crate::Result;

/// The folder whose section was at the top of the grid when photon last closed.
const LAST_FOLDER: &str = "last_folder";

impl Library {
    /// Reads a setting, or `None` when it has never been written.
    fn setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.reader();
        let value = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .ok();
        Ok(value)
    }

    /// Writes a setting, replacing any previous value.
    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.writer().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// The folder to scroll back to on launch, or `None` when there is nothing to restore.
    ///
    /// A folder that no longer exists is reported as `None` rather than handed to the UI to
    /// fail on: a watched folder can be removed, or a directory deleted, between two runs,
    /// and the stored id then points at a row that has been cascaded away. The check is a
    /// join here rather than a lookup in the caller because only the database knows.
    pub fn last_folder(&self) -> Result<Option<i64>> {
        let Some(id) = self
            .setting(LAST_FOLDER)?
            .and_then(|v| v.parse::<i64>().ok())
        else {
            return Ok(None);
        };
        let conn = self.reader();
        let exists = conn
            .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
            .is_ok();
        Ok(exists.then_some(id))
    }

    /// Records the folder the grid is showing, for the next launch.
    pub fn set_last_folder(&self, folder_id: i64) -> Result<()> {
        self.set_setting(LAST_FOLDER, &folder_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::{seed_folder, temp_library};
    use std::path::Path;

    #[test]
    fn the_last_folder_survives_a_reopen() {
        let (dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.set_last_folder(folder).unwrap();
        drop(lib);

        // Reopening the same file is the whole point: this is state for the *next* run.
        let reopened = crate::library::Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(reopened.last_folder().unwrap(), Some(folder));
    }

    #[test]
    fn a_folder_that_no_longer_exists_is_not_restored() {
        // A watched folder removed between two runs takes its folder rows with it. The
        // stored id then points at nothing, and the UI must land at the top of the grid
        // rather than ask the grid for an offset that cannot exist.
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.set_last_folder(folder).unwrap();

        lib.remove_watched_folder(watched).unwrap();

        assert_eq!(lib.last_folder().unwrap(), None);
    }

    #[test]
    fn there_is_nothing_to_restore_in_a_fresh_library() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.last_folder().unwrap(), None);
    }

    #[test]
    fn writing_the_last_folder_again_replaces_it() {
        let (_dir, lib) = temp_library();
        let (watched, first) = seed_folder(&lib, Path::new("/p"));
        let second = lib.upsert_folder(watched, Some(first), "/p/b", 1).unwrap();

        lib.set_last_folder(first).unwrap();
        lib.set_last_folder(second).unwrap();

        assert_eq!(lib.last_folder().unwrap(), Some(second));
    }
}
