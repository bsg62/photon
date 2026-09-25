# Picasa Albums Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the albums Picasa 3 recorded in each folder's `.picasa.ini` in photon's Albums list, read-only, mirrored on every scan.

**Architecture:** The INI parser gains album definitions and per-photo memberships. A Picasa album is an ordinary `albums` row carrying a `picasa_token` (schema 17), and its members are ordinary `album_items` rows written only by the scan's existing post-walk Picasa pass, so every reader of albums (the Album view, counts, the viewer) serves them unchanged. photon's four album writers refuse a token-bearing album; the UI hides those actions.

**Tech Stack:** Rust (rusqlite, photon-core), Tauri 2 (photon-app), Svelte 5 + TypeScript (ui/), vitest.

**Spec:** `docs/superpowers/specs/2026-09-24-photon-picasa-albums-design.md` - read it before starting; this plan argues from it.

## Global Constraints

- Branch `feat/picasa-albums`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **photon never writes an INI's album sections.** No task adds a writer; `set_stars` must keep preserving every byte it does not own.
- No new dependency, Rust or npm. No native library.
- **A new test must be demonstrated to fail with its change reverted**, by an exact replacement (not a loose `sed`), and the probe recorded in the task's commit message. A compile error is not proof. The two tests the plan marks **pin** protect existing behaviour and are said to pass before the change, on purpose.
- The TypeScript mirror (`ui/src/lib/api.ts`) changes in the same commit as the Rust struct it mirrors; `npm run check` typechecks test files too.
- A schema bump updates the hardcoded version literals rather than loosening them to `MIGRATIONS.len()`.
- Every user-visible count keeps `hidden = 0`; case-insensitive matching is done in Rust, never with SQL `lower()` or `COLLATE NOCASE`.
- The Rust gate before every commit: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. The UI gate on commits touching `ui/`: `npm run check` (0 errors, 0 warnings) and `npm test`.
- Never launch the GUI to verify. Anything needing eyes goes on the README's smoke checklist.

## Review Focus

1. **A rescan of an unchanged INI** must count `realbumed == 0` - otherwise every scan rebuilds the grid forever. Pinned in Task 3 (`upserting_an_unchanged_definition_counts_nothing_and_a_rename_counts_one`) and Task 4 (`a_picasa_album_follows_the_ini_without_the_photo_changing`).
2. **A Picasa album that loses its last photo while it is open** leaves the list; the grid must not then offer "Remove from" or "Right-click a photo to add it" for it. Pinned in Task 6 (`isOwnAlbum` is false for an absent id).
3. **A Picasa album and a photon album with the same name** - "Add to album" and the viewer's checkboxes must still target only photon's. Pinned in Task 6 (`ownAlbums` same-name case).
4. **Token case differing between the header and `albums=`** (`[.album:9C0E]` vs `albums=9c0e`) must match. Pinned in Task 1.
5. **A Picasa album whose only member is hidden** has a live count of 0 and leaves the sidebar, as the spec's "no photos left" rule says, while an empty photon album stays. Pinned in Task 2 (`albums_with_counts_leaves_out_an_empty_picasa_album_only`).

---

### Task 1: Parse Picasa albums from the INI

**Files:**
- Modify: `crates/photon-core/src/picasa.rs` (`FolderIni`, `parse_folder`, new const)
- Test: `crates/photon-core/src/picasa.rs` (`mod tests`)

**Interfaces:**
- Produces: `FolderIni.albums: HashMap<String, String>` (lowercased token → name) and `FolderIni.item_albums: HashMap<String, Vec<String>>` (lowercased file name → lowercased tokens, deduplicated, in INI order). Consumed by Task 4.

- [ ] **Step 1: Write the failing tests** - add to `mod tests` in `picasa.rs`:

```rust
    #[test]
    fn reads_picasa_albums_and_each_photos_membership() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[.album:9C0E]\nname=Holiday 2009\ntoken=9c0e\ndate=2009-08-14T10:22:31+02:00\n\
              [.album:1f2e]\nname=Wedding\n\
              [IMG_0412.JPG]\nalbums= 9c0e , 1F2E,,9C0E\nstar=yes\n\
              [b.jpg]\nalbums=\n",
        );
        let ini = read_folder(dir.path()).unwrap();
        assert_eq!(
            ini.albums,
            HashMap::from([
                ("9c0e".to_string(), "Holiday 2009".to_string()),
                ("1f2e".to_string(), "Wedding".to_string()),
            ]),
            "tokens are lowercased like every section name; the name keeps its case"
        );
        assert_eq!(
            ini.item_albums,
            HashMap::from([(
                "img_0412.jpg".to_string(),
                vec!["9c0e".to_string(), "1f2e".to_string()]
            )]),
            "trimmed, lowercased, empties and repeats dropped; an empty albums= is no membership"
        );
        assert_eq!(stars(dir.path()), vec!["img_0412.jpg"], "the photo's star still reads");
    }

    #[test]
    fn an_album_section_is_never_a_photo() {
        // `[.album:t]` used to be read as a photo named `.album:t`. Its keys are the album's:
        // none of them may become a star, a hidden flag or a face. An `.album` section with no
        // `name=`, or with an empty token, defines nothing - recorded under its token it would
        // rename an album another folder named.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[.album:t]\nstar=yes\nhidden=yes\nfaces=rect64(4000200080006000),abc\n\
              [.album:]\nname=No token\n\
              [.album:u]\ntoken=u\n",
        );
        let ini = read_folder(dir.path()).unwrap();
        assert!(ini.stars.is_empty(), "{:?}", ini.stars);
        assert!(ini.hidden.is_empty(), "{:?}", ini.hidden);
        assert!(ini.faces.is_empty(), "{:?}", ini.faces);
        assert!(ini.albums.is_empty(), "{:?}", ini.albums);
    }

    #[test]
    fn starring_leaves_album_sections_byte_for_byte() {
        // A pin, not a probe: `rewrite` already keeps every line it does not own, so this
        // passes before the feature by design. It is here so a writer that later learns about
        // albums cannot start rewriting them - photon never writes an album.
        let dir = tempfile::tempdir().unwrap();
        let before: &[u8] = b"[.album:9c0e]\r\nname=Holiday\r\ntoken=9c0e\r\n[a.jpg]\r\nalbums=9c0e\r\n";
        write_file(dir.path(), ".picasa.ini", before);
        set_star(dir.path(), "b.jpg", true).unwrap();
        let mut expected = before.to_vec();
        expected.extend_from_slice(b"[b.jpg]\r\nstar=yes\r\n");
        assert_eq!(ini(dir.path(), ".picasa.ini"), expected);
    }
```

