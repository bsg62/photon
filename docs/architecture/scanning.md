# Scanning, moved files and the watcher

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

### Scanning, and why "unchanged" matters

`scanner.rs` walks with `walkdir` and calls `describe()` **only for files whose size or mtime
changed**. Anything derived from data that can change *without* the photo file changing —
Picasa's per-directory `.picasa.ini` stars are the worked example — cannot be read in
`describe()`; it needs its own pass after the walk, over `WalkOutcome.walked`.

`walk_tree` has **two** callers: `scan_watched` (a whole root) and `scan_subtree` (the file
watcher's path). Wiring a post-walk pass into only the first leaves the common case broken while
every test passes. `scan_subtree`'s `folder_ids` is pre-seeded by `seed_ancestors` with every
ancestor, so a per-folder pass must use `walked`, not `folder_ids`. The one post-walk pass
today is `apply_picasa`, which applies stars, faces, hidden flags *and* albums from one read
of the folder's INI. It finds that INI from the walk's own listing (`read_folder_listed`), not
by listing the directory again - but a listing is a snapshot, and a star photon writes during
a long scan lands after it, so an INI the walk did not see is still probed for, and one it
did see and is gone falls back to `read_folder`; so does any folder whose listing the walk
could not complete. A pass that trusted the listing alone would zero that star. The hidden flag is followed on *change* (`items.picasa_hidden` records
the INI's last answer), not mirrored like a star: photon never writes `hidden=`, so a mirror
would undo every unhide in photon on the next scan.

`apply_picasa` has a third caller: `refresh_picasa`, the INI pass. The watcher reports a
folder whose only changed files are its INI (`picasa::is_ini_write`, which also knows photon's
own temporary) in `Changed::ini_dirs`, and those are reread without a walk under the same scan
slot, queued in `Pending::ini`, which has no cap - photon starring across many folders and a
Picasa album rename used to overflow the walk queue into a rescan of the whole root. A
rename of the writer's temporary name must change `is_ini_write` with it, or every star
becomes a walk again.

**The metadata backfill.** `items.exif_version` records which generation of
`read_image_meta` last read a file; `metadata::EXIF_VERSION` is the current one. An
unchanged file whose stored version is behind is re-described and written through
`update_item_meta` (camera columns, keywords, `taken_at`, the version; not the fingerprint
columns, not `rating`). `taken_at` is included because a capture date outside 1970..tomorrow
is refused (`plausible_taken_at`) and the backfill is the only way an unchanged file is
re-dated. Adding a field to `describe()` without bumping `EXIF_VERSION` leaves every
existing photo without it forever. The position (`gps_lat`/`gps_lon`, schema 23, read
from the EXIF GPS IFD by `read_gps`) rides in `CameraMeta`, so the four item writers
(`move_items` is the fourth, below) carry it with the camera columns. Keywords come from the file (XMP `dc:subject` and IPTC
2:25, `keywords.rs`) into `item_tags`; every writer of an item row goes through
`write_tags`. Every *reader* of keywords goes through `EFFECTIVE_TAGS` or `TAG_FILTER` in
`library/tags.rs`, which apply the user's rename/remove rules; a reader of `item_tags` that
bypasses them shows tags the user renamed or removed.

**The walk does not enter a recycle bin**, nor anything with a dot-name. One predicate,
`passed_over`, for the two places that test a name: `walk_tree`'s `filter_entry`, and
`scan_subtree`'s check of the path below the watched root. The second is there because the
watcher hands `scan_subtree` the directory an event came from and `walk_tree` exempts its own
depth 0: without it a subtree scan indexes what every full scan skips, marks missing and purges
(`subtree_scan_of_a_hidden_directory_indexes_nothing`,
`subtree_scan_of_a_recycle_bin_indexes_nothing`). The bins are `RECYCLE_BINS`: `$RECYCLE.BIN`
(inside the root when a drive's root is watched), Synology's `#recycle` and QNAP's `@Recycle`
(inside it when the share is), matched as a whole component, without ASCII case, at any depth;
`.Trashes` and `.Trash-1000` are dot-names already. Not XP's `RECYCLER`, and no other plain
word: matched without case it is a user's own folder "Recycler", whose photos the upgrade would
purge (`a_recycle_bin_is_not_walked` walks one). They are skipped
because of the feature below: deleting in Explorer or over SMB is a rename on the same volume
that keeps size and mtime, so the row was followed *into the bin* and the deleted photo stayed
in its albums and person views until the bin was emptied
(`a_photo_deleted_into_a_recycle_bin_leaves_the_library`). The watched root itself is never
passed over, whatever it is called (`a_watched_root_named_like_a_recycle_bin_is_scanned`; the
exemption had no test before it). A photo an earlier photon indexed inside a bin is no longer
found, so the next two scans that reach the folder above it mark it missing and purge it:
intended (`a_photo_indexed_inside_a_recycle_bin_by_an_earlier_photon_is_purged`). Except when
those were the watched root's *only* photos: the walk then sees no file, the empty-root guard
in `scan_watched` reads a root with rows and no file as an unplugged drive, marks it offline
and returns before anything is marked or purged, and the rows stay until a photo appears
outside the bin. That is the guard, not the bin rule, and it is why those fixtures hold a
`keep.jpg`.

**A renamed or moved photo keeps its row** (spec `2026-10-05-photon-follow-moved-files-design.md`,
schema 26). Everything photon keeps about a photo hangs on its `items` row, and a path the walk
does not find is marked missing and purged, while the path it has never seen is a new row with
none of it - which cost a reorganised folder its albums, edits, names, and the hours of face
work. So when the walk is about to insert a file, it first asks which row that file used to be.
The rule is `moved::pick` (pure: the filesystem arrives as a closure) and `moved::Probe` (the
stats): `Library::move_candidates` finds the rows with the file's size, mtime and kind
(`items_moved`, a plain index on `(size, mtime_ms)`, not partial, so it is outside the
partial-index tie in Schema; `the_move_lookup_is_served_by_its_index` is its plan test); `pick`
then wants the same width, height and `taken_at` (the date ignored for a row an older reader
described), the old file gone, and exactly one row left, the file name breaking a tie. Gone is
`NotFound` and nothing else - a permission error or a timed-out share is "cannot tell" - or the
old path still opens something that `paths::canonicalize` resolves to the new path, which is
a case-only rename on macOS and Windows, or a file or folder replaced by a symlink to where it
went. By what the old path *resolves to*, not by `paths::same_path`, which folds case by
platform and not by volume: on a case-sensitive volume under macOS two identical files `a.jpg`
and `A.JPG` would have traded one row back and forth on every scan. The drive must be there
(rule 4 of the five the spec numbers: the row's watched folder online, its root a non-empty
directory, checked once per watched folder per scan), because an unplugged or unmounted drive
answers `NotFound` for every file on it - `an_offline_or_empty_root_gives_no_candidates`. The
exception is the folder being walked, whatever its stored flag: the flag is written only after
the walk, and without it a drive that came back with renamed files had every rename inserted
as new and its row purged (`a_drive_that_comes_back_with_a_renamed_file_keeps_its_row`).

Two bounds on the rule. **More than 32 rows of one size, mtime and kind are no candidates at
all** (`MOVE_CANDIDATE_CAP`: the lookup reads 33 and then answers none, before anything is asked
of the filesystem). Each row that fits costs a stat, so in a cluster of identical files -
uncompressed frames, placeholders, a dataset unpacked with one mtime - every file asked about
every other: 4,000 of them took 20 s to import and 41 s to scan after a rename, against 0.4 s
and 0.5 s with the cap. The price is that a file moved inside such a cluster is a new photo
(`a_move_is_followed_among_identical_rows_only_up_to_the_cap`). The `LIMIT` bounds the rows
read and changes no outcome; the count check after it is what the tests can see. **And a read
that fails is an error, not a "no"**: `Probe::gone` and `pick` are fallible, and a failed read
of the watched folders fails the scan before `finish_mark_purge`, which loses nothing.
Swallowed, it read as "no drive is there": every renamed file of the walk was inserted as new
and the same scan marked the old rows missing, with nothing logged
(`a_scan_that_cannot_ask_which_drives_are_there_fails_before_it_calls_a_file_new`). A watched
folder that is merely not listed, removed meanwhile, is a state and stays "not there".

It runs in `flush_new` (`find_moves`, then `apply_moves`, then the insert of what is left), which
is `walk_tree`'s, so `scan_watched` and `scan_subtree` both have it; the watcher reports a move as
its two directories, scanned separately in either order, so a rule that paired a walk's vanished
and new files would have caught only moves whose ends lie in one walk. `find_moves` is two
passes, so a row is claimed by one file: the file that kept the row's name has it and the other is
new, within one batch (`of_two_files_that_fit_one_row_the_one_with_its_name_has_it`); across
batches the first file met has the row, and no row is ever picked for two files
(`a_row_is_claimed_by_one_file_and_the_next_takes_the_one_left`), nor guessed between when two fit
(`two_rows_that_fit_are_not_guessed_between`). `Library::move_items` is `update_items`' opposite:
the file is the same, so the hashes, the look-alike group, the detections, `face_version`, the
edit, the rating and every row hanging on the id stay
(`move_items_repoints_the_row_and_keeps_what_hangs_on_it`). What it writes is the place;
`missing_since = NULL`, so a row the source's scan had already marked missing is revived with
its detections, where `update_items` would have deleted them; the metadata just read, the
file's own keywords among it (`write_tags`); `thumb_state = 0`; `hidden` and `picasa_hidden` as
Hidden photos describes; and the GC epoch. A copy is not a move - its original is still there,
so rule 2 (the row's file is gone) refuses it (`a_copy_beside_the_original_is_a_new_row`).

Two hazards come with it, both closed by `mark_missing_at` and `purge_at`, which write only a row
still at the path the walk's `known` has for it and report the rows changed. The walk's own
`known` still lists a followed row at its old path, which the walk will never find, so by id alone
the scan that had just followed a move marked it missing - or, when an earlier scan of the folder
it left had marked it already, purged it with everything on it
(`a_row_already_missing_is_not_purged_by_the_walk_that_follows_it`). And a scan of another
watched folder, each having its own slot, may have read `known` before the move was written (the
stale writes in `a_move_is_followed_when_only_the_destination_is_scanned`). The guard covers
those two writes only: `update_items` and `update_item_meta` still write by id from the same
list, left so because they need a file back at the old path of a row followed elsewhere, within
one walk. The same race has `move_items` answer `false` for a row no longer at its old path
(purged or claimed since the lookup), and `apply_moves` hands that file back for the insert:
dropped, it would stay out of the library until some later scan
(`a_file_whose_row_went_away_is_inserted`, on `apply_moves` directly, a race having no seam).
`ScanReport::moved` counts the rows and is in `touched_rows`; `apply_moves` logs each followed
row at debug level with its id and both paths, and `run_scan` logs a scan that touched rows at
info with its counters, because a row followed to the wrong file shows nowhere else.

**The thumbnails come along.** The key is made of the path, so the moved row names thumbnails that
are not cached, and re-rendering a renamed folder of 5,000 photos would also 404 the People
page's face crops until it was done. `ScanSink::moved(&[(old_key, new_key)])` has the engine's
reporter call `ThumbCache::rename` for each pair (a `fs::rename` per size, a missing one
skipped, a failure logged and costing one render, never a wrong picture); the keys are
`Item::thumb_key()` before and after, so an edited photo's are the ones carried. It is called
before `indexed` for the same rows: the row is `Pending` regardless, and the worker that then
reaches it finds the files and marks it `Ready` without decoding. `apply_moves` makes both
reports before it touches folders, which can fail: the rows are already re-pointed, and an
error there must not leave their thumbnails neither carried nor queued
(`a_failed_folder_inheritance_still_reports_the_rows_it_moved`).

**A renamed folder keeps its name and Hide folder flag, by a narrower rule than it looks.** A
folder row is found by path, so a renamed directory is a new row; `inherit_folder_flags(from, to)`
hands the old row's `alias` and `hidden` to the new one when the old directory is gone (or is
the new one under another spelling, `moved::vacated` again), the new folder has neither of its
own, and **the walk created its row** (`FolderRows::created`: a directory it had no row at the
path of). Without the last, photos of a hidden, named folder moved into a long-standing folder
whose old directory was then deleted hid and renamed it
(`photos_moved_into_a_folder_that_was_already_there_do_not_rename_or_hide_it`). The cost to state
plainly: the name and flag are lost whenever an earlier scan made the new folder's row before the
old directory was gone. The commonest is a folder made first and filled afterwards: "New
folder" in the file manager, which the watcher scans two seconds later, then the photos dragged
in and the old folder deleted - the photos keep their own hidden flag, but the folder has no
name and is not hidden, so a photo added to it later is visible. The others: a folder moved file
by file across volumes with the watcher scanning mid-move, a child's subtree scan seeding its
parent (`seed_ancestors`), a walk that errored after `ensure`, a first scan cancelled before a
flush. An alias equal to the new directory's own name is not carried, by the comparison
`set_folder_alias` stores NULL by (`repeats_name`): copied, it stuck, and the directory renamed
once more was still shown under the name it had left
(`an_alias_that_is_the_new_folders_own_name_is_not_inherited`). A folder row that is gone by
then gives `Ok(false)`, not an error (`a_folder_row_that_is_gone_gives_and_takes_nothing`): the
scanner calls it after `from` may have been emptied and pruned by another watched folder's scan.
A flag is taken through `set_folder_hidden`, so photos of the new folder inserted before the
move was noticed are hidden with it (and no order here is load-bearing: inheriting before the
batch's insert, like asking the root before the stat, saves one call and changes no outcome).

**What does not follow, on purpose.** A photo moved out of the library and back later (its row is
purged by the second scan that misses it; keeping vanished rows for a period was declined for
now), a copy made first and the original deleted later (at the copy there was no move; at the
deletion nothing is new), a file edited and moved at once (its size and time differ), and Picasa's
data except with the folder's INI - `apply_picasa` mirrors the INI of the folder the photo is in
*now*, under the name it has *now*, so a photo moved alone loses its Picasa faces and albums.
**And its star, one set in photon included, which a rename in place loses as well**: the star is
the INI's `star=` line under the file's name (`set_star` writes it there) and `rating` mirrors
the INI, so the new name has no line and the pass clears it
(`a_photo_moved_without_its_ini_loses_its_picasa_star`), while a folder renamed or moved takes
its INI along and keeps every star (`a_folder_moved_with_its_ini_keeps_its_stars`). Carrying it
would be the scanner writing `star=yes` under the new name, which Conventions makes a spec-level
decision; it has not been taken. Three more, by rule: moving every photo out of a watched
folder into another (the emptied root is what an unmounted volume leaves behind - to rule 4
when it has no entry at all, and to the scanner's own guard, which marks it offline, when it
has no photo and its scan comes first - so the photos arrive as new rows and the old ones stay
as an offline drive's); a move to a filesystem with
coarser timestamps, FAT's two seconds or exFAT's ten milliseconds (the mtime is not the one
stored); and a volume mounted inside a watched folder and unplugged on its own (rule 4 looks at
the watched root only, so its files are `NotFound` under a root that is there: they are marked
missing and purged as deleted files are, and a copy of one turning up elsewhere meanwhile is
taken for it). A case-only rename on a case-insensitive mount under Linux (vfat, exFAT, CIFS)
is a fourth: `realpath` does not fold case there, so the old spelling resolves to itself, rule 3
answers "another file", and the rename is a new row and a purge. **A move between volumes is a
copy then a delete**, so if the watcher scans the destination while the originals are still
there the copies are new photos, and when the originals go their rows are purged. A single file
is normally followed (both halves land inside the watcher's two-second debounce, `DEBOUNCE`); a
long folder move between drives while photon runs may be followed in part. Two tests,
`a_case_only_file_rename_keeps_its_row` and
`a_case_only_folder_rename_keeps_its_rows_its_name_and_its_hide_flag`, are compiled everywhere
and ignored on Linux (`cfg_attr(..., ignore = ...)`): no filesystem a Linux runner has can make
the case-only rename. They run in CI on macOS and Windows and are the only tests of rule 3's
positive half (the old path opens this very file) on a case-insensitive filesystem - do not
un-ignore or delete them.

**The watcher drops access events** (`watcher/fs.rs`, `may_have_changed`). On Linux, notify
registers for inotify's open and close events, so reading a file's EXIF, listing a
directory or walkdir entering one all arrive as `Access` events on that directory. A scan
does all three to every directory it walks; treated as changes they scheduled the next
subtree scan of the same directories two seconds after every scan, forever. Anything that
makes the watcher react to more event kinds must keep a scan's own reads out. The debouncer
runs with `NoCache` on every platform, not its `RecommendedCache`: on Windows and macOS that
is a file-id map which walks every root and opens (Windows) or stats (macOS) every file under
the debouncer's lock, again on every rescan flag - minutes on a network share - and it only
stitches a rename's two halves together, which photon never needs, since each half is reduced
to a directory to rescan either way.

`skip_mark_purge` is set by a walkdir error carrying **no** path — a mid-iteration `read_dir`
failure (walkdir `lib.rs:1026`), not something the filesystem can be made to do on demand. An
unlistable walk root sets it too, but `read_stars` lists the directory as well, so that
branch's star behaviour cannot be tested from the filesystem either way.

**Every stored path is `paths::canonicalize`, never `dunce::canonicalize` or
`fs::canonicalize`.** dunce strips the verbatim `\\?\` prefix from disk paths only, so a
network share canonicalizes to `\\?\UNC\server\share\...`: a form the Windows shell rejects
(`ILCreateFromPathW` returns null, which is Reveal failing with "failed to convert path to
ITEMIDLIST") and nobody recognises as their share. `paths::simplified_unc` rewrites it to
`\\server\share\...`, and migration 10 rewrote the three path columns of libraries indexed
before it. The form has to be the *same everywhere*: `paths::same_path` compares
component-wise, so one path in each form is two different folders to photon - a re-added
root rather than a recognised one, and a `scan_subtree` whose `strip_prefix` misses its own
watched root. `paths::overlaps` is symmetric - it answers "do these two collide",
which is the question `add_watched_folder` asks. "Is this inside the library" is
`paths::is_within(child, root)`: an export destination checked with `overlaps` refuses a
folder that merely *contains* a watched root, and then says the opposite of what is true.

