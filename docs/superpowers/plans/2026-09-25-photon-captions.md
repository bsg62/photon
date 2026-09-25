# Captions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read the caption a photo carries (XMP `dc:description`, else IPTC 2:120), store it, find it with search, and show it under the photo in the viewer and slideshow and in the info panel.

**Architecture:** The bounded prefix read that already yields keywords (`keywords.rs`) returns the caption too; the scanner stores it in a new `items.caption` column (schema 18) through every item writer, and `EXIF_VERSION` 3 backfills existing photos. Search adds it as a haystack; `ViewerItem.caption` carries it to the UI, where a pure function formats the line under the photo.

**Tech Stack:** Rust (quick-xml 0.41, rusqlite), Tauri 2, Svelte 5 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-25-photon-captions-design.md` - read it first.

## Global Constraints

- Branch `feat/captions`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **photon never writes a photo.** Captions are read only; no task adds a writer.
- **EXIF `ImageDescription` is never read.**
- Sources in order: XMP `dc:description` (`x-default`, else first `rdf:li`), then IPTC 2:120 (first record). Trimmed; empty is `None`; capped at 2,000 characters on a character boundary.
- No new dependency. No native library.
- **A new test must be demonstrated to fail with its change reverted**, by an exact replacement, and the probe recorded in the commit message. A compile error is not proof.
- The TypeScript mirror (`ui/src/lib/api.ts`) changes in the same commit as the Rust struct it mirrors.
- A schema bump updates the hardcoded version literals, not `MIGRATIONS.len()`.
- Case-insensitive matching stays in Rust.
- The Rust gate before every commit: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`. The UI gate on commits touching `ui/`: `npm run check` (0 errors, 0 warnings), `npm test`.
- Never launch the GUI to verify.

## Two rulings against the spec's text

- **`lib.item_caption(id)` instead of an `Item.caption` field.** The spec says `Item` gains `caption` "and every query that builds an `Item` selects it". Only `viewer_item` needs the caption, and `Item` is built by several queries and used everywhere; one reader with one query delivers the same `ViewerItem.caption` with no risk to the others. Task 2 updates the spec line.
- **No `pictureChanged` pin test.** `picture.ts`'s `Picture` is `Pick<ViewerItem, 'thumbKey' | 'width' | 'height' | 'orientation' | 'thumbState'>`: a caption cannot reach the comparison without changing that type, and a test passing a caption would not typecheck. Task 5 updates the spec's testing line.

## Review Focus

1. **A camera that writes junk into EXIF `ImageDescription`** must not caption the photo. Pinned in Task 1 (`exif_image_description_is_not_a_caption` - the reader never looks there; a fixture with only EXIF ImageDescription yields `None`).
2. **A caption with a multi-byte character at the 2,000-character cut** must not panic or split the character. Pinned in Task 1 (`a_long_caption_is_cut_on_a_character_boundary`).
3. **An upgraded library** must gain captions on its first scan without the photo file changing, and exactly once. Pinned in Task 2 (`an_unchanged_photo_gains_its_caption_from_the_backfill`).
4. **A photo whose caption was removed from the file** must lose it on the next scan (the file changed, so `update_items` rewrites the column to NULL). Pinned in Task 2 (`update_items_rewrites_the_caption_including_to_none`).
5. **A caption of only whitespace or line breaks** must show nothing under the photo (no empty glass strip). Pinned in Task 1 (whitespace-only is `None`) and Task 5 (`photoCaptionLine` returns `null`).

---

### Task 1: Read captions from XMP and IPTC

**Files:**
- Modify: `crates/photon-core/src/xmp.rs` (new `description_from_xml`, tests)
- Modify: `crates/photon-core/src/iptc.rs` (walker generalised by dataset, new `caption_in`, tests)
- Modify: `crates/photon-core/src/keywords.rs` (new `Embedded`, `read_embedded`, `embedded_in`, `MAX_CAPTION_CHARS`, tests)
- Modify: `crates/photon-core/src/testutil.rs` (two fixture helpers, `ExifSpec.description`)

**Interfaces:**
- Produces: `pub struct Embedded { pub keywords: Vec<String>, pub caption: Option<String> }` (derive `Clone, Debug, Default, PartialEq`); `pub fn read_embedded(path: &Path) -> Embedded`; `pub fn embedded_in(prefix: &[u8]) -> Embedded`; `pub const MAX_CAPTION_CHARS: usize = 2000;` in `keywords.rs`. `read_keywords`/`keywords_in` stay and return `.keywords`. `xmp::description_from_xml(&str) -> Option<String>`, `iptc::caption_in(&[u8]) -> Option<String>`. Consumed by Task 2.

- [ ] **Step 1: Fixture helpers** - in `testutil.rs`, beside `iptc_app13` and `xmp_packet_with_subjects`:

