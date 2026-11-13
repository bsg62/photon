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
    r#"
-- Look-alikes: photos that are the same picture without being the same bytes - a re-saved
-- JPEG, an emailed copy at 2048px, a re-export from another program. `content_hash` cannot
-- see those, and the README says so.
--
-- `percep_hash` is a 64-bit difference hash of the photo's own 256px grid thumbnail, so it
-- costs no decode the thumbnail has not already paid for, and it is of the photo *as photon
-- shows it* (the thumbnail is keyed by `Item::thumb_key()`, which mixes in the edit).
-- NULL until that thumbnail exists.
--
-- `similar_group` holds the smallest item id in the photo's look-alike group, or NULL when
-- the photo resembles nothing. It is recomputed wholesale after each scan rather than
-- updated in place: an incremental version has to reason about a group *splitting* when a
-- photo is purged, which is the kind of state that goes quietly wrong.
ALTER TABLE items ADD COLUMN percep_hash INTEGER;
ALTER TABLE items ADD COLUMN similar_group INTEGER;

-- Serves the widened Duplicates filter. Partial, because almost every row is NULL.
CREATE INDEX items_similar_group ON items(similar_group) WHERE similar_group IS NOT NULL;
"#,
    r#"
-- Hidden photos: Picasa's Hide. A hidden photo leaves every view and count and waits in the
-- Hidden view; the file is untouched. A flag on the row, like a star, so a rewritten file
-- keeps it (`update_items` does not touch the column) and a renamed one loses it with its
-- old row, as its albums and edits do.
ALTER TABLE items ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;

-- Serves the Hidden view and its sidebar count without reading the rest of the library.
-- Shaped like `items_folder`, which the grid's per-folder walk reads through.
CREATE INDEX items_hidden ON items(folder_id, taken_at) WHERE hidden = 1 AND missing_since IS NULL;
"#,
    r#"
-- What Picasa's INI last said about hiding the photo (`hidden=yes`): NULL until the Picasa
-- pass has read it, then 0 or 1. photon never writes that line, so the two sides cannot be
-- kept equal the way stars are; instead the INI is followed on *change*, and this column is
-- how a change is told from a repeat. A photo the user unhid in photon stays visible until
-- Picasa's own answer changes again.
ALTER TABLE items ADD COLUMN picasa_hidden INTEGER;
"#,
    r#"
-- The look-alike distance is stored as the number it means, and what Conservative and Loose
-- mean moved: probing the buckets one bit away made grouping exact up to 7 bits, where it
-- was exact only up to 3, so Conservative is 7 and Loose 10. A library that stored a choice
-- keeps the choice, not the old number: left at 3, the Settings dialog would show neither
-- option selected. Anything else stored (a hand edit, a newer photon) is left to the clamp.
UPDATE settings SET value = CASE value WHEN '3' THEN '7' WHEN '6' THEN '10' ELSE value END
 WHERE key = 'similar_distance';
"#,
    r#"
-- Picasa's Hide Folder. The flag is the folder's own answer; what it means is carried by
-- `items.hidden` - hiding the folder hides its photos, and a photo inserted into a hidden
-- folder is inserted hidden - so visibility stays one rule, read from one column.
ALTER TABLE folders ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
-- Picasa's albums (`[.album:<token>]` in a folder's INI) are rows here too, so every reader
-- of albums serves them unchanged. The token is what the scan matches an album by; NULL marks
-- one of photon's own, and a unique index admits any number of NULLs.
ALTER TABLE albums ADD COLUMN picasa_token TEXT;
CREATE UNIQUE INDEX albums_picasa_token ON albums(picasa_token);
"#,
    r#"
-- The caption the photo carries (XMP dc:description, else IPTC 2:120), read by the
-- scanner. NULL until a scan has read the file under EXIF_VERSION 3; the backfill
-- re-reads every unchanged photo once to fill it in.
ALTER TABLE items ADD COLUMN caption TEXT;
"#,
    r#"
-- When the INI that last named a Picasa album was written (its mtime, in ms). A name is only
-- taken from an INI newer than this, so a stale copy of a folder - a backup made before a
-- rename in Picasa - cannot take turns with the renamed folders on every scan. NULL for
-- photon's own albums, for a token only referenced so far, and for albums named before
-- this column existed: the next definition that differs names it once.
ALTER TABLE albums ADD COLUMN picasa_named_at INTEGER;
"#,
    r#"
-- A video's running time in milliseconds, from its container (`photon_core::video`). NULL for
-- every photo, and for a video whose container did not say.
ALTER TABLE items ADD COLUMN duration_ms INTEGER;
"#,
    r#"
-- A name the user gave the folder in photon, shown in place of its directory name. NULL
-- for none. Never read from or written to the disk: the directory keeps its name, and a
-- folder whose row is pruned (its directory gone, renamed or moved) loses the alias with it.
ALTER TABLE folders ADD COLUMN alias TEXT;
"#,
    r#"
-- The Starred and Videos views read only the rows they show. Each index is shaped like
-- `items_hidden`, which is why Hidden was already fast: the grid's driver (`folder_order`)
-- groups the view's rows by folder, and the outer walk reads each folder's rows in capture
-- order, so an index on `(folder_id, taken_at)` holding exactly those rows serves both
-- without touching the rest. Before, both went through `items_folder`, which holds every
-- row: Starred and Videos read the whole library twice to return a few percent of it.
--
-- `items_starred` was `(rating)`, which served the Starred count and nothing else. The count
-- still reads it, but only because it is the last-created of the partial indexes its WHERE
-- implies (`items_pending`, `items_size` and `items_recent` tie with it; so for the Videos
-- count and `items_videos`) - see CLAUDE.md, "Schema". The counts' plan test pins it.
--
-- There is deliberately no such index for All (`missing_since IS NULL AND hidden = 0`):
-- every visible view's WHERE implies that predicate, so it would tie with these two and
-- take them over. CLAUDE.md, "Schema", has the reasoning.
DROP INDEX items_starred;
CREATE INDEX items_starred ON items(folder_id, taken_at) WHERE rating >= 1 AND missing_since IS NULL;
CREATE INDEX items_videos ON items(folder_id, taken_at) WHERE kind = 1 AND missing_since IS NULL;
"#,
    r#"
