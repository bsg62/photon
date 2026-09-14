use crate::{Error, Result};
use rusqlite::Connection;

/// Each entry upgrades the schema by one version; `PRAGMA user_version` records the current one.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE watched_folders (
    id     INTEGER PRIMARY KEY,
    path   TEXT NOT NULL UNIQUE,
    online INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE folders (
    id         INTEGER PRIMARY KEY,
    watched_id INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    path       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    sort_key   TEXT NOT NULL,
    seen_scan  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX folders_watched ON folders(watched_id);
CREATE INDEX folders_parent ON folders(parent_id);
CREATE TABLE items (
    id            INTEGER PRIMARY KEY,
    folder_id     INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    path          TEXT NOT NULL UNIQUE,
    file_name     TEXT NOT NULL,
    kind          INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    mtime_ms      INTEGER NOT NULL,
    width         INTEGER NOT NULL,
    height        INTEGER NOT NULL,
    orientation   INTEGER NOT NULL,
    taken_at      INTEGER NOT NULL,
    thumb_state   INTEGER NOT NULL DEFAULT 0,
    thumb_error   TEXT,
    missing_since INTEGER
);
CREATE INDEX items_folder ON items(folder_id, taken_at);
CREATE INDEX items_pending ON items(thumb_state) WHERE missing_since IS NULL;
"#,
    r#"
-- Nullable on purpose: NULL means "not read yet", 0 means "read and unrated". A NOT NULL
-- DEFAULT 0 could not tell those apart, and since the scanner only re-reads a file whose
-- size or mtime changed, photos indexed before this feature would never be looked at
-- again — a Picasa-rated library would upgrade and show an empty Starred view.
ALTER TABLE items ADD COLUMN rating INTEGER;
CREATE INDEX items_starred ON items(rating) WHERE rating >= 1 AND missing_since IS NULL;
"#,
    r#"
-- Small, hand-written UI state that has to outlive the process: at present only the folder
-- the grid was last showing. It lives here rather than in the webview's `localStorage`
-- because that sits in the webview profile directory, is thrown away whenever that cache is
-- cleared, and cannot be reached from a test. Values are TEXT because this table is not
-- worth a column per setting; each caller owns the parsing of its own key.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#,
];

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let supported = MIGRATIONS.len() as i64;
    if current > supported {
        return Err(Error::SchemaTooNew {
            found: current,
            supported,
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings table arriving in a library that already holds a user's photos. The
    /// upgrade must add it and leave everything else alone — nothing here rewrites rows, but
    /// that is the claim worth pinning, since it is the path every existing install takes.
    #[test]
    fn the_third_migration_adds_settings_to_a_populated_version_two_library() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2i64).unwrap();
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
            "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, \
             height, orientation, taken_at, rating) \
             VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1, 3)",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let settings: i64 = conn
            .query_row("SELECT count(*) FROM settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(settings, 0, "a new library remembers nothing yet");
        let rating: Option<i64> = conn
            .query_row(
                "SELECT rating FROM items WHERE path = '/p/a.jpg'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            rating,
            Some(3),
            "the upgrade must not disturb existing rows"
        );
    }

    #[test]
    fn the_second_migration_upgrades_a_version_one_library_without_touching_its_rows() {
        // The upgrade that has never run in anger: a library created by an earlier photon,
        // with a user's photos already indexed.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();
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
            "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, \
             height, orientation, taken_at) VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1)",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let (path, rating): (String, Option<i64>) = conn
            .query_row("SELECT path, rating FROM items", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(path, "/p/a.jpg", "the existing row survives untouched");
        assert_eq!(rating, None, "and reads as unread, not as unrated");
    }
}