```rust
/// An APP13 payload carrying the given IIM record-2 datasets in order, e.g.
/// `&[(120, b"caption"), (25, b"keyword")]`.
pub fn iptc_app13_datasets(datasets: &[(u8, &[u8])]) -> Vec<u8> {
    let mut iim = Vec::new();
    for (dataset, value) in datasets {
        iim.extend_from_slice(&[0x1C, 2, *dataset]);
        iim.extend_from_slice(&(value.len() as u16).to_be_bytes());
        iim.extend_from_slice(value);
    }
    let mut payload = b"Photoshop 3.0\0".to_vec();
    payload.extend_from_slice(b"8BIM");
    payload.extend_from_slice(&0x0404u16.to_be_bytes());
    payload.extend_from_slice(&[0, 0]);
    payload.extend_from_slice(&(iim.len() as u32).to_be_bytes());
    payload.extend_from_slice(&iim);
    if iim.len() % 2 == 1 {
        payload.push(0);
    }
    payload
}

/// An XMP packet whose `dc:description` `rdf:Alt` holds `(xml:lang, text)` entries in order,
/// the text XML-escaped. A `None` language writes the `rdf:li` without the attribute.
pub fn xmp_packet_with_description(entries: &[(Option<&str>, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(lang, text)| {
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            match lang {
                Some(lang) => format!(r#"<rdf:li xml:lang="{lang}">{escaped}</rdf:li>"#),
                None => format!("<rdf:li>{escaped}</rdf:li>"),
            }
        })
        .collect();
    xmp_packet_with(
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        &format!("<dc:description><rdf:Alt>{items}</rdf:Alt></dc:description>"),
    )
}
```

And give `ExifSpec` an `ImageDescription`: add, after `modified`,

```rust
    /// `ImageDescription` (IFD0 0x010E), ASCII - what cameras fill with their own name.
    pub description: Option<&'a str>,
```

and in the IFD0 builder push `ascii_entry(0x010e, description)` **before** the `0x010f` make entry, keeping IFD0's tags in ascending order as TIFF requires.

Make `iptc_app13` delegate to it (`iptc_app13_datasets(&keywords.iter().map(|k| (25, *k)).collect::<Vec<_>>())`) so the two cannot drift.

- [ ] **Step 2: Failing tests.**

In `xmp.rs` `mod tests` (import `xmp_packet_with_description`, `xmp_packet_with_subjects` from testutil):

```rust
    #[test]
    fn the_default_language_description_is_the_caption() {
        let xml = xmp_packet_with_description(&[(Some("de"), "Oma"), (Some("x-default"), "Grandma")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Grandma"));
    }

    #[test]
    fn without_a_default_language_the_first_entry_is_the_caption() {
        let xml = xmp_packet_with_description(&[(Some("de"), "  Oma  "), (Some("fr"), "Mamie")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Oma"));
    }

    #[test]
    fn a_description_with_a_character_reference_is_read_whole() {
        let xml = xmp_packet_with_description(&[(Some("x-default"), "Tom & Jerry")]);
        assert_eq!(description_from_xml(&xml).as_deref(), Some("Tom & Jerry"));
    }

    #[test]
    fn keywords_and_empty_descriptions_are_not_captions() {
        assert_eq!(description_from_xml(&xmp_packet_with_subjects(&["beach"])), None);
        let blank = xmp_packet_with_description(&[(Some("x-default"), "   ")]);
        assert_eq!(description_from_xml(&blank), None);
    }
```

In `iptc.rs` `mod tests` (import `iptc_app13_datasets`, `jpeg_with_segments`):

```rust
    #[test]
    fn the_caption_is_dataset_2_120_beside_the_keywords() {
        let app13 = iptc_app13_datasets(&[(25, b"lake"), (120, b"caf\xe9 at dawn"), (120, b"second")]);
        let jpeg = jpeg_with_segments(8, 8, &[(0xED, &app13)]);
        assert_eq!(caption_in(&jpeg).as_deref(), Some("café at dawn"), "Latin-1, first record");
        assert_eq!(keywords_in(&jpeg), ["lake"], "a caption is not a keyword");
    }

    #[test]
    fn no_caption_dataset_is_no_caption() {
        let jpeg = jpeg_with_segments(8, 8, &[(0xED, &iptc_app13_datasets(&[(25, b"lake")]))]);
        assert_eq!(caption_in(&jpeg), None);
        assert_eq!(caption_in(&jpeg_bytes(8, 8)), None);
    }
```

In `keywords.rs` `mod tests` (import `iptc_app13_datasets`, `xmp_packet_with_description`, `jpeg_with_segments`, `jpeg_with_exif_spec`, `ExifSpec`):

