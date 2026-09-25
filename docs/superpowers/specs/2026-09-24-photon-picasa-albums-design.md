# Picasa albums

2026-09-24. Approved in conversation the same day, section by section.

## What it is

photon shows the albums Picasa 3 recorded in its per-folder INI, next to photon's own albums,
so a library moved over from Picasa keeps its albums the way stars and faces already come
across. Picasa owns them: photon reads them on every scan and never edits them.

## The format, and how sure we are of it

Picasa 3 writes each album a folder's photos belong to into that folder's `.picasa.ini`
(or `Picasa.ini`): one `[.album:<token>]` section per album, with at least `name=`, repeated
in every folder that has a member; and on each member photo's own section an
`albums=<token>[,<token>...]` line.

```ini
[.album:9c0e2f7a41b3d58e6f1a2b3c4d5e6f70]
name=Holiday 2009
token=9c0e2f7a41b3d58e6f1a2b3c4d5e6f70
date=2009-08-14T10:22:31+02:00

[IMG_0412.JPG]
albums=9c0e2f7a41b3d58e6f1a2b3c4d5e6f70,1f2e3d4c5b6a79880f1e2d3c4b5a6978
star=yes
```

**This is documented Picasa 3 behaviour, not yet checked against a real file**: the user had
no Picasa library at hand when this was designed. The first entry of the smoke checklist is
confirming it on one; if it does not hold, the parser changes before release. Picasa also
keeps albums in its own database (`albums.pmp`); an album that only ever lived there, never in
an INI, does not appear, and reading that database is its own project.

## Decisions

- **Picasa owns a Picasa album.** Mirrored from the INI like faces: photon cannot rename,
  delete, add to or remove from one, and a change made in Picasa shows on the next scan. The
  user chose this over a one-time import into editable photon albums and over a mix of both.
  Copying one into an editable album is already possible: select its photos, **Add to album**.
- **One list in the sidebar.** Picasa albums are mixed into **Albums**, sorted by name with
  photon's own, marked by a small icon. The user chose this over a separate heading.
- **The same tables, not parallel ones.** A Picasa album is a row in `albums` carrying a
  `picasa_token`; its members are ordinary `album_items` rows. `GridView::Album`, the counts,
  `item_albums`, the viewer's album list and the refresh chain therefore need nothing new. The
  alternative - separate tables and a `GridView::PicasaAlbum` - would duplicate every one of
  them and every future album feature, against "a view is just a different row set".
- **An album with no photos left leaves the sidebar but keeps its row**, so a folder that comes
  back finds the same album again.
- **The name is the one in the INI read most recently.** Picasa writes the same definition into
  every folder, so they normally agree; this is how `[Contacts2]` names already behave.

## Reading (`picasa.rs`)

`FolderIni` gains two fields, filled by the same parse and the same `classify` as stars and
faces, so the reader and the writer keep sharing one line classifier:

- `albums: HashMap<String, String>` - token to name, from each `[.album:<token>]` section's
  `name=`. An `.album` section with no `name=` defines nothing: recorded under its token as a
  name, it would rename an album another folder's INI had named, back and forth with every
  scan. Its token is still an album as soon as a photo names it (below), listed under the
  token until some INI gives it a name.
- `item_albums: HashMap<String, Vec<String>>` - lowercased file name to its tokens, from
  `albums=`, split on commas and trimmed; empty tokens are dropped and repeats removed.

Tokens are compared and stored lowercased. A token an `albums=` line names that no INI defines
still counts, the way a face with an unknown contact does. A header starting `.album:` (compared
case-insensitively) always opens an album, never a photo's section, so its keys - `name=`,
`token=`, `date=` - never attach to a photo. A photo whose file name really is `.album:...`
therefore gets no stars, faces or albums; `:` is not allowed in a Windows file name, where
Picasa ran, so no real library has one.

Nothing in an `.album` section is ever written: `set_stars` already preserves every byte it
does not recognise, and a test pins it for an INI that has albums.

## Storage (migration 17)

```sql
ALTER TABLE albums ADD COLUMN picasa_token TEXT;
CREATE UNIQUE INDEX albums_picasa_token ON albums(picasa_token);
```

NULL for photon's own albums (a unique index admits any number of NULLs). `album_items` is
unchanged. The literal version numbers in `library/mod.rs` move to 17 and the migration test
seeds from `MIGRATIONS[..16]`; the table count does not change.

**The guard.** `rename_album`, `delete_album`, `add_to_album` and `remove_from_album` refuse an
album with a token, with a new `Error::PicasaAlbum`. The UI never offers those actions on one;
the guard is what makes that true over IPC. The scan writes through its own
`set_picasa_album_items`, which touches only token-bearing albums' rows, so nothing needs a
way around the guard.

## The scan (`scanner.rs`)

In `apply_folder_ini`, after contacts and before stars:

1. `lib.upsert_picasa_albums(&ini.albums, referenced_tokens)` - a defined album is inserted or,
   if its stored name differs, renamed; a token only referenced is inserted with
   `INSERT OR IGNORE` under the token as its name, so it never overwrites a real name and the
   order folders are scanned in does not matter. Returns token to album id.
2. `apply_folder_albums(lib, folder_id, names, &ini.item_albums, &ids)` - like
   `apply_folder_faces`: per photo in the folder, the INI's Picasa albums against its stored
   token-bearing `album_items` rows; only photos that differ are written, in `BATCH` chunks,
   through `set_picasa_album_items`. A photo's photon albums are never touched.

A folder whose INI cannot be read (`read_folder` answering `None`, which includes a symlinked
INI) keeps its memberships; a folder with no INI, or an INI with no `albums=` lines, loses them.
That is the faces rule: "the INI says nothing" is an answer, "the INI could not be read" is not.

