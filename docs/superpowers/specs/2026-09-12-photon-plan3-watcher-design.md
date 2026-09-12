# photon v1 — Plan 3: Filesystem Watcher Design

**Date:** 2026-09-12
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md` (amended by this plan's scope decision, below)
**Builds on:** `photon-core` (Plan 1, `1e7d510`) and `photon-app` + `ui` (Plan 2, `c98cfea`)

## 1. Scope

photon currently learns about changes only at startup or when the user clicks Rescan. This plan makes it notice changes as they happen.

### In scope

- A `watcher` module in photon-core wrapping `notify`, with debouncing.
- A **subtree scan**: the existing scanner, restricted to one directory and everything under it.
- A `WatcherService` in photon-app that turns filesystem events into subtree scans through the existing scan manager.
- Graceful degradation when the OS won't let photon watch everything.
- Re-registering watches when an offline drive comes back.

### Out of scope

- Packaging and installers — Plan 4.
- HEIC/AVIF and video — deferred beyond v1 for now.
- Any new UI surface. The watcher reuses the existing `library-changed` and `scan-progress` events, and the status bar.

### v1 scope change

The parent spec lists HEIC/AVIF and video among v1's formats. They are deferred: v1 ships JPEG, PNG, GIF and WebP. The parent spec is amended to match, and `MediaKind` keeps its single `Image` variant.

## 2. Engine changes (photon-core)

### 2.1 Subtree scan

`scan_subtree(lib, watched, dir, scan_id, options, progress) -> Result<ScanReport>` scans one directory and its descendants.

It must restrict **all four** of the scanner's effects to that subtree. `scan_watched` compares what it saw against every item under the watched root; pointing that logic at one directory would make everything outside look deleted. So:

1. **The walk** covers `dir` only, with the same hidden-file and exclusion rules.
2. **The known-items set** it diffs against covers only items under `dir`. Membership is resolved through the folder tree, not string prefixes: a recursive query finds the folder row for `dir` and all its descendants through `parent_id`, and the known set is the items in those folders. This avoids escaping `%` and `_` in paths entirely, and a directory with no folder row yet correctly yields an empty set.
3. **Marking and purging** apply only to that known set. The soft-delete-then-purge rule is unchanged: a file missed once is hidden, and only forgotten when a later scan of a reachable directory misses it again.
4. **Pruning** removes only folders within the subtree, using the same descendant query.

**A directory that no longer exists** is not an error. The scan walks the nearest ancestor that still exists within the watched root instead, which is what correctly handles a deleted folder: the parent's walk sees it gone, hides its items and prunes it.

`scan_watched` keeps its current behaviour and is still what startup and manual rescans use.

### 2.2 Watcher module

`watcher::Watcher` wraps `notify` with `notify-debouncer-full`:

- It watches each online watched root recursively.
- Events are debounced for **2 seconds**, then reported as a set of affected **directories** (a file event reports its parent directory).
- The `notify`-facing part is deliberately thin. The policy — debouncing, mapping directories to watched folders, coalescing, dropping excluded paths — sits behind a seam so it can be tested without touching a real filesystem.
- Registration failures are reported per root rather than failing the watcher.

## 3. App changes (photon-app)

A `WatcherService` owned by `Engine`:

- Starts after the first startup scans, and re-registers when watched folders are added or removed.
- Maps each changed directory to its watched folder, drops anything inside `Engine::excluded`, and requests a subtree scan through the existing `ScanManager`, so watcher-driven work is serialised per folder and cancellable exactly like a manual rescan.
- **Collisions:** if a full scan of that folder is already running, the subtree request is dropped, because the full scan will see those files. If a subtree scan for the same folder is running, at most one follow-up is queued, and further events coalesce into it.
- **Degraded roots:** a root whose watch could not be registered goes on a periodic rescan every **5 minutes**, and the UI is told once.
- **Offline roots:** every **30 seconds**, offline roots are checked for reappearance. One that is back is rescanned and its watch re-registered, so its photos undim on their own.

No new IPC commands. The existing `library-changed`, `scan-progress` and `folder-status` events already carry everything the UI needs; degraded watching is surfaced through the status bar.

## 4. Error handling

- **Registration failure** (watch limits, permissions) never fails a folder: it degrades to periodic rescan, says so once, and retries registration on the next poll rather than spinning.
- **Events inside photon's own cache or database directory** are dropped before a scan is considered.
- **A file still being written** is handled by existing behaviour: the debounce absorbs the burst, a half-written file that fails to decode stays pending rather than being marked broken, and when its size and timestamp settle a later event re-indexes it.
- **A renamed folder** arrives as changes to both directories: the old subtree scan hides its items, the new one adds them. They take new identities and new thumbnails, because a photo's cache key includes its path.
- **A watcher thread that dies** is logged, marks its root degraded, and falls back to periodic rescan.
- **A burst larger than the debounce window** is bounded by the scan manager: one scan per folder at a time, with one coalesced follow-up.

## 5. Testing

- **Subtree scan** carries the most risk and gets the most tests:
  - items outside the scanned subtree are never marked missing;
  - a file deleted inside it is hidden, then purged by a later scan;
  - pruning and purging stay inside the subtree;
  - excluded directories are skipped;
  - a vanished directory scans its nearest existing ancestor;
  - cancellation leaves nothing marked.
- **Watcher policy** is tested as pure functions over synthetic events: debouncing, directory-to-folder mapping, coalescing, and dropping excluded paths. One real-filesystem test exists but is excluded from CI, where its timing is flaky.
- **`WatcherService`** is tested with an injected fake event source: an event in an excluded folder starts no scan; an event during a full scan is dropped; a burst becomes a single scan; a degraded root is polled.
- **Manual smoke checklist** gains: copying a photo into a watched folder makes it appear within a few seconds; deleting one removes it; unplugging and replugging a drive recovers on its own; and on a library large enough to exhaust watch limits, the degraded notice appears rather than silence.

## 6. Success criteria

- A photo copied into a watched folder appears in the grid within about 5 seconds, without the user doing anything.
- A photo deleted on disk disappears from the grid, and is only forgotten once a later scan confirms it.
- No filesystem event can cause an item outside the changed directory to be hidden or deleted.
- A library too large to watch entirely still works, with live updates replaced by periodic rescans and the user told once.
- An unplugged drive's folder recovers on its own within about a minute of being plugged back in.
