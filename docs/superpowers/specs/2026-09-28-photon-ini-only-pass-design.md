# An INI-only change rereads the INI instead of walking the folder

Date: 2026-09-28. Section 7 of `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`
("photon's own INI writes echo back as subtree scans").

## The problem

The watcher reduces every event to its directory (`changed_dirs`, `watcher/fs.rs`), so a
changed `.picasa.ini` schedules a *subtree scan* of its folder: a walk of the folder and
everything below it, a stat of every file, then `apply_picasa` reading the INI. Each watched
root has one scan slot; a batch naming many folders starts one scan and queues the rest, and
past `MAX_PENDING_DIRS` (8) `insert_pending` collapses the queue into a **full rescan of the
root**.

Two ordinary actions hit that:

- **photon's own writes.** Starring photos across more than 8 folders writes 8+ INIs (through
  a temporary `.picasa.ini.photon-<pid>-<seq>.tmp` renamed over the original). Photon has
  already written the ratings to the database, so every one of those scans finds nothing to
  change; with the overflow, the whole root is rescanned for nothing.
- **Picasa's writes.** Renaming a Picasa album rewrites every member folder's INI. Those are
  real changes photon must read - but only the INIs changed, not the photos.

## Decision

When the only thing that changed in a folder is its INI, photon rereads that INI and applies
it, without walking the folder. Rejected:

- **Suppressing photon's own echoes by fingerprint** (size, mtime, inode of the INI it wrote):
  helps photon's writes only, not Picasa's; std has no stable file id on Windows; and on FAT a
  2-second mtime could hide a Picasa write landing just after photon's, losing a star.
- **A smarter overflow rule** (a larger cap, the lowest common ancestor): every folder is
  still walked, and siblings directly under the root still collapse to it.

## Design

### 1. Telling an INI-only change apart (`photon-core`, `watcher/fs.rs`)

- `changed_dirs` looks at each event path's **file name** before reducing it to a directory.
  A path is *INI-only* when its name is `.picasa.ini` or `Picasa.ini` (ASCII case-insensitive,
  as the reader matches), or photon's own temporary `<ini name>.photon-*.tmp`. One predicate,
  `picasa::is_ini_write(name)`, in `picasa.rs` beside the writer that makes those names - the
  same reason the writer and the reader share one line classifier.
- **A folder is INI-only when every changed path in the batch that reduces to it is
  INI-only.** Any other path into it - a photo, a subdirectory, an event on the directory
  itself - makes it an ordinary walk. A foreign tool's temp-then-rename onto the INI (Picasa's
  own, if it uses one) is folded by `notify-debouncer-full` onto the final path, so it arrives
  as the INI itself and is correctly INI-only; only a foreign temporary that survives
  debouncing - left behind, or split across batches by the debounce window - is seen as its own
  path and makes the folder a walk.
- `changed_dirs` returns `(dirs, ini_dirs, lost)`: a directory in both is in `dirs` only.
- A deleted INI still arrives as a path named `.picasa.ini`: INI-only. The pass then finds no
  INI and clears that folder's stars, faces and Picasa albums, as a scan does today - the INI
  is the only authority.

### 2. The INI pass (`photon-core`, `scanner.rs`)

- `pub fn refresh_picasa(lib, watched, dirs: &[PathBuf], cancel: &AtomicBool) ->
  Result<IniPass>`, beside `scan_subtree`, where
  `IniPass { report: ScanReport, needs_walk: Vec<PathBuf> }`.
- If the root is offline it does what `scan_subtree` does (marks it offline, reports
  `offline`). A `dir` outside the root is skipped.
- Each `dir` is looked up in `folders` by its stored path (the watcher already canonicalized
  it). A folder with no row has never been scanned: an INI appearing in a new folder is not
  this pass's to handle, and the dir is returned in `needs_walk`.
