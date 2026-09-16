# photon — Picasa Stars Design

**Date:** 2026-09-13
**Status:** Approved design, pending implementation plan
**Supersedes:** the rating source in `docs/superpowers/specs/2026-09-13-photon-starred-photos-design.md`
**Builds on:** v0.3.1
**Write support:** superseded by `2026-09-16-photon-set-star-design.md`, which narrows the
invariant below to photo files and adds a star toggle that writes the INI. §5's "the INI is
the only authority" is what that design is built against; the rest of this document stands.

## 1. Why this replaces the XMP source

The starred-photos feature shipped in v0.3.0 reading `xmp:Rating` from each photo's embedded XMP. **That was the wrong source.** Picasa does not write ratings into the photos: it writes a per-directory INI file beside them, `Picasa.ini` or `.picasa.ini`, holding one section per file:

```ini
[filename.jpg]
star=yes
backuphash=12223
[nextfilename.jpg]
star=yes
backuphash=1332313
```

A section header is a bare filename; the lines under it are that file's properties until the next header. `star` is optional and defaults to `no`. **Every other property is ignored** — photon reads stars, not Picasa's metadata in general.

So a Picasa-rated library shows nothing starred in photon today, which is the entire point of the feature. This design changes where the star comes from. It changes nothing about what a star *is* or how the Starred view works.

### The invariant stands

> photon never writes to, moves or deletes files inside watched folders.

The INI files are **read only**. photon never writes, creates or deletes a `Picasa.ini`, and never edits a star. Write support remains a separate, later decision — and would be a larger one here than it was for XMP, since it would mean writing into a file another program owns.

## 2. Scope

### In scope

- Reading `star` from `.picasa.ini` / `Picasa.ini`, per directory, during scanning.
- Clearing stars that the INI does not confirm (§5).
- Keeping the existing `rating` column, the partial index, and the Starred view exactly as they are.

### Out of scope

- **Setting or clearing a star inside photon.** Unchanged from the previous design.
- **XMP ratings as a star source.** The reader in `crates/photon-core/src/xmp.rs` stays — it is written, tested and read-only, and is wanted later for other metadata — but nothing consults it during scanning. Its tests stay green; it simply has no caller in the scan path.
- Picasa's other INI properties, its `.picasaoriginals` folders, contacts, albums and crop/edit records.
- Sidecar `.xmp` files, IPTC, and the MWG reconciliation rules.

## 3. The parser

A new `crates/photon-core/src/picasa.rs` exposes roughly:

```rust
/// The starred file names in one directory's Picasa INI, lowercased.
pub fn read_stars(dir: &Path) -> HashSet<String>
```

It never fails: an unreadable, malformed or absent file yields an empty set, because one bad INI must not fail a scan of a hundred thousand photos.

**No INI crate.** The format needed here is a section header and one key, and the real files carry Picasa's own quirks — duplicate sections, stray bytes, non-UTF-8 — that a strict parser would reject outright where a lenient hand-rolled one simply skips the line. This also keeps the dependency count where the packaging work left it.

**Which file.** `.picasa.ini` is preferred; `Picasa.ini` is read only when the dotted name is absent. They are **never merged**: two files disagreeing would produce a set matching neither, and a star removed in one but left in the other would linger with no way to explain it. Both names are matched case-insensitively, since a Windows-written library read on Linux may carry any casing.

**Which photos.** A directory's INI describes the photos *in that directory only*. Section names are bare filenames with no path separators, and Picasa writes one INI per directory — so nothing is inherited by subfolders. A deep tree simply has an INI in each folder that needs one.

**What counts as a star.** `star` is compared case-insensitively against `yes`, `true` and `1`. Anything else, including a missing key, is not a star. The leniency is deliberate and one-directional: being strict costs a silently missing star, which is the exact bug this design exists to fix.

**Filename matching is case-insensitive.** Picasa came from Windows, where the filesystem is case-insensitive, so `[DSC_0001.JPG]` and `dsc_0001.jpg` on disk are the same photo. Names are lowercased on both sides. The rare cost — two files in one directory differing only in case, possible on Linux — is that they share a star; the alternative silently loses stars for whole libraries.

**The read is bounded** the same way the XMP read is, for the same reason: a stray enormous file must not pull unbounded memory through a scan.

## 4. Where stars are applied — not in `describe()`

This is the part the previous design would have got wrong, and it is worth stating plainly because the obvious implementation does not work.

`describe()` builds a `NewItem` and is called **only for photos that are new or whose size or mtime changed**. Unchanged photos take this branch and are never written:

```rust
Some(k) if k.size == size && k.mtime_ms == mtime_ms && !k.missing => {
    report.unchanged += 1
}
```

