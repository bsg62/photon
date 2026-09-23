# Hiding a folder

2026-09-23. Approved in conversation the same day.

## What it is

Picasa's Hide Folder, on top of photon's Hide (`2026-09-23-photon-hide-photos-design.md`).
Right-click a folder in the sidebar and choose **Hide folder**: every photo in it is hidden,
and so is every photo that lands in it later, until **Unhide folder**. The user chose the
second half explicitly over "hide what is there now".

## Decisions

- **Stored on the folder, applied to its photos.** `folders.hidden INTEGER NOT NULL DEFAULT 0`,
  schema 16. Hiding the folder sets the flag and `items.hidden` on every live photo in it, in
  one transaction; unhiding clears both. Visibility stays a single rule - `items.hidden` - so
  the fifteen-odd queries that already honour it (`Shown`, the counts, `duplicate_ids!`,
  `copies_of`, `similar_of`) need nothing, and no query joins `folders` to decide what to show.
- **New photos inherit it.** `insert_items` writes `hidden` from the folder's flag, so a photo
  the scanner adds - a new file, or one renamed or moved into the folder, which is a new row -
  arrives hidden. Both `walk_tree` callers insert through it, so neither can be forgotten.
- **Unhiding the folder unhides everything in it**, including photos hidden one by one before.
  A folder-level answer is the one the user asked for; remembering per-photo hides underneath
  would need a second flag for a case nobody raised.
- **A photo unhidden by hand inside a hidden folder stays visible.** It is an exception the
  user made; rescans leave it alone, because only *new* rows inherit the flag.
- **Not recursive.** The sidebar lists each directory on its own, as Picasa did; hiding a
  folder does not hide its subfolders, and a subfolder created later is not hidden.
- **Picasa's `hidden=yes`** keeps acting per photo, followed on change as before.
- **A folder whose directory disappears** loses its row when pruned, and with it the flag -
  the same limitation a renamed photo's albums and edits have.

## UI

The sidebar folder row's context menu gains **Hide folder**, or **Unhide folder** on a
flagged folder. Nothing else is new: sidebar folder rows are the current view's sections, so
a hidden folder leaves All's list and appears in Hidden's without any code. `Folder` gains
`hidden` in `api.ts` and `mock.js`; the engine rebuilds the grid after the write.

## Tests

- `hiding_a_folder_hides_its_photos_and_only_its_own` - its photos leave All and appear in
  Hidden; a subfolder's photos and a sibling's do not.
- `a_photo_added_to_a_hidden_folder_arrives_hidden` - through `insert_items`, and end to end
  through a scan and a subtree scan.
- `a_photo_unhidden_inside_a_hidden_folder_stays_visible` across a rescan.
- `unhiding_a_folder_shows_everything_in_it`.
- Migration 16: an existing folder comes out visible; version tripwires move to 16.
- Engine: the write rebuilds the grid; the UI's menu label is effect wiring, on the smoke list.
