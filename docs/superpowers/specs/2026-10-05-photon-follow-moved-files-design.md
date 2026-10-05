# A photo renamed or moved outside photon keeps what photon knows about it

Date: 2026-10-05. Approved in conversation the same day: scope "moves and renames inside
watched folders", and the rule below. One departure from what was approved is marked
**Changed since approval**.

## The problem

Everything photon keeps about a photo hangs on its row in `items`: its albums
(`album_items`), the keywords added in photon (`item_user_tags`), its turns and crop
(`edit_turns`, `edit_crop`), the hidden flag, the faces photon found on it and the names the
user confirmed (`detected_faces`, with `face_rejections` beneath), its hashes.

The scanner knows a file only by its path. A file renamed or moved in a file manager is a
path the walk does not find, which is marked missing and purged by the next scan
(`finish_mark_purge`), and a path it has never seen, which is a new row. The new row has
none of the above. README and CLAUDE.md record this four times as a limitation ("a photo
renamed or moved on disk leaves its albums", "comes back unedited", "comes back visible", "its
faces lose their confirmations"). Since 0.48.0 the cost of reorganising a folder is also the
hours of face detection and recognition spent on it, and every name confirmed in it.

## Decision

When the scanner is about to add a file as new, it first looks for the library row this file
used to be, and re-points that row to the new path instead.

Rejected:

- **Pairing vanished and new files at the end of one walk.** It sees only moves whose two
  ends lie in the same walk. The watcher reports a move as its two directories, scanned
  separately in either order, and a move between two watched folders is two scans by
  construction. It also has to merge two rows after the fact, since the new one is inserted
  batches before the walk knows what is left over.
- **Identity by content hash.** A file that is gone cannot be hashed, so every photo would
  have to be hashed in advance: a full read of the library that the duplicate finder is
  built to avoid.
- **Keeping the rows of vanished photos for a period**, so a photo moved out of the library
  and brought back later is recognised. Asked about and declined for now (scope 1); see
  "Out of scope".

## The rule

A file the walk would insert (`walk_tree`'s `None` arm: no row has its path) is *the moved
file of* a row when all of these hold:

1. **Same file, as far as can be told without reading it:** equal `size`, `mtime_ms` and
   `kind`, and equal `width`, `height` and `taken_at` - which `describe` has just read for
   the new file anyway. A rename or a move within one filesystem changes none of them; nor
   does a copy by any file manager that keeps timestamps. `taken_at` is left out of the
   comparison when the row's `exif_version` is behind `EXIF_VERSION`: an older reader may
   have dated it differently.
2. **The row's file is gone:** `fs::symlink_metadata` of the row's recorded path answers
   `NotFound`. Any other error is "cannot tell" and the row is not a candidate. A row
   already marked missing is a candidate like any other; the stat is still made, since
   "missing at the last scan" is not "gone now".
3. **Or it is this very file:** the old path still opens something, and
   `paths::canonicalize(old)` is the new path. That is a case-only rename on macOS or
   Windows, where the old spelling still opens the file, and a file replaced by a symlink to
   where it went. Not `paths::same_path`, as first written: that folds case by platform
   rather than by volume, so on a case-sensitive volume under macOS two identical files
   `a.jpg` and `A.JPG` would have traded one row back and forth on every scan.
4. **Its drive is there:** the row's watched folder is online - or is the one being walked,
   whatever its stored flag says - its root is a directory, and that directory is not empty.
   The exception is what lets a drive come back with renamed files: the flag is only set
   once the walk is over, and a walk that has just produced a file from the root is the
   evidence it waits for. An unplugged drive's files all answer `NotFound`; so do the
   files of an unmounted volume whose mount point was left behind, which is why the scanner
   has its empty-root guard, and this is its cousin: any entry in the root will do here,
   where the scanner's own guard counts photos. Checked once per watched folder per scan. A
   failed read of the watched folders fails the scan, before anything is marked missing:
   treated as "no drive is there", it would insert every renamed file as new and let the
   same scan mark the old rows missing.
5. **There is exactly one such row.** With several, the ones whose `file_name` equals the
   new file's are kept; if that leaves exactly one, it is the row. Otherwise the file is
   inserted as new: photon does not guess which photo's albums and names to hand over.
   More than 32 rows of one size, time and kind is "do not guess" as well, decided by the
   lookup's `LIMIT` before any file is asked about: a library of identical files otherwise
   paid a stat per row per file (measured by the whole-branch review: 4,000 identical files,
   20 s to import against 0.2 s).

A row is claimed by one file. Two new files that both fit one row (two copies of a deleted
original) are decided by the file name, within one batch of the walk: the file named like
the row has it, and the other is new. Across batches the first file the walk meets has the
row - the price of deciding at insert time rather than at the end of the walk, which
"Decision" explains.

The match is decided in the scanner, by a function that takes "is this path gone" and "is
this watched folder there" as closures, so every branch is tested without a filesystem.

### Why not more

Equal size and modification time to the millisecond, with equal pixel size and capture date,
of two *different* photos, one of which has just vanished as the other appears: on a
filesystem with coarse timestamps (FAT's two seconds) this needs two files of identical byte
length written in the same instant. If it ever happens, one photo carries the other's albums
and edit until the user removes them - recoverable, and far rarer than the loss this replaces.

## What re-pointing writes

`Library::move_items`, one transaction per batch, for each `(id, NewItem)`:

- `folder_id`, `path`, `file_name` from the new file; `missing_since = NULL`;
- the metadata `update_item_meta` writes, the file's own keywords (`item_tags`) among it:
  the file was described, so a row behind `EXIF_VERSION` is brought up to date in passing,
  and for a row that was current the values written are the ones already stored;
- `thumb_state = 0`, `thumb_error = NULL`: the thumbnail key is made of the path, so the row
  names thumbnails that are not there yet (see "Thumbnails");
- `hidden = 1` if the destination folder is hidden (`folders.hidden`), else unchanged - the
  rule `insert_items` applies to a new row, without un-hiding a photo the user hid;
- `settings::bump_thumb_gc_epoch` in the same transaction: `path` is part of the thumbnail
  key. The tripwire test in `settings.rs` walks it, and `purge_at`, with the four it had;
- `picasa_hidden = NULL` when the row is hidden after the write. The INI's last answer was
  about the photo under its old name in its old folder; kept, the hidden pass read the new
  folder's silence as Picasa un-hiding the photo, and a photo hidden through Picasa came
  back visible on a rename (found by the whole-branch review). Forgotten, the pass's
  first-read rule follows a `hidden=yes` and ignores a missing line. A visible row keeps
  the answer: one the user un-hid in photon must not be re-hidden when its folder is
  renamed with its INI.

Left as they are, which is the point: `edit_turns`, `edit_crop`, `content_hash`,
`percep_hash`, `similar_group`, `face_version`, `rating`, and every row in
`album_items`, `item_user_tags`, `detected_faces`, `face_rejections` and `faces`.
This is what `update_items` must not be used for: it clears the hashes and deletes the
detections, because there the file's content changed.

`rating`, Picasa's `faces` and Picasa's album memberships are then put right
by `apply_picasa`, which runs after every walk over the folders it reached and mirrors the
INI of the folder the photo is in *now*. A photo moved with its folder finds the same INI
and nothing changes; a photo moved alone has left its INI behind and loses its Picasa star,
Picasa faces and Picasa albums - which is what Picasa itself does with a file moved outside
it. So does a single file renamed in place: the INI is keyed by file name. That includes a
star set in photon, which is the same `star=` line. Carrying it would mean the scanner
writing an INI with no gesture from the user, which the project's conventions make a
decision of its own; it is not taken here. `apply_picasa` reads which folders hold Picasa data from the library after the walk, so
it sees the re-pointed rows.

### Folders

A folder's photon name (`folders.alias`) and its Hide folder flag (`folders.hidden`) hang on
the folder's row, which is found by path too. When a batch re-points rows from folder *A*
into folder *B*, and *A*'s directory is gone (or canonicalises to *B*'s, a case-only
rename), and *B* is a folder this walk created, with no alias and not hidden, *B* takes
*A*'s alias and hidden flag. "This walk created" is what tells a renamed folder from a
merge: photos of a hidden folder moved into a long-standing one must not hide and rename
it. The cost is every case where an earlier scan made the new folder's row before the old
directory was gone, and the name and flag are then lost: a folder made first and filled
afterwards (the watcher scans the empty folder two seconds after it appears), a folder
moved file by file between volumes with a scan mid-move, a child's subtree scan seeding its
parent, a walk that failed after making the row, a first scan cancelled before a flush.

An alias that merely repeats the new directory's name is not carried, as `set_folder_alias`
would not store it: carried, it would stick through the next rename. A flag taken over is written through
`set_folder_hidden`, so files of *B* inserted before the move was noticed are hidden with
it. Done before the batch's remaining files are inserted, so those inherit it the usual way.
The first folder to arrive wins; a folder with a name or a flag of its own keeps them.

A folder row that has gone by the time its flags are asked for (pruned by a scan of the
watched folder it was in) gives nothing, and is not an error.

`settings.last_folder` already answers `None` for a folder that no longer exists.

### Thumbnails

**Changed since approval.** The approved design re-rendered the thumbnails of a moved photo.
The spec carries them over instead, because it is cheap and the alternative has a visible
cost: a renamed folder of 5,000 photos would be decoded again, and until then the People
page's face crops (`/face/<id>/<thumb key>`, cut from the cached preview) would 404.

`ScanSink` gains `moved(&[(old_key, new_key)])`, called from the flush before `indexed`.
The engine's reporter renames each size's cached file from the old key to the new
(`ThumbCache::rename`, a `fs::rename` per size, missing files skipped). Keys are
`Item::thumb_key()` before and after, so an edited photo's thumbnails are the ones carried.
The row is `Pending` regardless; the thumbnail worker finds the files cached under the new
key and marks it `Ready` without decoding, as it does for any cached photo. A rename that
fails costs one re-render, never a wrong picture: a key names one picture.

A video's poster frame is carried the same way, which matters more: it can only be drawn
again while photon's window is open.

## Recycle bins

Added after the whole-branch review. The walk skipped only dot-names, so the recycle bin of
a watched drive root (`$RECYCLE.BIN`) or NAS share (`#recycle`, `@Recycle`)
was walked like any folder. Deleting a photo there is a rename on the same volume, so the
rule above would have followed it into the bin, with its albums: "I deleted it and it is
still in my album". The scan now enters none of them, in `walk_tree` and in `scan_subtree`
alike; the watched root itself is exempt, as it is from the dot rule. Windows XP's
`RECYCLER` is left out on purpose: it is a plain word, and a user's own folder of that
name would be passed over and its photos purged. A photo an earlier
photon indexed inside one leaves the library at the next scans.

## Guards

Each has a test that fails without it.

- **A re-pointed row is not marked missing by the walk that re-pointed it.** That walk's
  `known` still lists the old path, which it will never find. `mark_missing` and
  `purge_items` take `(id, path)` and write `WHERE id = ?1 AND path = ?2`, and report the
  rows they changed, which is what `ScanReport` counts. One guard in SQL rather than
  removing the entry from `known`, because the same guard covers the next case.
- **A concurrent scan of another watched folder cannot mark or purge it.** Each watched
  folder has its own scan slot; a scan of the folder the photo left may have loaded `known`
  before the move was written. The path no longer matches, so its write changes nothing.
- **An unplugged or unmounted drive** gives no candidates (rule 4).
- **A copy is not a move.** The original is still there (rule 2), so the copy is a new row
  and the original keeps everything.
- **The scan refreshes the grid.** `ScanReport::moved` counts re-pointed rows and is folded
  into `touched_rows`; without it a scan that only followed a rename would leave the grid
  showing the old path's folder.

## Schema

Migration 26: `CREATE INDEX items_moved ON items(size, mtime_ms)`. The lookup runs once per
new file, and the existing `items_size` is partial (`missing_since IS NULL`), so it does not
hold the rows a previous scan already marked missing - prime candidates. The first scan of a
100,000-photo folder makes 100,000 probes that find nothing.

Tried on 2026-10-05 before writing this: with the index added, every query-plan test in
photon-core still passes; the only failures were the version tripwires. The index is not
partial, so it is not in the creation-order tie CLAUDE.md describes. The lookup gets its
own plan test.

An older photon refuses a library at schema 26. The release is a minor.

## Not changed

No UI, no IPC command, no setting. A moved photo simply stays in its albums. The viewer and
the grid selection hold a photo by id, so a photo on screen when its file is renamed stays
on screen; its `thumbKey` changes, so `pictureChanged` reloads it once.

## Testing

Scanner tests on real temporary directories, through both `scan_watched` and
`scan_subtree`, each asserting the row's id is unchanged and an album, a user keyword, an
edit, the hidden flag and a detected face are still on it:

- a file renamed in place; a file moved to another folder; a folder renamed;
- the destination scanned before the source (`scan_subtree` of the new directory only), and
  the source scanned first (the row already missing, then revived with its detections, where
  `update_items` would have deleted them);
- a move between two watched folders;
- a copy beside the original: two rows, the original untouched;
- two candidates: no match; two candidates, one with the file's name: that one;
- a watched folder whose root is missing, and one whose root is empty: no match;
- different pixel size or capture date: no match;
- a renamed folder keeps its alias and its Hide folder flag, and a file added to it in the
  same scan arrives hidden;
- a photo moved without its INI loses its Picasa star; moved with it, `restarred` is 0;
- `mark_missing` and `purge_items` with a stale path change nothing;
- `ScanReport::moved` is counted and makes `touched_rows` true on its own.

Unit tests of the matching function with injected closures for rules 2-5, including the
case-only rename, which no Linux filesystem in CI can produce. Library tests for
`move_items` (what it writes, what it leaves, the GC epoch) and the lookup's plan. A
`ThumbCache::rename` test, and an engine test that a moved photo's thumbnail is `Ready`
again without a render.

Every test is shown to fail with its change reverted, as the project requires.

## Documentation

README: the four places that say a renamed or moved photo loses its albums, edit, hidden
flag and folder name now say it keeps them, and what still does not follow (below).
CLAUDE.md: the Conventions paragraph and "What clears what" (a renamed file is no longer a
purge and a new row), the thumbnail-GC list (five writers), `ScanReport`'s counters, the
schema numbers. Smoke checklist: rename a photo in an album, rename a folder with a hidden
photo and a named face, move a photo between two watched folders. Release notes: the
Upgrading line for schema 26.

## Out of scope

- **Out of the library and back later.** A photo moved to a folder photon does not watch is
  purged by the second scan that misses it, as today. Following it would mean keeping
  vanished rows for a period, with its own questions (how long, what the counts show
  meanwhile).
- **A copy made first and the original deleted later.** At the copy there was no move; at
  the deletion nothing is new.
- **A file changed and moved at once** (edited in another program and saved under a new
  name). Its size and time differ: it is a new photo.
- **Picasa's own data** follows only with the folder's INI, as above.
- **A volume mounted inside a watched folder** and unplugged on its own is not covered by
  rule 4, which looks at the watched root.
- **A rename that changes only Unicode normalisation** on macOS should now be followed by
  rule 3, since the old spelling canonicalises to the new one; nothing tests it.
