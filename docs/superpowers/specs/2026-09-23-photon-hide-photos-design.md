# Hiding photos

2026-09-23

## What it is for

The user asked for "a delete function - not a real delete of course, but a hide function to
clean up duplicates etc." photon never deletes, moves or writes a photo (CLAUDE.md,
Conventions), so the cleanup it can offer is Picasa 3's: **Hide**. A hidden photo leaves every
view, count and group, and waits in a **Hidden** view where it can be unhidden. The file on
disk is untouched; the fact lives in `library.db` beside stars and edits.

The duplicate workflow it serves: open Duplicates, pick the copy you don't want, Hide. Its
twin then has no visible copy, so both leave Duplicates at once - the view shrinks as you
work through it, which is what "clean up" means.

## Decisions taken without the user

The user was away and said to act as I see fit; these are the calls made, for them to overturn.

- **A flag on the row, not a table of paths.** `items.hidden INTEGER NOT NULL DEFAULT 0`,
  schema 13. Like albums and edits it is keyed by item id, so a file renamed outside photon
  comes back visible when its old row is purged: the recorded limitation those already have.
  A rewritten file (same path, new bytes) keeps the flag, as it keeps its star -
  `update_items` does not touch the column.
- **Hidden means gone from everything user-visible**, not only from All: every grid view
  (Search, Recent, Starred, People, Albums, Tags, Duplicates, Copies), every sidebar count, the
  tile copy mark and the info panel's copy lists. Anything else leaves a photo the user put away
  turning up again in a corner, which is the one thing a hide must not do.
- **Not hidden from bookkeeping.** Hidden photos are still scanned, thumbnailed (the Hidden view
  shows them), content-hashed and perceptually hashed, and their Picasa stars and faces still
  apply. Unhiding is then instant and complete, with nothing to catch up.
- **Duplicate membership is filtered at query time**, in `duplicate_ids!`, not by keeping hidden
  photos out of `similar_group`. The query is what the view reads, so hiding takes effect in the
  same refresh with no regroup pass to wait for. The cost: a look-alike group can still be
  *chained* through a hidden photo (A~H~C with H hidden keeps A and C together). With the pixel
  confirmation both links are close anyway, and a regroup on every hide is not worth that.
- **Settings → Folders' per-root photo count is unchanged.** It counts what is on disk, and the
  scan progress bar compares it with the scan's own counts, which include hidden files.
- **Where the actions live:** the grid's context menu (**Hide photo** / **Hide 12 photos**, and
  **Unhide …** in the Hidden view) and the viewer's context menu (**Hide photo** / **Unhide
  photo**). No keyboard shortcut in this first cut: `H` is free, but a destructive-looking action
  on one unmodified key deserves the user's say.
- **The sidebar row appears only while something is hidden** (or the Hidden view is showing),
  as Duplicates does: a permanent "(0)" row is noise. Icon: Lucide `eye-off`.
- **Locate in photon** from a hidden photo lands in Hidden, not All, since All no longer holds it.

## How visibility is enforced

`grid_query` and `folder_order` are the choke point for every grid view but Recent. They gain
a `Shown` argument - `Shown::Visible` (`AND i.hidden = 0`) or `Shown::Hidden`
(`AND i.hidden = 1`) - rather than the filter being folded into each view's own filter string,
so every caller has to *say* which set it wants and a new view cannot forget. Only
`GridView::Hidden` passes `Shown::Hidden`. Recent's own query, the counts (`starred_count`,
people, albums, tags, duplicates), `duplicate_ids!`, `copies_sql` and `similar_of` each gain
`AND hidden = 0`.

The partial indexes stay as they are: each is `WHERE missing_since IS NULL`, which the extended
`WHERE` still implies, so SQLite keeps using them; the existing plan tests prove it. A new partial
index `items_hidden ON items(folder_id, taken_at) WHERE hidden = 1 AND missing_since IS NULL`
serves the Hidden view and `hidden_count` without scanning the library.

## IPC and UI

- `set_items_hidden(ids, hidden) -> usize` (the three files, plus `api.ts` and `mock.js`).
  `Engine::set_items_hidden` writes in one statement, counts rows that actually changed, and
  refreshes the grid when that is non-zero. Unknown and missing ids are skipped.
- `GridInfo.hiddenCount`, `GridView::Hidden` (`'hidden'`), `ViewerItem.hidden` (the viewer's
  menu label, and Locate's target).
- After a hide or unhide in the grid the selection moves to the nearest photo that stays - the
  one after the lead, or before it at the end - as a file manager does after a delete, so the
  arrow keys carry on through Duplicates. It is chosen from the index the user was looking at,
  *before* the write (the rebuild's event can land before the command returns), and its new
  offset is asked for by id. Nothing hidden stays selected, or the next action would reach
  photos the user can no longer see. A lead that leaves the view some other way (hidden from
  the viewer, unstarred in Starred) is dropped from the selection by `rebindSelection` too.
- A hidden photo lists no copies (it has no copy mark either), so Hidden's tile menu offers no
  "Show N duplicates". A Copies view whose anchor is hidden keeps the anchor's visible copies and
  says "X is hidden; these are its copies" (`CopiesOf.hidden`).
- A folder row clicked while Hidden is showing jumps within Hidden: the sidebar's folders are
  the view's sections, and a folder whose photos are all hidden is not in All at all.
- The viewer needs nothing new for the photo on screen leaving the view: its `orphaned` state
  (built for unstarring in Starred) keeps it on screen, drops the "n / m", and ArrowRight
  continues from where it was. `viewer_item` must keep answering for a hidden photo.

## Tests, and what each pins

- Migration 13: an existing library gains the column, every row visible; the version tripwires
  move to 13 by hand; the table count does not move.
- `a_hidden_photo_leaves_every_view` - each `GridView` but Hidden, with a photo that would
  otherwise be in it (starred, tagged, in an album, with a face, matching a search, recent).
- `the_hidden_view_holds_only_hidden_photos`.
- `hiding_a_copy_takes_its_twin_out_of_duplicates` - for byte-identical and for look-alike twins.
- `hidden_photos_are_not_counted` - starred, people, albums, tags, duplicates.
- `a_rewritten_file_stays_hidden` - `update_items` keeps the flag.
- `the_hidden_view_is_served_by_its_index` - plan test.
- Engine: `set_items_hidden` refreshes the grid and reports only real changes; `viewer_item`
  answers for a hidden photo.
- UI: `locateItem` goes to Hidden for a hidden photo (pure module test).

The menus and the sidebar row are effect wiring with no component harness; they go on the
README's smoke checklist.

## Not in this design

A keyboard shortcut; hiding a whole folder (Picasa could); a "show hidden photos in place"
toggle. Each is a follow-up if the user wants it. Two more, from the branch review:

- **Picasa's own `hidden=yes`** - done in a follow-up (schema 14): read by the Picasa pass and
  followed on *change*, recorded in `items.picasa_hidden`. photon never writes the line, so
  mirroring it would undo every unhide in photon on the next scan; following changes lets the
  most recent answer win. The first read follows `hidden=yes` but not a missing line, so a
  photo hidden in photon before the pass existed stays hidden.
- **A keyword carried only by hidden photos** - done in a follow-up: `TagCount` carries
  `count` (visible photos, the sidebar's number; the sidebar leaves out a 0) and `total` (the
  tag manager's number). The list keeps every keyword, so the rename check still asks before
  merging onto one only hidden photos carry, and the keyword suggestions still offer it.
