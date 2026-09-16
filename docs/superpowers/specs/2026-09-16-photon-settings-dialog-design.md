# photon — Settings Dialog Design

**Date:** 2026-09-16
**Status:** Approved design, implemented
**Builds on:** v0.12.2

## 1. What changes

photon has no settings surface. Managing watched folders lives in the sidebar: an
"Add folder…" button at the top, and the watched roots pinned above the year groups, whose
context menu is the only place to rescan or remove one.

This design adds a **Settings dialog**, opened from a gear button in the top bar, and makes
it the single home for folder management. The sidebar becomes pure navigation.

The dialog has two sections in v1:

- **Folders** — the watched roots, each with its details and actions, plus "Add folder…".
- **About** — version, library location, licence.

Sections are a left-hand list inside the dialog, so a later section (appearance, thumbnail
cache, …) is one more entry rather than a new surface.

### Out of scope

Any setting that changes behaviour (there are none to move yet), a separate native window,
a native menu bar, and persisting which section was last open.

## 2. Form: a modal dialog

A gear button sits at the right end of the top bar, beside the search box. It opens an
overlay over the whole app, in the same shape as the Viewer:

- `App.svelte` owns `settingsOpen` and renders `<Settings onclose=…/>` after the app grid.
  While it is open, the top bar, sidebar, splitter and grid are `inert`, exactly as they are
  for the viewer.
- Escape, the close button, and a click on the backdrop close it. Focus moves into the
  dialog on open and back to the gear button on close.
- It is a `role="dialog"` with `aria-modal="true"` and a labelled heading.
- The gear button is itself inert while the viewer is open, so the two overlays never stack.

A separate Tauri window was considered and rejected: it needs its own window config,
capability grant and cross-window event wiring, and every piece of live state the Folders
section shows (scan progress, online, degraded) already lives in the main window's
`library` store.

## 3. The Folders section

One row per watched folder, in `library.folders.watched` order:

| Field | Source |
|---|---|
| Name (last path segment) and full path | `WatchedFolder.path` |
| Status: *Online* / *Offline* / *Live updates limited* / *Scanning… N files* | `online`, `library.degraded`, `library.scans` |
| Photo count | new, §5 |

Actions per row:

- **Rescan** — disabled while that folder is scanning, as today.
- **Reveal in file manager** — reveals the watched root itself (§5).
- **Remove from photon…** — same confirm dialog and wording as today.

Below the list, **Add folder…** opens the directory picker exactly as the sidebar button does
now; a refusal (nested folder) surfaces through `library.reportError` as today. An empty list
reads "No folders yet. Add one to start building your library."

The rows reuse the existing calls (`api.addFolder`, `api.rescanFolder`, `api.removeFolder`)
and `library.refreshFolders()`. The add/remove logic moves out of `FolderTree.svelte`
unchanged.

A row for an offline, never-scanned root must still render and still offer Rescan and
Remove — that is the case the sidebar pinning existed for (see §4), and the dialog now
carries that guarantee.

## 4. The sidebar after the move

- The "Add folder…" toolbar is removed.
- The pinned watched-root rows are removed, and with them `jumpToRoot`, the `watched` menu
  target and "Remove from photon" in the sidebar menu. The comment explaining why roots
  were pinned goes too; its reasoning moves to the Settings component, where the guarantee
  now lives.
- A folder row's context menu keeps **Rescan** and **Reveal in file manager** — both are
  navigation-adjacent and act on the row the user is pointing at.
- Scan progress is no longer shown as a spinner in the sidebar. The status bar already
  reports it ("Scanning X… N files"), and the Folders section shows it per root.
- The "No folders yet." empty state becomes a button, "Add a folder in Settings…", which
  opens the dialog on the Folders section. First launch still adds Pictures unattended, so
  this is only seen after removing every folder.

`FolderTree` gains an `onopensettings` prop for that button; `App` passes the same opener
the gear uses.

## 5. Backend additions

Four commands (`watched_folder_stats`, `app_info`, `reveal_watched`, `reveal_library`),
none of which writes anything, each through the usual three files (`commands.rs`, `ipc.rs`,
`app.rs`) with their TypeScript mirrors in `api.ts`.

**`watched_folder_stats() -> Vec<WatchedFolderStats>`** — `{ watchedId, photoCount }` for
every watched root. One grouped query in `photon-core`:

```sql
SELECT f.watched_id, count(*) FROM items i JOIN folders f ON f.id = i.folder_id
WHERE i.missing_since IS NULL
GROUP BY f.watched_id
```

A root with no items is absent from the result and shown as 0. Soft-deleted items are
left out, so the count agrees with the All view; an offline root's items are not
soft-deleted, so they still count. It is a separate call rather than a field on
`WatchedFolder` so `list_folders` — called on every scan-progress `done` and folder-status
event — does not grow an aggregate over `items`. The dialog fetches it on open and again on
each `library-changed` while open.

**`app_info() -> AppInfo`** — `{ version, libraryPath, licence }`: `CARGO_PKG_VERSION`,
the `db_path` the engine was opened with, and `CARGO_PKG_LICENSE`. The library path is
shown selectable with a "Reveal" button (a new `reveal_library` command).

**`reveal_watched(watched_id)`** — reveals a watched root. `reveal_folder` takes a *folder*
id, and an offline or never-scanned root may have no folder row, so the Folders section
cannot go through it.

Both reveal commands follow `reveal_folder`'s split: `commands.rs` resolves the path
(`watched_path`, `library_path`) and is what the tests exercise; the `ipc.rs` wrapper hands
it to `tauri_plugin_opener::reveal_item_in_dir`.

No schema change, no new plugin, no capability change.

## 6. Tests

- `photon-core`: `watched_photo_counts` counts per root, omits an empty root, counts items
  under nested folders against their root, and skips soft-deleted items. Demonstrated to
  fail with the `GROUP BY` removed or the `missing_since` filter dropped.
- `photon-app`: `app_info` returns the engine's `db_path`; `watched_path` on an unknown id
  is an error, not a panic.
- UI: the status label for a row (scanning beats offline beats degraded beats online) is a
  pure function in `ui/src/lib/settings.ts` with a vitest table. Dialog open/close and focus
  wiring is a Svelte effect with no harness (see CLAUDE.md) — covered by `svelte-check` and
  the smoke checklist, and the commit says so.

## 7. Smoke checklist changes

Replace the "Add folder…", "Rescan works…" and "offline, never scanned … still appears in
the sidebar" items with:

- [ ] The gear in the top bar opens Settings; Escape, the close button and a backdrop click
  close it, and focus returns to the gear.
- [ ] Settings → Folders lists every watched folder with its path, photo count and status;
  the status shows scan progress live.
- [ ] "Add folder…" in Settings adds a folder. Adding a folder inside a watched one is
  refused with a clear message.
- [ ] Rescan works from Settings and from a sidebar folder row. "Remove from photon…" asks
  first, works during a scan, and leaves the files on disk.
- [ ] A watched folder whose drive is offline and which has never been scanned appears in
  Settings as Offline, and can still be rescanned or removed from there.
- [ ] Settings → About shows the version matching the release tag, and "Reveal" opens the
  library's folder.
- [ ] After removing every folder, the sidebar offers "Add a folder in Settings…", which
  opens Settings on Folders.