Editing a star in Picasa rewrites the INI. **It does not touch the photo.** So on any normal rescan every photo looks unchanged, `describe()` is never called for it, and a star read there would never be written — the same silent staleness this design is replacing, one layer down.

**Stars are therefore applied as their own pass, per folder, after the walk.** The walk already builds `folder_ids: HashMap<PathBuf, i64>` as it enters each directory, which carries exactly what the pass needs: the path to find the INI, and the folder id to write against. For each folder seen by the walk:

1. read that folder's INI (§3);
2. write the stars for **all** of that folder's photos in one statement, independent of whether any photo changed.

`NewItem.rating` is no longer set from metadata during scanning. Newly inserted photos take the same pass; nothing needs a rating before it runs.

**The cost is one small file read and one statement per folder**, which is negligible beside walking the folder's photos, and is why §5's "re-read every scan" is affordable rather than a compromise.

## 5. The INI is the only authority

A scan **sets** the star for every photo in a folder from that folder's INI: starred where the INI says so, unstarred where it does not — including when the INI is missing entirely or has no section for that photo.

The alternative — only ever adding stars — was rejected. Un-starring a photo in Picasa, or deleting the INI, would leave photon's star stuck with no way to clear it short of deleting the library, which is the same class of staleness as the bug being fixed.

The consequence is stated rather than discovered: **photon's stars are a mirror of the INI files, not a store of their own.** Anything photon holds that the INI does not confirm is cleared on the next scan. With no way to set a star inside photon (§2), nothing is lost by this today; it is the constraint that any future write support will have to be designed against.

## 6. Storage is unchanged

`star=yes` becomes `rating = 1`, which is exactly what Picasa's single star means.

No migration, no new column, no index change. `starred` remains `rating >= 1`, the partial index `items_starred` still serves it, and the Starred view, its count and its sidebar row are untouched. The column keeps its `0-5` type so a future source with real ratings — the XMP reader, kept for exactly this — can populate it without another migration.

**`NULL` still means "not read yet"**, and `0` means "read, and not starred". Unchanged from the previous design.

**This wants a rebuilt library.** The column's meaning changes: it stops carrying an XMP rating and starts carrying a Picasa star. A library from v0.3.x holds XMP-derived values that no INI has confirmed, and §5's pass will correct them on the first scan of each folder — but only for folders that scan reaches. Deleting the library is the honest way to get a clean state, and the README says so.

## 7. Error handling

- **A missing, unreadable or malformed INI**: an empty star set, so every photo in that folder reads as unstarred (§5). Logged at debug, not surfaced — a folder without an INI is the normal case, not an error.
- **A non-UTF-8 INI**: read lossily rather than rejected; Picasa files from old Windows locales are not necessarily UTF-8, and a section header that survives lossy decoding still matches.
- **A folder the pass cannot read**: skipped, leaving that folder's existing stars untouched rather than clearing them. Failing to read is not evidence that the stars are gone — unlike a successfully-read INI with no entry, which is.
- **A scan cancelled mid-walk**: the pass runs only for folders the walk completed, consistent with the scanner's existing rule that anything unreached is unknown rather than changed.

## 8. Testing

- **The parser**, over fixture directories: a file starred with `star=yes`; `YES`, `true` and `1` accepted; a section with no `star` key; a section with `star=no`; other properties (`backuphash`) ignored; a filename matched case-insensitively; a malformed file; a missing file; a file with duplicate sections; a non-UTF-8 file.
- **File choice**: `.picasa.ini` preferred when both exist, and — the test that pins §3 — the two are *not* merged, so a photo starred only in `Picasa.ini` reads as unstarred when `.picasa.ini` is present and omits it.
- **Subfolder isolation**: a parent's INI does not star a photo in a child folder.
- **The pass**: stars appear for photos whose files did not change, which is the case `describe()` cannot serve and the whole reason §4 exists; a star removed from the INI is cleared on the next scan; a folder whose INI is deleted loses its stars; an unreadable folder keeps them.
- **Manual checklist**: a Picasa-starred folder shows its stars in photon after a scan; starring a photo in Picasa and rescanning makes it appear; no photo file's modification time changes as a result of running photon.

That last item is the one no automated test can honestly make, and it is the invariant this whole design rests on.

## 9. Success criteria

- A folder starred in Picasa shows exactly those photos under Starred, without anyone re-starring anything.
- Starring or un-starring in Picasa is reflected in photon after a rescan, with no library rebuild.
- No file inside a watched folder is written, moved or deleted — verifiable by modification times being unchanged after a full scan.
- The Starred view, its count and its sidebar row behave exactly as they did in v0.3.1.