```rust
    fn jpeg_with(xmp: Option<String>, iptc: &[(u8, &[u8])]) -> Vec<u8> {
        let mut segments: Vec<(u8, Vec<u8>)> = Vec::new();
        if let Some(packet) = xmp {
            let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            app1.extend_from_slice(packet.as_bytes());
            segments.push((0xE1, app1));
        }
        if !iptc.is_empty() {
            segments.push((0xED, iptc_app13_datasets(iptc)));
        }
        let refs: Vec<(u8, &[u8])> = segments.iter().map(|(m, b)| (*m, b.as_slice())).collect();
        jpeg_with_segments(8, 8, &refs)
    }

    #[test]
    fn xmp_wins_over_iptc_and_iptc_alone_is_enough() {
        let both = jpeg_with(
            Some(xmp_packet_with_description(&[(Some("x-default"), "From Lightroom")])),
            &[(120, b"From Picasa")],
        );
        assert_eq!(embedded_in(&both).caption.as_deref(), Some("From Lightroom"));
        let iptc_only = jpeg_with(None, &[(120, b"From Picasa")]);
        assert_eq!(embedded_in(&iptc_only).caption.as_deref(), Some("From Picasa"));
    }

    #[test]
    fn a_whitespace_caption_is_none_and_keywords_still_come_through() {
        let jpeg = jpeg_with(None, &[(120, b" \n\t "), (25, b"lake")]);
        let embedded = embedded_in(&jpeg);
        assert_eq!(embedded.caption, None);
        assert_eq!(embedded.keywords, ["lake"]);
    }

    #[test]
    fn a_long_caption_is_cut_on_a_character_boundary() {
        // 1,999 ASCII characters, then a two-byte `é` as character 2,000, then more: a cut by
        // bytes would land inside the `é` and panic; the cut is by characters.
        let text = format!("{}é tail", "a".repeat(MAX_CAPTION_CHARS - 1));
        let jpeg = jpeg_with(Some(xmp_packet_with_description(&[(Some("x-default"), &text)])), &[]);
        let caption = embedded_in(&jpeg).caption.unwrap();
        assert_eq!(caption.chars().count(), MAX_CAPTION_CHARS);
        assert!(caption.ends_with('é'));
    }

    #[test]
    fn exif_image_description_is_not_a_caption() {
        // Cameras write their own name there; a file with nothing else has no caption.
        let spec = ExifSpec {
            description: Some("OLYMPUS DIGITAL CAMERA"),
            ..ExifSpec::default()
        };
        let jpeg = jpeg_with_exif_spec(8, 8, &spec);
        assert!(
            jpeg.windows(22).any(|w| w == b"OLYMPUS DIGITAL CAMERA"),
            "the fixture really carries the EXIF text"
        );
        assert_eq!(embedded_in(&jpeg).caption, None);
    }
```

This test is a **pin** - no reader looks at EXIF, so it passes before the change too; record that in the commit.

- [ ] **Step 3: Run** `cargo test -p photon-core --lib xmp iptc keywords` - compile errors (new functions). Not the proof; Step 6 is.

- [ ] **Step 4: Implement.**

`xmp.rs`, after `subjects_from_xml` - same event walk and the same `&str` comparisons the file already uses (`e.name().as_ref() == "…"`, `attr.key.as_ref() == "…"`):

```rust
/// The caption in an XMP packet: `dc:description`'s `rdf:Alt` entry whose `xml:lang` is
/// `x-default`, or else its first entry, trimmed. `None` when there is no non-empty one.
///
/// `x-default` first because that is the entry a tool writes when the user typed one caption;
/// the language-tagged ones are translations. Resolves references the way
/// `subjects_from_xml` does, so a caption with an ampersand arrives whole.
pub fn description_from_xml(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut in_description = false;
    // Some(whether this entry is x-default) while inside an rdf:li of dc:description.
    let mut item: Option<bool> = None;
    let mut current = String::new();
    let mut first: Option<String> = None;
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "dc:description" => in_description = true,
                "rdf:li" if in_description => {
                    let is_default = e.attributes().flatten().any(|attr| {
                        attr.key.as_ref() == "xml:lang" && attr.value.as_ref() == "x-default"
                    });
                    item = Some(is_default);
                    current.clear();
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                "dc:description" => in_description = false,
                "rdf:li" => {
                    if let Some(is_default) = item.take() {
                        let text = current.trim();
                        if !text.is_empty() {
                            if is_default {
                                return Some(text.to_string());
                            }
                            first.get_or_insert_with(|| text.to_string());
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if item.is_some() => current.push_str(t.as_ref()),
            Ok(Event::CData(t)) if item.is_some() => current.push_str(&t),
            Ok(Event::GeneralRef(r)) if item.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    current.push(c);
                } else if let Some(text) = quick_xml::escape::resolve_predefined_entity(&r) {
                    current.push_str(text);
                }
            }
            _ => {}
        }
    }
    first
}
```

If a comparison's types differ from what compiles in 0.41 (e.g. `attr.value` needs `.as_ref()` into `&str` differently), follow `rating_from_xml`'s attribute handling exactly and note it.

