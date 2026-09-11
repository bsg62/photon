# photon v1 — Plan 2: App Shell + UI Design

**Date:** 2026-09-11
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`. That spec stays binding; this document narrows it for Plan 2.
**Builds on:** `photon-core` (Plan 1, merged as `1e7d510`).

## 1. Scope

### In scope

- Engine (`photon-core`) changes deferred from Plan 1's final review: folder validation, scan exclusions, scan cancellation, and waiting thumbnail requests with duplicate requests merged.
- `photon-app`: the Tauri 2 shell, which covers the engine lifecycle, the scan manager, typed commands, events, and the `photon://` protocol.
- `ui/`: Svelte 5 + TypeScript, with a Picasa-style folder tree, a virtualized square-tile grid, a full-window viewer and a status bar.
- On first launch, the OS Pictures folder is added automatically.
- Extras:
  - Manage watched folders: add through a native picker, remove, rescan, and see scan progress.
  - Reveal a photo in the file manager.

### Out of scope

- HEIC/AVIF, video, the `notify` filesystem watcher, and packaging with native libraries (all in Plan 3).
- A thumbnail size slider, a viewer filmstrip, editing, albums, tags and search.
- End-to-end WebDriver tests.

## 2. Engine changes (`photon-core`)

### 2.1 Watched-folder validation

`Library::add_watched_folder` becomes `add_watched_folder(path, excluded: &[PathBuf])`:

1. **Canonicalise the path** with `dunce::canonicalize`, which resolves symlinks and avoids the `\\?\` prefix on Windows. A path that doesn't exist is an error (`Error::FolderNotFound`).
2. **Compare with existing folders.** Comparison is by path component, and ignores case on macOS and Windows (`cfg(any(target_os = "macos", windows))`).
   - An exact duplicate returns the existing watched folder, as it does today.
   - If the new path is an ancestor or a descendant of an existing watched folder, the call fails with `Error::FolderOverlap { existing: String }`.
3. **Check the exclusion list.** If the path is equal to, inside, or an ancestor of any `excluded` path (the thumbnail cache directory and the database directory), the call fails with `Error::FolderExcluded { path: String }`.

Tests that use folders which don't exist must create them in temporary directories.

### 2.2 Scanner exclusions and cancellation

`scan_watched` takes a `ScanOptions { excluded: Vec<PathBuf>, cancel: Arc<AtomicBool> }` argument.

- **Exclusions:** `filter_entry` skips any entry whose path is inside an excluded path (component-wise `starts_with`).
- **Cancellation:** the cancel flag is checked before each entry.
  - When it is set, the scanner flushes its pending batches and returns `ScanReport { cancelled: true, .. }`.
  - A cancelled scan never marks items missing, purges them or prunes folders, because it didn't see the whole tree.

### 2.3 Waiting thumbnail requests

Add `ThumbService::request(id, size, timeout: Duration) -> Result<PathBuf>`:

1. **Fast path:** if the item's cache file exists, return it right away.
2. **Failed items:** if the item is already `Failed`, return `Error::ThumbFailed(message)`.
3. **Otherwise wait for a worker:**
   - Raise the item to `Priority::Visible` and register a waiter.
   - The queue tracks items that are in progress. When a worker finishes an item, whether it succeeded or failed, it wakes every waiter for that id.
   - Many simultaneous requests for one id produce exactly one decode.
4. **Timeout:** after `timeout` (the app passes 30s), return `Error::ThumbTimeout(id)`. The job stays queued.

`get_or_generate` remains as a way to generate on the calling thread (the viewer uses it for previews when there are no workers, for example in tests). The protocol only uses `request`.

### 2.4 Thumbnail cache key in grid rows

`GridEntry` gains `thumb_key: u64`, the item's fingerprint. `GridEntry` stays `Copy`.

- `thumb_key` is serialised as a 16-character lowercase hex string, because JavaScript numbers can't hold a u64 exactly.
- The UI puts it in thumbnail URLs, so the webview can cache thumbnails forever. When a file changes, its fingerprint changes, which gives a new URL, so a stale thumbnail is never shown.

## 3. App (`crates/photon-app`)

### 3.1 Engine and lifecycle

`Engine` is held as Tauri managed state and contains:

- `Arc<Library>`, opened at `<app_data_dir>/library.db`;
- `Arc<ThumbCache>` at `<app_cache_dir>/thumbs`;
- `ThumbService` with `default_workers()` workers;
- `grid: RwLock<Arc<GridIndex>>`;
- `version: AtomicU64`;
- the `ScanManager`.

**Startup order:**

1. Open the library and build the grid from the database.
2. Show the window, which is usable immediately.
3. Spawn background work:
   - If there are no watched folders, add the Pictures folder (`app.path().picture_dir()`), provided it exists.
   - Rescan every watched folder.
   - Call `enqueue_pending`.
   - Collect thumbnail garbage once, at low priority, after the first scans finish.

**Shutdown:** cancel all scans, then drop the `ThumbService`, which joins its workers.

### 3.2 ScanManager

- At most one scan runs per watched folder at a time. Each scan runs on its own thread with its own cancel flag.
- Requesting a scan for a folder that is already scanning is a no-op.
- After each progress callback that added or changed items, and again at the end:
  - rebuild the `GridIndex` from `grid_entries()`, at most 4 times a second;
  - swap the `Arc`;
  - increment `version`;
  - emit `library-changed`;
  - call `enqueue_pending`.
- Progress is emitted as a `scan-progress` event at most 4 times a second.
- `remove_folder` cancels that folder's scan and waits for it to finish before deleting.

### 3.3 Commands

All commands return `Result<T, AppError>`. `AppError` serialises to `{ kind: string, message: string }`. Field names are camelCase.

| Command | Returns |
|---|---|
| `list_folders()` | `{ watched: WatchedFolder[], folders: Folder[] }` |
| `add_folder(path)` | `WatchedFolder` and starts a scan. Errors: `folderNotFound`, `folderOverlap`, `folderExcluded` |
| `remove_folder(watchedId)` | `()` |
| `rescan_folder(watchedId)` | `()` |
| `grid_info()` | `{ version, len, sections: Section[] }` |
| `grid_rows(offset, count)` | `{ version, rows: GridEntry[] }`, with count capped at 1000 |
| `grid_offset_of_folder(folderId)` | `number \| null` |
| `set_visible(ids)` | `()` |
| `viewer_item(id)` | `{ id, path, fileName, width, height, orientation, takenAt, thumbKey, thumbState, thumbError }` |
| `neighbours(id, radius)` | `number[]`, nearest first |
| `reveal_in_file_manager(id)` | `()`, using `tauri-plugin-opener`'s `reveal_item_in_dir` |

The folder picker is `tauri-plugin-dialog`, called from the UI; the chosen path goes to `add_folder`.

### 3.4 Events

- `library-changed { version, len }`
- `scan-progress { watchedId, filesSeen, added, changed, done, cancelled }`
- `folder-status { watchedId, online }`

### 3.5 The `photon://` protocol