`HashMap` is already imported by `use super::*;` (the module imports `std::collections::{HashMap, HashSet}`). `stars`, `ini` and `write_file` are existing test helpers in this module.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib picasa::tests`
Expected: compile error (no `albums` field). That is not the proof - Step 5 is.

- [ ] **Step 3: Implement** - in `picasa.rs`:

Add to `FolderIni`, after `contacts`:

```rust
    /// Picasa album token to name, from each `[.album:<token>]` section's `name=`. Tokens are
    /// lowercased like every section name. An album section with no name defines nothing
    /// here; its token still becomes an album once a photo names it (`item_albums`).
    pub albums: HashMap<String, String>,
    /// The Picasa album tokens per lowercased file name, from `albums=`, lowercased,
    /// deduplicated, in the INI's order.
    pub item_albums: HashMap<String, Vec<String>>,
```

Add beside `CONTACTS_SECTION`:

```rust
/// The prefix of a Picasa album's section, `[.album:<token>]`. Compared lowercased, like every
/// header. The section is the album's own, never a photo's.
const ALBUM_SECTION_PREFIX: &str = ".album:";
```

In `parse_folder`, in the `Line::Key` arm, directly after `let Some(name) = &section else { continue; };`, insert:

```rust
                if let Some(token) = name.strip_prefix(ALBUM_SECTION_PREFIX) {
                    // An album's own keys: none of them is a photo's star, flag or face.
                    let token = token.trim();
                    if !token.is_empty() && key.eq_ignore_ascii_case("name") && !value.is_empty() {
                        ini.albums.insert(token.to_string(), value.to_string());
                    }
                    continue;
                }
```

and add a branch to the photo-key chain, after the `faces` branch:

```rust
                } else if key.eq_ignore_ascii_case("albums") {
                    let tokens = value
                        .split(',')
                        .map(|t| t.trim().to_lowercase())
                        .filter(|t| !t.is_empty());
                    for token in tokens {
                        let list = ini.item_albums.entry(name.clone()).or_default();
                        if !list.contains(&token) {
                            list.push(token);
                        }
                    }
```

Update the module doc's first line to mention albums: `//! Reads Picasa's per-directory stars, hidden flags, faces, contacts and albums, and writes star flags.`

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib picasa`
Expected: all pass.

- [ ] **Step 5: Probe**
  - Delete the `if let Some(token) = name.strip_prefix(ALBUM_SECTION_PREFIX) { ... continue; }` block (restore from a saved copy afterwards): `an_album_section_is_never_a_photo` and `reads_picasa_albums_and_each_photos_membership` must fail.
  - Replace `.map(|t| t.trim().to_lowercase())` with `.map(|t| t.trim().to_string())`: `reads_picasa_albums_and_each_photos_membership` must fail (Review Focus 4).
  - Replace `if !list.contains(&token) {` with `if true {`: the same test must fail.
  - Restore; all pass.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/picasa.rs
git commit -m "feat(picasa): read albums from the INI" -m "<which probe failed which test; the byte-for-byte test is a pin that passes before the change>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Schema 17, the guard, and `AlbumSummary.picasa`

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (append migration 17; version literals; new migration test)
- Modify: `crates/photon-core/src/library/mod.rs:152,178` (version literals)
- Modify: `crates/photon-core/src/error.rs` (new variant)
- Modify: `crates/photon-core/src/library/albums.rs` (guard, `AlbumSummary`, `albums_with_counts`, tests)
- Modify: `ui/src/lib/api.ts:146`, `ui/src/lib/library.test.ts:140,150` (TS mirror)

**Interfaces:**
- Produces: column `albums.picasa_token TEXT` with unique index `albums_picasa_token`; `Error::PicasaAlbum(i64)`; `AlbumSummary { id, name, count, picasa: bool }` (TS: `picasa: boolean`). Tasks 3-6 rely on all three.

- [ ] **Step 1: Write the failing tests**

In `schema.rs` `mod tests`, next to `migration_16_leaves_every_existing_folder_visible`:

```rust
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
        assert_eq!(version, 17);
    }
```

In `albums.rs` `mod tests`, a helper and five tests:

```rust
    /// A Picasa album as the scan would leave it, before the scan's own writer exists.
    fn picasa_album(lib: &Library, token: &str, name: &str) -> i64 {
        let conn = lib.writer();
        conn.execute(
            "INSERT INTO albums (name, created_ms, picasa_token) VALUES (?1, 0, ?2)",
            params![name, token],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn a_picasa_album_cannot_be_renamed() {
        let (_dir, lib) = temp_library();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(lib.rename_album(id, "Mine"), Err(Error::PicasaAlbum(_))));
        assert_eq!(lib.album(id).unwrap().unwrap().name, "Holiday");
        let own = lib.create_album("Trip", 1).unwrap();
        lib.rename_album(own.id, "Zurich").unwrap();
    }

    #[test]
    fn a_picasa_album_cannot_be_deleted() {
        let (_dir, lib) = temp_library();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(lib.delete_album(id), Err(Error::PicasaAlbum(_))));
        assert!(lib.album(id).unwrap().is_some());
        let own = lib.create_album("Trip", 1).unwrap();
        lib.delete_album(own.id).unwrap();
    }

    #[test]
    fn nothing_can_be_added_to_a_picasa_album() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(lib.add_to_album(id, &ids, 5), Err(Error::PicasaAlbum(_))));
        assert!(lib.item_albums(ids[0]).unwrap().is_empty());
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 5).unwrap();
    }

    #[test]
    fn nothing_can_be_removed_from_a_picasa_album() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        let id = picasa_album(&lib, "t", "Holiday");
        lib.writer()
            .execute(
                "INSERT INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, 0)",
                params![id, ids[0]],
            )
            .unwrap();
        assert!(matches!(lib.remove_from_album(id, &ids), Err(Error::PicasaAlbum(_))));
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![id]);
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 5).unwrap();
        lib.remove_from_album(own.id, &ids).unwrap();
    }

    #[test]
    fn albums_with_counts_leaves_out_an_empty_picasa_album_only() {
        // An empty Picasa album has lost every photo in this library and leaves the list; an
        // empty photon album is one the user just made and stays. A Picasa album whose only
        // member is hidden counts 0 like every hidden-aware count, and leaves too.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/h.jpg", 2),
            ])
            .unwrap();
        let (shown, hidden) = (ids[0], ids[1]);
        lib.set_hidden(&[hidden], true).unwrap();
        let empty_picasa = picasa_album(&lib, "e", "Gone");
        let hidden_only = picasa_album(&lib, "h", "Only hidden");
        let listed = picasa_album(&lib, "l", "Holiday");
        let own = lib.create_album("Trip", 1).unwrap();
        for (album, item) in [(listed, shown), (hidden_only, hidden)] {
            lib.writer()
                .execute(
                    "INSERT INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, 0)",
                    params![album, item],
                )
                .unwrap();
        }

        let rows: Vec<(i64, bool, i64)> = lib
            .albums_with_counts()
            .unwrap()
            .into_iter()
            .map(|a| (a.id, a.picasa, a.count))
            .collect();
        assert_eq!(rows, vec![(listed, true, 1), (own.id, false, 0)]);
        assert!(lib.album(empty_picasa).unwrap().is_some(), "left out, not deleted");
    }