`iptc.rs`: rename `iim_keywords(bytes, out)` to `iim_values(bytes: &[u8], dataset: u8, out: &mut Vec<String>)`, comparing `dataset == KEYWORDS_DATASET` → `dataset_here == dataset` (keep trimming and empty-dropping); add `const CAPTION_DATASET: u8 = 120;` beside `KEYWORDS_DATASET` with a comment ("Caption-Abstract, where Picasa writes the caption"); `keywords_in` passes `KEYWORDS_DATASET`; new:

```rust
/// The first IPTC caption (2:120, Caption-Abstract) in the leading bytes of a JPEG, decoded
/// like keywords. Picasa writes a caption typed under a photo here.
pub fn caption_in(prefix: &[u8]) -> Option<String> {
    let mut captions = Vec::new();
    for payload in app13_segments(prefix) {
        for resource in photoshop_resources(payload) {
            iim_values(resource, CAPTION_DATASET, &mut captions);
        }
    }
    captions.into_iter().next()
}
```

`keywords.rs`: update the module doc to "The keywords and caption embedded in a photo file, from XMP and IPTC." and add:

```rust
/// A caption longer than this is cut, by characters: it is one more search haystack in every
/// search, and no caption a person types is anywhere near it.
pub const MAX_CAPTION_CHARS: usize = 2000;

/// What a photo file says about itself in text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Embedded {
    pub keywords: Vec<String>,
    /// XMP `dc:description`, else IPTC 2:120. Never EXIF `ImageDescription`: cameras fill it
    /// with their own name, and trusting it would caption every photo from that camera.
    pub caption: Option<String>,
}

/// The keywords and caption in `path`, from one bounded read. Never fails: an unreadable
/// file has neither.
pub fn read_embedded(path: &Path) -> Embedded {
    let Ok(mut file) = File::open(path) else {
        return Embedded::default();
    };
    let mut buf = Vec::new();
    if file
        .by_ref()
        .take(crate::xmp::MAX_PREFIX as u64)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Embedded::default();
    }
    embedded_in(&buf)
}

/// [`read_embedded`] over bytes already read.
pub fn embedded_in(prefix: &[u8]) -> Embedded {
    let packet = crate::xmp::packet_in(prefix);
    let caption = packet
        .as_deref()
        .and_then(crate::xmp::description_from_xml)
        .or_else(|| crate::iptc::caption_in(prefix))
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .map(|c| c.chars().take(MAX_CAPTION_CHARS).collect());
    Embedded { keywords: keywords_from(packet.as_deref(), prefix), caption }
}
```

Move today's `keywords_in` body into a private `keywords_from(packet: Option<&str>, prefix: &[u8]) -> Vec<String>` (so the packet is found once), and make `keywords_in(prefix)` call `keywords_from(crate::xmp::packet_in(prefix).as_deref(), prefix)` and `read_keywords(path)` return `read_embedded(path).keywords`.

- [ ] **Step 5: Run** `cargo test -p photon-core --lib -- xmp iptc keywords`; all pass (existing keyword tests included).

- [ ] **Step 6: Probe**, each exact and restored:
  - In `description_from_xml`, replace `if is_default {` with `if false {`: `the_default_language_description_is_the_caption` fails.
  - Replace `first.get_or_insert_with(|| text.to_string());` with `first = Some(text.to_string());`: `without_a_default_language_the_first_entry_is_the_caption` fails.
  - Replace `const CAPTION_DATASET: u8 = 120;` with `= 121;`: `the_caption_is_dataset_2_120_beside_the_keywords` fails.
  - Replace `captions.into_iter().next()` with `captions.into_iter().last()`: the same test fails.
  - In `embedded_in`, swap the two sources (`crate::iptc::caption_in(prefix)` first, XMP in `or_else`): `xmp_wins_over_iptc_and_iptc_alone_is_enough` fails.
  - Delete `.filter(|c| !c.is_empty())`: `a_whitespace_caption_is_none_and_keywords_still_come_through` fails.
  - Replace `.take(MAX_CAPTION_CHARS)` with `.take(usize::MAX)`: `a_long_caption_is_cut_on_a_character_boundary` fails.

- [ ] **Step 7: Gate and commit** `feat(metadata): read a photo's caption from XMP and IPTC` (files: xmp.rs, iptc.rs, keywords.rs, testutil.rs), recording the probes and the EXIF pin.

---

### Task 2: Store captions, and backfill them

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (migration 18, version literals, migration test)
- Modify: `crates/photon-core/src/library/mod.rs` (version literals)
- Modify: `crates/photon-core/src/library/items.rs` (`NewItem.caption`, three writers, `item_caption`, tests)
- Modify: `crates/photon-core/src/metadata.rs` (`EXIF_VERSION` 3 and its doc)
- Modify: `crates/photon-core/src/scanner.rs` (`describe` uses `read_embedded`; tests)
- Modify: every `NewItem { ... }` literal in the workspace (`caption: None`) - `crates/photon-core/src/testutil.rs`, `crates/photon-core/benches/grid.rs`, `crates/photon-core/src/library/tags.rs`, `crates/photon-core/src/library/items.rs`, `crates/photon-app/src/commands.rs`, and whatever else the compiler lists
- Modify: `docs/superpowers/specs/2026-09-25-photon-captions-design.md` (the `item_caption` ruling)

