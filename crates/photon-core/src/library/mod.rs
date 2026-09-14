mod folders;
mod items;
mod schema;
mod settings;

pub use folders::{Folder, WatchedFolder};
pub use items::{Item, KnownItem, NewItem, RECENT_LIMIT};

use crate::Result;
use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::path::Path;

/// The photon library database. One connection is reserved for writes, a second
/// serves reads so the UI can query while a scan is writing (SQLite WAL mode).
pub struct Library {
    write: Mutex<Connection>,
    read: Mutex<Connection>,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let write = Connection::open(path)?;
        configure(&write)?;
        schema::migrate(&write)?;
        let read = Connection::open(path)?;
        configure(&read)?;
        Ok(Self {
            write: Mutex::new(write),
            read: Mutex::new(read),
        })
    }

    fn writer(&self) -> MutexGuard<'_, Connection> {
        self.write.lock()
    }

    fn reader(&self) -> MutexGuard<'_, Connection> {
        self.read.lock()
    }
}

fn configure(conn: &Connection) -> Result<()> {
    // journal_mode returns a row, so it cannot go through execute_batch.
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error,
        testutil::{temp_library, watch},
    };

    #[test]
    fn open_creates_schema_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("library.db");
        drop(Library::open(&path).unwrap());
        let lib = Library::open(&path).unwrap();
        let version: i64 = lib
            .reader()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let tables: i64 = lib
            .reader()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('watched_folders', 'folders', 'items', 'settings')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 4);
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        drop(Library::open(&path).unwrap());
        rusqlite::Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            Library::open(&path),
            Err(Error::SchemaTooNew {
                found: 99,
                supported: 3
            })
        ));
    }

    #[test]
    fn watched_folder_lifecycle() {
        let (_dir, lib) = temp_library();
        let a = watch(&lib, "/photos/a");
        let again = watch(&lib, "/photos/a");
        assert_eq!(a, again);
        assert!(a.online);
        let b = watch(&lib, "/photos/b");

        lib.set_watched_online(b.id, false).unwrap();
        let all = lib.watched_folders().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            all[1],
            WatchedFolder {
                id: b.id,
                path: "/photos/b".into(),
                online: false
            }
        );

        lib.remove_watched_folder(a.id).unwrap();
        assert_eq!(lib.watched_folders().unwrap().len(), 1);
    }
}