```

Add `picasa: false` to both `AlbumSummary` literals in `albums_are_created_renamed_listed_and_deleted` (they are the only hand-built `AlbumSummary` values in the workspace).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib albums migration_17`
Expected: compile errors (`PicasaAlbum`, `picasa`), then assertion failures once Step 3's types exist but before the guard lines do.

- [ ] **Step 3: Implement**

`schema.rs`, append to `MIGRATIONS`:

```rust
    r#"
-- Picasa's albums (`[.album:<token>]` in a folder's INI) are rows here too, so every reader
-- of albums serves them unchanged. The token is what the scan matches an album by; NULL marks
-- one of photon's own, and a unique index admits any number of NULLs.
ALTER TABLE albums ADD COLUMN picasa_token TEXT;
CREATE UNIQUE INDEX albums_picasa_token ON albums(picasa_token);
"#,
```

Version literals, exactly:

```bash
sed -i 's/assert_eq!(version, 16);/assert_eq!(version, 17);/' crates/photon-core/src/library/schema.rs crates/photon-core/src/library/mod.rs
sed -i 's/supported: 16$/supported: 17/' crates/photon-core/src/library/mod.rs
git diff --stat   # expect schema.rs and mod.rs only; grep 'version, 16' must now find nothing
```

`error.rs`, after `EmptyAlbumName`:

```rust
    #[error("this album comes from Picasa; change it in Picasa")]
    PicasaAlbum(i64),
```

`albums.rs`:

`AlbumSummary` gains, after `count`:

```rust
    /// Mirrored from Picasa's INI by the scan: listed and viewable, never edited here.
    pub picasa: bool,
```

A guard in `impl Library`:

```rust
    /// Refuses anything but one of photon's own albums: `NotFound` for no album,
    /// `PicasaAlbum` for one the scan mirrors from Picasa. Every photon write to an album
    /// asks this first; the UI hides those actions on a Picasa album, and this is what makes
    /// that true over IPC. The scan writes a Picasa album's members through
    /// `set_picasa_album_items`, which never comes here.
    fn own_album(&self, id: i64) -> Result<()> {
        let token: Option<Option<String>> = self
            .reader()?
            .query_row(
                "SELECT picasa_token FROM albums WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        match token {
            None => Err(Error::NotFound(id)),
            Some(Some(_)) => Err(Error::PicasaAlbum(id)),
            Some(None) => Ok(()),
        }
    }
```

Call it:
- `rename_album`: `self.own_album(id)?;` directly after `let name = valid_name(name)?;`.
- `delete_album`: `self.own_album(id)?;` as the first line.
- `add_to_album`: replace `if self.album(album_id)?.is_none() { return Err(Error::NotFound(album_id)); }` with `self.own_album(album_id)?;`.
- `remove_from_album`: `self.own_album(album_id)?;` as the first line. This also turns removing from a nonexistent album into `NotFound`; run the app crate's tests to confirm nothing relied on the old `Ok`.

`albums_with_counts`: select the flag and filter:

```rust
            "SELECT a.id, a.name, a.picasa_token IS NOT NULL,
                    (SELECT count(*) FROM album_items m JOIN items i ON i.id = m.item_id
                     WHERE m.album_id = a.id AND i.missing_since IS NULL AND i.hidden = 0)
             FROM albums a",
```

map `picasa: r.get(2)?, count: r.get(3)?`, and before the sort:

```rust
        // A Picasa album with no live photo has left this library; its row stays, so a
        // folder that comes back finds the same album. An empty photon album is one the user
        // just made, and stays listed.
        albums.retain(|a| !a.picasa || a.count > 0);
```

Update the doc comment above `albums_with_counts` to say so.

TS mirror, `ui/src/lib/api.ts:146`:

```ts
/** `picasa`: mirrored from Picasa's INI by the scan - listed and viewable, never edited. */
export interface AlbumSummary { id: number; name: string; count: number; picasa: boolean }
```

`ui/src/lib/library.test.ts:140,150`: both literals become `{ id: 1, name: 'Trip', count: 2, picasa: false }`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace && npm run check && npm test`
Expected: all pass.

- [ ] **Step 5: Probe** - each an exact line removal, restored after:
  - Remove `self.own_album(id)?;` from `rename_album`: `a_picasa_album_cannot_be_renamed` fails.
  - Remove it from `delete_album`: `a_picasa_album_cannot_be_deleted` fails.
  - In `add_to_album`, put back the old `if self.album(album_id)?.is_none() { return Err(Error::NotFound(album_id)); }`: `nothing_can_be_added_to_a_picasa_album` fails.
  - Remove it from `remove_from_album`: `nothing_can_be_removed_from_a_picasa_album` fails.
  - Replace `albums.retain(|a| !a.picasa || a.count > 0);` with `albums.retain(|a| a.count > 0);`: `albums_with_counts_leaves_out_an_empty_picasa_album_only` fails (the empty photon album disappears).
  - Delete the `retain` line: the same test fails (the empty and hidden-only Picasa albums appear).
  - Drop the new migration entry: `migration_17_keeps_existing_albums_as_photons_own` fails.

- [ ] **Step 6: Gate and commit** (Rust gate + UI gate)

```bash
git add crates/photon-core/src/library/schema.rs crates/photon-core/src/library/mod.rs crates/photon-core/src/error.rs crates/photon-core/src/library/albums.rs ui/src/lib/api.ts ui/src/lib/library.test.ts
git commit -m "feat(albums): schema 17 marks Picasa albums, and photon's writers refuse them" -m "<probes>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The library's Picasa-album writers

**Files:**
- Modify: `crates/photon-core/src/library/albums.rs`

**Interfaces:**
- Consumes: `albums.picasa_token` (Task 2).
- Produces, on `Library`:
  - `pub fn upsert_picasa_albums(&self, defined: &HashMap<String, String>, referenced: &HashSet<String>, now_ms: i64) -> Result<(HashMap<String, i64>, u64)>` - token → album id for every defined and referenced token, and the number of albums inserted or renamed.
  - `pub fn folder_picasa_albums(&self, folder_id: i64) -> Result<HashMap<i64, BTreeSet<i64>>>` - item id → Picasa album ids, live photos of one folder, photon albums excluded.
  - `pub fn set_picasa_album_items(&self, items: &[(i64, BTreeSet<i64>)], now_ms: i64) -> Result<()>` - replaces each item's Picasa memberships; never touches photon albums.

