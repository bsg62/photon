# photon Starred Photos Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** photon reads star ratings that already exist in photos' embedded XMP and offers a Starred view, so a library rated in Picasa arrives already starred.

**Architecture:** A container-agnostic XMP reader in `photon-core` finds the `<x:xmpmeta>` packet in a bounded prefix of a file and extracts `xmp:Rating`. The scanner stores it as it indexes, in a nullable `rating` column. The Starred view is the existing grid index rebuilt with a filter, so paging, sections and viewer navigation work unchanged.

This version expects a **fresh library**: every photo is rated as it is indexed, so no row is ever left unread and there is no backfill. An existing library carried across would show no stars until its photos are re-indexed.

**Tech Stack:** Rust (edition 2024, rust-version 1.88), `quick-xml` 0.42 (already in the lockfile, pure Rust), rusqlite with `PRAGMA user_version` migrations, Tauri 2 IPC, Svelte 5 + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-13-photon-starred-photos-design.md`. Its parent, `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`, stays binding.

## Global Constraints

- **photon never writes to, moves or deletes files inside watched folders.** This feature reads only. No XMP packet is modified and no sidecar is created. Any task that writes to a file under a watched folder is wrong.
- **Starred means `rating >= 1`.** Ratings run 0–5; `-1` means "rejected" and is stored as `0`.
- **The `rating` column is nullable.** `NULL` = not read yet, `0` = read and unrated. Nothing writes `NULL` today, since every row is rated at index time — it stays nullable because `NOT NULL` would be a one-way door, forcing another migration to relax it if a backfill is ever needed. Never give it a `NOT NULL DEFAULT`.
- **The XMP read is bounded to the first 256 KiB** of a file.
- **Reading metadata never fails.** A malformed packet, truncated file or unexpected structure yields "no rating", never an error that fails a scan.
- **No native library dependencies.** `quick-xml` is pure Rust and already vendored. Never add `xmp-toolkit` or anything wrapping a C/C++ SDK — it would undo photon's cross-platform packaging.
- Rust edition 2024, rust-version 1.88.
- **Every task ends with all four Rust gates**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo bench -p photon-core --bench grid --no-run`. Tasks touching the UI also need `npm run check` at 0 errors **and** 0 warnings, plus `npm test`.
- **A new test must be demonstrated to fail with its change reverted.** This project has shipped vacuous tests before.
- **A test may never depend on timing.** Hold state explicitly rather than racing a background thread.
- Never launch the GUI. Never push. Conventional Commits.

## File Structure

```
crates/photon-core/src/xmp.rs                  NEW: packet extraction + rating parse
crates/photon-core/src/lib.rs                  + pub mod xmp;
crates/photon-core/Cargo.toml                  + quick-xml
crates/photon-core/src/testutil.rs             + jpeg_with_xmp, png_with_xmp, gif_with_xmp, crc32
crates/photon-core/src/metadata.rs             ImageMeta gains `rating`
crates/photon-core/src/library/schema.rs       MIGRATIONS gains entry 2
crates/photon-core/src/library/items.rs        NewItem.rating; insert/update; rating queries
crates/photon-core/src/grid.rs                 GridEntry gains `starred`
crates/photon-core/src/scanner.rs              describe() carries the rating through
crates/photon-app/src/engine.rs                view mode; refresh_grid honours it
crates/photon-app/src/commands.rs              GridInfo.starred_count; set_grid_view
crates/photon-app/src/ipc.rs                   #[tauri::command(async)] wrapper
crates/photon-app/src/app.rs                   generate_handler! entry
ui/src/lib/api.ts                              types + setGridView
ui/src/lib/library.svelte.ts                   view state
ui/src/components/FolderTree.svelte            Starred row
README.md                                      smoke checklist additions
```

---

### Task 1: The XMP rating reader

**Files:**
- Create: `crates/photon-core/src/xmp.rs`
- Modify: `crates/photon-core/src/lib.rs`, `crates/photon-core/Cargo.toml`, `crates/photon-core/src/testutil.rs`

**Interfaces:**
- Produces:
  - `xmp::read_rating(path: &Path) -> Option<u8>` — reads at most `MAX_PREFIX` bytes, finds the packet, returns 0–5 or `None`.
  - `xmp::rating_from_xml(xml: &str) -> Option<u8>` — pure, the whole parse contract.
  - `xmp::MAX_PREFIX: usize = 256 * 1024`
  - `testutil::jpeg_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8>`, `png_with_xmp(w, h, rating)`, `gif_with_xmp(w, h, rating)`, `xmp_packet(rating: i32) -> String`

**Why container-agnostic:** the XMP packet is stored as plain text in every container photon supports — JPEG `APP1`, PNG uncompressed `iTXt`, WebP `XMP ` chunk, GIF Application Extension. Scanning a bounded prefix for `<x:xmpmeta` … `</x:xmpmeta>` finds all four without a parser per format. A compressed PNG `iTXt` is not handled and yields no rating; Adobe and Picasa write it uncompressed.