-- Where the photo was taken, in decimal degrees (north and east positive), from its EXIF GPS
-- IFD. Both NULL for a photo without a position. Filled by the metadata backfill
-- (`EXIF_VERSION` 4). No index: the one reader that filters on them is search, which reads
-- every row anyway.
ALTER TABLE items ADD COLUMN gps_lat REAL;
ALTER TABLE items ADD COLUMN gps_lon REAL;
"#,
    r#"
-- The faces photon finds itself (`face_detect`), beside the ones Picasa recorded in `faces`.
-- A table of its own: `set_item_faces` replaces a photo's Picasa faces wholesale on every
-- reread of its INI, and a later stage hangs an embedding on a detected face by `id`.
--
-- The rectangle is fractions of the picture AS SHOWN, edit applied, because that is the
-- preview the detector reads. `faces` rows are fractions of the unedited picture and are
-- mapped through the edit on read. `face_detect::merge` is the one place the two meet.
--
-- `landmarks` is the model's five points as ten little-endian f32 fractions.
CREATE TABLE detected_faces (
    id        INTEGER PRIMARY KEY,
    item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    left      REAL NOT NULL,
    top       REAL NOT NULL,
    right     REAL NOT NULL,
    bottom    REAL NOT NULL,
    landmarks BLOB NOT NULL,
    score     REAL NOT NULL
);
CREATE INDEX detected_faces_item ON detected_faces(item_id);
-- Which `face_detect::DETECTOR_VERSION` last looked at the photo, faces or none; NULL for
-- never. No index: the candidate query walks the table by id.
ALTER TABLE items ADD COLUMN face_version INTEGER;
"#,
    r#"
-- People: groups of faces photon takes for one person, named by the user or not. See
-- `photon_core::people` and `library/people.rs`. No centroid is stored: it is computed from
-- the faces at the start of each grouping step, so no writer can leave it out of date.
--
-- `people` and `detected_faces` are the first tables made AUTOINCREMENT; migration 27 does
-- the same for every other table whose ids the window holds. The People page holds their
-- ids and acts on them later, and an id the UI holds must never name a different row.
-- Without it SQLite hands the highest deleted id to the next insert, and the grouping step
-- deletes empty groups and creates new ones, as a re-detection deletes a photo's faces and
-- inserts new ones - a confirm, a rename or a delete would then land on a face or person
-- the user never saw.
CREATE TABLE people (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT,
    ignored INTEGER NOT NULL DEFAULT 0
);
-- A person's Picasa contacts. A table, not a column: a Picasa library can hold two
-- contacts for one human. A contact belongs to at most one person.
CREATE TABLE person_contacts (
    contact   TEXT PRIMARY KEY,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE
);
CREATE INDEX person_contacts_person ON person_contacts(person_id);
-- `detected_faces` is rebuilt rather than altered, since AUTOINCREMENT cannot be added to
-- an existing table. The rows keep their ids; AUTOINCREMENT then starts above the highest.
-- New columns: a face's vector (128 little-endian f32), the embedder that made it (set
-- even when the face was too small to have one), its group, whether the user put it
-- there, and whether it is to be left out of grouping. Nothing references the table yet
-- (`face_rejections` is created below), so the drop, run with foreign keys on, cascades
-- nowhere; migration 27, whose tables are referenced, has to run with them off.
CREATE TABLE detected_faces_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id           INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    left              REAL NOT NULL,
    top               REAL NOT NULL,
    right             REAL NOT NULL,
    bottom            REAL NOT NULL,
    landmarks         BLOB NOT NULL,
    score             REAL NOT NULL,
    embedding         BLOB,
    embedding_version INTEGER,
    person_id         INTEGER REFERENCES people(id) ON DELETE SET NULL,
    confirmed         INTEGER NOT NULL DEFAULT 0,
    ignored           INTEGER NOT NULL DEFAULT 0
);
INSERT INTO detected_faces_new (id, item_id, left, top, right, bottom, landmarks, score)
    SELECT id, item_id, left, top, right, bottom, landmarks, score FROM detected_faces;
DROP TABLE detected_faces;
ALTER TABLE detected_faces_new RENAME TO detected_faces;
CREATE INDEX detected_faces_item ON detected_faces(item_id);
CREATE INDEX detected_faces_person ON detected_faces(person_id);
-- "Not this person": the face is never put in that group again.
CREATE TABLE face_rejections (
    face_id   INTEGER NOT NULL REFERENCES detected_faces(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    PRIMARY KEY (face_id, person_id)
);
"#,
    // 26: the lookup a scan makes for each new file, to find the row it used to be
    // (`Library::move_candidates`). Not partial, unlike `items_size`: a row a previous scan
    // marked missing is the likeliest candidate of all.
    r#"
CREATE INDEX items_moved ON items(size, mtime_ms);
"#,
    r#"
-- Every other table whose ids the window holds becomes AUTOINCREMENT, as `people` and
-- `detected_faces` did in migration 25, so that an id names one row for ever. The window
-- keeps ids across frames and round trips - the selection, a dialog's subject, a queued
-- write - and `settings.last_folder` keeps one across launches. Without AUTOINCREMENT SQLite
-- hands the highest deleted id to the next insert, so a keyword picker left open while its
-- photo was purged could write to a photo indexed after it.
--
-- AUTOINCREMENT cannot be added to an existing table, so each one is rebuilt: a new table
-- with today's columns (the first migration's plus every ADD COLUMN since, in order), the
-- rows copied with their ids, the old table dropped and the new one renamed. Inserting the
-- old ids starts each sequence at the highest surviving one.
--
-- `run` applies this migration with foreign keys off (WITHOUT_FOREIGN_KEYS). Every one of
-- these tables is referenced with ON DELETE CASCADE, and a DROP TABLE with foreign keys on
-- deletes its rows first: dropping `watched_folders` would cascade through `folders` and
-- `items` into every album membership, keyword and face, and the upgrade would succeed. The
-- references in the new tables name the final tables, which the renames then satisfy.
--
-- The indexes are recreated in the order a version-26 library holds them in
-- `sqlite_master`. photon never runs ANALYZE, so among partial indexes that tie SQLite takes
-- the last one created: `items_starred` serves the Starred count only by being created after
-- `items_pending`, `items_recent` and `items_size` (see migration 22), and a reordering
-- would move that count to another plan.
CREATE TABLE watched_folders_new (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    path   TEXT NOT NULL UNIQUE,
    online INTEGER NOT NULL DEFAULT 1
);
INSERT INTO watched_folders_new (id, path, online)
    SELECT id, path, online FROM watched_folders;