It is registered with `register_asynchronous_uri_scheme_protocol`, so each request runs on a worker thread and the webview is never blocked.

- **`photon://thumb/<id>/<grid|preview>/<thumbKey>`**
  - Goes through `ThumbService::request(.., 30s)`.
  - Responds `200 image/webp` with `Cache-Control: max-age=31536000, immutable`.
  - `thumbKey` only makes the URL unique per file version, so the handler ignores it.
- **`photon://image/<id>`**
  - Streams the original file, with the MIME type chosen by file extension (jpeg, png, gif or webp).
- **Error responses:**
  - `404` for an unknown or missing item;
  - `422` for a `Failed` item, with its error message as a text body;
  - `503` for a timeout;
  - `500` for anything else.

URL shape: Linux and macOS use `photon://localhost/...`, and Windows uses `http://photon.localhost/...`. The UI builds URLs through one helper, `mediaUrl()`, that follows Tauri's convention for the current platform.

## 4. UI (`ui/`)

### 4.1 Structure

| Unit | Responsibility |
|---|---|
| `lib/api.ts` | The only module that calls `invoke` or `listen`. It has typed wrappers for every command and event, `mediaUrl()`, and hand-written TS types that mirror the Rust structs. |
| `lib/layout.ts` | Pure functions: `buildRows(sections, columns)` to a row model (header rows and tile rows, each with a fixed height); `rowAt(scrollTop)`; `rowTop(index)`; `itemsInRows(range)`; `rowOfItem(offset)`. |
| `lib/pages.ts` | Pure page cache with 200-item pages, keyed by `(version, page)`. It drops pages from older versions. |
| `lib/library.svelte.ts` | Runes store: grid info, folders, the page cache, scan status per watched folder, and selection. It subscribes to events. |
| `components/FolderTree.svelte` | The tree of watched folders and subfolders. Offline folders are dimmed and scanning folders show a spinner. Clicking a folder scrolls the grid to it. The context menu offers Rescan, Remove from photon and Reveal in file manager. A toolbar button offers "Add folder…". |
| `components/Grid.svelte` | Virtual scroller over `layout.ts`. It renders the visible rows plus two screens of buffer, loads the pages those rows need, and sends `set_visible` 150ms after scrolling stops. |
| `components/Tile.svelte` | A square tile: `<img src=mediaUrl(thumb/id/grid/thumbKey)>` with `object-fit: cover`. It shows a neutral placeholder until the image loads, a broken-image icon on error, and is dimmed if its folder is offline. |
| `components/Viewer.svelte` | A full-window overlay, described in 4.3. |
| `components/StatusBar.svelte` | Scan progress and the item count. |

