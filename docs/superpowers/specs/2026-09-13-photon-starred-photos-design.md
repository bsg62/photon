# photon — Starred Photos Design

**Date:** 2026-09-13
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`
**Builds on:** v0.2.0 (`e31730c`)

## 1. Scope

Picasa wrote star ratings into each photo's embedded XMP metadata. photon reads those ratings and offers a **Starred** view, so a library rated in Picasa arrives already starred and nobody re-does the work.

### In scope

- Reading `xmp:Rating` from embedded XMP during scanning, for the formats v1 supports.
- A `rating` column on `items`, and the first schema migration to run against libraries that already exist.
- Ratings read as photos are indexed, on a freshly built library (§4).
- A **Starred** entry in the sidebar, and a grid view filtered to `rating >= 1`.

### Out of scope

- **Setting or changing a star inside photon.** Deliberate, and the defining constraint of this design: photon is a *reader* of ratings here. Write support is a separate decision, taken later.
- **`.xmp` sidecar files.** Picasa embedded ratings, so a Picasa-rated library has nothing in sidecars, and probing for one per file would slow every scan to discover that. Tools that write sidecars (Lightroom, darktable) are not served by this version.
- Other rating conventions: Picasa's legacy `picasa.ini`, IPTC, and the MWG reconciliation rules.
- Filtering by a rating threshold in the UI. The column stores the full 0–5 value, so this becomes possible later without re-reading every file.

### The invariant stands

> photon never writes to, moves or deletes files inside watched folders.

This feature reads only. No XMP packet is modified, and no sidecar is created. That promise appears in the v1 spec, the README and every plan so far, and nothing here spends it.

## 2. Data model, and the first real migration

`items` gains one column:

```sql
ALTER TABLE items ADD COLUMN rating INTEGER;
CREATE INDEX items_starred ON items(rating) WHERE rating >= 1 AND missing_since IS NULL;
```

**The column is nullable on purpose**, even though nothing writes `NULL` today. `NULL` means *not read yet*; `0` means *read, and not rated*.

Since this version expects a fresh library (§4), every row gets a rating at index time and no `NULL` ever occurs. Nullable is kept for two reasons anyway. It is a one-way door: `NOT NULL` now would force *another* migration later to relax it, if photon ever gains users who cannot simply delete their library and need their existing photos backfilled. And it keeps the two states distinguishable, so a library that *is* carried across reads as "unread" rather than silently as "no stars".

**This is also the first migration to perform an actual upgrade.** `MIGRATIONS` currently holds a single entry that creates the schema from nothing; every library in the wild sits at `user_version = 1`. The machinery is sound — each entry runs in its own transaction, bumps `user_version`, and a library written by a newer photon is refused with `SchemaTooNew` — but it has never done the thing it exists to do, and people have real libraries installed from v0.1.0 and v0.2.0. The migration must be additive and must not rewrite existing rows.

## 3. Reading the rating

`metadata` gains a reader that returns `Option<u8>` and, like `read_image_meta`, **never fails**: any malformed packet, truncated file or unexpected structure yields `None`. One corrupt photo must not fail a scan of a hundred thousand.

**Where the packet lives**, per container:

| Format | Location |
|---|---|
| JPEG | `APP1` segment beginning `http://ns.adobe.com/xap/1.0/\0` |
| PNG | `iTXt` chunk with keyword `XML:com.adobe.xmp` |
| WebP | RIFF chunk `XMP ` |
| GIF | Application Extension block `XMP DataXMP` |

**The read is bounded to the first 256 KiB.** XMP sits near the start of every container above, and a rating lookup must never pull a 20MB photo through memory. 256 KiB comfortably contains a real XMP packet while declining to follow extended ones, and keeps the per-photo cost of a first scan predictable rather than scaling with photo size.

