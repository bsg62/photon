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
    r#"
-- Serves the Recent view (`items::recent_sql`), whose ORDER BY this matches column for
-- column and direction for direction: SQLite only walks an index for an ORDER BY it agrees
-- with exactly, and with LIMIT 500 an index walk stops after 500 rows where a scan sorted
-- every live row first (~37ms on 100k items, once per 250ms scan tick while Recent is
-- open). Partial, so the missing rows the view excludes cost nothing to keep out of it.
CREATE INDEX items_recent ON items(taken_at DESC, file_name DESC, id DESC) WHERE missing_since IS NULL;
"#,
    r#"
-- Camera metadata, keywords, Picasa faces and albums, in one entry because they share a
-- release and each other's plumbing (the parameterised views, the post-walk Picasa pass).
--
-- The camera columns are NULL when the camera wrote nothing, which is common, so NULL cannot
-- also mean "not read yet" the way it does for `rating`. `exif_version` carries that instead:
-- the generation of `describe()` that last read the file, 0 for every row that predates this
-- migration. The scanner re-reads an unchanged file whose version is behind
-- `metadata::EXIF_VERSION`, which is what backfills a library indexed before these columns
-- existed. A NOT NULL DEFAULT is right here precisely because it is *not* the marker.
ALTER TABLE items ADD COLUMN make TEXT;
ALTER TABLE items ADD COLUMN model TEXT;
ALTER TABLE items ADD COLUMN lens TEXT;
ALTER TABLE items ADD COLUMN focal_mm REAL;
ALTER TABLE items ADD COLUMN aperture REAL;
ALTER TABLE items ADD COLUMN exposure_s REAL;
ALTER TABLE items ADD COLUMN iso INTEGER;
ALTER TABLE items ADD COLUMN exif_version INTEGER NOT NULL DEFAULT 0;
-- Keywords read from the photo's own XMP and IPTC. A table rather than a joined column so
-- the Tags list is one GROUP BY and the Tag view one IN (…) filter.
CREATE TABLE item_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (item_id, tag)
);
CREATE INDEX item_tags_tag ON item_tags(tag);
-- Picasa's contacts, merged across every INI: a name recorded in one folder's [Contacts2]
-- resolves the same hash tagged in another folder whose INI never names it.
CREATE TABLE contacts (
    hash TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
-- One row per face Picasa recorded on a photo; the rectangle is fractions of the displayed
-- image, as Picasa's rect64 stores them. `contact` is not a foreign key: a face can be
-- stored before any INI has named its contact.
CREATE TABLE faces (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    contact TEXT NOT NULL,
    left    REAL NOT NULL,
    top     REAL NOT NULL,
    right   REAL NOT NULL,
    bottom  REAL NOT NULL
);
CREATE INDEX faces_item ON faces(item_id);
CREATE INDEX faces_contact ON faces(contact);
-- photon's own albums. Membership is by item id, so a photo renamed on disk (a new row to
-- the scanner) leaves its albums when the old row is purged; recorded in the design as a
-- known limitation.
CREATE TABLE albums (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE TABLE album_items (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    item_id  INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    added_ms INTEGER NOT NULL,
    PRIMARY KEY (album_id, item_id)
);
CREATE INDEX album_items_item ON album_items(item_id);
"#,
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
    r#"
-- The user's per-photo tag changes: a tag added to one photo (added = 1), or one of that
-- photo's own file keywords hidden on it (added = 0). Separate from item_tags for the same
-- reason tag_rules is: a rescan rewrites item_tags from the file, and the user's change
-- must outlive that. Suppressions name the *raw* keyword; additions name it as tag_rules
-- resolves it. The two namespaces meet only in remove_item_tag.
-- A suppression row deliberately outlives the keyword it hides: ON DELETE CASCADE fires
-- only when the *item* is purged, not when a rescan drops the item_tags row because the
-- file no longer carries the keyword. If the keyword later returns to the file, it comes
-- back invisible, with nothing in the UI explaining why; typing the name into the panel
-- (which flips the row to added = 1) is what recovers it.
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
    r#"
-- The duplicate finder. content_hash is the XXH3-128 of the file's bytes, and NULL for
-- nearly every row: only files that share a byte size with another file are ever read
-- (duplicates.rs), since two files of different sizes cannot be identical. A row whose
-- file changes goes back to NULL in update_items.
ALTER TABLE items ADD COLUMN content_hash BLOB;
-- Finds the sizes held by more than one file, which is the candidate query.
CREATE INDEX items_size ON items(size) WHERE missing_since IS NULL;
-- Finds the hashes held by more than one file: the Duplicates view and a photo's copies.
CREATE INDEX items_content_hash ON items(content_hash) WHERE content_hash IS NOT NULL;
"#,
    r#"
-- Non-destructive edits (edit.rs): clockwise quarter turns after the EXIF orientation, then
-- a crop of the turned picture packed as four 16-bit fractions. On items rather than in a
-- table of their own because the grid query derives every thumbnail key from them, and a
-- join per row of a 100k-row rebuild is not free. 0 and NULL are the untouched photo.
ALTER TABLE items ADD COLUMN edit_turns INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN edit_crop INTEGER;
"#,
    r#"
-- Network shares indexed before `paths::canonicalize` existed were stored in the verbatim
-- form `fs::canonicalize` returns, \\?\UNC\server\share\..., which the Windows shell
-- refuses (a dead Reveal) and no one recognises as their share. Rewrite them to
-- \\server\share\..., the form every path takes from now on.
--
-- This is not cosmetic and it cannot be skipped: `paths::same_path` compares paths
-- component-wise, so a root left in the old form while a new canonicalisation produces the
-- new one is a *different* folder to photon - re-adding the share would be allowed rather
-- than recognised, and `scan_subtree`'s `strip_prefix` of a canonicalised event directory
-- would miss its own watched root, which is the file watcher for that share gone quiet.
--
-- The rewrite is by prefix only, matching `paths::simplified_unc`; see the reasoning there
-- for why neither side tests components.
--
-- A thumbnail is cached under a key made from the item's path, so every thumbnail of a
-- rewritten photo is now garbage nothing will look for again. Ordered before the rewrite
-- because it asks whether there was one to do: every library but a Windows one with a
-- share in it must come out of this migration with its settings untouched.
INSERT INTO settings (key, value)
    SELECT 'thumb_gc_epoch', '1'
    WHERE EXISTS (SELECT 1 FROM items WHERE substr(path, 1, 8) = '\\?\UNC\')
    ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT);
UPDATE watched_folders SET path = '\\' || substr(path, 9) WHERE substr(path, 1, 8) = '\\?\UNC\';
UPDATE folders         SET path = '\\' || substr(path, 9) WHERE substr(path, 1, 8) = '\\?\UNC\';
UPDATE items           SET path = '\\' || substr(path, 9) WHERE substr(path, 1, 8) = '\\?\UNC\';
"#,
    r#"
-- A saved search: a name and the query string the search box would hold. Nothing about the
-- photos is stored, because a saved search is not a collection - the query is re-run on
-- every visit, so a photo indexed tomorrow appears in it without anything being updated.
-- That is also why there is no membership table to keep in step and no thumbnail to
-- orphan, which is what makes this the cheapest of photon's sidebar sections.
--
-- The query is stored exactly as typed, operators and all, and parsed by `search::Query`
-- on use. Storing a parsed form would freeze today's grammar into old rows: a library
-- saved before `camera:` existed must gain it by photon learning the word, not by a
-- migration rewriting what the user wrote.
CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    query      TEXT NOT NULL,
    created_ms INTEGER NOT NULL
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

    /// A library that indexed an SMB share before `paths::canonicalize` simplified the
    /// verbatim prefix. Every one of its three path columns has to be rewritten together:
    /// a watched root left in the old form no longer matches the folders under it.
    #[test]
    fn the_tenth_migration_rewrites_verbatim_unc_paths_to_the_share_form() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..9] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 9i64).unwrap();
        conn.execute(
            r"INSERT INTO watched_folders (id, path) VALUES (1, '\\?\UNC\10.0.0.1\photos')",
            [],
        )
        .unwrap();
        conn.execute(
            r"INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key)
             VALUES (1, 1, NULL, '\\?\UNC\10.0.0.1\photos\2024', '2024', '2024')",
            [],
        )
        .unwrap();
        conn.execute(
            r"INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width,
             height, orientation, taken_at, rating)
             VALUES (1, 1, '\\?\UNC\10.0.0.1\photos\2024\a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1, 3)",
            [],
        )
        .unwrap();
        // A local library must come through untouched, which is every library but this one.
        conn.execute(
            "INSERT INTO watched_folders (id, path) VALUES (2, 'C:\\photos')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let paths: Vec<String> = ["watched_folders", "folders", "items"]
            .iter()
            .map(|table| {
                conn.query_row(&format!("SELECT path FROM {table} WHERE id = 1"), [], |r| {
                    r.get(0)
                })
                .unwrap()
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                r"\\10.0.0.1\photos".to_string(),
                r"\\10.0.0.1\photos\2024".to_string(),
                r"\\10.0.0.1\photos\2024\a.jpg".to_string(),
            ],
            "the root, the folder under it and the photo are rewritten as one"
        );
        let local: String = conn
            .query_row("SELECT path FROM watched_folders WHERE id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            local, "C:\\photos",
            "a path without the prefix is left alone"
        );
        let rating: Option<i64> = conn
            .query_row("SELECT rating FROM items WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rating, Some(3), "and nothing else about the row moves");
        // The rewrite changed the key every one of that photo's thumbnails is cached under.
        let epoch: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'thumb_gc_epoch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(epoch, "1", "the orphaned thumbnails are marked collectable");
    }

    /// The Recent view's query, `ORDER BY taken_at DESC, file_name DESC, id DESC LIMIT 500`,
    /// had nothing to walk and so scanned and sorted every live row on every rebuild: ~37ms
    /// on 100k items, once per 250ms scan tick while Recent is open. The partial index
    /// covers exactly that order and turns it into a 500-row index walk.
    #[test]
    fn the_fourth_migration_adds_the_recent_index_to_a_populated_version_three_library() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3i64).unwrap();
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
        let indexed: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'items_recent'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1, "the Recent index exists after the upgrade");
        let rating: Option<i64> = conn
            .query_row(
                "SELECT rating FROM items WHERE path = '/p/a.jpg'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rating, Some(3), "and the existing row is untouched");
    }

    /// The rules table arriving in a library that already has keywords. Nothing existing
    /// may change: the keywords are what the rules are applied to.
    #[test]
    fn the_sixth_migration_adds_tag_rules_and_keeps_keywords() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5i64).unwrap();
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

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 11);
        let rules: i64 = conn
            .query_row("SELECT count(*) FROM tag_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rules, 0, "an upgraded library starts with no rules");
        let tag: String = conn
            .query_row("SELECT tag FROM item_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "beach");
    }

    /// The camera columns, keyword, face and album tables arriving in a populated library.
    /// The existing row must keep its rating and read as `exif_version = 0`: that default is
    /// what the scanner's backfill keys on, so a migration that set it to the current version
    /// would leave every pre-existing photo without camera metadata for good.
    #[test]
    fn the_fifth_migration_adds_metadata_tables_and_leaves_existing_rows_unread() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 4i64).unwrap();
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
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' \
                 AND name IN ('item_tags', 'contacts', 'faces', 'albums', 'album_items')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 5);
        let (rating, exif_version, make): (Option<i64>, i64, Option<String>) = conn
            .query_row(
                "SELECT rating, exif_version, make FROM items WHERE path = '/p/a.jpg'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            rating,
            Some(3),
            "the upgrade must not disturb existing rows"
        );
        assert_eq!(
            exif_version, 0,
            "an existing row reads as not yet read for metadata"
        );
        assert_eq!(make, None);
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
        assert_eq!(version, 11);
        let overlay: i64 = conn
            .query_row("SELECT count(*) FROM item_user_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            overlay, 0,
            "an upgraded library starts with no per-photo changes"
        );
        let tag: String = conn
            .query_row("SELECT tag FROM item_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "beach");
        let target: String = conn
            .query_row("SELECT target FROM tag_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(target, "seaside");
    }
    /// The column arrives NULL on every existing row, which is what makes the first scan
    /// after the upgrade hash the library's same-size files rather than trust a default.
    #[test]
    fn the_eighth_migration_adds_an_empty_content_hash_to_existing_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7i64).unwrap();
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

        migrate(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 11);
        let hash: Option<Vec<u8>> = conn
            .query_row("SELECT content_hash FROM items WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(hash, None);
    }
    /// Every existing photo must come out of the upgrade untouched: an edit is mixed into
    /// the thumbnail key, so a default that read as an edit would orphan the whole cache.
    #[test]
    fn the_ninth_migration_leaves_every_existing_photo_unedited() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8i64).unwrap();
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

        migrate(&conn).unwrap();

        let (turns, crop): (i64, Option<i64>) = conn
            .query_row(
                "SELECT edit_turns, edit_crop FROM items WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((turns, crop), (0, None));
    }

    /// A saved search is a name and a query string, nothing more: the query is re-run on
    /// every visit, so the rows a saved search shows follow the library. The migration must
    /// leave an existing library's own rows alone.
    #[test]
    fn the_eleventh_migration_adds_saved_searches_to_a_populated_version_ten_library() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..10] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 10i64).unwrap();
        conn.execute(
            "INSERT INTO albums (id, name, created_ms) VALUES (1, 'Trip', 5)",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO saved_searches (name, query, created_ms) VALUES ('Canon', 'camera:canon', 7)",
            [],
        )
        .unwrap();
        let (name, query): (String, String) = conn
            .query_row("SELECT name, query FROM saved_searches", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), query.as_str()), ("Canon", "camera:canon"));
        // The album the library already held is untouched.
        let albums: i64 = conn
            .query_row("SELECT count(*) FROM albums", [], |r| r.get(0))
            .unwrap();
        assert_eq!(albums, 1);
    }
}
