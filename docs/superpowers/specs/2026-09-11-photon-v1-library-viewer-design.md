# photon v1 — Library + Viewer Design

**Date:** 2026-09-11
**Status:** Approved design, pending implementation plan
**Amended:** 2026-09-12 — HEIC/AVIF and video deferred beyond v1. See `2026-09-12-photon-plan3-watcher-design.md` §1.

## 1. Vision and scope

photon is a cross-platform (Linux, macOS, Windows) desktop photo manager, a spiritual successor to Picasa 3. The long-term product covers library management, non-destructive editing, organization (albums, tags, ratings), faces and places. This spec covers **only v1: the library and a fast viewer**, the foundation everything else builds on.

### In scope (v1)

- Watched folders: photos stay where they are; photon never moves, renames or modifies user files.
- Incremental scanning plus live filesystem watching.
- A Picasa-style library window: folder tree on the left and one continuous, virtualized thumbnail grid on the right, with folder/date section headers.
- A full-screen viewer with instant preview, full-resolution swap and neighbour preloading.
- Formats: JPEG, PNG, GIF and WebP.
- Smooth operation up to about 100,000 items.

### Out of scope (v1)

HEIC/AVIF and video, deferred on 2026-09-12 after Plan 2; the schema and `MediaKind` leave room for them. Edits, albums, tags, ratings, captions, search, faces, maps, XMP sidecar reading/writing, RAW formats, export/upload, slideshows. The database schema must not preclude them, but no code or tables are added for them now.

## 2. Tech stack

- **Core:** Rust (stable), Cargo workspace.
- **Shell:** Tauri 2.
- **UI:** Svelte 5 + TypeScript, built with Vite.
- **Database:** SQLite through `rusqlite`, WAL mode, embedded migrations.
- **Native libraries:** none. v1 decodes JPEG, PNG, GIF and WebP with pure-Rust crates plus libwebp, which the `webp` crate vendors. libheif and ffmpeg arrive with HEIC/AVIF and video, which are deferred beyond v1.

## 3. Architecture

```
photon/
├── Cargo.toml               (workspace)
├── crates/
│   ├── photon-core/         (pure Rust library, no Tauri dependency)
│   │   ├── library/         SQLite schema, migrations, queries
│   │   ├── scanner/         folder walk, incremental diff, fs watcher
│   │   ├── metadata/        EXIF / video metadata extraction
│   │   ├── decode/          format detection + decoders
│   │   ├── thumbs/          priority job queue, workers, disk cache
│   │   └── grid/            in-memory ordered index for the grid
│   └── photon-app/          Tauri shell: IPC commands, events, photon:// protocol
└── ui/                      Svelte + TypeScript frontend
```

### Unit responsibilities

| Unit | Responsibility | Depends on |
|---|---|---|
| `library` | Owns the SQLite database: watched folders, folders, media items (path, size, mtime, fingerprint, capture date, dimensions, orientation, media kind, thumb state, error, deleted/offline flags). Only module that issues SQL. | rusqlite |
| `scanner` | Walks watched folders, classifies files (extension + magic bytes), diffs against `library` (new / changed / missing), batches writes. Owns the `notify` watcher and its debouncing. | library, metadata |
| `metadata` | Extracts capture date, dimensions and orientation from images (EXIF). Pure function of a file path. | kamadak-exif |
| `decode` | Turns a file into an oriented RGBA bitmap at a requested maximum size, using pure-Rust decoders. | image |
| `thumbs` | Priority queue plus worker pool (cores − 1). Produces 256px (grid) and 1600px (preview) WebP files in the OS cache directory. Supports priority bumps and synchronous "generate now" requests. | decode, library |
| `grid` | An in-memory ordered index of all visible items (about 16 bytes per item), sorted by folder then capture date, with section boundaries. Answers row-range and section-offset queries. Rebuilt or patched on library changes. | library |
| `photon-app` | Tauri commands, event emission (throttled), `photon://` protocol handler, app lifecycle (startup rescan, shutdown). | photon-core |
| `ui` | Folder tree, virtualized grid, viewer. Talks to Rust only through typed IPC commands, events and `photon://` URLs. | Tauri JS API |

`photon-core` has no Tauri dependency, so all of its logic can be tested headless.

## 4. Image transport: the `photon://` protocol

All pixels reach the webview through a custom URI scheme served by Rust. Pixel data never goes over IPC.

- `photon://thumb/<id>/<size>`, where `size` ∈ {`grid`, `preview`}: serves the cached WebP. On a cache miss, the item is generated synchronously (jumping the queue) and then served.
- `photon://image/<id>`: streams the original file with its correct MIME type.