DROP TABLE watched_folders;
ALTER TABLE watched_folders_new RENAME TO watched_folders;

CREATE TABLE folders_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    watched_id INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    path       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    sort_key   TEXT NOT NULL,
    seen_scan  INTEGER NOT NULL DEFAULT 0,
    hidden     INTEGER NOT NULL DEFAULT 0,
    alias      TEXT
);
INSERT INTO folders_new (id, watched_id, parent_id, path, name, sort_key, seen_scan, hidden, alias)
    SELECT id, watched_id, parent_id, path, name, sort_key, seen_scan, hidden, alias FROM folders;
DROP TABLE folders;
ALTER TABLE folders_new RENAME TO folders;
CREATE INDEX folders_watched ON folders(watched_id);
CREATE INDEX folders_parent ON folders(parent_id);

CREATE TABLE items_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
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
    missing_since INTEGER,
    rating        INTEGER,
    make          TEXT,
    model         TEXT,
    lens          TEXT,
    focal_mm      REAL,
    aperture      REAL,
    exposure_s    REAL,
    iso           INTEGER,
    exif_version  INTEGER NOT NULL DEFAULT 0,
    content_hash  BLOB,
    edit_turns    INTEGER NOT NULL DEFAULT 0,
    edit_crop     INTEGER,
    percep_hash   INTEGER,
    similar_group INTEGER,
    hidden        INTEGER NOT NULL DEFAULT 0,
    picasa_hidden INTEGER,
    caption       TEXT,
    duration_ms   INTEGER,
    gps_lat       REAL,
    gps_lon       REAL,
    face_version  INTEGER
);
INSERT INTO items_new (id, folder_id, path, file_name, kind, size, mtime_ms, width, height,
                       orientation, taken_at, thumb_state, thumb_error, missing_since, rating,
                       make, model, lens, focal_mm, aperture, exposure_s, iso, exif_version,
                       content_hash, edit_turns, edit_crop, percep_hash, similar_group,
                       hidden, picasa_hidden, caption, duration_ms, gps_lat, gps_lon,
                       face_version)
    SELECT id, folder_id, path, file_name, kind, size, mtime_ms, width, height,
           orientation, taken_at, thumb_state, thumb_error, missing_since, rating,
           make, model, lens, focal_mm, aperture, exposure_s, iso, exif_version,
           content_hash, edit_turns, edit_crop, percep_hash, similar_group,
           hidden, picasa_hidden, caption, duration_ms, gps_lat, gps_lon,
           face_version
      FROM items;
DROP TABLE items;
ALTER TABLE items_new RENAME TO items;
CREATE INDEX items_folder ON items(folder_id, taken_at);
CREATE INDEX items_pending ON items(thumb_state) WHERE missing_since IS NULL;
CREATE INDEX items_recent ON items(taken_at DESC, file_name DESC, id DESC) WHERE missing_since IS NULL;
CREATE INDEX items_size ON items(size) WHERE missing_since IS NULL;
CREATE INDEX items_content_hash ON items(content_hash) WHERE content_hash IS NOT NULL;
CREATE INDEX items_similar_group ON items(similar_group) WHERE similar_group IS NOT NULL;
CREATE INDEX items_hidden ON items(folder_id, taken_at) WHERE hidden = 1 AND missing_since IS NULL;
CREATE INDEX items_starred ON items(folder_id, taken_at) WHERE rating >= 1 AND missing_since IS NULL;
CREATE INDEX items_videos ON items(folder_id, taken_at) WHERE kind = 1 AND missing_since IS NULL;
CREATE INDEX items_moved ON items(size, mtime_ms);

CREATE TABLE albums_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    created_ms      INTEGER NOT NULL,
    picasa_token    TEXT,
    picasa_named_at INTEGER
);
INSERT INTO albums_new (id, name, created_ms, picasa_token, picasa_named_at)
    SELECT id, name, created_ms, picasa_token, picasa_named_at FROM albums;
DROP TABLE albums;
ALTER TABLE albums_new RENAME TO albums;
CREATE UNIQUE INDEX albums_picasa_token ON albums(picasa_token);

CREATE TABLE saved_searches_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    query      TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
INSERT INTO saved_searches_new (id, name, query, created_ms)
    SELECT id, name, query, created_ms FROM saved_searches;
DROP TABLE saved_searches;
ALTER TABLE saved_searches_new RENAME TO saved_searches;

-- A folder deleted before the upgrade while it held the highest id left no trace the
-- sequence can see, so its number would be handed out once more. If it is the folder
-- Settings remembers, the next launch would open on a stranger. The value is text and only
-- ever written by photon as an id, so anything that is not all digits is left alone, and so
-- is anything over 18 digits: CAST clamps it to the largest integer, and a sequence pinned
-- there leaves no id for the next folder.
--
-- The copy above always leaves a `folders` row to raise, even from an empty table, because
-- SQLite writes the sequence back at the end of every INSERT into an AUTOINCREMENT table.
-- That is not documented; `migration_27_never_hands_out_the_remembered_folders_id` removes
-- every folder to catch SQLite ever stopping.
WITH remembered (id) AS (
    SELECT CAST(value AS INTEGER) FROM settings
     WHERE key = 'last_folder'
       AND length(value) BETWEEN 1 AND 18
       AND value GLOB '[0-9]*' AND value NOT GLOB '*[^0-9]*'
)
UPDATE sqlite_sequence SET seq = (SELECT id FROM remembered)
 WHERE name = 'folders' AND (SELECT id FROM remembered) > seq;
"#,
];

/// Versions (1-based) whose migration rebuilds a table that others reference, and so
/// runs with foreign keys off. See `run`.
const WITHOUT_FOREIGN_KEYS: &[usize] = &[27];

pub fn migrate(conn: &Connection) -> Result<()> {
    run(conn, MIGRATIONS, WITHOUT_FOREIGN_KEYS)
}