- [ ] **Step 1: Write the failing tests** - in `albums.rs` `mod tests` (add `use std::collections::{BTreeSet, HashMap, HashSet};` to the test module):

```rust
    #[test]
    fn a_referenced_token_never_replaces_a_real_name_in_either_order() {
        let named = HashMap::from([("t".to_string(), "Holiday".to_string())]);
        let only_t = HashSet::from(["t".to_string()]);

        // The definition first, then a folder whose photo only names the token.
        let (_dir, lib) = temp_library();
        lib.upsert_picasa_albums(&named, &HashSet::new(), 1).unwrap();
        let (ids, changed) = lib.upsert_picasa_albums(&HashMap::new(), &only_t, 2).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Holiday");

        // The reference first: listed under its token until a definition names it.
        let (_dir2, lib2) = temp_library();
        let (first, changed) = lib2.upsert_picasa_albums(&HashMap::new(), &only_t, 1).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(lib2.album(first["t"]).unwrap().unwrap().name, "t");
        let (second, changed) = lib2.upsert_picasa_albums(&named, &only_t, 2).unwrap();
        assert_eq!(second["t"], first["t"], "the same album, not a second one");
        assert_eq!(changed, 1);
        assert_eq!(lib2.album(first["t"]).unwrap().unwrap().name, "Holiday");
    }

    #[test]
    fn upserting_an_unchanged_definition_counts_nothing_and_a_rename_counts_one() {
        // Counting a no-op would make every scan of a Picasa library rebuild the grid.
        let (_dir, lib) = temp_library();
        let holiday = HashMap::from([("t".to_string(), "Holiday".to_string())]);
        assert_eq!(lib.upsert_picasa_albums(&holiday, &HashSet::new(), 1).unwrap().1, 1);
        assert_eq!(lib.upsert_picasa_albums(&holiday, &HashSet::new(), 2).unwrap().1, 0);
        let summer = HashMap::from([("t".to_string(), "Summer".to_string())]);
        let (ids, changed) = lib.upsert_picasa_albums(&summer, &HashSet::new(), 3).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Summer");
    }

    #[test]
    fn the_scans_membership_writer_never_touches_photons_albums() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 1).unwrap();
        let (picasa, _) = lib
            .upsert_picasa_albums(
                &HashMap::from([("t".to_string(), "Holiday".to_string())]),
                &HashSet::new(),
                1,
            )
            .unwrap();

        lib.set_picasa_album_items(&[(ids[0], BTreeSet::from([picasa["t"]]))], 2)
            .unwrap();
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![own.id, picasa["t"]]);
        assert_eq!(
            lib.folder_picasa_albums(folder).unwrap(),
            HashMap::from([(ids[0], BTreeSet::from([picasa["t"]]))]),
            "photon's album is not reported to the pass"
        );

        lib.set_picasa_album_items(&[(ids[0], BTreeSet::new())], 3).unwrap();
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![own.id]);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-core --lib albums`
Expected: compile errors for the three new methods.

- [ ] **Step 3: Implement** - in `albums.rs` (`use std::collections::{BTreeSet, HashMap, HashSet};` at the top), in `impl Library`, under a `// ---- Picasa albums, written only by the scan ----` comment:

```rust
    /// Inserts the albums one INI defines and the tokens its photos name, and returns every
    /// one's id by token, with how many albums it inserted or renamed.
    ///
    /// A defined album takes the INI's name, so a rename in Picasa is followed. A token that
    /// is only referenced is inserted under the token itself and never renames anything:
    /// Picasa repeats an album's definition in every folder with a member, so whichever
    /// folder is scanned first the name ends up the one the definitions give. A rescan of an
    /// unchanged INI writes nothing and counts 0 - counting it would rebuild the grid after
    /// every scan of a Picasa library.
    pub fn upsert_picasa_albums(
        &self,
        defined: &HashMap<String, String>,
        referenced: &HashSet<String>,
        now_ms: i64,
    ) -> Result<(HashMap<String, i64>, u64)> {
        let mut ids = HashMap::new();
        if defined.is_empty() && referenced.is_empty() {
            return Ok((ids, 0));
        }
        let mut changed = 0u64;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut define = tx.prepare_cached(
                "INSERT INTO albums (name, created_ms, picasa_token) VALUES (?1, ?2, ?3)
                 ON CONFLICT(picasa_token) DO UPDATE SET name = excluded.name
                 WHERE name != excluded.name",
            )?;
            let mut refer = tx.prepare_cached(
                "INSERT INTO albums (name, created_ms, picasa_token) VALUES (?1, ?2, ?1)
                 ON CONFLICT(picasa_token) DO NOTHING",
            )?;
            let mut id_of = tx.prepare_cached("SELECT id FROM albums WHERE picasa_token = ?1")?;
            for (token, name) in defined {
                changed += define.execute(params![name, now_ms, token])? as u64;
            }
            for token in referenced.iter().filter(|t| !defined.contains_key(*t)) {
                changed += refer.execute(params![token, now_ms])? as u64;
            }
            for token in defined.keys().chain(referenced) {
                if !ids.contains_key(token) {
                    let id: i64 = id_of.query_row(params![token], |r| r.get(0))?;
                    ids.insert(token.clone(), id);
                }
            }
        }
        tx.commit()?;
        Ok((ids, changed))
    }

    /// The Picasa-album memberships of every live photo in one folder, by item id, for the
    /// Picasa pass to diff against what the INI says now. photon's own albums are left out:
    /// the pass never touches them.
    pub fn folder_picasa_albums(&self, folder_id: i64) -> Result<HashMap<i64, BTreeSet<i64>>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT m.item_id, m.album_id
             FROM album_items m
             JOIN items i ON i.id = m.item_id
             JOIN albums a ON a.id = m.album_id
             WHERE i.folder_id = ?1 AND i.missing_since IS NULL AND a.picasa_token IS NOT NULL",
        )?;
        let mut memberships: HashMap<i64, BTreeSet<i64>> = HashMap::new();
        for row in stmt.query_map(params![folder_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (item_id, album_id) = row?;
            memberships.entry(item_id).or_default().insert(album_id);
        }
        Ok(memberships)
    }

    /// Replaces each listed photo's Picasa-album memberships with the given set, in one
    /// transaction. Only Picasa albums' rows are deleted, so a photo keeps its photon albums.
    /// The album ids come from `upsert_picasa_albums`, which is why they are not checked
    /// here, and why this is the scan's writer and not a command's.
    pub fn set_picasa_album_items(&self, items: &[(i64, BTreeSet<i64>)], now_ms: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut clear = tx.prepare_cached(
                "DELETE FROM album_items WHERE item_id = ?1
                 AND album_id IN (SELECT id FROM albums WHERE picasa_token IS NOT NULL)",
            )?;
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, ?3)",
            )?;
            for (item_id, albums) in items {
                clear.execute(params![item_id])?;
                for album_id in albums {
                    insert.execute(params![album_id, item_id, now_ms])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
```