- [ ] **Step 1: Add the dependency**

In `crates/photon-core/Cargo.toml`, add to `[dependencies]`, keeping alphabetical order (after `parking_lot`):

```toml
quick-xml = "0.42"
```

- [ ] **Step 2: Add the fixtures**

Append to `crates/photon-core/src/testutil.rs`:

```rust
/// A minimal XMP packet carrying `xmp:Rating` as an attribute, the spelling Picasa writes.
pub fn xmp_packet(rating: i32) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

/// A JPEG carrying the packet in an APP1 segment, as a camera or Picasa writes it.
pub fn jpeg_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut app1 = vec![0xFF, 0xE1];
    let ns = b"http://ns.adobe.com/xap/1.0/\0";
    app1.extend_from_slice(&((2 + ns.len() + packet.len()) as u16).to_be_bytes());
    app1.extend_from_slice(ns);
    app1.extend_from_slice(packet.as_bytes());

    let jpeg = jpeg_bytes(w, h);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// CRC-32 (IEEE), computed bitwise so no table or dependency is needed. PNG chunks carry one.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A PNG carrying the packet in an uncompressed `iTXt` chunk, inserted after the IHDR.
pub fn png_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut data = Vec::new();
    data.extend_from_slice(b"XML:com.adobe.xmp\0"); // keyword + null
    data.push(0); // compression flag: uncompressed
    data.push(0); // compression method
    data.push(0); // language tag: empty, null-terminated
    data.push(0); // translated keyword: empty, null-terminated
    data.extend_from_slice(packet.as_bytes());

    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut typed = b"iTXt".to_vec();
    typed.extend_from_slice(&data);
    chunk.extend_from_slice(&typed);
    chunk.extend_from_slice(&crc32(&typed).to_be_bytes());

    // 8-byte signature, then IHDR (4 len + 4 type + 13 data + 4 crc = 25 bytes).
    let png = png_bytes(w, h);
    let split = 8 + 25;
    let mut out = png[..split].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[split..]);
    out
}

/// A GIF carrying the packet in an XMP Application Extension, inserted after the header.
/// The XMP GIF convention stores the packet so that a reader ignoring sub-block framing
/// still sees contiguous XML, which is exactly what `xmp::read_rating` relies on.
pub fn gif_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut ext = vec![0x21, 0xFF, 0x0B];
    ext.extend_from_slice(b"XMP DataXMP");
    ext.extend_from_slice(packet.as_bytes());
    ext.push(0x00); // block terminator

    let gif = encode(&solid(w, h), ImageFormat::Gif);
    // Header (6) + logical screen descriptor (7). No global colour table is emitted for
    // these solid images; if one were present it would follow and the packet would simply
    // sit after it, which the scan also tolerates.
    let split = 13.min(gif.len());
    let mut out = gif[..split].to_vec();
    out.extend_from_slice(&ext);
    out.extend_from_slice(&gif[split..]);
    out
}
```

- [ ] **Step 3: Write the failing tests**

Create `crates/photon-core/src/xmp.rs` containing ONLY this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{gif_with_xmp, jpeg_bytes, jpeg_with_xmp, png_with_xmp, write_file, xmp_packet};

    #[test]
    fn reads_the_rating_from_an_attribute() {
        assert_eq!(rating_from_xml(&xmp_packet(3)), Some(3));
        assert_eq!(rating_from_xml(&xmp_packet(1)), Some(1));
        assert_eq!(rating_from_xml(&xmp_packet(0)), Some(0));
        assert_eq!(rating_from_xml(&xmp_packet(5)), Some(5));
    }

    #[test]
    fn reads_the_rating_from_a_child_element() {
        // Both spellings are legal XMP; some tools write the element form.
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
            xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <xmp:Rating>4</xmp:Rating>
            </rdf:Description></rdf:RDF></x:xmpmeta>"#;
        assert_eq!(rating_from_xml(xml), Some(4));
    }

    #[test]
    fn a_rejected_or_out_of_range_rating_is_not_a_rating() {
        // -1 means "rejected" in XMP, which is not a star. 6 is out of range.
        assert_eq!(rating_from_xml(&xmp_packet(-1)), None);
        assert_eq!(rating_from_xml(&xmp_packet(6)), None);
    }

    #[test]
    fn xml_without_a_rating_or_that_is_malformed_yields_none() {
        assert_eq!(rating_from_xml(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"/>"#), None);
        assert_eq!(rating_from_xml("<not xml at all"), None);
        assert_eq!(rating_from_xml(""), None);
    }

    #[test]
    fn reads_a_rating_out_of_each_container() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = write_file(dir.path(), "a.jpg", &jpeg_with_xmp(8, 8, 2));
        let png = write_file(dir.path(), "a.png", &png_with_xmp(8, 8, 5));
        let gif = write_file(dir.path(), "a.gif", &gif_with_xmp(8, 8, 1));
        assert_eq!(read_rating(&jpg), Some(2));
        assert_eq!(read_rating(&png), Some(5));
        assert_eq!(read_rating(&gif), Some(1));
    }

    #[test]
    fn a_file_without_xmp_or_that_cannot_be_read_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write_file(dir.path(), "plain.jpg", &jpeg_bytes(8, 8));
        assert_eq!(read_rating(&plain), None);
        assert_eq!(read_rating(&dir.path().join("missing.jpg")), None);
    }

    #[test]
    fn a_packet_beyond_the_read_cap_is_not_found() {
        // The cap is what stops a rating lookup pulling a 20MB photo through memory, so it
        // has to actually bound the read rather than being advisory.
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![b'\0'; MAX_PREFIX];
        bytes.extend_from_slice(xmp_packet(4).as_bytes());
        let path = write_file(dir.path(), "late.jpg", &bytes);
        assert_eq!(read_rating(&path), None);

        // ...but one that ends just inside the cap is found.
        let packet = xmp_packet(4);
        let mut early = vec![b'\0'; MAX_PREFIX - packet.len()];
        early.extend_from_slice(packet.as_bytes());
        let path = write_file(dir.path(), "early.jpg", &early);
        assert_eq!(read_rating(&path), Some(4));
    }
}
```

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test -p photon-core xmp::`
Expected: FAIL — `rating_from_xml`, `read_rating` and `MAX_PREFIX` are not defined.

