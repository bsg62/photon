# photon — Camera metadata, keywords, people, albums and viewer rotation

**Date:** 2026-09-16
**Status:** Approved design, implemented
**Builds on:** v0.12.2 and the Settings dialog design

## 1. What changes

Six features, designed together because five of them share one mechanism — a view of the
grid selected by an argument — and one schema migration:

1. **Rotate in the viewer.** Display only. Nothing is written.
2. **Camera and lens metadata.** Make, model, lens, focal length, aperture, exposure and
   ISO, read from EXIF into the library and shown in a viewer info panel.
3. **Search on metadata.** The search box matches camera, lens, keywords and the capture
   date as well as the file and folder names.
4. **Picasa face tags, read only.** Names and face rectangles from `.picasa.ini`, shown in
   the info panel and as an overlay, with a People list in the sidebar.
5. **Albums.** photon's own virtual collections, stored in `library.db` only.
6. **Keywords, read only.** XMP `dc:subject` and IPTC keywords from the photo file, with a
   Tags list in the sidebar.

The promise from the set-star design stands unchanged: photon writes nothing but a single
`star=` line into Picasa's INI. Faces and keywords are read; albums live in photon's own
database; rotation is a CSS transform.

### Out of scope

Writing keywords or faces back to the file or the INI; importing Picasa's own albums (the
`[.album:…]` sections); multi-select in the grid; face detection; a map; ratings above one
star; persisting the rotation.

## 2. One index, more views

Everything downstream of `Engine` reads one `GridIndex` built for the current view. Today
that view is `All`, `Starred`, `Recent` or `Search`, and `Search` carries its query beside
the view in `ViewState`. This design generalises the query into an **argument** every
parameterised view interprets its own way:

| `GridView` | argument | rows |
|---|---|---|
| `Search` | the query | names, camera, lens, keywords, date contain any token |
| `Person` | a contact hash | photos with a face of that contact |
| `Album` | an album id | photos in that album |
| `Tag` | a keyword | photos carrying that keyword |

All four use `grid_query` with an extra `AND i.id IN (…)` filter, so a folder is placed by
its oldest *matching* photo and the sidebar's year groups keep agreeing with the grid, the
same property the Starred view has. The filter binds its argument as a parameter;
`entries_filtered` grows a parameter list rather than any caller formatting a string into
SQL.

`GridInfo` reports the view and its argument in typed fields — `searchQuery`, `person`,
`album`, `tag` — because the TypeScript mirror is hand-written and a single untyped `arg`
would push the interpretation into every consumer. `set_view` still clears the argument
when the target view has none, so a query or album cannot ride along into `All`.

Three new commands enter the views: `set_person_view`, `set_album_view`, `set_tag_view`.
They go through `rebuild_or_restore`, so a failed rebuild rolls back exactly as a failed
search does.

## 3. Camera metadata

### 3.1 What is read

`metadata::read_image_meta` already opens the file once for dimensions and EXIF. It now
also reads, all from the primary image's IFDs:

| field | EXIF tag | stored as |
|---|---|---|
| make | `Make` | `TEXT` |
| model | `Model` | `TEXT` |
| lens | `LensModel` | `TEXT` |
| focal length | `FocalLength` | `REAL` mm |
| aperture | `FNumber` | `REAL` f-number |
| exposure | `ExposureTime` | `REAL` seconds |
| ISO | `PhotographicSensitivity` | `INTEGER` |

Strings are trimmed of the padding and NULs cameras leave in them; an empty string is
`NULL`. Rationals with a zero denominator are `NULL`. Nothing is derived (no 35 mm
equivalent, no shutter-speed rounding) — formatting is the UI's, in `exif.ts`, where it is
pinned by tests.

### 3.2 Backfill: `exif_version`

The scanner calls `describe()` only for files whose size or mtime changed, so a library
indexed before this feature would never grow camera columns. The rating column solved the
same problem with `NULL` meaning "not read yet", which works for one column but not for
seven that are legitimately `NULL` when the camera wrote nothing.