- The known `(dir, folder_id)` pairs go through the existing `apply_picasa(lib, &walked,
  &IniEvidence::none())`, a new constructor meaning "no listing: probe every folder", so each
  folder is read by `read_folder`. One small directory listing per folder, not a walk; no
  photo is opened or stat'ed. Stars, faces, the hidden flag and albums are applied by the
  scan's own function, so the pass cannot drift from the scan's rules.
- The report carries `restarred`, `refaced`, `rehidden` and `realbumed`; `touched_rows()`
  decides whether the grid rebuilds, as for a scan.
- No mark or purge, no folder pruning, no thumbnails, no metadata backfill, no hashing
  request. `cancel` is checked between folders.

### 3. Scheduling (`photon-app`, `engine.rs` and `watch.rs`)

- **Engine.** `run_scan` takes `ScanKind { Full, Subtree(PathBuf), Ini(Vec<PathBuf>) }` in
  place of `Option<PathBuf>`; `start_ini_pass(watched, dirs) -> bool` starts one. It runs
  under the **same per-root scan slot**, so it never interleaves with a scan's own
  `apply_picasa` over the same folders (where the older INI read could land last). It emits
  no scan-progress events - nothing is being scanned from the user's point of view - refreshes
  the grid through `refresh_grid` only when `touched_rows()`, requests no hashing pass and
  records no full-scan time. `needs_walk` dirs are scanned as subtrees right after, on the same
  thread and slot.
- **Pending state per root** becomes `{ dirs: Vec<PathBuf>, ini: BTreeSet<PathBuf> }`.
  `dirs` keeps today's rules (`insert_pending`, the cap of 8, collapse to the root). `ini` has
  **no cap** - it is bounded by the folder count, and one pass takes the whole set. An INI dir
  already covered by a queued walk (`is_within`) is not added; queueing a walk drops the INI
  dirs it covers; a collapse to the root clears `ini`.
- **A batch** (`handle_batch(dirs, ini_dirs)`): walk dirs go through `plan_scans` exactly as
  today. INI dirs covered by a walk in the same batch are dropped (the walk rereads their INI);
  the rest are grouped per root, excluded directories and dirs outside every root dropped as
  `plan_scans` does, and each root gets one `start_ini_pass` with all of them - or they go
  into `pending.ini` if the slot is busy.
- **Draining** (every `TICK`): a root's pending walk goes first, one per tick as today (a walk
  rereads the INIs it covers); when no walk is pending, the whole INI set runs as one pass.
- `pending_len` counts both sets.

## Testing

Each test is shown to fail with its rule reverted (CLAUDE.md).

- `watcher/fs.rs` (pure, over constructed `DebouncedEvent`s): photon's star write (create,
  modify and rename of its temporary to the INI) is INI-only; an INI plus a photo in one
  folder is a walk; a directory event is a walk; a deleted INI is INI-only; an unrecognised
  temporary beside the INI is a walk.
- `picasa.rs`: `is_ini_write` accepts both INI names in any ASCII case and photon's temporary,
  and refuses a photo, `picasa.ini.bak`, and a temporary of another name.
- `scanner.rs` (temp library): a star written into an INI on disk is applied by the pass; a
  photo added to the same folder is *not* indexed (nothing was walked); an unknown folder
  comes back in `needs_walk`; a deleted INI clears the folder's stars; an offline root reports
  `offline`.
- `watch.rs` (real engine and fixtures, `handle_batch` called directly as the existing tests
  do): 20 INI-only folders arriving while the slot is held leave all 20 in `pending.ini` and
  nothing collapsed to the root, and one drain applies a star written into each INI; an INI
  dir inside a walk dir in the same batch leaves only the walk; a queued walk over an INI dir
  drops it from `ini`; `set_stars` across 12 folders followed by the batch `changed_dirs`
  makes of those writes' events starts no subtree scan and no root rescan, and the ratings
  stand.

## Out of scope

- Suppressing photon's own echoes entirely (the pass costs one INI read per folder).
- Any change to what a scan does with an INI.
- Raising `MAX_PENDING_DIRS` for walks.