**Interfaces:**
- Consumes: `keywords::read_embedded`, `Embedded` (Task 1).
- Produces: column `items.caption TEXT`; `NewItem.caption: Option<String>`; `pub fn item_caption(&self, id: i64) -> Result<Option<String>>` on `Library`. Consumed by Tasks 3-4.

- [ ] **Step 1: Failing tests.**

`schema.rs`, beside `migration_17_keeps_existing_albums_as_photons_own`:

```rust
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
        conn.execute("INSERT INTO watched_folders (id, path) VALUES (1, '/p')", []).unwrap();
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
        assert_eq!(version, 18);
    }
```

(If the `items` insert needs more NOT NULL columns at schema 17, add them with harmless values; read `MIGRATIONS` to see.)

`items.rs` `mod tests`:

```rust
    #[test]
    fn insert_items_stores_the_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut captioned = new_item(folder, "/p/a.jpg", 1);
        captioned.caption = Some("Grandma's 80th".into());
        let ids = lib.insert_items(&[captioned, new_item(folder, "/p/b.jpg", 2)]).unwrap();
        assert_eq!(lib.item_caption(ids[0]).unwrap().as_deref(), Some("Grandma's 80th"));
        assert_eq!(lib.item_caption(ids[1]).unwrap(), None);
    }

    #[test]
    fn update_items_rewrites_the_caption_including_to_none() {
        // A file edited to remove its caption is a changed file; its row must lose it.
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut it = new_item(folder, "/p/a.jpg", 1);
        it.caption = Some("old".into());
        let id = lib.insert_items(&[it.clone()]).unwrap()[0];
        it.caption = Some("new".into());
        lib.update_items(&[(id, it.clone())]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("new"));
        it.caption = None;
        lib.update_items(&[(id, it)]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap(), None);
    }

    #[test]
    fn update_item_meta_writes_the_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut it = new_item(folder, "/p/a.jpg", 1);
        let id = lib.insert_items(&[it.clone()]).unwrap()[0];
        it.caption = Some("backfilled".into());
        lib.update_item_meta(&[(id, it)]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("backfilled"));
    }
```

(`NewItem` must derive `Clone` for `it.clone()`; check - if it does not, build a second literal instead of cloning.)

`scanner.rs` `mod tests`, beside the camera backfill test (the one calling `forget_metadata_for_test`):

```rust
    #[test]
    fn a_new_photo_is_stored_with_its_caption() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let app13 = crate::testutil::iptc_app13_datasets(&[(120, b"Grandma's 80th")]);
        let a = write_file(&root, "a.jpg", &crate::testutil::jpeg_with_segments(4, 2, &[(0xED, &app13)]));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("Grandma's 80th"));
    }

    #[test]
    fn an_unchanged_photo_gains_its_caption_from_the_backfill() {
        // A library indexed under EXIF_VERSION 2 has no caption column filled; the photo is
        // unchanged, so only the backfill can read it - once.
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let app13 = crate::testutil::iptc_app13_datasets(&[(120, b"Grandma's 80th")]);
        let a = write_file(&root, "a.jpg", &crate::testutil::jpeg_with_segments(4, 2, &[(0xED, &app13)]));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        lib.writer()
            .execute("UPDATE items SET caption = NULL, exif_version = 2 WHERE id = ?1", [id])
            .unwrap();

        let report = scan(&lib, &watched, 2);
        assert_eq!((report.unchanged, report.changed, report.enriched), (1, 0, 1));
        assert!(report.touched_rows());
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("Grandma's 80th"));
        assert_eq!(scan(&lib, &watched, 3).enriched, 0, "read once, not on every scan");
    }
```

(Use `key`, `photos_root`, `scan`, `write_file` as the camera backfill test does; if `jpeg_with_segments(4, 2, …)` is not a decodable JPEG the scanner accepts, use whatever that test uses to build its file and add the APP13 segment to it.)

- [ ] **Step 2: Run** `cargo test -p photon-core --lib caption migration_18` - compile errors; not the proof.