- [ ] **Step 5: Write the implementation**

Insert ABOVE the test module in `crates/photon-core/src/xmp.rs`:

```rust
//! Reads `xmp:Rating` out of a photo's embedded XMP packet.
//!
//! The packet is plain text in every container photon supports — JPEG `APP1`, PNG
//! uncompressed `iTXt`, WebP `XMP ` chunk, GIF Application Extension — so a bounded scan
//! for the packet markers finds all of them without a parser per format. photon only ever
//! reads: nothing here writes to a file.

use quick_xml::Reader;
use quick_xml::events::Event;
use std::{fs::File, io::Read, path::Path};

/// How much of a file is searched for the packet. XMP sits near the start of every
/// container above; this stops a rating lookup pulling a 20MB photo through memory, and
/// keeps the per-photo cost of a first scan predictable rather than scaling with size.
pub const MAX_PREFIX: usize = 256 * 1024;

const PACKET_START: &[u8] = b"<x:xmpmeta";
const PACKET_END: &str = "</x:xmpmeta>";

/// The rating in a photo's embedded XMP, if it has one. Never fails: an unreadable file,
/// a truncated packet or malformed XML all yield `None`, because one bad photo must not
/// fail a scan of a hundred thousand.
pub fn read_rating(path: &Path) -> Option<u8> {
    let mut file = File::open(path).ok()?;
    let mut buf = Vec::new();
    file.by_ref()
        .take(MAX_PREFIX as u64)
        .read_to_end(&mut buf)
        .ok()?;
    let start = buf
        .windows(PACKET_START.len())
        .position(|w| w == PACKET_START)?;
    let text = String::from_utf8_lossy(&buf[start..]);
    let end = text.find(PACKET_END).map(|i| i + PACKET_END.len())?;
    rating_from_xml(&text[..end])
}

/// Reads `xmp:Rating` from an XMP packet, in either the attribute or the child-element
/// spelling. Returns `None` for a missing, malformed or out-of-range value — including
/// `-1`, which means "rejected" rather than a star.
pub fn rating_from_xml(xml: &str) -> Option<u8> {
    let mut reader = Reader::from_str(xml);
    let mut in_rating = false;
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => return None,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"xmp:Rating"
                        && let Ok(value) = attr.unescape_value()
                        && let Some(rating) = parse_rating(&value)
                    {
                        return Some(rating);
                    }
                }
                if e.name().as_ref() == b"xmp:Rating" {
                    in_rating = true;
                }
            }
            Ok(Event::Text(t)) if in_rating => {
                // No unescaping: `BytesText` has no `unescape` method in quick-xml 0.42,
                // and a rating is digits — there is nothing an entity could encode here.
                if let Some(rating) = parse_rating(&String::from_utf8_lossy(&t)) {
                    return Some(rating);
                }
                in_rating = false;
            }
            _ => {}
        }
    }
}

fn parse_rating(value: &str) -> Option<u8> {
    let n: i32 = value.trim().parse().ok()?;
    (0..=5).contains(&n).then_some(n as u8)
}
```

Every `quick-xml` call above is verified against the vendored 0.42 source: `Reader::from_str` and `read_event()` on the slice reader (`reader/slice_reader.rs:27,75`), `Attribute.key` as a public `QName` field and `unescape_value() -> XmlResult<Cow<str>>` (`events/attributes.rs:33,264`). `BytesText` has no `unescape`, which is why the text branch reads the bytes directly.

- [ ] **Step 6: Register the module**