### 4.2 Grid

- **Tile size:** 160px square with an 8px gap. `columns = floor((width + gap) / (tile + gap))`.
- **Header rows:** each section gets a 32px header row showing the folder name and path, with its tiles in rows below. The row model is rebuilt when the width or the sections change.
- **Offline dimming:** a tile is dimmed when its folder belongs to a watched folder whose `online` is false. The UI maps folder to watched folder from the `list_folders` data.
- **Selection and keyboard:**
  - A single selection.
  - Arrow keys move the selection by one tile, or by one column vertically, crossing section boundaries.
  - Enter opens the viewer.
  - Ctrl/Cmd+Shift+R reveals the selected photo in the file manager.

### 4.3 Viewer

- When the viewer opens, it shows the preview thumbnail right away. It then loads `photon://image/<id>` with `Image.decode()` and swaps it in once decoded.
  - Originals use CSS `image-orientation: from-image`.
  - Previews are already correctly oriented.
- It preloads the previous and next two items, using the `neighbours` command.
- Keys:
  - ←/→ move to the previous or next item;
  - Home/End jump to the first or last item;
  - Esc closes the viewer and restores the grid's scroll position and selection.
- A `Failed` item shows its `thumbError` message instead of an image.

## 5. Error handling

- **Command errors** show as a dismissible toast. Folder errors name the folder they clash with.
- **Protocol errors:**
  - the tile or viewer shows a broken state;
  - a `503` gets one automatic retry after 2s;
  - a `422` never retries.
- **An unreachable folder** goes offline: its tree entry and tiles are dimmed, and nothing is deleted. This is the Plan 1 behaviour.
- **A library that fails to open** (for example, a schema from a newer photon) shows a blocking error dialog and exits. It never overwrites the database.

## 6. Testing

- **`photon-core`:** unit tests for every change in §2:
  - overlap, duplicate and exclusion checks, including a case-insensitive test compiled only on macOS and Windows;
  - scan exclusion and cancellation (a cancelled scan leaves no items missing);
  - `request`: many concurrent requests for one id cause one decode; a failed item errors immediately; a timeout returns `ThumbTimeout`.
- **`photon-app`:** Rust tests for the command functions and the protocol handler. These are written as plain functions over `Engine`, so no webview is needed, and run against a temporary library with real JPEG files. They cover:
  - status codes, MIME types and headers;
  - `grid_rows` capping;
  - `add_folder` error kinds;
  - `remove_folder` cancelling a running scan.
- **`ui/`:** Vitest tests for `layout.ts` (row building, hit-testing, section boundaries), `pages.ts`, and the keyboard navigation logic. There are no component snapshot tests.
- **CI:** the workflow gains `npm ci`, `npm run check` (svelte-check), `npm test`, and a `cargo build -p photon-app` job on all three OSes. The Linux job installs `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev` and `librsvg2-dev`.
- **Manual smoke checklist** (in the plan):
  - on first launch, the Pictures folder is added and scanned;
  - scrolling stays smooth at 60fps while indexing;
  - the viewer shows an image within 100ms of opening;
  - unplugging a drive dims its folder;
  - removing a folder mid-scan works;
  - reveal-in-file-manager works on each OS.

## 7. Success criteria

- A fresh install shows Pictures thumbnails within seconds of launch, without the user doing anything.
- Scrolling a 100k-item grid stays at 60fps. Thumbnail generation never uses more than the worker pool's CPU budget, however fast the user scrolls.
- The viewer shows a picture within about 100ms, then the full resolution.
- Adding an overlapping or excluded folder is refused with a clear message.