- [ ] **Step 3: Implement.**
  - `schema.rs`: append

    ```rust
        r#"
    -- The caption the photo carries (XMP dc:description, else IPTC 2:120), read by the
    -- scanner. NULL until a scan has read the file under EXIF_VERSION 3; the backfill
    -- re-reads every unchanged photo once to fill it in.
    ALTER TABLE items ADD COLUMN caption TEXT;
    "#,
    ```

    and move the version literals: `sed -i 's/assert_eq!(version, 17);/assert_eq!(version, 18);/' crates/photon-core/src/library/schema.rs crates/photon-core/src/library/mod.rs` and `sed -i 's/supported: 17$/supported: 18/' crates/photon-core/src/library/mod.rs`; then `grep -n 'version, 17\|supported: 17' crates/photon-core/src/library/` must find nothing.
  - `metadata.rs`: `pub const EXIF_VERSION: i64 = 3;`, and add to its doc: "3 is the first generation that reads the photo's caption (`keywords::read_embedded`); the bump is what captions photos indexed under 2."
  - `items.rs`: `NewItem` gains, after `tags`:

    ```rust
        /// The caption the file carries (`keywords::read_embedded`), `None` for none.
        pub caption: Option<String>,
    ```

    `insert_items`: add `caption` to the column list and `?20` to `VALUES` before the `coalesce(...)` hidden expression (keep `hidden` last), and `it.caption` as the 20th param. `update_items`: add `caption = ?20` to the SET list and `it.caption` as the 20th param. `update_item_meta`: add `caption = ?11` and `it.caption` as the 11th param, and add "the caption" to its doc comment's list of what it rewrites. New reader beside `item`:

    ```rust
        /// The photo's caption, for the viewer. Not a field of `Item`: only the viewer reads it,
        /// and every other query that builds an `Item` would carry it for nothing.
        pub fn item_caption(&self, id: i64) -> Result<Option<String>> {
            Ok(self
                .reader()?
                .query_row("SELECT caption FROM items WHERE id = ?1", params![id], |r| r.get(0))
                .optional()?
                .flatten())
        }
    ```

    (import `OptionalExtension` if `items.rs` does not already).
  - `scanner.rs` `describe`: `let embedded = read_embedded(entry.path());` and `tags: embedded.keywords, caption: embedded.caption,`; import `keywords::read_embedded` in place of `read_keywords` if nothing else uses it.
  - Every other `NewItem { ... }` literal: `caption: None,` (let `cargo build --workspace --all-targets` list them).
  - Spec: in "Storage (migration 18)", replace "`Item` gains `caption` and every query that builds an `Item` selects it." with "`Library::item_caption(id)` reads it for the viewer; `Item` does not carry it, since only the viewer needs it."

- [ ] **Step 4: Run** `cargo test --workspace`; all pass.

- [ ] **Step 5: Probe**, exact, restored:
  - Remove `caption` from `insert_items` (the column, the `?20` and the param - one coherent revert): `insert_items_stores_the_caption` fails.
  - Remove `caption = ?20` and its param from `update_items`: `update_items_rewrites_the_caption_including_to_none` fails.
  - Remove `caption = ?11` and its param from `update_item_meta`: `update_item_meta_writes_the_caption` and `an_unchanged_photo_gains_its_caption_from_the_backfill` fail.
  - Replace `pub const EXIF_VERSION: i64 = 3;` with `= 2;`: `an_unchanged_photo_gains_its_caption_from_the_backfill` fails (nothing re-reads it).
  - In `describe`, replace `caption: embedded.caption,` with `caption: None,`: `a_new_photo_is_stored_with_its_caption` fails.
  - Drop the migration entry: `migration_18_leaves_every_existing_photo_without_a_caption` fails.

- [ ] **Step 6: Gate and commit** `feat(library): store captions, schema 18, and backfill them with EXIF_VERSION 3`, recording the probes.

---

### Task 3: Search captions

**Files:**
- Modify: `crates/photon-core/src/library/items.rs` (`search_entries`, a test)

**Interfaces:**
- Consumes: `items.caption` (Task 2).

- [ ] **Step 1: Failing test** - beside `search_finds_a_photo_by_its_camera_lens_keyword_and_date`:

```rust
    #[test]
    fn search_finds_a_photo_by_a_word_of_its_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut captioned = new_item(folder, "/p/IMG_1.jpg", 1);
        captioned.caption = Some("Grandma's 80th, Lisbon".into());
        let ids = lib.insert_items(&[captioned, new_item(folder, "/p/IMG_2.jpg", 2)]).unwrap();
        let found = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q).unwrap().into_iter().map(|e| e.id).collect()
        };
        assert_eq!(found("lisbon"), vec![ids[0]], "a caption word, any case");
        assert_eq!(found("grandma LISBON"), vec![ids[0]], "words AND across the caption");
        assert!(found("madrid").is_empty());
    }
```

(Use the existing test's way of running a search - if it is not `entries_for(GridView::Search, q)` with those types, copy that test's call.)

- [ ] **Step 2: Run** it; it fails (no caption haystack).

- [ ] **Step 3: Implement** - in `search_entries`, add `, i.caption` at the end of the selected columns (after the `group_concat` subquery) and, after the tags haystack:

```rust
                if let Some(caption) = r.get::<_, Option<String>>(base + 9)? {
                    haystacks.push(caption);
                }
```

Update the doc comment on `GridView::Search` in `crates/photon-core/src/grid.rs` ("file or folder name, camera, keywords or date") to include the caption, and CLAUDE.md's Search paragraph list ("file and folder name, make, model, lens, ... keywords through `EFFECTIVE_TAGS`, and the capture date") to add "the caption".

