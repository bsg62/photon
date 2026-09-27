# Folder aliases

2026-09-27. Approved in conversation the same day.

## What it is

A name for a folder that photon shows in place of its directory name. `DCIM_2019_0412` can read
**Easter at Grandma's** in the sidebar and the grid without anything changing on disk. It is a
photon-only feature: the alias is kept in `library.db` and nowhere else. Picasa's INI is neither
read for one nor written.

## Decisions

- **Keyed by folder id.** `folders.alias TEXT` is nullable, schema 21. NULL means no alias. Both
  `upsert_folder`'s `ON CONFLICT` branch and a rescan leave the column alone, so an alias survives
  every rescan of a directory that is still there.
- **A folder whose directory disappears loses its alias** when `prune_folders` deletes the row.
  A directory renamed or moved on disk is a new row, so it comes back without one. This is the
  same recorded limitation that a renamed photo's albums and edits have, and that a folder's
  hidden flag has.
- **It replaces the name in the sidebar and the grid.** The sidebar row shows the alias. The grid
  header shows the alias where the name used to be. Beside it, the header already shows the full
  path, which still ends in the real directory name, so nothing is lost. The sidebar row's `title`
  tooltip is already the path, too.
- **The name sort orders folders by what the sidebar shows**, so an aliased folder sorts under its
  alias (`FOLDER_ORDER.name` in `folders.ts`). The grid's flat name sort compares *file* names, not
  folder names, so the alias does not affect it.
- **Search matches both.** `search_entries` adds the alias as another haystack next to the folder
  name. A person who aliased a folder searches by the alias, and the real name still finds it
  because it is still on screen in the grid header's path.
- **An empty alias clears it.** The input is trimmed. Empty text, or text equal to the directory
  name, is stored as NULL, so "rename it back" does not leave behind an alias identical to the
  name. Aliases are capped at 255 characters, counted as characters and not bytes.
- **Settings is untouched.** The watched-folder list there is about places on disk and keeps
  showing paths. A watched root that holds photos also appears as an ordinary sidebar row, and it
  can have an alias there like any other folder.

## Backend

- Migration 21: `ALTER TABLE folders ADD COLUMN alias TEXT;`. No table is added, so the
  table-count tripwire stays at 12. The two version literals in `library/mod.rs` move to 21.
- `Folder` gains `alias: Option<String>`, which `list_folders` reports.
- `Library::set_folder_alias(folder_id, alias: Option<&str>)` normalises the value as described
  above and writes it. An unknown folder id is an error, not a silent no-op.
- `Engine::set_folder_alias` writes the alias and then calls `refresh_grid()`. Search results
  depend on the alias, so an open Search view has to rebuild. A change that skips the refresh
  chain leaves the grid showing stale results.
- The IPC command `set_folder_alias(folderId, alias: string | null)` is added in the usual three
  places (`commands.rs`, `ipc.rs`, `app.rs`), with an answer in `mock.js` (`SILENT`).

## UI

- `Folder` in `api.ts` gains `alias: string | null`. It is also added to the folder rows in
  `mock.js`, and to every `Folder` literal in the tests.
- The display name comes from one helper, `folderLabel(f) = f.alias ?? f.name` in `folders.ts`.
  `folderRows` uses it, and so does the grid header (`Grid.svelte`). No other code reads
  `f.name` for display.
- The sidebar folder row's context menu gains **Rename in photon…** (the label says where the rename happens: the directory keeps its name). It opens an inline field in place of
  the row's name, pre-filled with the current label and selected. Enter commits and Escape
  cancels, the same as the album editor. The field is a third `createAlbumEditor` instance, with
  `blankClears`. As with the album and saved-search fields, only one field is open at a time
  because opening another blurs the first, and a blur *commits* it: starting a folder rename
  saves a half-typed album name rather than discarding it, and the other way round. (Written
  first as "replaces"; corrected after the branch review to what the three editors do.)
- When the folder has an alias, the menu also offers **Use folder name**, which clears it.
- After the write, `library.setFolderAlias` calls `refreshFolders()`, as `setFolderHidden` does:
  a library change refetches collections, not folders.

## Tests

- Core:
  - `a_folder_alias_is_trimmed_and_cleared_by_empty_or_the_real_name`
  - `an_alias_survives_a_rescan`, through `upsert_folder`, and end to end through a scan and a
    subtree scan
  - `search_matches_a_folders_alias_and_still_its_name`
  - Migration 21: an existing folder comes out with no alias. The version tripwires move to 21.
- App:
  - `list_folders` reports the alias.
  - `set_folder_alias` bumps the grid version, and an open search for the new alias finds the
    folder's photos.
- UI:
  - `folderRows` shows the alias.
  - The name sort orders by the alias.
  - The editor factory's rename mode works for a folder id.
- Effect wiring: the menu items and the inline field go on the smoke checklist.

## Not in this release

- Aliases for albums, people and tags. Albums already have their own names. People and tags come
  from Picasa and from the files.
- Showing the alias in the viewer's info panel. It shows the file's path, and that stays.