In `crates/photon-core/src/lib.rs`, add `pub mod xmp;` alongside the existing `pub mod` declarations, in alphabetical position.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p photon-core xmp::`
Expected: PASS (7 tests).

- [ ] **Step 8: Prove the cap test is real**

Temporarily raise `MAX_PREFIX` to `512 * 1024`, re-run `cargo test -p photon-core xmp::a_packet_beyond_the_read_cap`, and confirm it FAILS (the late packet is now found). Restore `256 * 1024` and confirm it passes. Record this in your report.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/Cargo.toml crates/photon-core/src/xmp.rs crates/photon-core/src/lib.rs crates/photon-core/src/testutil.rs Cargo.lock
git commit -m "feat(core): read xmp:Rating from a photo's embedded XMP packet"
```

---

### Task 2: The rating column and its migration

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs`, `crates/photon-core/src/library/items.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `items.rating` — `INTEGER`, nullable. `NULL` = not read yet, `0` = read and unrated.
  - `NewItem.rating: Option<u8>`
  - `Library::starred_count(&self) -> Result<usize>`

Nothing else is added. A `set_rating` or `items_needing_rating` would have no production caller now that ratings arrive with the row: the scanner writes them through `insert_items` and `update_items`, and the tests below construct rows the same way. A public method with no caller is scope creep.