- [ ] **Step 4: Run** `cargo test -p photon-core --lib search`; passes.

- [ ] **Step 5: Probe** - delete the three lines pushing the caption haystack: the new test fails. Restore.

- [ ] **Step 6: Gate and commit** `feat(search): a photo's caption is a search haystack` (items.rs, grid.rs, CLAUDE.md), recording the probe.

---

### Task 4: `ViewerItem.caption`

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (`ViewerItem`, `viewer_item`, a test)
- Modify: `ui/src/lib/api.ts` (`ViewerItem.caption`)
- Modify: `crates/xtask/screenshots/mock.js` (`viewerItem` gains a caption)

**Interfaces:**
- Consumes: `Library::item_caption` (Task 2).
- Produces: `ViewerItem.caption: Option<String>` / TS `caption: string | null`. Consumed by Task 5.

- [ ] **Step 1: Failing test** - in `commands.rs` tests:

```rust
    #[test]
    fn viewer_item_reports_the_caption() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        assert_eq!(viewer_item(&f.engine, id).unwrap().caption, None);
        f.engine
            .lib
            .writer()
            .execute("UPDATE items SET caption = 'Grandma' WHERE id = ?1", [id])
            .unwrap();
        assert_eq!(viewer_item(&f.engine, id).unwrap().caption.as_deref(), Some("Grandma"));
    }
```

(If `Library::writer` is not reachable from photon-app tests, set the caption through `update_item_meta` with a `NewItem` built the way `viewer_item_carries_camera_keywords_faces_and_albums` builds one, with `caption: Some("Grandma".into())`.)

- [ ] **Step 2: Run** it; compile error, then (after the field exists) a failing assertion.

- [ ] **Step 3: Implement.** `ViewerItem` gains, after `tags`:

```rust
    /// The caption the photo carries, if any. Shown under the photo and in the info panel.
    pub caption: Option<String>,
```

`viewer_item` fills it with `engine.lib.item_caption(item.id)?`. `api.ts`'s `ViewerItem` gains `/** The caption the photo carries (XMP or IPTC), shown under it. */ caption: string | null;` beside `tags`. `mock.js`'s `viewerItem(id)` gains `caption: id === 3 ? "Grandma's 80th, on the terrace in Lisbon" : null,` so the default viewer shot (id 3) shows one.

- [ ] **Step 4: Run** `cargo test --workspace`, `npm run check`, `npm test`; all pass.

- [ ] **Step 5: Probe** - replace `engine.lib.item_caption(item.id)?` with `None`: the test fails. Restore.

- [ ] **Step 6: Commit** `feat(app): ViewerItem carries the caption` (commands.rs, api.ts, mock.js), recording the probe.

---

### Task 5: Show the caption

**Files:**
- Create: `ui/src/lib/photo-caption.ts`, `ui/src/lib/photo-caption.test.ts`
- Modify: `ui/src/components/Viewer.svelte` (the line under the photo; the info panel section; styles)
- Modify: `README.md` (a Captions paragraph; smoke checklist)
- Modify: `docs/superpowers/specs/2026-09-25-photon-captions-design.md` (the pictureChanged ruling)

**Interfaces:**
- Consumes: `ViewerItem.caption` (Task 4).
- Produces: `photoCaptionLine(caption: string | null): string | null`.

- [ ] **Step 1: Failing tests** - `ui/src/lib/photo-caption.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { photoCaptionLine } from './photo-caption';

describe('photoCaptionLine', () => {
  it('is the caption on one line, runs of whitespace and line breaks collapsed', () => {
    expect(photoCaptionLine("Grandma's 80th,\n  Lisbon")).toBe("Grandma's 80th, Lisbon");
    expect(photoCaptionLine('  Lisbon\t\r\n')).toBe('Lisbon');
  });

  it('is nothing for no caption or only whitespace', () => {
    expect(photoCaptionLine(null)).toBeNull();
    expect(photoCaptionLine('')).toBeNull();
    expect(photoCaptionLine(' \n\t ')).toBeNull();
  });
});
```

- [ ] **Step 2: Run** `npm test -w ui -- src/lib/photo-caption.test.ts`; fails (no module).

- [ ] **Step 3: Implement** `ui/src/lib/photo-caption.ts`:

```ts
/** The caption line under the photo in the viewer: the photo's own caption on one line.
 *  Not `caption.ts`, which builds the viewer's file-name line - the two are different texts
 *  that happen to share a word. Pure, so each rule is pinned by a test. */

/** Line breaks and runs of whitespace become single spaces, because the line under the photo
 *  is clamped to two lines and a break would spend one of them on nothing; the info panel
 *  shows the caption with its breaks. `null` means draw nothing - not an empty strip. */
export function photoCaptionLine(caption: string | null): string | null {
  const line = (caption ?? '').replace(/\s+/g, ' ').trim();
  return line === '' ? null : line;
}
```

