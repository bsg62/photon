//! The library database: one SQLite file, read through a pool and written through one
//! connection.
//!
//! A query that reads every live photo writes its liveness test with a unary plus,
//! `+missing_since IS NULL`. The plus changes no result; it only stops SQLite from choosing
//! an index by that term. `items_size`, `items_pending` and `items_recent` are all partial
//! on exactly that predicate, and photon never runs `ANALYZE`, so the planner has no
//! statistics and assumes a partial index holds half the table: walking one looks cheaper
//! than reading every row, and the bare term is all it takes to make it walk one. Without
//! `items_size` it walks `items_pending`, which costs little - nearly every row has the same
//! `thumb_state`, so that index's order is nearly table order. The duplicate finder's
//! `items_size` is the one it picks now, and size order is random against table order, so
//! every row becomes its own B-tree descent: on a synthetic 300k-photo library (2026-09-27)
//! the Videos count took 232ms walking it and 25ms as a scan.
//!
//! A query meant to be served by a partial index whose predicate includes the term - Recent,
//! the thumbnail queue, the grid views and the Starred, Videos and Hidden counts, the
//! look-alike and duplicate candidates - keeps the bare form: with the plus it could not use
//! its own index either.
//! `ANALYZE` is not the fix: statistics would re-plan every query here at once, and the plan
//! tests that pin today's plans run without them. Every whole-library read has a plan test
//! that fails when its plus goes.

mod albums;
mod detected_faces;
mod duplicates;
mod faces;
mod folders;
mod hidden;
mod items;
mod schema;
mod searches;
mod settings;
mod similar;
mod stats;
mod tags;

pub use albums::{Album, AlbumSummary};
pub use detected_faces::{EmbedCandidate, FaceCandidate};
pub use duplicates::{CopiesArg, HashCandidate, ItemCopy};
pub use faces::{ItemFace, Person};
pub use folders::{Folder, WatchedFolder};
pub use items::{FolderItem, Item, KnownItem, NewItem, RECENT_LIMIT, is_starred};
pub use searches::SavedSearch;
pub use settings::{GridTile, ThemeChoice};
pub use similar::{HashedPhoto, SimilarCandidate};
pub use stats::{CameraCount, LensCount, LibraryStats, STATS_TOP, YearCount};
pub use tags::{TagCount, TagRule};

use crate::Result;
use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::{
    ops::Deref,
    path::{Path, PathBuf},
};

/// Read connections kept open between uses. Enough for the thumbnail workers, the scan
/// thread and a few protocol requests to all be reading at once; a burst beyond it opens
/// connections that are closed again on return rather than kept, so a flick through the
/// grid cannot leave hundreds of file handles behind.
const MAX_IDLE_READERS: usize = 8;

/// The photon library database. One connection is reserved for writes; reads come from a
/// pool, because SQLite in WAL mode serves any number of concurrent readers and photon has
/// several — the grid rebuild and pending-thumbnail sweep during a scan, an `item` lookup
/// per thumbnail request and per worker job, the Starred count behind every `grid_info`.
/// Behind a single shared read connection they all queued, and a scan's ~170ms of queries
/// per 250ms tick stalled every tile the user was watching load.
pub struct Library {
    path: PathBuf,
    write: Mutex<Connection>,
    readers: Mutex<Vec<Connection>>,
    /// How many times the writer was taken, for tests that pin a path as write-free.
    #[cfg(test)]
    writes: std::sync::atomic::AtomicUsize,
    /// How many times a reader was handed out, for tests that count a path's queries.
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

/// A pooled read connection. Returned to the pool on drop.
struct Reader<'a> {
    lib: &'a Library,
    conn: Option<Connection>,
}

impl Deref for Reader<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("connection is present until drop")
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        let conn = self.conn.take().expect("dropped once");
        let mut pool = self.lib.readers.lock();
        if pool.len() < MAX_IDLE_READERS {
            pool.push(conn);
        }
    }
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let write = Connection::open(path)?;
        configure(&write)?;
        schema::migrate(&write)?;
        // One reader opened eagerly, so a database that can be written but not read again
        // (a permissions oddity, a WAL file owned by someone else) fails here rather than
        // at the first query.
        let read = open_reader(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            write: Mutex::new(write),
            readers: Mutex::new(vec![read]),
            #[cfg(test)]
            writes: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            reads: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// The database file this library was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn writer(&self) -> MutexGuard<'_, Connection> {
        #[cfg(test)]
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.write.lock()
    }

    /// How many times the writer has been taken since the library was opened.
    #[cfg(test)]
    pub(crate) fn writes_for_test(&self) -> usize {
        self.writes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many times a reader has been handed out since the library was opened.
    #[cfg(test)]
    pub(crate) fn reads_for_test(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The steps of `sql`'s query plan, for the tests that pin which index serves a query.
    /// With no statistics the planner never looks at the data, so an empty library plans a
    /// query exactly as a full one does.
    #[cfg(test)]
    pub(crate) fn query_plan(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Vec<String> {
        let conn = self.reader().unwrap();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        stmt.query_map(params, |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// A read connection: a pooled one if any is idle, otherwise a freshly opened one.
    /// Never waits for another reader to finish.
    fn reader(&self) -> Result<Reader<'_>> {
        #[cfg(test)]
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pooled = self.readers.lock().pop();
        let conn = match pooled {
            Some(conn) => conn,
            None => open_reader(&self.path)?,
        };
        Ok(Reader {
            lib: self,
            conn: Some(conn),
        })
    }
}

fn open_reader(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
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
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 25);
        let tables: i64 = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('watched_folders', 'folders', 'items', 'settings', 'item_tags', 'contacts', 'faces', 'albums', 'album_items', 'tag_rules', 'item_user_tags', 'saved_searches', 'detected_faces', 'people', 'person_contacts', 'face_rejections')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 16);
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
                supported: 25
            })
        ));
    }

    /// SQLite in WAL mode serves any number of readers at once, and photon has several: the
    /// grid rebuild and the pending-thumbnail sweep every 250ms of a scan, one `item` lookup
    /// per thumbnail request and per worker job, the Starred count behind every `grid_info`.
    /// Behind one shared connection they all queued, so a scan's ~170ms of queries per tick
    /// stalled every tile the user was watching load. A second reader must be able to run
    /// while the first is still held.
    #[test]
    fn a_second_reader_is_not_blocked_by_the_first() {
        let (_dir, lib) = temp_library();
        let lib = std::sync::Arc::new(lib);
        let held = lib.reader().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let other = lib.clone();
        std::thread::spawn(move || {
            let count: i64 = other
                .reader()
                .unwrap()
                .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
                .unwrap();
            tx.send(count).unwrap();
        });
        let answered = rx.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        assert_eq!(
            answered,
            Ok(0),
            "a reader held elsewhere must not block this query"
        );
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
