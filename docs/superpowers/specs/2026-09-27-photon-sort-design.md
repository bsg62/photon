# Sorting the grid and the sidebar

2026-09-27

## What

A sort control in the top bar, beside the photo sizes: **Date taken** (the default), **Date
modified**, **Name**, **Size**, and a toggle that reverses whichever is chosen. It sorts the grid
and the sidebar's folder list, in every view, and is remembered across launches as one setting
(`grid_sort` in the `settings` table, stored as `name` or `-name` for reversed).

## Semantics

- **Date taken** is the order photon has always had: folders by their oldest photo, newest
  first, each read oldest to newest (Recent: newest first). Folder headers, the timeline and the
  sidebar's year groups are unchanged. Reversed, the whole list turns over: oldest folder first,
  each read newest to oldest, and the sidebar's years and folders run oldest first to match.
- **Date modified, Name, Size** are about the photos, not the folders. Every photo in the view is
  sorted together (newest change first, A to Z, largest first) and the grid is laid out flat,
  with no folder headers and no timeline, as Recent is. Ties keep the view's own order (a stable
  sort), so the grid never reshuffles between rebuilds.
- **Name** ignores case and reads digit runs as numbers (`IMG_2` before `IMG_10`), in Rust
  (`sort::natural_cmp`) for the grid and with `Intl.Collator({ numeric: true })` for folder names:
  SQL here folds ASCII only.
- **The sidebar** under a non-date sort lists every folder in one list with no year headings:
  by folder name, by the total size of its photos in the view, or by its most recently modified
  photo. Clicking a folder scrolls to the first of its photos in the flat grid.

## How

- `photon_core::sort::Sort { key, reverse }` with `arrange` (the sort) and `layout` (flat unless
  by date). `Library::sorted_entries(view, arg, sort)` runs the view's query and arranges it; a
  name sort fetches file names in one side query, since `GridEntry` is `Copy`.
- The sort lives in the engine's `ViewState`, so it travels the refresh chain and the epoch
  guard exactly like the view. `rebuild_or_restore` now restores the whole previous state, not
  the view and argument by name. `Engine::set_sort` stores the setting only after the rebuild
  succeeded.
- `FolderTally` gained `bytes` and `modified_ms`; `GridInfo` gained `sort`; one new command,
  `set_sort`. In the UI, `LibraryStore.setSort` shares the view chain but runs no view-switch
  hooks (the search box keeps its text), and clears the selection, whose Shift+click anchor is
  an offset.
- Under a non-date sort neither the launch restore nor "All photos" (`returnToAll`) jumps to the
  last folder browsed: that place is in the date order. A sort change scrolls the grid to the top,
  like a view change (`viewKey` carries the sort).
- `LibraryStore.sort` is the sort last asked for while a change is in flight, else the grid's:
  the control builds the next change on it, so two quick clicks compose, and a refused change
  puts the control back.
- Ctrl+A in All selects one folder's section by date; in a flat sort it selects the whole list,
  as in any other view, since there is no folder to scope it to.

## Known limits

- The grid's name sort has no collation tables (no ICU, by policy): an accented initial sorts
  after `z`. The sidebar's `Intl.Collator` sorts it beside its base letter, so the two lists can
  disagree on accents and punctuation, though never on case or digits.
- The background thumbnail backlog still fills in date order; what is on screen is asked for
  first whatever the sort, so this only decides the order off-screen tiles are rendered in.

## Not in this

- A list (details) view. "List" in the request meant the sidebar's folder list.
- Sorting within folders while keeping headers under a non-date key.
- A sort per view.