- [ ] **Step 4: Run** it; passes.

- [ ] **Step 5: Probe** - replace `.replace(/\s+/g, ' ')` with `.replace(/ +/g, ' ')`: the first test fails. Replace `return line === '' ? null : line;` with `return line;`: the second fails. Restore.

- [ ] **Step 6: Wire the viewer** - `Viewer.svelte`:
  - Script: `import { photoCaptionLine } from '../lib/photo-caption';` and `const captionLine = $derived(item ? photoCaptionLine(item.caption) : null);`.
  - Markup, directly before the `<!-- The star and the caption share one bottom-centred row ... -->` comment and outside `{#if crop.active}`:

    ```svelte
      <!-- The photo's own caption, not the file-name line in the bar. A sibling of .bar, not
           inside it: the slideshow's quiet state fades the bar, and the caption is what a
           slideshow is watched for. Hidden while cropping, when the space is the crop tool's. -->
      {#if captionLine && !crop.active}
        <p class="photo-caption" title={item?.caption ?? ''}>{captionLine}</p>
      {/if}
    ```
  - Info panel, directly after `<p class="info-path" …>`:

    ```svelte
      {#if item.caption?.trim()}
        <h3>Caption</h3>
        <p class="info-caption">{item.caption.trim()}</p>
      {/if}
    ```
  - Styles, beside `.bar`: add `.photo-caption` to the shared glass rule (`.bar, .zoom, .close, .info, .photo-caption { background: var(--glass); … }`) and

    ```css
      /* Above the bar, centred like it, and clear of the zoom control the same way. Two lines
         at most: a long caption must not climb over the photo; the info panel has it whole. */
      .photo-caption {
        position: absolute; bottom: 64px; left: 50%; transform: translateX(-50%);
        max-width: calc(100% - 428px); margin: 0; padding: var(--s-1) var(--s-3);
        border-radius: var(--r-3); color: var(--text); font-size: var(--t-2); text-align: center;
        display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
      }
      .info-caption { margin: 0; white-space: pre-line; }
    ```

    Use only tokens `ui/src/tokens.css` declares (check `--r-3` and `--s-3` exist; if not, use the nearest declared ones `.bar` uses). Measure the bar's real height against `bottom: 64px` with the screenshots (Step 8) and adjust so the caption sits just above the bar with a small gap.
  - Do **not** add `.photo-caption` to `.quiet .bar, .quiet .zoom, .quiet .close` - it must stay during a quiet slideshow.

- [ ] **Step 7: README** - in "### Camera data, keywords, people and albums", after the paragraph that ends "…and the Picasa albums the photo is in." add:

```markdown
A photo's caption - what you typed under it in Picasa, or its description in Lightroom, Bridge
or digiKam - is shown under the photo in the viewer and during a slideshow, and at the top of
the info panel. photon reads it from the photo's XMP or IPTC; the text many cameras put in EXIF
("OLYMPUS DIGITAL CAMERA") is ignored. Search finds a word of it. photon does not edit
captions: that would mean writing the photo.
```

and in the smoke checklist, after the Picasa album items:

```markdown
- [ ] A photo captioned in Picasa shows its caption under the photo, at the top of the info panel, and during a slideshow after the controls fade.
- [ ] Searching a word of that caption finds the photo.
- [ ] A photo whose camera wrote only an EXIF description (e.g. "OLYMPUS DIGITAL CAMERA") shows no caption.
```

- [ ] **Step 8: Gates and screenshots** - `npm run check` (0/0), `npm test`, the Rust gate. If Chromium is available, `cargo run -p xtask -- screenshots` and look at `viewer-light.png`/`viewer-dark.png` (caption above the bar, not overlapping it, not over the zoom control) and `viewer-info-*.png` (Caption section); otherwise say so in the commit.

- [ ] **Step 9: Spec** - in the spec's Testing section, replace "`pictureChanged` returns false for two items differing only in caption (a pin: the function never compared it, and must not start)." with "`pictureChanged` takes `Pick<ViewerItem, …>` without `caption`, so a caption cannot reach it without a type change; no test is needed."

- [ ] **Step 10: Commit** `feat(ui): the caption under the photo and in the info panel` (photo-caption.ts, its test, Viewer.svelte, README.md, the spec), recording the probes and that the component wiring has no test (no component harness).

---

### Task 6: Whole-branch review

- [ ] Run both gates on the branch head.
- [ ] One independent reviewer on the whole branch, pointed at: the `EXIF_VERSION` bump's cost and correctness on a real library (every unchanged photo re-read once; nothing else reset - `rating`, stars, thumbnails, edits); every item writer carrying the caption; the viewer's `.photo-caption` against the quiet slideshow state, the crop tool and the zoom control; and whether any Review Focus line is left undefended.
- [ ] Fix what it confirms, each fix with a probed test, then push and open a PR only with the user's go-ahead.