fn run(conn: &Connection, migrations: &[&str], without_foreign_keys: &[usize]) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let supported = migrations.len() as i64;
    if current > supported {
        return Err(Error::SchemaTooNew {
            found: current,
            supported,
        });
    }
    // The migrations before the first one run without foreign keys commit one by one, so a
    // check that failed only at that one would leave the library at a version in between,
    // which the photon used before may not open - and the window says nothing was changed.
    // Those earlier migrations run with foreign keys on, so they can neither break a
    // reference nor (an insert of a broken one failing) mend one: what the check would find
    // then is there now, and it is looked for before anything is written. When the first
    // such migration is the next one, its own check already leaves the library as it was,
    // and a second pass over a large library is not made.
    let first = (without_foreign_keys.iter().copied())
        .filter(|&version| version as i64 > current && version <= migrations.len())
        .min();
    if let Some(first) = first.filter(|&first| first as i64 > current + 1) {
        let violations = foreign_key_violations(conn)?;
        if violations > 0 {
            return Err(Error::ForeignKeyCheck {
                version: first as i64,
                violations,
            });
        }
    }
    for (index, sql) in migrations.iter().enumerate().skip(current as usize) {
        let version = (index + 1) as i64;
        if without_foreign_keys.contains(&(index + 1)) {
            // Dropping a table that others reference with `ON DELETE CASCADE` deletes
            // their rows while foreign keys are on. The pragma is a silent no-op inside
            // a transaction, so it is switched before one begins.
            conn.pragma_update(None, "foreign_keys", false)?;
            let done = apply_checked(conn, sql, version);
            // On every path, or the connection would go on without its cascades. ON, and
            // not whatever it was before: photon's one caller is `Library::open`, after
            // `configure()` has turned them on.
            conn.pragma_update(None, "foreign_keys", true)?;
            done?;
        } else {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
    }
    Ok(())
}

/// One migration with foreign keys already off. The check is inside the transaction so a
/// failure rolls back and leaves the library at the old version, which the previous photon
/// still opens.
fn apply_checked(conn: &Connection, sql: &str, version: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(sql)?;
    let violations = foreign_key_violations(&tx)?;
    if violations > 0 {
        return Err(Error::ForeignKeyCheck {
            version,
            violations,
        });
    }
    tx.pragma_update(None, "user_version", version)?;
    tx.commit()?;
    Ok(())
}