**Parsing** uses `quick-xml`, which is already in the dependency tree and is pure Rust. That matters: photon has no native library dependencies, which is what made packaging tractable across three platforms, and the obvious alternative (`xmp-toolkit`, wrapping Adobe's C++ SDK) would undo it. `xmp:Rating` appears either as an attribute on an `rdf:Description` element or as a child element, and both spellings are read.

**Values.** XMP ratings run 0–5, with `-1` meaning "rejected". Starred means `rating >= 1`, which is what Picasa's single star writes. The full value is stored rather than a boolean, because it is free to keep and expensive to recover later.

## 4. Where ratings come from

New and changed files get their rating during `describe()`, alongside EXIF, at the single call site that already builds `NewItem`. That is the only path that writes a rating.

**This version expects a fresh library.** photon has one user and a private repository, so rebuilding the library is free — and rebuilding means every photo is indexed anew and rated as it goes, leaving no row unread. There is therefore no backfill: nothing walks existing rows looking for ratings it never read.

The consequence is stated plainly rather than left to be discovered: **a library carried across from v0.2.0 shows no stars**, because the scanner only re-reads a file whose size or mtime changed, and none of them will have. Delete the library and let it rebuild. The README says so.

This is the piece to revisit when the repository goes public. At that point users cannot be told to delete their libraries, and a backfill — a low-priority pass over rows with `rating IS NULL`, resumable because progress is the absence of nulls — becomes necessary. The nullable column (§2) is what keeps that option open without a second migration.

**A known limitation, stated rather than discovered.** Change detection is size-plus-mtime. A rating edited by another tool is picked up only if that edit changed the file's mtime — which rewriting embedded XMP normally does, since the packet lives inside the file. A tool that preserved mtime would leave photon showing a stale star until something else about the file changed. This is inherent to the existing scanner, not introduced here.

## 5. The Starred view

The engine holds a **view mode**: `All` or `Starred`. `grid_entries` takes the mode, and `refresh_grid` rebuilds the in-memory index for the active one.

Everything downstream is unchanged — paging, folder sections, viewer navigation and neighbour preloading all work because the index is simply a different set of rows. Switching views costs one query and a rebuild, the same work startup already performs inside the spec's sub-second budget for 100k items.

The rejected alternative was a second index kept in sync alongside the first. It buys instant switching at the price of two indexes to invalidate on every library change — a large new surface for staleness bugs, to speed up an operation that is already fast and rarely performed.

Sections in the Starred view remain per-folder, as in the All view. Switching views returns the grid to the top, because a scroll position in one set means nothing in the other.

## 6. The sidebar entry

A **Starred** row sits with the pinned watched roots, above the year groups, showing the count of starred photos.

**The count must not come from the grid index**, because the index only ever holds the active view — while the All view is on screen there is no starred index to count. `GridInfo` therefore carries a `starred_count` in both views, obtained by a `COUNT(*)` over `rating >= 1 AND missing_since IS NULL` alongside each index rebuild. That query is served by the partial index from §2, so it costs a lookup rather than a scan. Clicking it switches the grid to the Starred view; clicking any folder switches back to All and jumps to that folder, as today. The active view is visually distinct, so it is never ambiguous which set is on screen.

When nothing is starred the row still appears, showing zero — its absence would be indistinguishable from the feature not existing, and would leave a user who expected their Picasa stars with nothing to look at and no explanation.

## 7. Error handling

- **Malformed or absent XMP**: the item's rating is `0`, recorded as read. The scan continues.
- **An unreadable file during scanning**: no rating is stored for it, and the scanner's existing retry behaviour applies unchanged — reading a rating adds no new failure path.
- **A failed migration**: the app shows an error and refuses to open, per the v1 policy. This is the first release where that path can actually be reached with a user's library at stake.
- **A library from a newer photon**: already refused with `SchemaTooNew`; unchanged.

## 8. Testing

- **Container extraction and rating parse**, as unit tests over generated fixtures: JPEG, PNG, WebP and GIF each carrying a known `xmp:Rating`, in both the attribute and element spellings; a file with XMP but no rating; a file with no XMP at all; a truncated packet; a packet larger than the read cap; and a rating of `-1`.
- **The migration**, run against a database created at `user_version = 1` with rows already in it: the column appears, existing rows read `NULL`, and no row is otherwise modified.
- **The view filter**: the Starred index contains exactly the items with `rating >= 1`, sections are correct within it, and switching back restores the full set.
- **Manual checklist additions**: on a freshly built library, photos rated in Picasa show under Starred once the first scan finishes; the count matches; no photo file's modification time changes as a result of running photon.

That last manual item is the one that matters most and the one no automated test can honestly make: it verifies the invariant this whole design is built around.

## 9. Success criteria

- A folder of photos starred in Picasa appears under Starred in photon without anyone re-rating anything.
- A freshly built library rates every photo as it indexes them, so Starred is populated by the time the first scan finishes.
- No file inside a watched folder is written, moved or deleted — verifiable by modification times being unchanged after a full scan.
- Switching between All and Starred is fast enough not to feel like a mode change on a 100k library.