`items.exif_version INTEGER NOT NULL DEFAULT 0` records which generation of `describe()`
last read the file. `metadata::EXIF_VERSION` is the current one. `KnownItem` carries the
row's value, and a file the walk finds unchanged whose version is behind is re-described
and written through `update_item_meta`, which touches the metadata and keyword columns
only: not the fingerprint columns, so no thumbnail is orphaned and no `thumb_gc_epoch`
bump is needed, and not `rating`, which the Picasa pass owns. The count is
`ScanReport::enriched` and folds into `touched_rows`, because the Search and Tag views are
built from these columns.

Bumping `EXIF_VERSION` is how a future field gets backfilled: the next scan of each folder
re-reads every file once. It is a per-file header read, not a decode.

## 4. Keywords

`keywords::read_keywords(path)` reads one bounded prefix of the file (`xmp::MAX_PREFIX`)
and takes keywords from two places in it, in this order, de-duplicated exactly:

- **XMP** `dc:subject`: every `rdf:li` inside it. Entities and character references are
  resolved. The packet is found the way `xmp::read_rating` finds it, so PNG, WebP and GIF
  containers work as well as JPEG.
- **IPTC IIM** dataset 2:25 in the JPEG APP13 `Photoshop 3.0` segment, resource `0x0404`.
  UTF-8 if the bytes are valid UTF-8, Latin-1 otherwise; Picasa itself wrote both.

Keywords live in `item_tags(item_id, tag)`, written by `insert_items`, `update_items` and
`update_item_meta` inside the row's own transaction. `ON DELETE CASCADE` follows a purge.
The Tags list is `tags_with_counts()` over live items; the Tag view filters on the table.

Picasa's own `keywords=` line in `.picasa.ini` is not read: for JPEGs Picasa writes the
keywords into the file, which is the source above.

## 5. Faces

### 5.1 The INI

Picasa 3.9 stores faces in the same per-directory INI as stars:

```
[Contacts2]
b5d3a7e4f1c2d9a8=Ada Lovelace;;
[IMG_0001.jpg]
faces=rect64(4d2a3c1f8e5b6a70),b5d3a7e4f1c2d9a8;rect64(…),ffffffffffffffff
```

`rect64(hex)` packs four 16-bit fractions — left, top, right, bottom — of the displayed
image's width and height into one 64-bit value, written without leading zeros. A contact
hash of all `f`s is Picasa's "ignored face" and is dropped. A `Contacts2` value is
`name;email;…`; only the name is kept.

`picasa::read_stars` becomes `picasa::read_folder`, returning stars, faces per file name
and contacts in one read, through the same `classify` the writer uses. `Option` keeps its
meaning: `None` is "could not read, leave everything alone".

### 5.2 Storage and the pass

```
contacts(hash TEXT PRIMARY KEY, name TEXT NOT NULL)
faces(item_id REFERENCES items ON DELETE CASCADE, contact TEXT, left, top, right, bottom REAL)
```

Contacts are merged across every INI, so a name recorded in one folder resolves a hash
tagged in another. Faces are applied per walked folder in the same post-walk pass as stars
(`apply_picasa`), which is what makes them survive an unchanged photo — Picasa naming a
face rewrites the INI, not the JPEG. Each item's faces are replaced only when they differ
from what is stored, so an agreeing folder costs no write. The count is
`ScanReport::refaced` and folds into `touched_rows`.

The People list is every contact with a face on a live item, with its count. A face whose
hash has no name anywhere is stored but not listed; it appears once the name turns up.

### 5.3 Display

The viewer's info panel lists the names. While the panel is open, each face is outlined
over the photo with its name. The rectangle is placed by `faceBox()` in `faces.ts`: the
image is `object-fit: contain` inside a frame, so the drawn image's rectangle is computed
from the oriented dimensions and the frame's, and the face fractions map into it. Picasa
records faces against the displayed (oriented) image, so the oriented dimensions are the
right ones.

