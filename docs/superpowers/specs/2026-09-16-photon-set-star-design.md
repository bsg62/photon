# photon — Set Star Design

**Date:** 2026-09-16
**Status:** Approved design, implemented
**Supersedes:** the "write support" deferral in `2026-09-13-photon-picasa-stars-design.md` §1–§2
**Builds on:** v0.11.0

## 1. What changes, and the promise it changes

Since v0.4 photon's stars are a mirror of Picasa's per-directory INI files, and nothing in
photon can set one. This design adds a star button to the viewer and a star badge on grid
tiles. A star set in photon is written into the same `.picasa.ini` Picasa reads, so an
existing Picasa installation sees it, and Picasa's own stars keep flowing the other way.

That is a write inside a watched folder, which every spec so far forbade. The promise is
narrowed, deliberately and in one place:

> photon never writes, moves or deletes **photo files**. The only file it writes inside a
> watched folder is Picasa's own `.picasa.ini` (or `Picasa.ini`), only to set or clear a
> star, and every other byte of that file is preserved.

The INI is still the only authority (2026-09-13 §5). photon writes the authority first and
mirrors it into the database second, so nothing photon holds is ever something the INI does
not confirm for longer than the next scan.

### Out of scope

A keyboard shortcut, toggling from the grid, ratings above one star, and any other INI
property. Suppressing the file watcher's reaction to photon's own write — see §5 for why it
is wanted.

## 2. The writer

`picasa::set_star(dir, file_name, starred) -> io::Result<bool>` returns whether the file
changed.

**Which file.** The one the reader would read: `.picasa.ini` preferred, `Picasa.ini`
otherwise, both matched case-insensitively. A folder with only `Picasa.ini` has that file
edited in place — creating `.picasa.ini` beside it would make the reader prefer the new
file and silently drop every other star in the folder. A folder with no INI gets a new
`.picasa.ini` when starring, and nothing at all when unstarring.

**Bytes, not strings.** The file is split into lines as raw bytes; each line is decoded
lossily only to be classified, and every line the edit does not touch is copied back as it
was. Picasa's INI carries face tags, crops and edit records, and files from old Windows
locales are not UTF-8; decoding the whole file and re-encoding it would corrupt both.

**One classifier.** The reader's `parse_stars` and the writer's `rewrite` go through the
same `classify(line)`. A writer with its own idea of a header or a key would drift from the
reader — appending a section the reader already matched, or leaving a `Star = 1` line the
reader counts — and the two halves of the module would then disagree about the file they
both own.

**Starring.** The first `star` line in the first matching section becomes `star=yes`; any
further `star` lines in matching sections are dropped (Picasa has written sections twice,
and the reader counts a star anywhere). A matching section with no `star` line gets one
right after its header. No matching section means a new one at the end, named with the
on-disk casing. **Unstarring** drops every `star` line in every matching section and leaves
the now-empty section, as Picasa does.

**Line endings** follow the file's first newline; a new or newline-less file gets CRLF,
which is what Picasa writes. An unterminated last line is terminated before anything is
appended to it.

**A no-op is not a write.** A file that already says what was asked comes back `false`
and is not touched, so its modification time does not move.

**Bounded.** A file over `MAX_INI` is refused with `FileTooLarge`. Rewriting a partial read
would throw away everything past the cut.

**Atomic.** The new contents go to `.picasa.ini.photon-<pid>-<seq>.tmp` in the same
directory, fsynced, then renamed over the original; a crash cannot leave a truncated INI.
The temporary is a dotfile with no media extension, so the scanner's walk ignores it. On
unix the original's permissions are copied onto it; on Windows it is created with the
hidden attribute when the original had one, because Picasa marks the file hidden and a
rename replaces attributes along with contents. Both are done with `std` alone — the
constant `FILE_ATTRIBUTE_HIDDEN = 0x2` is a literal, not a dependency — which is why the
temporary is hand-rolled rather than `tempfile::NamedTempFile`, whose builder cannot set
attributes.

**Known limitation.** If Picasa itself holds the INI open without share-delete at that
moment, the rename fails with a sharing violation; the command errors, nothing is written,
and the toast names the file. The README says to try again.

## 3. Where the star goes

`Engine::set_star(id, starred)`:

1. take the engine's `ini_write` lock;
2. load the item; a missing or soft-deleted row is `NotFound` (its folder may be an
   unmounted drive, and the INI photon would create there would be the only thing on it);
3. `picasa::set_star` on the item's directory and file name, mapped to `Error::IniWrite`
   with the file's path so the message names it;
4. `Library::set_ratings` for that one row;
5. `refresh_grid`.