- [ ] **Step 4: Run the tests** - `cargo test -p photon-core --lib albums`; all pass.

- [ ] **Step 5: Probe**, exact replacements, restored after:
  - In `refer`, replace `DO NOTHING` with `DO UPDATE SET name = excluded.name`: `a_referenced_token_never_replaces_a_real_name_in_either_order` fails.
  - In `define`, delete the line `WHERE name != excluded.name`: `upserting_an_unchanged_definition_counts_nothing_and_a_rename_counts_one` fails (Review Focus 1).
  - In `clear`, delete the line `AND album_id IN (SELECT id FROM albums WHERE picasa_token IS NOT NULL)` (keeping the `",` terminator on the line above): `the_scans_membership_writer_never_touches_photons_albums` fails.
  - In `folder_picasa_albums`, delete ` AND a.picasa_token IS NOT NULL`: the same test fails.

- [ ] **Step 6: Gate and commit**

```bash
git add crates/photon-core/src/library/albums.rs
git commit -m "feat(albums): the scan's writers for Picasa albums" -m "<probes>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Mirror albums in the scan's Picasa pass

**Files:**
- Modify: `crates/photon-core/src/scanner.rs` (`ScanReport`, `touched_rows`, `PicasaApplied`, `apply_picasa`, `apply_folder_ini`, new `apply_folder_albums`, the three `ScanReport { ... }` literals and one assignment that copy `applied`, tests)
- Modify: `CLAUDE.md` (counters list; the post-walk pass sentence)

**Interfaces:**
- Consumes: `FolderIni.albums`, `FolderIni.item_albums` (Task 1); `upsert_picasa_albums`, `folder_picasa_albums`, `set_picasa_album_items` (Task 3); `AlbumSummary.picasa` (Task 2).
- Produces: `ScanReport::realbumed: u64`, folded into `touched_rows`.

- [ ] **Step 1: Write the failing tests** - in `scanner.rs` `mod tests`, beside `a_face_named_in_picasa_is_picked_up_without_the_photo_changing`:

```rust
    #[test]
    fn a_picasa_album_follows_the_ini_without_the_photo_changing() {
        // Like faces: an album is an INI change, so it takes the `unchanged` branch and only
        // the post-walk pass sees it. An agreeing rescan counts nothing, or every scan of a
        // Picasa library would rebuild the grid; a rename alone must refresh, because only
        // the sidebar shows it.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(&root, ".picasa.ini", b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n");
        let report = scan(&lib, &watched, 2);
        assert_eq!(report.unchanged, 1, "the photo itself did not change");
        assert!(report.realbumed > 0);
        assert!(report.touched_rows());
        let listed = |lib: &Library| -> Vec<(String, i64, bool)> {
            lib.albums_with_counts()
                .unwrap()
                .into_iter()
                .map(|a| (a.name, a.count, a.picasa))
                .collect()
        };
        assert_eq!(listed(&lib), vec![("Holiday".to_string(), 1, true)]);

        let report = scan(&lib, &watched, 3);
        assert_eq!(report.realbumed, 0, "an agreeing folder writes nothing");

        write_file(&root, ".picasa.ini", b"[.album:t]\nname=Summer\n[a.jpg]\nalbums=t\n");
        let report = scan(&lib, &watched, 4);
        assert_eq!(report.realbumed, 1, "the rename, and no membership moved");
        assert!(report.touched_rows(), "a rename alone refreshes the sidebar");
        assert_eq!(listed(&lib), vec![("Summer".to_string(), 1, true)]);

        write_file(&root, ".picasa.ini", b"[.album:t]\nname=Summer\n[a.jpg]\nbackuphash=1\n");
        let report = scan(&lib, &watched, 5);
        assert_eq!(report.realbumed, 1);
        assert!(listed(&lib).is_empty(), "Picasa took the photo out, and the album is empty");
    }

    #[test]
    fn a_picasa_album_spanning_two_folders_is_one_album() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(4, 2));
        write_file(&root, "b/two.jpg", &jpeg_bytes(4, 3));
        write_file(&root, "a/.picasa.ini", b"[.album:t]\nname=Holiday\n[one.jpg]\nalbums=t\n");
        // The second folder only names the token: its name must not win.
        write_file(&root, "b/.picasa.ini", b"[two.jpg]\nalbums=T\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let albums: Vec<(String, i64)> = lib
            .albums_with_counts()
            .unwrap()
            .into_iter()
            .map(|a| (a.name, a.count))
            .collect();
        assert_eq!(albums, vec![("Holiday".to_string(), 2)]);
    }

    #[test]
    fn a_subtree_scan_applies_picasa_albums_too() {
        // The watcher's path. Passes only because `scan_subtree` shares `apply_picasa` with
        // `scan_watched`; the mistake it catches is wiring the album pass into one of them.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        write_file(&root, "sub/.picasa.ini", b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n");
        let report = scan_sub(&lib, &watched, &root.join("sub"), 2);
        assert!(report.realbumed > 0);
        assert!(report.touched_rows());
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn an_unreadable_ini_keeps_a_photos_picasa_albums() {
        // A pin on the existing `None` branch of `apply_picasa`, for the new data: a symlinked
        // INI is refused by the reader (v0.28.2) and must read as no evidence, not as "no
        // albums". Passes before this task's code as long as the album pass sits inside
        // `apply_folder_ini`, which is the point.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[.album:t]\nname=Holiday\n[a.jpg]\nalbums=t\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);

        let elsewhere = dir.path().join("elsewhere.ini");
        std::fs::write(&elsewhere, b"[a.jpg]\nbackuphash=1\n").unwrap();
        std::fs::remove_file(root.join(".picasa.ini")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join(".picasa.ini")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert_eq!(report.realbumed, 0);
        assert_eq!(lib.albums_with_counts().unwrap().len(), 1);
    }
```

`temp_library`, `photos_root`, `write_file`, `jpeg_bytes`, `scan` and `scan_sub` are the helpers `a_subtree_scan_applies_picasas_hidden_flag_too` already uses.

- [ ] **Step 2: Run to see them fail** - `cargo test -p photon-core --lib scanner`; compile error on `realbumed`.

- [ ] **Step 3: Implement** - in `scanner.rs`:

`ScanReport`, after `rehidden`:

```rust
    /// Photos whose Picasa-album memberships the Picasa pass changed this scan, plus Picasa
    /// albums it inserted or renamed. Like `restarred`, the photo itself never changes; a
    /// rename alone counts, because only the sidebar shows it.
    pub realbumed: u64,
```

`touched_rows`: add `+ self.realbumed` after `+ self.rehidden`, and extend its doc comment's list: "`refaced`, `rehidden`, `realbumed` and `enriched` count because a view can be built from what they change: the Person view from faces, the Album view and its sidebar list from Picasa's albums, every view from the hidden flag, ...".

`PicasaApplied`: add `realbumed: u64,`. `apply_picasa`: add `applied.realbumed += folder.realbumed;`.

Every place that copies `applied` into a report - find them with `grep -n 'rehidden: applied.rehidden\|report.rehidden = applied.rehidden' crates/photon-core/src/scanner.rs` (three struct literals and one assignment today) - gets the matching `realbumed: applied.realbumed,` / `report.realbumed = applied.realbumed;` beside it.

`apply_folder_ini` - albums first, so an album exists before anything else reads it:

```rust
/// Contacts first, so a face written below can already resolve its name; then albums, stars,
/// faces and hidden flags from one read of the folder's item names.
fn apply_folder_ini(lib: &Library, folder_id: i64, ini: &FolderIni) -> Result<PicasaApplied> {
    lib.upsert_contacts(&ini.contacts)?;
    let names = lib.folder_item_names(folder_id)?;
    Ok(PicasaApplied {
        realbumed: apply_folder_albums(lib, folder_id, &names, ini)?,
        restarred: apply_folder_stars(lib, &names, &ini.stars)?,
        refaced: apply_folder_faces(lib, folder_id, &names, &ini.faces)?,
        rehidden: apply_folder_hidden(lib, folder_id, &names, &ini.hidden)?,
    })
}
```

New function, after `apply_folder_faces`:

```rust
/// Mirrors the folder's Picasa albums onto its photos, and returns how many photos'
/// memberships moved plus how many albums were inserted or renamed.
///
/// Mirrored like faces, not followed on change like `hidden=`: photon never writes an album,
/// so there is no photon-side answer to protect, and an INI that no longer lists a photo in an
/// album means Picasa took it out. A photo's photon albums are out of reach of this:
/// `set_picasa_album_items` deletes only Picasa albums' rows. Only photos that differ are
/// written, so an agreeing folder costs nothing.
fn apply_folder_albums(
    lib: &Library,
    folder_id: i64,
    names: &[(i64, String, Option<i64>)],
    ini: &FolderIni,
) -> Result<u64> {
    let referenced: HashSet<String> = ini.item_albums.values().flatten().cloned().collect();
    let (ids, upserted) = lib.upsert_picasa_albums(&ini.albums, &referenced, crate::now_ms())?;
    let current = lib.folder_picasa_albums(folder_id)?;
    let none = BTreeSet::new();
    let changes: Vec<(i64, BTreeSet<i64>)> = names
        .iter()
        .filter_map(|(id, name, _)| {
            let wanted: BTreeSet<i64> = ini
                .item_albums
                .get(name)
                .into_iter()
                .flatten()
                .filter_map(|token| ids.get(token).copied())
                .collect();
            (wanted != *current.get(id).unwrap_or(&none)).then_some((*id, wanted))
        })
        .collect();
    let moved = changes.len() as u64;
    for chunk in changes.chunks(BATCH) {
        lib.set_picasa_album_items(chunk, crate::now_ms())?;
    }
    Ok(upserted + moved)
}
```

Add `BTreeSet` and `HashSet` to the file's `std::collections` import.

`CLAUDE.md`:
- In "Today the counters beyond the obvious four are `restarred`, `refaced` (Picasa faces), `rehidden` (Picasa's `hidden=yes`) and `enriched` (the metadata backfill)." make it "... `rehidden` (Picasa's `hidden=yes`), `realbumed` (Picasa's albums) and `enriched` (the metadata backfill)."
- In "The one post-walk pass today is `apply_picasa`, which applies stars, faces *and* hidden flags from one `picasa::read_folder`." make it "which applies stars, faces, hidden flags *and* albums from one `picasa::read_folder`."
- In the Conventions bullet listing what is read from the INI and never written ("Faces, contacts and `hidden=` flags are read from the same INI and never written"), add albums: "Faces, contacts, albums and `hidden=` flags ...".

- [ ] **Step 4: Run the tests** - `cargo test --workspace`; all pass.

- [ ] **Step 5: Probe**, exact, restored after:
  - Delete `+ self.realbumed` from `touched_rows`: `a_picasa_album_follows_the_ini_without_the_photo_changing` fails at "a rename alone refreshes the sidebar".
  - Replace `realbumed: apply_folder_albums(lib, folder_id, &names, ini)?,` with `realbumed: 0,`: the follows, spanning and subtree tests fail.
  - Replace `.filter_map(|token| ids.get(token).copied())` with `.filter_map(|_| None)`: the follows test fails.
  - Record in the commit message that `an_unreadable_ini_keeps_a_photos_picasa_albums` is a pin, and that `a_subtree_scan_applies_picasa_albums_too` fails only with the pass wired out of the shared function.

- [ ] **Step 6: Gate and commit**

```bash
git add crates/photon-core/src/scanner.rs CLAUDE.md
git commit -m "feat(scan): mirror Picasa albums in the post-walk pass" -m "<probes; the pins>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The app end to end

**Files:**
- Test: `crates/photon-app/src/commands.rs` (`mod tests`)

**Interfaces:**
- Consumes: everything above through the app's `fixture` (the helper `viewer_item_carries_camera_keywords_faces_and_albums` uses: `fixture(&[(path, bytes)])`, `f.photos`, `f.add_photos()`, `f.ids()`, `f.engine`).

- [ ] **Step 1: Write the test** - beside `viewer_item_carries_camera_keywords_faces_and_albums`:

```rust
    #[test]
    fn a_picasa_album_reaches_the_sidebar_the_viewer_and_its_view() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        std::fs::write(
            f.photos.join("a/.picasa.ini"),
            b"[.album:t]\nname=Holiday\n[one.jpg]\nalbums=t\n",
        )
        .unwrap();
        f.add_photos();

        let album = list_albums(&f.engine)
            .unwrap()
            .into_iter()
            .find(|a| a.picasa)
            .expect("the scan imported the album");
        assert_eq!((album.name.as_str(), album.count), ("Holiday", 1));

        let one = f
            .ids()
            .into_iter()
            .find(|&id| f.engine.lib.item(id).unwrap().unwrap().path.ends_with("one.jpg"))
            .unwrap();
        assert_eq!(viewer_item(&f.engine, one).unwrap().albums, vec![album.id]);

        set_album_view(&f.engine, album.id).unwrap();
        let info = grid_info(&f.engine);
        assert_eq!(
            (info.view, info.album, info.len),
            (GridView::Album, Some(album.id), 1)
        );
        assert!(
            add_to_album(&f.engine, album.id, &f.ids()).is_err(),
            "the guard holds over IPC"
        );
    }
```

- [ ] **Step 2: Run** - `cargo test -p photon-app a_picasa_album_reaches`; passes (the feature is in).

- [ ] **Step 3: Probe** - in `scanner.rs`, replace `realbumed: apply_folder_albums(lib, folder_id, &names, ini)?,` with `realbumed: 0,`: this test fails at "the scan imported the album". Restore.

- [ ] **Step 4: Gate and commit**

```bash
git add crates/photon-app/src/commands.rs
git commit -m "test(app): a Picasa album from the INI to the viewer and the Album view" -m "<probe>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The UI

**Files:**
- Create: `ui/src/lib/albums.ts`, `ui/src/lib/albums.test.ts`
- Modify: `ui/src/lib/icons.ts` (new `images` icon)
- Modify: `ui/src/components/FolderTree.svelte` (album rows, ~lines 297-320)
- Modify: `ui/src/components/Grid.svelte` (empty message ~613, menu ~683-755)
- Modify: `ui/src/components/Viewer.svelte` (script; info panel ~806-825; styles ~1060)
- Modify: `crates/xtask/screenshots/mock.js` (`list_albums` ~123, the viewer item's `albums` ~72)

**Interfaces:**
- Consumes: `AlbumSummary.picasa` (Task 2).
- Produces: `ownAlbums(albums: AlbumSummary[]): AlbumSummary[]`, `isOwnAlbum(albums: AlbumSummary[], albumId: number | null): boolean`, `picasaAlbumsOf(albums: AlbumSummary[], memberOf: number[]): AlbumSummary[]`.

- [ ] **Step 1: Write the failing tests** - `ui/src/lib/albums.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { AlbumSummary } from './api';
import { isOwnAlbum, ownAlbums, picasaAlbumsOf } from './albums';

const trip: AlbumSummary = { id: 1, name: 'Holiday', count: 2, picasa: false };
const holiday: AlbumSummary = { id: 2, name: 'Holiday', count: 5, picasa: true };
const wedding: AlbumSummary = { id: 3, name: 'Wedding', count: 9, picasa: true };
const all = [trip, holiday, wedding];

describe('ownAlbums', () => {
  it("keeps photon's albums only, even beside a Picasa album of the same name", () => {
    expect(ownAlbums(all)).toEqual([trip]);
  });
});

describe('isOwnAlbum', () => {
  it('is true only for a listed photon album', () => {
    expect(isOwnAlbum(all, 1)).toBe(true);
    expect(isOwnAlbum(all, 2)).toBe(false);
  });

  it('is false for an album no longer listed, and for no album', () => {
    // A Picasa album that lost its last photo leaves the list while it may still be open.
    expect(isOwnAlbum(all, 42)).toBe(false);
    expect(isOwnAlbum(all, null)).toBe(false);
  });
});

describe('picasaAlbumsOf', () => {
  it("lists the Picasa albums a photo is in, in the list's order, and none of photon's", () => {
    expect(picasaAlbumsOf(all, [3, 1, 2])).toEqual([holiday, wedding]);
    expect(picasaAlbumsOf(all, [1])).toEqual([]);
  });
});
```

- [ ] **Step 2: Run** - `npm test -w ui -- src/lib/albums.test.ts`; fails (no module).

- [ ] **Step 3: Implement the module** - `ui/src/lib/albums.ts`:

```ts
import type { AlbumSummary } from './api';

/** photon's own albums: the ones a photo can be added to or taken out of, and that can be
 *  renamed and deleted. Picasa's are mirrored from its INI, and the backend refuses every
 *  one of those writes on them. */
export function ownAlbums(albums: AlbumSummary[]): AlbumSummary[] {
  return albums.filter((a) => !a.picasa);
}

/** Whether `albumId` is one of photon's own albums in `albums`. Asked this way round, not as
 *  "is it Picasa's": a Picasa album that just lost its last photo has left the list, and an
 *  id that is not listed must not read as editable. */
export function isOwnAlbum(albums: AlbumSummary[], albumId: number | null): boolean {
  return albums.some((a) => a.id === albumId && !a.picasa);
}

/** The Picasa albums a photo is in, in the list's order, for the info panel's read-only rows. */
export function picasaAlbumsOf(albums: AlbumSummary[], memberOf: number[]): AlbumSummary[] {
  const member = new Set(memberOf);
  return albums.filter((a) => a.picasa && member.has(a.id));
}
```

- [ ] **Step 4: Run** - `npm test -w ui -- src/lib/albums.test.ts`; passes.

- [ ] **Step 5: Probe** - replace the body of `isOwnAlbum` with `return !albums.some((a) => a.id === albumId && a.picasa);`: the "no longer listed" test fails (Review Focus 2). Replace `ownAlbums`'s filter with `(a) => a.name !== 'x'`: the same-name test fails. Restore.

- [ ] **Step 6: Vendor the icon** - Lucide's `images`, from the exact version `icons.ts` names:

```bash
cd /tmp/claude-1000/-home-dh-Projects-photon/b4474647-90f3-48b6-a7dd-133c1fbedc42/scratchpad && npm pack lucide-static@1.47.0 && tar xzf lucide-static-1.47.0.tgz package/icons/images.svg && cat package/icons/images.svg
```

Copy the elements inside `<svg>` verbatim (no attributes of the `<svg>` itself) into `ICONS` as `images: '<...>'`, and add `| 'images'` to `IconName` after `'folder'`. Lucide's licence already covers it (`THIRD-PARTY-NOTICES.md`); it does not derive from Feather, so the header's Feather list is unchanged. If `npm pack` cannot reach the registry, stop and ask - do not reconstruct the path data from memory.

- [ ] **Step 7: Wire the sidebar** - `FolderTree.svelte`, the album `<button class="node">`:

```svelte
        <button
          class="node"
          class:active={library.info.view === 'album' && library.info.album === album.id}
          title={album.name}
          onclick={() => show(() => library.setAlbumView(album.id))}
          oncontextmenu={(e) => (album.picasa ? e.preventDefault() : albumContextMenu(e, album))}
        >
          <span class="name">{album.name}</span>
          {#if album.picasa}
            <!-- Picasa's own album, mirrored from its INI: no menu, since Rename and Delete
                 are all it would hold and the backend refuses both. -->
            <span class="picasa" role="img" aria-label="From Picasa" title="From Picasa. Change it in Picasa."
              ><Icon name="images" size={12} /></span
            >
          {/if}
          <span class="count">{album.count.toLocaleString()}</span>
        </button>
```

and in `<style>`: `.picasa { display: inline-flex; flex: none; color: var(--text-dim); margin-left: 4px; }`. Update the `<!-- Albums: photon's own, so the group is editable. -->` comment to: `<!-- Albums: photon's own, editable, and Picasa's, mirrored from its INI and read-only. -->`.

- [ ] **Step 8: Wire the grid** - `Grid.svelte`: `import { isOwnAlbum, ownAlbums } from '../lib/albums';`.

The empty message:

```svelte
        {:else if library.info.view === 'album'}
          {#if isOwnAlbum(library.albums, library.info.album)}
            “{library.albumName(library.info.album)}” is empty. Right-click a photo to add it.
          {:else}
            This album has no photos in the library.
          {/if}
```

The menu's first line: `{@const albumId = library.info.view === 'album' && isOwnAlbum(library.albums, library.info.album) ? library.info.album : null}`, with a comment above it: `<!-- Only photon's own album offers "Remove from": Picasa's are changed in Picasa. -->`.

The "Add to album" loop: `{#each ownAlbums(library.albums) as album (album.id)}`.

- [ ] **Step 9: Wire the viewer** - `Viewer.svelte` script, near `membership`:

```ts
  import { ownAlbums, picasaAlbumsOf } from '../lib/albums';
  // The info panel's checkboxes are photon's albums only; Picasa's are listed read-only, and
  // only the ones this photo is in - every one unchecked would bury photon's own.
  const ownAlbumList = $derived(ownAlbums(library.albums));
  const picasaHere = $derived(item ? picasaAlbumsOf(library.albums, item.albums) : []);
```

(put the `import` with the other imports). The panel:

```svelte
      <h3>Albums</h3>
      {#if ownAlbumList.length}
        <ul class="albums">
          {#each ownAlbumList as album (album.id)}
            <li>
              <label>
                <input
                  type="checkbox"
                  checked={membership.has(album.id)}
                  disabled={membership.busy(album.id)}
                  onchange={() => toggleAlbum(album.id)}
                />
                {album.name}
              </label>
            </li>
          {/each}
        </ul>
      {:else if !picasaHere.length}
        <p class="info-muted">No albums yet. Create one in the sidebar.</p>
      {/if}
      {#if picasaHere.length}
        <ul class="albums picasa-albums">
          {#each picasaHere as album (album.id)}
            <li title="From Picasa. Change it in Picasa."><Icon name="images" size={12} />{album.name}</li>
          {/each}
        </ul>
      {/if}
```

Style: `.picasa-albums li { display: flex; align-items: center; gap: 8px; padding: 2px 0; color: var(--text-dim); }`. Confirm `Icon` is already imported in `Viewer.svelte`; import it if not.

- [ ] **Step 10: The screenshot mock** - `crates/xtask/screenshots/mock.js`:

```js
    list_albums: () => [
      { id: 1, name: 'Best of 2025', count: 96, picasa: false },
      { id: 3, name: 'Holiday 2009', count: 63, picasa: true },
      { id: 2, name: 'Lisbon', count: 48, picasa: false },
    ],
```

and the viewer item's `albums: [1],` becomes `albums: [1, 3],`.

- [ ] **Step 11: Gates** - `npm run check` (0 errors, 0 warnings) and `npm test`; the Rust gate (the `screenshots.rs` test reads `api.ts`). If Chromium is available, `cargo run -p xtask -- screenshots` and look at the sidebar and viewer-info shots; otherwise say so in the commit message.

- [ ] **Step 12: Commit**

```bash
git add ui/src/lib/albums.ts ui/src/lib/albums.test.ts ui/src/lib/icons.ts ui/src/components/FolderTree.svelte ui/src/components/Grid.svelte ui/src/components/Viewer.svelte crates/xtask/screenshots/mock.js
git commit -m "feat(ui): Picasa albums in the sidebar, the grid menu and the info panel" -m "<probes; component wiring is svelte-check and the smoke checklist - no component harness>" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: README and smoke checklist

**Files:**
- Modify: `README.md` (new section after `### Stars and Picasa`; the info-panel sentence in `### Camera data, keywords, people and albums`; the smoke checklist)

- [ ] **Step 1: The section** - after the `### Stars and Picasa` section's last paragraph, insert:

```markdown
### Picasa albums

Albums you made in Picasa 3 appear in photon's **Albums** list, marked with a small
stacked-photos icon, once the folders holding their photos have been scanned. photon reads
them from the same `.picasa.ini` Picasa writes beside the photos and follows every change on
the next scan, but never changes them: renaming, deleting, adding and removing are Picasa's.
To make one editable, open it, select its photos and add them to an album of your own. An
album Picasa kept only in its own database, never written into a folder's INI, does not
appear.
```

- [ ] **Step 2: The info panel sentence** - in `### Camera data, keywords, people and albums`, change "and checkboxes for photon's albums." to "checkboxes for photon's albums; and the Picasa albums the photo is in."

- [ ] **Step 3: Smoke checklist** - insert after the checklist's last line that mentions albums (`grep -n -i 'album' README.md`, taking the last hit below `## Manual smoke checklist`); if none, after the checklist's first item:

```markdown
- [ ] On a real Picasa library, `grep -rl --include='*icasa.ini' '^\[\.album:' <library>` finds INIs, and their albums appear under Albums with the Picasa icon and Picasa's photo counts. **This confirms the INI album format, which was designed from documentation; if it fails, the parser changes before release.**
- [ ] Right-clicking a Picasa album opens no menu; the grid's "Add to album" does not list it; its view offers no "Remove from".
- [ ] The info panel lists the photo's Picasa albums below the album checkboxes, without checkboxes.
- [ ] Renaming an album in Picasa and rescanning renames it in photon.
```

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: Picasa albums" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Whole-branch gate and independent review

- [ ] **Step 1:** Run the full Rust gate and the UI gate on the branch head; all green.
- [ ] **Step 2:** Dispatch one independent reviewer on the whole branch (`git diff main...feat/picasa-albums`), per CLAUDE.md's convention. Point it at: the effect wiring in `Viewer.svelte` (`picasaHere` derived from `item` - does it update when a rescan lands while the viewer is open, and does anything new make `pictureChanged` reload the photo?); what the feature *arms* in old code (the four guarded writers' other callers, `remove_from_album`'s new `NotFound`, `albums_changed`); `apply_folder_albums` running before stars inside the per-folder error boundary; and every probe recorded in the commit messages.
- [ ] **Step 3:** Fix what it confirms, each fix with its own probed test, and re-run both gates.
- [ ] **Step 4:** Push and open a PR (`git -c credential.helper='!gh auth git-credential' push https://github.com/bsg62/photon.git HEAD:refs/heads/feat/picasa-albums`, then `gh pr create`), and wait for CI with `gh pr checks N --watch` (after checks exist). Do not merge or release without the user.