This is the existing post-walk pass over `WalkOutcome.walked`, so `scan_watched` and
`scan_subtree` both get it without new wiring.

**`ScanReport::realbumed`** counts photos whose Picasa membership changed plus albums inserted
or renamed, and is folded into `touched_rows` - a rename alone must refresh, because only the
sidebar shows it. A rescan of an unchanged INI must count 0, or the grid rebuilds after every
scan forever. CLAUDE.md's list of counters gains it.

**Reaching the screen** needs nothing new: `touched_rows` → the end-of-scan `refresh_grid` →
`library_changed` → the UI re-reads the grid, and on every grid version it already runs
`refreshCollections()` (`library.svelte.ts`), which re-fetches `listAlbums`.

**An existing library** fills in on its first scan after the upgrade: the INI pass reads every
walked folder whether or not its photos changed, so there is no backfill and no `EXIF_VERSION`
bump. Purging a photo already cascades its `album_items`. Hidden photos keep their membership
and are left out of counts and the view by the existing `hidden = 0` filters, as in photon's
own albums.

## IPC and the TypeScript mirror

No new commands. `AlbumSummary` gains `picasa: bool`, mirrored in `api.ts` in the same commit,
and every `AlbumSummary` literal in the UI tests follows. `albums_with_counts` leaves out a
Picasa album whose live count is 0; an empty photon album is still listed. `ViewerItem.albums`
stays a list of ids and already includes Picasa albums.

## UI

- **Sidebar (`FolderTree.svelte`).** One list, as today. A Picasa album carries a vendored
  Lucide icon after its name, titled "From Picasa. Change it in Picasa." It has no context
  menu: its only items would be Rename and Delete. Clicking it opens the Album view.
- **Grid (`Grid.svelte`).** **Add to album** lists photon albums only. **Remove … from** is
  offered only when the open album is one of photon's own - asked as "is it in the list and
  not Picasa's", not "is it Picasa's", because a Picasa album that just lost its last photo
  has left the list and would otherwise read as editable, and the empty message does not say "Right-click a
  photo to add it" (a Picasa album is empty on screen only in a race, since an empty one leaves
  the sidebar).
- **Viewer info panel (`Viewer.svelte`).** Checkboxes for photon albums, as today. Below them,
  the Picasa albums this photo is in, as plain rows with the icon and no checkbox. Picasa albums
  it is not in are not listed: every one of them unchecked would bury photon's own albums in a
  large library. `album-membership.svelte.ts` is unchanged; it only ever sees photon album ids.
- The filtering (photon albums for the menu and checkboxes, the photo's Picasa albums for the
  panel) lives in a pure module with vitest tests; the component wiring is `svelte-check` and
  the smoke checklist.
- **Screenshots.** `mock.js`'s canned `listAlbums` gains a Picasa album so the sidebar and
  info-panel shots show the icon. No new command, so the `api.ts` coverage test is unaffected.
- **README.** A short **Picasa albums** section by **Stars and Picasa**: read from the INI,
  read-only, followed on every scan, and that an album Picasa kept only in its database will
  not show.

## Testing

Every test is shown to fail with its own change reverted, on an input where the reverted code
answers differently.

- **Parser:** definitions and `albums=` lines, with a mixed-case file name, spaces in the list,
  an empty and a repeated token; an `.album` section with no `name=`; `.album` keys not leaking
  into a photo; `set_stars` leaving `.album` bytes identical.
- **Library:** migration 16 → 17 on a populated library (existing albums keep their data, token
  NULL); each of the four writers refusing a Picasa album and accepting a photon one;
  `albums_with_counts` leaving out an empty Picasa album and keeping an empty photon one;
  `set_picasa_album_items` never touching photon albums, on a photo that is in both kinds.
- **Scanner:** an album spanning two folders is one album; a photo's `albums=` line removed
  drops it, an unreadable INI keeps it; a rename in one INI renames it, and a referenced-only
  token never replaces a real name in either scan order; `realbumed` makes `touched_rows` true
  with no photo file touched, for a membership change and for a rename alone; the pass runs
  through `scan_subtree` as well as `scan_watched`.
- **App:** `viewer_item` lists a Picasa album's id; `set_album_view` on one shows its members.
- **UI:** the pure filtering module.

## Smoke checklist additions

1. On a real Picasa library: `grep -rl '^\[\.album:' --include='*icasa.ini'` finds INIs, and
   their albums appear in the sidebar with the Picasa icon and Picasa's counts.
2. A Picasa album has no context menu; **Add to album** does not list it; its view offers no
   **Remove from**.
3. The info panel lists the photo's Picasa albums below the checkboxes, without checkboxes.
4. Renaming an album in Picasa and rescanning renames it in photon.

## Limits

- Albums only in Picasa's database do not appear (above).
- Membership is by item id. A file renamed through Picasa is found under its new name, because
  Picasa rewrites the INI section too; a file renamed outside Picasa loses the membership, which
  is also what Picasa would then show.
- `date=`, `location=` and `description=` of an album are not read.
- Two INIs that define one token with different names (a backup copy of a folder written
  before a rename in Picasa, say) are settled by age, since 2026-09-26 (schema 19,
  `albums.picasa_named_at`): a name is taken only from an INI newer than the one that named the
  album last, so the rename - which Picasa writes into every member folder - wins and the stale
  copy never renames it back. The remaining edge: a stale copy whose INI gets a *new* mtime (copied
  back with a tool that does not preserve timestamps) is then the newest, and names the album
  once, until Picasa next writes a newer INI.
- The pass reads before it writes: an album INI that says what the library already holds takes
  no write lock (also 2026-09-26).

## Not in this feature

Converting a Picasa album into an editable photon album (select all, **Add to album** does it),
writing anything to an INI's album sections, and searching by album name.