**This is the first migration to upgrade a library that already exists on disk.** `MIGRATIONS` currently holds one entry that builds the schema from nothing, and every installed library sits at `user_version = 1`. The new entry must be purely additive and must not rewrite existing rows.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/photon-core/src/library/items.rs`:

```rust
    /// `new_item` builds a row with `rating: None`; this is the same row with a rating, as
    /// the scanner produces once it has read the file's XMP.
    fn rated(folder: i64, path: &str, taken_at: i64, rating: u8) -> NewItem {
        NewItem {
            rating: Some(rating),
            ..new_item(folder, path, taken_at)
        }
    }

    #[test]
    fn only_photos_rated_at_least_one_star_are_counted() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[
            rated(folder, "/p/a.jpg", 1, 3),
            rated(folder, "/p/b.jpg", 2, 0),
            rated(folder, "/p/c.jpg", 3, 1),
        ])
        .unwrap();

        // Three rated photos, two of them starred: zero stars is a read rating, not a star.
        assert_eq!(lib.starred_count().unwrap(), 2);
    }

    #[test]
    fn an_unread_rating_is_not_counted_as_starred() {
        // `new_item` leaves `rating` NULL, which is what an unread row looks like. NULL is
        // not >= 1, so it must not reach the Starred count — SQL comparisons against NULL
        // are never true, and this pins that rather than trusting it.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_missing_item_is_not_counted_as_starred() {
        // A soft-deleted photo must not inflate the Starred count, the same way it does
        // not appear in the grid.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[rated(folder, "/p/a.jpg", 1, 4)]).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 1);
        lib.mark_missing(&ids, 1).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_changed_photo_keeps_the_rating_its_rescan_read() {
        // `update_items` runs when a file's size or mtime changed, and carries the rating
        // the fresh scan read — so a star added in another program survives a rescan.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[rated(folder, "/p/a.jpg", 1, 0)]).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);

        lib.update_items(&[(ids[0], rated(folder, "/p/a.jpg", 1, 5))])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 1);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core library::items`
Expected: FAIL — `starred_count` and `NewItem.rating` are not defined.

- [ ] **Step 3: Add the migration**

In `crates/photon-core/src/library/schema.rs`, append a second entry to `MIGRATIONS`, after the existing one:

```rust
const MIGRATIONS: &[&str] = &[r#"
... existing entry, unchanged ...
"#, r#"
-- Nullable on purpose: NULL means "not read yet", 0 means "read and unrated". A NOT NULL
-- DEFAULT 0 could not tell those apart, and since the scanner only re-reads a file whose
-- size or mtime changed, photos indexed before this feature would never be looked at
-- again — a Picasa-rated library would upgrade and show an empty Starred view.
ALTER TABLE items ADD COLUMN rating INTEGER;
CREATE INDEX items_starred ON items(rating) WHERE rating >= 1 AND missing_since IS NULL;
"#];
```

- [ ] **Step 4: Thread the column through `NewItem` and its SQL**

In `crates/photon-core/src/library/items.rs`:

Add to `NewItem`, after `taken_at`:

```rust
    /// `None` when the file has not been read for a rating yet.
    pub rating: Option<u8>,
```

In `insert_items`, extend the statement and its parameters:

```rust
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at, rating)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
```

and add `it.rating` as the last bound parameter, after `it.taken_at`.

In `update_items`, extend the SET clause — note the parameter numbering starts at `?2` because `?1` is the id:

```rust
                "UPDATE items SET folder_id = ?2, path = ?3, file_name = ?4, kind = ?5, size = ?6, mtime_ms = ?7,
                        width = ?8, height = ?9, orientation = ?10, taken_at = ?11, rating = ?12,
                        thumb_state = 0, thumb_error = NULL, missing_since = NULL
                 WHERE id = ?1",
```

and add `it.rating` as the last bound parameter, after `it.taken_at`.

- [ ] **Step 5: Add the queries**

Add to `impl Library` in `crates/photon-core/src/library/items.rs`:

```rust
    /// How many photos carry at least one star. Served by the `items_starred` partial index.
    ///
    /// `rating >= 1` also excludes unread rows without a second clause: a comparison
    /// against NULL is never true in SQL.
    pub fn starred_count(&self) -> Result<usize> {
        let conn = self.reader();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM items WHERE rating >= 1 AND missing_since IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }
```

- [ ] **Step 6: Fix the fixture**

In `crates/photon-core/src/testutil.rs`, add `rating: None,` to the `NewItem` literal in `new_item`, after `taken_at`.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p photon-core library::items`
Expected: PASS.

- [ ] **Step 8: Prove the migration is additive**

Write this test in the `tests` module of `crates/photon-core/src/library/schema.rs` (create the module if absent, with `use super::*;`):

```rust
    #[test]
    fn the_second_migration_upgrades_a_version_one_library_without_touching_its_rows() {
        // The upgrade that has never run in anger: a library created by an earlier photon,
        // with a user's photos already indexed.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();
        conn.execute(
            "INSERT INTO watched_folders (id, path) VALUES (1, '/p')", []).unwrap();
        conn.execute(
            "INSERT INTO folders (id, watched_id, parent_id, path, name, sort_key) \
             VALUES (1, 1, NULL, '/p', 'p', 'p')", []).unwrap();
        conn.execute(
            "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, \
             height, orientation, taken_at) VALUES (1, '/p/a.jpg', 'a.jpg', 0, 1, 1, 1, 1, 1, 1)",
            []).unwrap();

        migrate(&conn).unwrap();

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let (path, rating): (String, Option<i64>) = conn
            .query_row("SELECT path, rating FROM items", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(path, "/p/a.jpg", "the existing row survives untouched");
        assert_eq!(rating, None, "and reads as unread, not as unrated");
    }
```

Run it, then temporarily change the migration to `ALTER TABLE items ADD COLUMN rating INTEGER NOT NULL DEFAULT 0;` and confirm the test FAILS on the `rating == None` assertion. Restore the nullable form. Record this in your report.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/library/schema.rs crates/photon-core/src/library/items.rs crates/photon-core/src/testutil.rs
git commit -m "feat(core): nullable rating column, its migration and queries"
```

---

### Task 3: The scanner reads ratings, and the grid carries them

**Files:**
- Modify: `crates/photon-core/src/metadata.rs`, `crates/photon-core/src/scanner.rs`, `crates/photon-core/src/grid.rs`, `crates/photon-core/src/library/items.rs`

**Interfaces:**
- Consumes: `xmp::read_rating` (Task 1), `NewItem.rating` (Task 2).
- Produces: `ImageMeta.rating: Option<u8>`; `GridEntry.starred: bool`, serialised as `starred`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/photon-core/src/metadata.rs`:

```rust
    #[test]
    fn reads_the_xmp_rating_alongside_exif() {
        use crate::testutil::jpeg_with_xmp;
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "star.jpg", &jpeg_with_xmp(4, 2, 3));
        assert_eq!(read_image_meta(&path).rating, Some(3));
    }

    #[test]
    fn a_photo_without_xmp_reads_as_unrated_rather_than_unread() {
        // Zero, not None: the file was read and had nothing to say. None means "never
        // looked at", which after a scan would be a lie.
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "plain.png", &png_bytes(3, 5));
        assert_eq!(read_image_meta(&path).rating, Some(0));
    }
```

Add to the `tests` module in `crates/photon-core/src/grid.rs`:

```rust
    #[test]
    fn entries_carry_whether_they_are_starred() {
        let json = serde_json::to_string(&GridEntry {
            starred: true,
            ..entry(7, 1)
        })
        .unwrap();
        assert!(json.contains(r#""starred":true"#), "got {json}");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core metadata:: grid::`
Expected: FAIL — `ImageMeta.rating` and `GridEntry.starred` are not defined.

- [ ] **Step 3: Add the rating to `ImageMeta`**

In `crates/photon-core/src/metadata.rs`, add to `ImageMeta` after `taken_at`:

```rust
    /// `Some(0..=5)` once the file has been read; `None` only if it could not be opened.
    pub rating: Option<u8>,
```

and in `read_image_meta`, after the `ImageMeta` literal is built, set:

```rust
    // Zero rather than None when there is no packet: the file was read and had nothing to
    // say. None is reserved for rows no scan has ever looked at.
    meta.rating = Some(crate::xmp::read_rating(path).unwrap_or(0));
```

Place this before the `if let Some(exif) = ...` block so it runs regardless of EXIF.

- [ ] **Step 4: Carry it into `NewItem`**

In `crates/photon-core/src/scanner.rs`, in `describe()`, add to the `NewItem` literal after `taken_at`:

```rust
        rating: meta.rating,
```

- [ ] **Step 5: Add `starred` to `GridEntry`**

In `crates/photon-core/src/grid.rs`, add to `GridEntry` after `kind`:

```rust
    /// True when the photo carries at least one star in its XMP rating.
    pub starred: bool,
```

In `crates/photon-core/src/library/items.rs`, extend `grid_entries`'s query and row mapping — select the rating and derive the flag:

```rust
            "SELECT i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {GRID_ORDER}"
```

and in the `GridEntry` construction, after `kind`:

```rust
                    starred: r.get::<_, Option<i64>>(10)?.unwrap_or(0) >= 1,
```

In `crates/photon-core/src/testutil.rs`, add `starred: false,` to any `GridEntry` literal; in `crates/photon-core/src/grid.rs`'s test `entry()` helper, add `starred: false,` after `kind`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p photon-core`
Expected: PASS. Note the camelCase serialisation test in `grid.rs` will need `"starred":false` added to its expected JSON, in field order after `kind`.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src
git commit -m "feat(core): scan ratings into the library and carry them on grid entries"
```

---

### Task 4: The Starred view and its IPC

**Files:**
- Modify: `crates/photon-core/src/library/items.rs`, `crates/photon-app/src/engine.rs`, `crates/photon-app/src/commands.rs`, `crates/photon-app/src/ipc.rs`, `crates/photon-app/src/app.rs`

**Interfaces:**
- Consumes: `Library::starred_count` (Task 2), `GridEntry.starred` (Task 3).
- Produces:
  - `GridView` enum (`All`, `Starred`), serde `camelCase`, shared via `photon_core::grid`.
  - `Library::grid_entries_for(view: GridView) -> Result<Vec<GridEntry>>`
  - `Engine::set_view(&self, view: GridView) -> Result<()>`, `Engine::view(&self) -> GridView`
  - `GridInfo.starred_count: usize` and `GridInfo.view: GridView`
  - Tauri command `set_grid_view(view: GridView)`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/photon-core/src/library/items.rs`:

```rust
    #[test]
    fn the_starred_view_contains_exactly_the_starred_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
            ])
            .unwrap();
        lib.update_items(&[
            (ids[0], NewItem { rating: Some(0), ..new_item(folder, "/p/a.jpg", 1) }),
            (ids[1], NewItem { rating: Some(1), ..new_item(folder, "/p/b.jpg", 2) }),
            (ids[2], NewItem { rating: Some(5), ..new_item(folder, "/p/c.jpg", 3) }),
        ])
        .unwrap();

        let all: Vec<i64> = lib.grid_entries_for(GridView::All).unwrap().iter().map(|e| e.id).collect();
        let starred: Vec<i64> = lib.grid_entries_for(GridView::Starred).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(all, ids);
        assert_eq!(starred, vec![ids[1], ids[2]], "unrated and zero-rated are excluded");
    }
```

Add to the `tests` module in `crates/photon-app/src/engine.rs`:

```rust
    #[test]
    fn switching_to_the_starred_view_rebuilds_the_grid_with_only_starred_photos() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        f.add_photos();
        f.engine.wait_for_scans();
        let ids = f.ids();
        // Star one of them the way a scan would, then rebuild the index for the new view.
        let item = f.engine.lib.item(ids[0]).unwrap().unwrap();
        f.engine
            .lib
            .update_items(&[(
                ids[0],
                photon_core::library::NewItem {
                    folder_id: item.folder_id,
                    path: item.path.clone(),
                    file_name: "one.jpg".into(),
                    kind: photon_core::media::MediaKind::Image,
                    size: item.size,
                    mtime_ms: item.mtime_ms,
                    width: 16,
                    height: 16,
                    orientation: 1,
                    taken_at: 1,
                    rating: Some(2),
                },
            )])
            .unwrap();

        f.engine.set_view(GridView::Starred).unwrap();
        assert_eq!(f.engine.grid().1.len(), 1);
        assert_eq!(f.engine.view(), GridView::Starred);

        f.engine.set_view(GridView::All).unwrap();
        assert_eq!(f.engine.grid().1.len(), 2, "switching back restores the full set");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core grid_entries_for; cargo test -p photon-app starred_view`
Expected: FAIL — `GridView`, `grid_entries_for`, `set_view` and `view` are not defined.

- [ ] **Step 3: Add the view type and the filtered query**

In `crates/photon-core/src/grid.rs`, add:

```rust
/// Which set of photos the grid shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridView {
    #[default]
    All,
    Starred,
}
```

and add `use serde::Deserialize;` to that file's imports.

In `crates/photon-core/src/library/items.rs`, rename the body of `grid_entries` into a view-aware form and keep the old name as a thin caller, so existing call sites are untouched:

```rust
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        self.grid_entries_for(GridView::All)
    }

    /// The grid's rows for one view. `Starred` filters to `rating >= 1`, which the
    /// `items_starred` partial index serves.
    pub fn grid_entries_for(&self, view: GridView) -> Result<Vec<GridEntry>> {
        let filter = match view {
            GridView::All => "",
            GridView::Starred => "AND i.rating >= 1",
        };
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {filter} {GRID_ORDER}"
        ))?;
        // ...the existing row mapping, unchanged...
    }
```

Import `GridView` in that file alongside `GridEntry`.

- [ ] **Step 4: Hold the view on the engine**

In `crates/photon-app/src/engine.rs`, add a field beside `grid`:

```rust
    view: RwLock<GridView>,
```

initialised to `RwLock::new(GridView::All)`. Change `refresh_grid` to build from the active view:

```rust
        let index = Arc::new(GridIndex::build(self.lib.grid_entries_for(*self.view.read())?));
```

and add:

```rust
    pub fn view(&self) -> GridView {
        *self.view.read()
    }

    /// Switches which photos the grid shows and rebuilds the index. Rebuilding is the same
    /// work startup already does; a second index kept in sync would be a large new surface
    /// for staleness bugs to speed up something already fast and rarely done.
    pub fn set_view(&self, view: GridView) -> Result<()> {
        *self.view.write() = view;
        self.refresh_grid()
    }
```

- [ ] **Step 5: Extend `GridInfo` and add the command**

In `crates/photon-app/src/commands.rs`, add to `GridInfo` after `sections`:

```rust
    pub starred_count: usize,
    pub view: GridView,
```

and in `grid_info`, populate them — note `starred_count` is queried in **both** views, because the sidebar shows it while the All view is on screen and the index then holds no starred rows to count:

```rust
pub fn grid_info(engine: &Engine) -> GridInfo {
    let (version, grid) = engine.grid();
    GridInfo {
        version,
        len: grid.len(),
        sections: grid.sections().to_vec(),
        starred_count: engine.lib.starred_count().unwrap_or(0),
        view: engine.view(),
    }
}

pub fn set_grid_view(engine: &Engine, view: GridView) -> CmdResult<()> {
    engine.set_view(view)?;
    Ok(())
}
```

`CmdResult<T>` is `Result<T, AppError>` (commands.rs:21) and `?` performs the conversion, exactly as `rescan_folder` does.

In `crates/photon-app/src/ipc.rs`, add the wrapper, matching the file's existing style:

```rust
#[tauri::command(async)]
pub fn set_grid_view(engine: Eng<'_>, view: photon_core::grid::GridView) -> Result<(), AppError> {
    commands::set_grid_view(&engine, view)
}
```

In `crates/photon-app/src/app.rs`, add `ipc::set_grid_view,` to the `tauri::generate_handler![...]` list at line 118, after `ipc::grid_offset_of_folder`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Prove the view test is real**

Temporarily make `grid_entries_for` ignore its argument (always use the `All` filter) and confirm `switching_to_the_starred_view_rebuilds_the_grid_with_only_starred_photos` FAILS on the length assertion. Restore it. Record this in your report.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src crates/photon-app/src
git commit -m "feat: a Starred view, as the grid index rebuilt with a filter"
```

---

### Task 5: The Starred row in the sidebar

**Files:**
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/library.svelte.ts`, `ui/src/components/FolderTree.svelte`, `README.md`

**Interfaces:**
- Consumes: `GridInfo.starredCount`, `GridInfo.view`, and the `set_grid_view` command (Task 4).

- [ ] **Step 1: Extend the API types**

In `ui/src/lib/api.ts`:

```ts
export type GridView = 'all' | 'starred';
export interface GridInfo { version: number; len: number; sections: Section[]; starredCount: number; view: GridView }
```

and add to the `api` object, after `gridOffsetOfFolder`:

```ts
  setGridView: (view: GridView) => invoke<void>('set_grid_view', { view }),
```

- [ ] **Step 2: Add the view switch to the store**

In `ui/src/lib/library.svelte.ts`, add a method to the class that switches view and refreshes, matching how `refreshFolders` is written:

```ts
  /** Switches which photos the grid shows. The backend rebuilds its index, so the grid is
   *  reloaded from scratch rather than patched. */
  async setView(view: GridView): Promise<void> {
    try {
      await api.setGridView(view);
      await this.refresh();
    } catch (e) {
      this.reportError(e);
    }
  }
```

Import `type GridView` from `./api`. The store's reload method is `refresh()` (`library.svelte.ts:67`), which is what the snippet calls.

Also widen the initial value at `library.svelte.ts:9`, or TypeScript rejects it against the extended `GridInfo`:

```ts
  info = $state<GridInfo>({ version: -1, len: 0, sections: [], starredCount: 0, view: 'all' });
```

- [ ] **Step 3: Add the Starred row**

In `ui/src/components/FolderTree.svelte`, add above the `{#each roots ...}` block:

```svelte
  <button
    class="root starred"
    class:active={library.info.view === 'starred'}
    onclick={() => library.setView('starred')}
    title="Photos rated in another program"
  >
    <span class="name">★ Starred</span>
    <span class="count">({library.info.starredCount})</span>
  </button>
```

and make a folder click return to the All view — in the existing folder-row `onclick`, call `library.setView('all')` before `onjump(row.folderId)` when the current view is `starred`:

```svelte
        onclick={() => {
          if (library.info.view === 'starred') library.setView('all');
          onjump(row.folderId);
        }}
```

Add to the `<style>` block:

```css
  .starred.active { background: #ffffff14; }
```

The row is shown even when the count is zero: its absence would be indistinguishable from the feature not existing, leaving someone who expected their Picasa stars with nothing to look at and no explanation.

- [ ] **Step 4: Run the UI gates**

Run: `npm run check` — expect 0 errors and 0 warnings.
Run: `npm test` — expect all tests passing.

- [ ] **Step 5: Document the upgrade, and extend the smoke checklist**

**First**, add an upgrade note to `README.md`, immediately after the Install section's table and before the "photon is not code-signed" heading. Without this the checklist item below references a README line that does not exist, and the spec's §4 claim that "the README says so" is false:

```markdown
### Upgrading to a version with Starred photos

photon reads star ratings from each photo's embedded XMP as it indexes it. A library built
by an earlier version has no ratings recorded, and photon will not re-read a file whose size
and modification time have not changed — so Starred would stay empty.

Delete the library and let photon rebuild it. It lives in your user data directory
(`photon/library.db`); your photos are untouched, since photon never writes to watched
folders. Rebuilding re-reads every photo, ratings included.
```

**Then** append to the manual smoke checklist:

```markdown
- [ ] On a freshly built library, photos rated in Picasa show under Starred with a matching count, once the first scan has finished.
- [ ] Clicking Starred shows only starred photos; clicking any folder returns to the full library at that folder.
- [ ] A library carried over from v0.2.0 shows no stars until it is deleted and rebuilt, as the README's upgrade note says.
- [ ] After a full scan, no photo file's modification time has changed — photon reads ratings and never writes them.
```

That last item is the one no automated test can honestly make, and it is the one that verifies the invariant this whole feature is built around: photon reads ratings and never writes them.

- [ ] **Step 6: Commit**

```bash
npm run check
npm test
git add ui/src README.md
git commit -m "feat(ui): a Starred row in the sidebar, switching the grid view"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §1 Read-only; invariant | Global constraints; Task 5 checklist item |
| §2 Nullable column, migration | Task 2 (steps 3, 8) |
| §3 Packet locations, bounded read, quick-xml, values | Task 1 |
| §4 Scan-time rating; fresh library; mtime limitation | Task 3 (scan); Task 5 (README) |
| §5 View mode, index rebuilt with a filter | Task 4 |
| §6 Sidebar row, count in both views | Task 4 (`starred_count`), Task 5 (row) |
| §7 Error handling: malformed XMP, unreadable file | Task 1 (`None` contract) |
| §8 Testing | Tasks 1–4 tests; Task 5 checklist |
| §9 Success criteria | Task 5 checklist |

**Deliberate deviation from §8:** the spec lists fixtures for JPEG, PNG, WebP and GIF. This plan builds JPEG, PNG and GIF fixtures plus a byte-level cap test, and no WebP fixture. The reader is container-agnostic — it scans a bounded prefix for the packet, which is plain text in all four containers — so the per-container risk is low, and the project's `webp` crate is decode-oriented, making a hand-built RIFF fixture disproportionate. **Cost if wrong:** a WebP whose packet sits somewhere unexpected goes unread, surfacing as a missing star rather than a failure.

**Type consistency:** `rating` flows as `Option<u8>` through `ImageMeta` → `NewItem` → SQL `?11` (insert) and `?12` (update), and is never read back as a number — it reaches the UI only as the derived `GridEntry.starred: bool` and `GridInfo.starredCount: usize`. `GridView` is defined once in `photon_core::grid`, serialised `camelCase` (`all` / `starred`), and consumed by the TypeScript union of the same two strings.

**Verified against the source, not assumed.** Five things this plan originally hedged on were checked and pinned instead:

- `quick-xml` 0.42's event API — slice `Reader::from_str` and `read_event()` (`reader/slice_reader.rs:27,75`), `Attribute.key` as a public `QName` and `unescape_value()` (`events/attributes.rs:33,264`), and the *absence* of `BytesText::unescape`, which is why the text branch reads bytes directly.
- `photon-core`'s `testutil` is private (`mod testutil;`, lib.rs:15) and the crate has no `[features]` section — so Task 4 cannot borrow its fixtures or hide a helper behind a feature, and builds its scenario through the public `Library` API. As first drafted, that task would not have compiled.
- `CmdResult<T> = Result<T, AppError>` (commands.rs:21), with `?` doing the conversion.
- `startup` defines `shutting_down()` as a closure and calls it as `if shutting_down() { return; }`.
- The store's reload method is `refresh()` (`library.svelte.ts:67`), and its `info` initialiser at line 9 must widen or TypeScript rejects it.

**Known unverifiable-until-run:** whether the GIF fixture's 13-byte split point matches what `image`'s encoder emits for these solid images. If it does not, Task 1 Step 7 fails loudly — the packet simply is not found — rather than passing for the wrong reason.