## 6. Albums

```
albums(id INTEGER PRIMARY KEY, name TEXT NOT NULL, created_ms INTEGER NOT NULL)
album_items(album_id REFERENCES albums ON DELETE CASCADE,
            item_id REFERENCES items ON DELETE CASCADE,
            added_ms INTEGER NOT NULL, PRIMARY KEY (album_id, item_id))
```

Commands: `list_albums` (with live counts), `create_album`, `rename_album`,
`delete_album`, `add_to_album`, `remove_from_album`. A mutation that can change the album
currently on screen refreshes the grid through `Engine::albums_changed`; the album list
itself is refetched by the UI after its own call and on every `library-changed`, so no
new event is needed.

Membership is by item id. A photo renamed on disk is a new row to the scanner, and the old
row is purged two scans later with its memberships. Stars survive that because they live
in the INI; album membership does not, and that is recorded as a known limitation rather
than solved with a fingerprint match.

In the UI: an **Albums** group in the sidebar with an inline "New album…" input and a
context menu to rename or delete; a context menu on grid tiles with "Add to album ▸",
"Remove from album" in the album view, and "Reveal in file manager"; and album checkboxes
in the viewer's info panel.

## 7. Rotation

`r` rotates clockwise, `Shift+R` counter-clockwise, with two buttons in the viewer's bar.
The photo sits in a frame that is the viewport's size, or the viewport's size with width
and height swapped for a quarter turn, rotated as a whole; `object-fit: contain` then fits
the photo to the frame, so a landscape photo turned on its side fits the viewport's
height, and zoom and pan apply outside the rotation exactly as before. The face overlay
lives inside the frame and turns with it. Rotation resets on every navigation, like zoom.

`rotated(current, direction)` in `nav.ts` is the whole of the logic, pinned by a test.

## 8. Search

`search_entries` selects the camera columns and the capture date beside the two names and
hands the matcher one haystack per row:

- file name, folder name (as before)
- make, model, lens (when present)
- `NNmm`, `f/N.N`, `isoNNN`, so "50mm", "f/1.8" and "iso3200" find what a person would type
- the capture date as `YYYY-MM-DD`, so "2024" and "2024-06" work without a folder named so
- the photo's keywords, joined

Tokens stay OR-ed. "iso 400" is therefore "iso" or "400", which matches everything with an
ISO; the joined spelling is what the haystack answers, and the placeholder says so.

## 9. Sidebar

Above the years: Starred, Recent, then three collapsible groups — Albums, People, Tags —
each with a count in its header. Albums opens expanded; People and Tags open collapsed,
because a real library has hundreds of tags and the years must stay reachable. Collapse
state is session state, not persisted.

## 10. The migration

One entry, version 5: seven `ALTER TABLE items ADD COLUMN`s, `exif_version`, and the five
new tables with their indexes. `library/mod.rs` asserts `5`; the migration test seeds from
`MIGRATIONS[..4]` and checks an existing row's rating and `exif_version = 0` survive, since
the backfill depends on that default.

## 11. Tests that discriminate

- `a_photo_indexed_before_the_camera_columns_is_re_read_on_the_next_scan`: the backfill.
  Reverting the `exif_version` check leaves every camera column `NULL` forever.
- `a_face_named_in_picasa_is_picked_up_without_the_photo_changing`: the post-walk pass,
  the same shape as the star test.
- `keywords_come_from_xmp_and_iptc_and_are_not_duplicated`.
- `the_album_view_places_a_folder_by_its_oldest_member`: the driver filter, as for Starred.
- `search_finds_a_photo_by_its_camera_lens_keyword_and_date`.
- UI: `rotated`, `faceBox`, the `exif.ts` formatters, the album/people/tag group builders.