**The order is fixed by who is the authority.** A database write that landed without the
file would be undone by the next scan's Picasa pass, and the star would simply vanish. A
file write that landed without the database is corrected by the scan photon's own write
triggers (§5). So: INI fails → nothing anywhere has changed, the UI reverts, consistent.
Database fails after the INI → the UI reverts on the error and the watcher's scan puts the
row right within seconds, at which point the star appears on its own. Grid refresh fails →
the database is right and the grid is stale until the next rebuild, the same class as every
other refresh failure in `engine.rs`.

**The lock.** Tauri runs async commands concurrently on a worker pool. Two stars into one
folder are two read-modify-writes of the same file; unserialised, the second read misses the
first write and the rename drops it. The lock is held across the database write as well, so
rows land in the order the file did. No test can fail deterministically with the lock
removed; the reasoning is here instead.

## 4. The command and the mirror

`commands::set_star` → `ipc::set_star` → `generate_handler!` → `api.setStar`. `ViewerItem`
gains `starred`, from the same `is_starred(rating)` the grid rows use, so the two cannot
disagree. `viewer_item` now refuses a soft-deleted row with `NotFound`; §6 depends on it.

## 5. The file watcher, and why photon's own write is not suppressed

photon's write fires a notify event for the directory, which after the two-second debounce
becomes a subtree scan of it. That scan's Picasa pass re-reads the INI photon just wrote,
finds every row already agrees, reports `restarred == 0`, and rebuilds nothing. It is a
cheap consistency check and the mechanism that heals step 4's failure case above. **Do not
add a self-write suppression**; the engine's doc comment says the same.

The one race it leaves: a scan that read the INI *before* photon's write and applies its
stars *after* photon's database write reverts the row and rebuilds, so the UI briefly shows
the old state. photon's write has queued another subtree scan of that directory (into the
pending set if the folder's scan slot is busy), which re-reads the new INI and puts the
star back. The database is wrong for seconds; the INI is never wrong. That heal does not
happen until the next scan when the watcher failed to start, waits up to five minutes on a
degraded root, or is skipped and logged if the follow-up's database write fails. Picasa
rewriting the INI between photon's read and write is a lost update in either direction and
is out of scope.

## 6. The viewer

A `★`/`☆` button at the bottom, immediately left of the caption, driven by a small state
machine (`createStarToggle`) so it can be tested: the flip is optimistic, a second click
while one is in flight is dropped, a failure reverts and reports — and the revert checks
that the button is still bound to the photo it toggled, because the user can navigate while
the write is in flight and a revert landing on the next photo would flip a star nobody
touched. A press on the button while zoomed is not the start of a pan.

**Leaving the view.** Unstarring while Starred is showing removes the photo from the grid,
and the viewer's rebind — which re-finds its photo by id after every rebuild — gets no
offset back. Before this design that meant "This photo is no longer available", which is
wrong here: the photo exists, the user is looking at it. The viewer now asks `viewer_item`
for the photo; if that succeeds the photo has merely left the view and it stays on screen,
without a position in the caption, until the next navigation; if it fails the photo is gone
and the message is right. An orphaned photo holds no offset, so ArrowRight goes to whatever
now sits at its old offset (its right-hand neighbour) rather than skipping one, and the
offset handed back on close is clamped to the view.

**The tile badge** renders `GridEntry.starred`, which was already carried and unused. The
grid re-fetches its rows after the rebuild, so the badge and the sidebar count follow the
toggle with no further plumbing.

## 7. Testing

The writer over fixture directories, asserting exact bytes: creation, editing `Picasa.ini`
in place, precedence when both exist, byte preservation around a replaced line (comments,
`backuphash`, a non-UTF-8 byte, CRLF), insertion after a header, appending a section,
removing every duplicate `star` line, case-insensitive and whitespace-tolerant headers, the
no-op, line endings, the size cap, a failed write leaving no temporary, and — per platform
— unix permissions and the Windows hidden attribute surviving. The engine: the INI is
written and the grid follows; a rescan after the write agrees and rebuilds nothing (the
test a database-only implementation fails); unstarring in Starred drops the photo; a
missing or unknown photo is refused and no INI appears; an oversized INI fails with
`iniWrite` and leaves the database and the grid version alone (the test the other order
fails). The toggle machine: the optimistic flip, the dropped second click, the revert, the
revert declined after a re-bind. The rebind probe, the badge and the button wiring are
Svelte effects, verified by `svelte-check` and the README's smoke checklist.

## 8. Success criteria

- A star set in photon shows in Picasa on its next look at the folder, and one cleared in
  photon disappears there.
- Every line of an INI that is not that photo's `star=` line is byte-identical after a
  toggle; a folder's face tags and edits survive.
- No photo file is written, moved or deleted, ever.
- A star set in photon survives the next scan without a library rebuild.
- The Starred view, its count and the sidebar behave as before.