Responses carry `Cache-Control` headers so the webview's own cache helps on repeat views.

## 5. Data flow

### 5.1 Adding a watched folder

1. The UI calls `add_watched_folder(path)`. Rust persists it and starts a scan job right away, without waiting.
2. The scanner walks the tree on a background thread and filters by extension plus magic bytes. It writes to the database in batches of about 500 rows per transaction.
   - New file: insert with `thumb_state = pending`.
   - Changed file (mtime or size differs): update metadata and set `thumb_state = pending`.
   - Missing file: soft-delete (`missing_since = now`). The row is hard-deleted only when a later scan of a reachable folder confirms the file is still absent.
3. Metadata is extracted during the scan. The capture date falls back to file mtime when there is no EXIF or container date.
4. The app emits `scan-progress` (throttled to about 4 per second) and `library-changed` events. The UI refreshes affected visible rows.

### 5.2 Thumbnail generation

- A single priority queue: items currently visible in the grid come first (the UI reports visible IDs on scroll, debounced), then viewer neighbours, then everything else in grid order.
- Cache key: a content fingerprint of `hash(path) + mtime + size`. The cache path comes from the fingerprint; orphaned files are garbage-collected at startup in a low-priority task.
- The grid shows a neutral placeholder tile until the thumbnail is ready, then swaps it in.

### 5.3 Grid

- The `grid` index holds every non-deleted item, sorted by folder (tree order) then capture date, with a section per folder.
- IPC: `grid_rows(offset, count)` returns lightweight rows (id, section, aspect ratio, media kind, thumb state). `grid_sections()` returns section headers with their offsets. `grid_offset_of_section(folder_id)` returns where a folder's section starts.
- The UI uses a virtual scroller that renders only the visible rows plus a buffer. Clicking a folder in the tree scrolls the grid to that folder's section.

### 5.4 Viewer

- Double-click (or Enter) opens the viewer on an item. It shows `photon://thumb/<id>/preview` immediately, then swaps in `photon://image/<id>` once it has decoded.
- The previous and next 2 items are preloaded. Arrow keys navigate, Esc returns to the grid at the same scroll position.
- EXIF orientation is applied in Rust for all generated images (thumbnails and previews), and through CSS `image-orientation: from-image` for streamed originals.

### 5.5 Filesystem watching and startup

- A `notify` watcher on each watched folder. Events are debounced (about 2 seconds) and turned into targeted rescans of the affected directories.
- At startup: the grid is loaded from the database right away (UI is usable immediately), then a full incremental rescan runs in the background, since watchers miss changes made while the app was closed.

## 6. Error handling

- **Decode failure:** `thumb_state = failed` plus an error message stored on the item. The grid shows a broken-image tile, and the viewer shows the error. Not retried until the file's fingerprint changes.
- **Unreachable watched folder** (for example an unplugged drive): the folder is marked offline in the tree. Its items stay in the library and appear dimmed. Missing-file hard deletion is suspended for offline folders.
- **Database:** WAL mode, one writer connection owned by `library` plus a small read pool. Migrations run at startup inside a transaction. If a migration fails, the app shows an error and refuses to open rather than risk corrupting the library.
- **Errors in code:** `thiserror` types in `photon-core`, turned into typed error payloads at the IPC boundary. Logging uses `tracing`, with a rotating log file in the OS log directory.

## 7. Testing

- **`photon-core` unit tests:** scanner diff rules (new, changed, missing, offline), capture-date fallback, fingerprinting, grid index queries and patching. Fixture libraries are built in temporary directories.
- **Decoder fixtures:** a small generated set covering JPEG in all 8 EXIF orientations, PNG, GIF, WebP, and deliberately corrupt files.
- **Performance benchmarks** (criterion, on a synthetic 100k-item library):
  - `grid_rows` page query: under 50ms.
  - Warm startup to first grid data: under 1s.
- **UI:** Vitest for the Svelte components, especially the virtual scroller's offset and row math.
- **CI:** GitHub Actions matrix on Linux, macOS and Windows that builds and runs all tests. Installable packages (AppImage/deb, dmg, msi) come with packaging, which is Plan 4.

## 8. Success criteria

- Adding a folder of 100k photos shows the first thumbnails within seconds, and the grid stays responsive (60fps scrolling) while indexing continues.
- Opening any photo in the viewer shows an image within about 100ms (the preview), with full resolution shortly after.
- Every supported format displays on all three operating systems.
- photon never writes to, moves or deletes files inside watched folders.