/// How many rows reference a row that is not there.
fn foreign_key_violations(conn: &Connection) -> Result<usize> {
    Ok(conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOY: &[&str] = &[
        "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE child (parent_id INTEGER NOT NULL REFERENCES parent(id) ON DELETE CASCADE);",
        "CREATE TABLE parent_new (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL); \
         INSERT INTO parent_new (id, name) SELECT id, name FROM parent; \
         DROP TABLE parent; ALTER TABLE parent_new RENAME TO parent;",
    ];

    fn toy_connection(dir: &tempfile::TempDir) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(dir.path().join("toy.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn
    }

    fn pragma(conn: &rusqlite::Connection, name: &str) -> i64 {
        conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
            .unwrap()
    }

    fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// Dropping a table other tables reference cascades into them while foreign keys are
    /// on, so a rebuild of it must run with them off, and put them back after.
    #[test]
    fn a_rebuild_run_without_foreign_keys_keeps_the_rows_that_reference_it() {
        let dir = tempfile::tempdir().unwrap();
        let conn = toy_connection(&dir);
        run(&conn, &TOY[..1], &[]).unwrap();
        conn.execute("INSERT INTO parent (id, name) VALUES (1, 'a')", [])
            .unwrap();
        conn.execute("INSERT INTO child (parent_id) VALUES (1)", [])
            .unwrap();

        run(&conn, TOY, &[2]).unwrap();

        assert_eq!(count(&conn, "child"), 1);
        assert_eq!(pragma(&conn, "user_version"), 2);
        assert_eq!(pragma(&conn, "foreign_keys"), 1);
        conn.execute("DELETE FROM parent WHERE id = 1", []).unwrap();
        assert_eq!(count(&conn, "child"), 0);
    }

    /// The check runs inside the transaction so a library that does not hold together stays
    /// at the old version, which the previous photon still opens.
    #[test]
    fn a_failing_foreign_key_check_rolls_back_and_leaves_foreign_keys_on() {
        let dir = tempfile::tempdir().unwrap();
        let conn = toy_connection(&dir);
        run(&conn, &TOY[..1], &[]).unwrap();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute("INSERT INTO child (parent_id) VALUES (99)", [])
            .unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();

        let err = run(&conn, TOY, &[2]).unwrap_err();

        assert!(
            matches!(
                err,
                Error::ForeignKeyCheck {
                    version: 2,
                    violations: 1
                }
            ),
            "{err:?}"
        );
        assert_eq!(pragma(&conn, "user_version"), 1);
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'parent'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!sql.contains("AUTOINCREMENT"), "{sql}");
        assert_eq!(count(&conn, "child"), 1);
        assert_eq!(pragma(&conn, "foreign_keys"), 1);
    }

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
            "INSERT INTO watched_folders (id, path) VALUES (3, '/p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key) \
             VALUES (5, 3, NULL, '/p', 'p', 'p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width, \
             height, orientation, taken_at, rating) \
             VALUES (8, 5, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1, 3)",
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
        // Migration 27 rebuilds these tables; the window and Settings hold their ids, so a
        // copy that numbered the rows afresh would point each one at something else.
        for (table, id) in [("watched_folders", 3), ("folders", 5), ("items", 8)] {
            let ids: Vec<i64> = conn
                .prepare(&format!("SELECT id FROM {table}"))
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(ids, [id], "{table}");
        }
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
        assert_eq!(version, 27);
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
        assert_eq!(version, 27);
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
        assert_eq!(version, 27);
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

    /// The columns arrive empty on a library that already has photos, and the photos
    /// arrive intact. `content_hash` is set on the seeded row on purpose: it is the column
    /// the look-alike columns sit beside, and an upgrade that rebuilt the table would lose
    /// it - every duplicate in the library would have to be hashed again from disk.
    #[test]
    fn migration_12_adds_the_similarity_columns_to_an_existing_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..11] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 11i64).unwrap();
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
             height, orientation, taken_at, content_hash) \
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1, x'0102')",
            [],
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (percep, group, content): (Option<i64>, Option<i64>, Option<Vec<u8>>) = conn
            .query_row(
                "SELECT percep_hash, similar_group, content_hash FROM items WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (percep, group),
            (None, None),
            "an existing photo must come out of the upgrade ungrouped and unhashed"
        );
        assert_eq!(
            content,
            Some(vec![1u8, 2]),
            "the duplicate hash it already had was lost"
        );
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        // Hardcoded, like every other version assertion here: `MIGRATIONS.len()` would
        // agree with itself whatever the list did, which is the tripwire removed.
        assert_eq!(version, 27);
    }

    /// Every folder in an existing library comes out of the upgrade visible.
    #[test]
    fn migration_16_leaves_every_existing_folder_visible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..15] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 15i64).unwrap();
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
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let hidden: i64 = conn
            .query_row("SELECT hidden FROM folders WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            hidden, 0,
            "an existing folder came out of the upgrade hidden"
        );
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// Every album in an existing library comes out of the upgrade as photon's own.
    #[test]
    fn migration_17_keeps_existing_albums_as_photons_own() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..16] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 16i64).unwrap();
        conn.execute(
            "INSERT INTO albums (id, name, created_ms) VALUES (1, 'Trip', 5)",
            [],
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (name, token): (String, Option<String>) = conn
            .query_row(
                "SELECT name, picasa_token FROM albums WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), token), ("Trip", None));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// Every Picasa album in an existing library comes out of the upgrade with no recorded
    /// INI age, so the next INI that names it differently renames it once.
    #[test]
    fn migration_19_leaves_existing_albums_without_a_naming_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..18] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 18i64).unwrap();
        conn.execute(
            "INSERT INTO albums (id, name, created_ms, picasa_token) VALUES (1, 'Holiday', 5, 't')",
            [],
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (name, named_at): (String, Option<i64>) = conn
            .query_row(
                "SELECT name, picasa_named_at FROM albums WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), named_at), ("Holiday", None));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// Every photo in an existing library comes out of the upgrade uncaptioned, for the
    /// backfill to fill in.
    #[test]
    fn migration_18_leaves_every_existing_photo_without_a_caption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..17] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 17i64).unwrap();
        conn.execute(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key) VALUES (1, 1, NULL, '/p', 'p', 'p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        assert_eq!(lib.item_caption(1).unwrap(), None);
        let version: i64 = lib
            .reader()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// A stored Conservative or Loose keeps meaning Conservative or Loose.
    #[test]
    fn migration_15_moves_a_stored_look_alike_choice_to_its_new_distance() {
        for (before, after) in [("0", "0"), ("3", "7"), ("6", "10"), ("5", "5")] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("library.db");
            let conn = rusqlite::Connection::open(&path).unwrap();
            for sql in &MIGRATIONS[..14] {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 14i64).unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('similar_distance', ?1), ('theme', '3')",
                [before],
            )
            .unwrap();
            drop(conn);

            let lib = crate::library::Library::open(&path).unwrap();
            let conn = lib.reader().unwrap();
            let read = |key: &str| -> String {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .unwrap()
            };
            assert_eq!(read("similar_distance"), after, "stored {before}");
            assert_eq!(read("theme"), "3", "another setting was rewritten");
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, 27);
        }
    }

    /// Every photo in an existing library comes out of the upgrade visible - a default of
    /// anything else would empty the grid of everyone who upgrades.
    #[test]
    fn migration_13_leaves_every_existing_photo_visible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..12] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 12i64).unwrap();
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
             height, orientation, taken_at, rating) \
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (hidden, rating): (i64, i64) = conn
            .query_row("SELECT hidden, rating FROM items WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(
            hidden, 0,
            "an existing photo came out of the upgrade hidden"
        );
        assert_eq!(rating, 1, "the star it already had was lost");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// Every folder in an existing library comes out of the upgrade with no alias, so the
    /// sidebar keeps showing the directory names it showed before.
    #[test]
    fn migration_21_leaves_existing_folders_without_an_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..20] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 20i64).unwrap();
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
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (name, alias): (String, Option<String>) = conn
            .query_row("SELECT name, alias FROM folders WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), alias), ("p", None));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// A library at schema 21 comes out of the upgrade with the Videos index, and with
    /// `items_starred` reshaped from `(rating)` to the per-folder walk's
    /// `(folder_id, taken_at)` - a `CREATE INDEX` alone would have failed on the name, and
    /// an old-shaped index left under it would serve the count but never the view.
    #[test]
    fn migration_22_adds_the_view_indexes_and_reshapes_the_starred_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..21] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 21i64).unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        for (index, predicate) in [
            (
                "items_starred",
                "WHERE rating >= 1 AND missing_since IS NULL",
            ),
            ("items_videos", "WHERE kind = 1 AND missing_since IS NULL"),
        ] {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    [index],
                    |r| r.get(0),
                )
                .unwrap_or_else(|e| panic!("{index} missing: {e}"));
            assert!(sql.ends_with(predicate), "{index}: {sql}");
            let mut stmt = conn
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .unwrap();
            let columns: Vec<String> = stmt
                .query_map([index], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(columns, ["folder_id", "taken_at"], "{index}");
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 27);
    }

    /// A library at schema 23 with photos in it comes out with the table, and every photo
    /// unlooked-at.
    #[test]
    fn migration_24_adds_detected_faces_to_a_populated_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..23] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 23i64).unwrap();
        conn.execute_batch(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
             INSERT INTO folders (id, watched_id, path, name, sort_key) VALUES (1, 1, '/p', 'p', 1);
             INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
             VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1);",
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let (faces, version): (i64, Option<i64>) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM detected_faces), face_version FROM items",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((faces, version), (0, None));
        let user: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user, 27);
    }

    #[test]
    fn migration_25_adds_people_to_a_library_with_detected_faces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..24] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 24i64).unwrap();
        conn.execute_batch(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
             INSERT INTO folders (id, watched_id, path, name, sort_key) VALUES (1, 1, '/p', 'p', 1);
             INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1);
             INSERT INTO detected_faces (id, item_id, left, top, right, bottom, landmarks, score)
             VALUES (7, 1, 0.1, 0.2, 0.3, 0.4, zeroblob(40), 0.9);",
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        // The rebuild keeps each face's id and what migration 24 stored.
        let kept: (i64, i64, f64, f64, f64, f64, Vec<u8>, f64) = conn
            .query_row(
                "SELECT id, item_id, left, top, right, bottom, landmarks, score FROM detected_faces",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(kept, (7, 1, 0.1, 0.2, 0.3, 0.4, vec![0u8; 40], 0.9));
        // An upgraded library counts on from the highest id it had: its faces' ids are
        // never handed out again either.
        {
            let w = lib.writer();
            w.execute("DELETE FROM detected_faces WHERE id = 7", [])
                .unwrap();
            w.execute(
                "INSERT INTO detected_faces (item_id, left, top, right, bottom, landmarks, score)
                 VALUES (1, 0.1, 0.2, 0.3, 0.4, zeroblob(40), 0.9)",
                [],
            )
            .unwrap();
            assert_eq!(w.last_insert_rowid(), 8);
        }
        let indexes: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index'
                   AND tbl_name = 'detected_faces'
                   AND name IN ('detected_faces_item', 'detected_faces_person')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 2);
        let (version, person, confirmed, ignored): (Option<i64>, Option<i64>, i64, i64) = conn
            .query_row(
                "SELECT embedding_version, person_id, confirmed, ignored FROM detected_faces",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((version, person, confirmed, ignored), (None, None, 0, 0));
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table'
                   AND name IN ('people', 'person_contacts', 'face_rejections')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 3);
        let user: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user, 27);
    }

    /// The move lookup's index arriving in a populated library: the rows stay, the index is there.
    #[test]
    fn migration_26_adds_the_move_index_to_a_populated_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..25] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 25i64).unwrap();
        conn.execute_batch(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
             INSERT INTO folders (id, watched_id, path, name, sort_key) VALUES (1, 1, '/p', 'p', 1);
             INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1);",
        )
        .unwrap();
        drop(conn);

        let lib = crate::library::Library::open(&path).unwrap();
        let conn = lib.reader().unwrap();
        let user: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user, 27);
        let items: i64 = conn
            .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 1);
        let index: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'items_moved'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
    }

    /// The highest id deleted and a row inserted: the new row's id is above it. An id the
    /// window holds (a selection, a dialog's subject, a queued write, the People page's
    /// rows) must never come to name a different row.
    fn next_id_after_deleting_the_highest(
        lib: &crate::library::Library,
        insert: &str,
        table: &str,
    ) -> (i64, i64) {
        let w = lib.writer();
        w.execute(insert, []).unwrap();
        w.execute(insert, []).unwrap();
        let highest: i64 = w
            .query_row(&format!("SELECT max(id) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        w.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [highest])
            .unwrap();
        w.execute(insert, []).unwrap();
        (highest, w.last_insert_rowid())
    }

    #[test]
    fn a_deleted_persons_id_is_never_reused() {
        let (_dir, lib) = crate::testutil::temp_library();
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            "INSERT INTO people (name) VALUES (NULL)",
            "people",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_faces_id_is_never_reused() {
        let (dir, lib) = crate::testutil::temp_library();
        let (_, folder) = crate::testutil::seed_folder(&lib, dir.path());
        let item = lib
            .insert_items(&[crate::testutil::new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            &format!(
                "INSERT INTO detected_faces (item_id, left, top, right, bottom, landmarks, score)
                 VALUES ({item}, 0.1, 0.2, 0.3, 0.4, zeroblob(40), 0.9)"
            ),
            "detected_faces",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_watched_folders_id_is_never_reused() {
        let (_dir, lib) = crate::testutil::temp_library();
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            "INSERT INTO watched_folders (path) VALUES (hex(randomblob(8)))",
            "watched_folders",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_folders_id_is_never_reused() {
        let (_dir, lib) = crate::testutil::temp_library();
        let watched = crate::testutil::watch(&lib, "/p").id;
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            &format!(
                "INSERT INTO folders (watched_id, path, name, sort_key)
                 VALUES ({watched}, hex(randomblob(8)), 'f', 'f')"
            ),
            "folders",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_items_id_is_never_reused() {
        let (dir, lib) = crate::testutil::temp_library();
        let (_, folder) = crate::testutil::seed_folder(&lib, dir.path());
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            &format!(
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width,
                                    height, orientation, taken_at)
                 VALUES ({folder}, hex(randomblob(8)), 'a.jpg', 0, 1, 1, 1, 1, 1, 1)"
            ),
            "items",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_albums_id_is_never_reused() {
        let (_dir, lib) = crate::testutil::temp_library();
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            "INSERT INTO albums (name, created_ms) VALUES ('a', 1)",
            "albums",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    #[test]
    fn a_deleted_saved_searches_id_is_never_reused() {
        let (_dir, lib) = crate::testutil::temp_library();
        let (deleted, next) = next_id_after_deleting_the_highest(
            &lib,
            "INSERT INTO saved_searches (name, query, created_ms) VALUES ('s', 'q', 1)",
            "saved_searches",
        );
        assert!(next > deleted, "{next} after {deleted}");
    }

    /// The tables migration 27 rebuilds.
    const REBUILT: [&str; 5] = [
        "watched_folders",
        "folders",
        "items",
        "albums",
        "saved_searches",
    ];

    /// A library as photon 0.56 left it: schema 26, built on a plain connection, then
    /// filled by `fill`.
    fn version_26_library(dir: &tempfile::TempDir, fill: &str) -> std::path::PathBuf {
        let path = dir.path().join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..26] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 26i64).unwrap();
        conn.execute_batch(fill).unwrap();
        path
    }

    fn values(conn: &rusqlite::Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = conn.prepare(sql).unwrap();
        let columns = stmt.column_count();
        stmt.query_map([], |r| (0..columns).map(|i| r.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn is_constraint_error(result: rusqlite::Result<usize>) -> bool {
        matches!(
            result,
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation
        )
    }

    /// Every table photon keeps, in the order the snapshot reads them.
    const EVERY_TABLE: [&str; 16] = [
        "watched_folders",
        "folders",
        "items",
        "settings",
        "item_tags",
        "contacts",
        "faces",
        "albums",
        "album_items",
        "tag_rules",
        "item_user_tags",
        "saved_searches",
        "detected_faces",
        "people",
        "person_contacts",
        "face_rejections",
    ];

    /// A folder whose watched folder is not there, put in with foreign keys off - as no
    /// photon writes, but another program or a hand at the file might have left.
    const BROKEN: &str = "PRAGMA foreign_keys = OFF;
        INSERT INTO watched_folders (id, path) VALUES (1, '/p');
        INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key)
        VALUES (1, 1, NULL, '/p', 'p', 'p'), (2, 99, NULL, '/q', 'q', 'q');
        INSERT INTO albums (id, name, created_ms) VALUES (1, 'Trip', 5);";

    /// The five tables' rows, to hold a failed upgrade to.
    fn rebuilt_rows(conn: &rusqlite::Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        (REBUILT.iter())
            .map(|t| values(conn, &format!("SELECT * FROM {t} ORDER BY rowid")))
            .collect()
    }

    /// The spec's rollback, through migration 27 itself: a library whose references do not
    /// hold together is not upgraded, and is left as it was - version 26, its tables as
    /// they were, foreign keys on - so that photon 0.56 still opens it. The first pending
    /// migration is 27, so this is the check inside its transaction, not `run`'s check
    /// beforehand.
    #[test]
    fn migration_27_leaves_a_library_whose_references_are_broken_at_version_26() {
        let dir = tempfile::tempdir().unwrap();
        let path = version_26_library(&dir, BROKEN);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let before = rebuilt_rows(&conn);

        let err = migrate(&conn).unwrap_err();

        assert!(
            matches!(
                err,
                Error::ForeignKeyCheck {
                    version: 27,
                    violations: 1
                }
            ),
            "{err:?}"
        );
        assert_eq!(pragma(&conn, "user_version"), 26);
        for table in REBUILT {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(!sql.contains("AUTOINCREMENT"), "{table}: {sql}");
        }
        assert_eq!(rebuilt_rows(&conn), before);
        assert_eq!(pragma(&conn, "foreign_keys"), 1);
    }

    /// Migrations 25 and 26 commit one by one before 27 runs. Had the check been made only
    /// in 27's transaction, a library at 24 whose references do not hold together was left
    /// at 26: neither the photon used before, which knows only 24, could open it, nor this
    /// one, and the window said nothing was changed. It is looked for before anything runs.
    #[test]
    fn a_broken_reference_leaves_an_older_library_at_the_version_it_had() {
        let dir = tempfile::tempdir().unwrap();
        let conn = rusqlite::Connection::open(dir.path().join("library.db")).unwrap();
        for sql in &MIGRATIONS[..24] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 24i64).unwrap();
        conn.execute_batch(BROKEN).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let before = rebuilt_rows(&conn);

        let err = migrate(&conn).unwrap_err();

        assert!(
            matches!(
                err,
                Error::ForeignKeyCheck {
                    version: 27,
                    violations: 1
                }
            ),
            "{err:?}"
        );
        assert_eq!(pragma(&conn, "user_version"), 24);
        assert_eq!(rebuilt_rows(&conn), before);
        assert_eq!(pragma(&conn, "foreign_keys"), 1);
    }

    /// The rebuild drops tables that others reference with `ON DELETE CASCADE`. Run with
    /// foreign keys on, as every photon connection is, the drop of `watched_folders` alone
    /// would take every folder and photo with it, and their albums, keywords and faces, and
    /// the upgrade would still report success.
    #[test]
    fn migration_27_keeps_every_row_and_reference() {
        let dir = tempfile::tempdir().unwrap();
        let path = version_26_library(
            &dir,
            "INSERT INTO watched_folders (id, path, online) VALUES (1, '/p', 1), (2, '/q', 0);
             INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key, hidden, alias)
             VALUES (1, 1, NULL, '/p', 'p', 'p', 0, NULL),
                    (2, 1, 1, '/p/c', 'c', 'p/c', 1, 'Child'),
                    (3, 2, NULL, '/q', 'q', 'q', 0, NULL);
             INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width,
                                height, orientation, taken_at, rating, content_hash, caption,
                                gps_lat, gps_lon, edit_turns, percep_hash, similar_group)
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 10, 1, 4, 3, 1, 100, 3, x'0102', 'cap',
                     1.5, 2.5, 1, 42, 1),
                    (2, 2, '/p/c/b.jpg', 'b.jpg', 0, 20, 2, 4, 3, 6, 200, NULL, NULL, NULL,
                     NULL, NULL, 0, NULL, NULL),
                    (7, 3, '/q/v.mp4', 'v.mp4', 1, 30, 3, 4, 3, 1, 300, 0, NULL, NULL,
                     NULL, NULL, 0, NULL, NULL),
                    (9, 3, '/q/gone.jpg', 'gone.jpg', 0, 40, 4, 4, 3, 1, 400, NULL, NULL,
                     NULL, NULL, NULL, 0, NULL, NULL);
             DELETE FROM items WHERE id = 9;
             INSERT INTO albums (id, name, created_ms, picasa_token, picasa_named_at)
             VALUES (1, 'Trip', 5, 'tok', 9), (2, 'Mine', 6, NULL, NULL);
             INSERT INTO album_items (album_id, item_id, added_ms) VALUES (1, 1, 11), (2, 7, 12);
             INSERT INTO item_tags (item_id, tag) VALUES (1, 'beach');
             INSERT INTO item_user_tags (item_id, tag, added) VALUES (2, 'mine', 1);
             INSERT INTO tag_rules (tag, target) VALUES ('beach', 'seaside');
             INSERT INTO contacts (hash, name) VALUES ('abc', 'Ann');
             INSERT INTO faces (item_id, contact, left, top, right, bottom)
             VALUES (1, 'abc', 0.1, 0.2, 0.3, 0.4);
             INSERT INTO people (id, name, ignored) VALUES (1, 'Ann', 0), (2, NULL, 0);
             INSERT INTO person_contacts (contact, person_id) VALUES ('abc', 1);
             INSERT INTO detected_faces (id, item_id, left, top, right, bottom, landmarks,
                                         score, person_id, confirmed)
             VALUES (5, 2, 0.1, 0.2, 0.3, 0.4, zeroblob(40), 0.9, 1, 1);
             INSERT INTO face_rejections (face_id, person_id) VALUES (5, 2);
             INSERT INTO saved_searches (id, name, query, created_ms)
             VALUES (1, 'Canon', 'camera:canon', 7);
             INSERT INTO settings (key, value) VALUES ('last_folder', '2');",
        );
        let snapshot = |conn: &rusqlite::Connection| -> Vec<_> {
            EVERY_TABLE
                .iter()
                .map(|t| values(conn, &format!("SELECT * FROM {t} ORDER BY rowid")))
                .collect()
        };
        let before = snapshot(&rusqlite::Connection::open(&path).unwrap());
        assert!(
            before.iter().all(|rows| !rows.is_empty()),
            "every table holds a row for the upgrade to lose"
        );

        let lib = crate::library::Library::open(&path).unwrap();

        let conn = lib.reader().unwrap();
        for ((table, before), after) in EVERY_TABLE.iter().zip(&before).zip(snapshot(&conn)) {
            assert_eq!(*before, after, "{table}");
        }
        assert_eq!(
            values(&conn, "PRAGMA foreign_key_check"),
            Vec::<Vec<_>>::new()
        );
        assert_eq!(pragma(&conn, "user_version"), 27);
        drop(conn);
        let w = lib.writer();
        w.execute(
            "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width,
                                height, orientation, taken_at)
             VALUES (1, '/p/new.jpg', 'new.jpg', 0, 1, 1, 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        assert!(w.last_insert_rowid() > 7, "{}", w.last_insert_rowid());
        // The cascades survive the rebuild: the writer has foreign keys on, and the new
        // tables carry the old `REFERENCES … ON DELETE CASCADE`.
        w.execute("DELETE FROM watched_folders WHERE id = 1", [])
            .unwrap();
        assert_eq!(count(&w, "folders WHERE watched_id = 1"), 0);
        assert_eq!(count(&w, "items WHERE folder_id IN (1, 2)"), 0);
        assert_eq!(count(&w, "album_items WHERE item_id = 1"), 0);
        assert_eq!(count(&w, "item_tags"), 0);
        assert_eq!(
            count(&w, "items"),
            1,
            "the other watched folder's photo stays"
        );
    }

    /// Without `ANALYZE` SQLite picks among partial indexes that tie by creation order, so
    /// the rebuilt tables' indexes must come back in the order a 0.56 library had them, or a
    /// sidebar count can quietly move to a worse plan.
    #[test]
    fn migration_27_keeps_the_tables_shape_and_their_indexes_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = version_26_library(&dir, "");
        let shape = |conn: &rusqlite::Connection| -> Vec<_> {
            REBUILT
                .iter()
                .map(|t| {
                    (
                        values(conn, &format!("PRAGMA table_info({t})")),
                        values(conn, &format!("PRAGMA foreign_key_list({t})")),
                        values(
                            conn,
                            &format!(
                                "SELECT name, sql FROM sqlite_master \
                                 WHERE type = 'index' AND tbl_name = '{t}' ORDER BY rowid"
                            ),
                        ),
                    )
                })
                .collect()
        };
        let before = shape(&rusqlite::Connection::open(&path).unwrap());

        let lib = crate::library::Library::open(&path).unwrap();

        let conn = lib.reader().unwrap();
        for ((table, before), after) in REBUILT.iter().zip(&before).zip(shape(&conn)) {
            assert_eq!(before.0, after.0, "{table}: columns");
            assert_eq!(before.1, after.1, "{table}: foreign keys");
            assert_eq!(before.2, after.2, "{table}: indexes");
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(sql.contains("AUTOINCREMENT"), "{sql}");
        }
    }

    /// The paths' `UNIQUE` and the albums' unique token are what the scanner's upserts
    /// and the Picasa album pass rely on; the rebuild must keep both.
    #[test]
    fn migration_27_keeps_the_unique_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = version_26_library(
            &dir,
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
             INSERT INTO folders (id, watched_id, path, name, sort_key) VALUES (1, 1, '/p', 'p', 'p');
             INSERT INTO items (id, folder_id, path, file_name, kind, size, mtime_ms, width,
                                height, orientation, taken_at)
             VALUES (1, 1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1);
             INSERT INTO albums (id, name, created_ms, picasa_token) VALUES (1, 'a', 1, 'tok');",
        );

        let lib = crate::library::Library::open(&path).unwrap();

        let w = lib.writer();
        for (what, sql) in [
            (
                "watched folder",
                "INSERT INTO watched_folders (path) VALUES ('/p')",
            ),
            (
                "folder",
                "INSERT INTO folders (watched_id, path, name, sort_key) VALUES (1, '/p', 'p', 'p')",
            ),
            (
                "item",
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width,
                                    height, orientation, taken_at)
                 VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1)",
            ),
            (
                "album",
                "INSERT INTO albums (name, created_ms, picasa_token) VALUES ('b', 2, 'tok')",
            ),
        ] {
            assert!(is_constraint_error(w.execute(sql, [])), "a second {what}");
        }
    }

    /// `settings.last_folder` is the one id photon stores across a restart. A remembered
    /// folder that was removed before the upgrade left no trace AUTOINCREMENT can see, so
    /// without the migration raising the sequence a new folder could take its number and
    /// the next launch would open on it. Also checked with every folder gone: the migration
    /// relies on SQLite writing a `folders` sequence row even for an empty copy, which is not
    /// documented, and this case is what would catch it stopping.
    #[test]
    fn migration_27_never_hands_out_the_remembered_folders_id() {
        for removed in ["id = 2", "id IN (1, 2)"] {
            let dir = tempfile::tempdir().unwrap();
            let path = version_26_library(
                &dir,
                &format!(
                    "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
                     INSERT INTO folders (id, watched_id, path, name, sort_key)
                     VALUES (1, 1, '/p', 'p', 'p'), (2, 1, '/p/c', 'c', 'c');
                     INSERT INTO settings (key, value) VALUES ('last_folder', '2');
                     DELETE FROM folders WHERE {removed};"
                ),
            );

            let lib = crate::library::Library::open(&path).unwrap();

            let w = lib.writer();
            w.execute(
                "INSERT INTO folders (watched_id, path, name, sort_key) VALUES (1, '/p/n', 'n', 'n')",
                [],
            )
            .unwrap();
            assert!(
                w.last_insert_rowid() > 2,
                "{removed}: {}",
                w.last_insert_rowid()
            );
        }
    }

    /// The setting is text, and only a number above every folder may move the sequence: not
    /// one with letters after its digits (CAST would read "5abc" as 5), and not one too long
    /// for an id (CAST clamps it to the largest integer, which would leave no id to give).
    #[test]
    fn migration_27_ignores_a_last_folder_that_is_not_above_every_folder() {
        for stored in ["garbage", "-5", "1", "5abc", "18446744073709551616"] {
            let dir = tempfile::tempdir().unwrap();
            let path = version_26_library(
                &dir,
                &format!(
                    "INSERT INTO watched_folders (id, path) VALUES (1, '/p');
                     INSERT INTO folders (id, watched_id, path, name, sort_key)
                     VALUES (1, 1, '/p', 'p', 'p'), (2, 1, '/p/b', 'b', 'b'),
                            (3, 1, '/p/c', 'c', 'c');
                     INSERT INTO settings (key, value) VALUES ('last_folder', '{stored}');"
                ),
            );

            let lib = crate::library::Library::open(&path)
                .unwrap_or_else(|e| panic!("last_folder {stored:?}: {e}"));

            let w = lib.writer();
            w.execute(
                "INSERT INTO folders (watched_id, path, name, sort_key) VALUES (1, '/p/n', 'n', 'n')",
                [],
            )
            .unwrap();
            assert_eq!(w.last_insert_rowid(), 4, "last_folder {stored:?}");
        }
    }
}
