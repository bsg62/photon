# Show a photo's duplicates

2026-09-22

The Duplicates view answers "which photos in my library have a copy?" and nothing more: it
holds every such photo at once, in folder order, so the three files that are one picture sit
wherever their folders happen to sort. The question a person actually has in front of one
photo is narrower — *where are the other copies of this one?* — and today there are two
half-answers to it. The viewer's info panel lists the copies as paths ("Identical copy",
"Looks the same"), each a link that locates it in the grid one at a time; and compare mode
can put two to four photos side by side, once you have found and selected them.

This adds the direct answer: **right-click a photo, "Show N duplicates"**, and the grid holds
that photo and its copies, and nothing else.

## Decisions taken before the design

- **A grid view, not compare.** Compare holds at most four photos, so a group of five would
  be cut off, and a grid of the group is what makes every other tool — star, compare, export,
  reveal — work on it without a second mechanism.
- **The menu item appears only when the photo has copies.** Nearly every photo has none, so
  an item that is always there would almost always lead to an empty view. The grid rows do
  not know which photos have copies, so the menu asks.

## What a person sees

Right-click **one** photo (a single-photo selection; the item never appears for several). The
menu opens at once, as now; a lookup of the photo's copy count runs alongside, and when it
answers with at least one the item **Show 1 duplicate** / **Show N duplicates** appears,
placed after *Reveal in file manager* and before *Star*. A lookup that answers after the menu
has closed, or after it has reopened on a different photo, changes nothing.

Choosing it switches the grid to the **Copies** view: the photo itself plus its copies. The
photo that was clicked is selected and scrolled into view — it is the one the person was
looking at, and among near-identical thumbnails it is otherwise easy to lose.

In the sidebar, under the Duplicates row, an indented row **Copies of IMG_1234.jpg** appears
while the view is open and is the active row. It is not a saved place: leaving the view
removes it, and clicking Duplicates (or any other row) leaves the view.

If the group shrinks while it is shown — a copy was deleted and the rescan purged it — the
view keeps showing what is left. When only the photo itself remains, the grid shows it with
the line *No other copies of IMG_1234.jpg any more.* When the photo itself is gone as well,
the grid is empty with the same line; the sidebar row keeps the name it was opened with.

## What "its copies" means

Exactly the set the viewer's info panel already lists for the photo (`viewer_item`'s
`copies`): live rows sharing its non-NULL `content_hash` (byte-identical) and live rows
sharing its non-NULL `similar_group` (look-alikes), a row in both counted once. The view adds
the photo itself. One definition in two places would drift, so the count and the view are
written against the same rules as `copies_of` and `similar_of` and a test holds them
together.

Not transitive beyond that: a copy of a look-alike that is in neither of the photo's groups
is not shown. In practice the union-find already puts an identical twin of a look-alike into
the same `similar_group`, since the two have the same perceptual hash.

## Backend

**`GridView::Copies`**, a parameterised view like `Album`: the anchor photo's id is the view
argument in `ViewState.arg`, and `takes_argument` includes it. `entries_for` parses it as an
id the way `Album` does (an argument that is not an id names nothing, and gives an empty grid
rather than an error that would roll the view back) and binds it to `COPIES_FILTER` in
`library/duplicates.rs`:

```sql
AND i.missing_since IS NULL AND (
  i.id = ?1
  OR i.content_hash = (SELECT content_hash FROM items WHERE id = ?1)
  OR i.similar_group = (SELECT similar_group FROM items WHERE id = ?1))
```

A NULL `content_hash` or `similar_group` on the anchor compares as NULL and matches nothing,
which is what keeps a photo with no hash from pulling in every other unhashed row. Through
`entries_filtered`, so the filter reaches the folder-order driver as well as the outer
`WHERE`, as for every membership view: a folder is placed by its oldest *matching* photo, or
the sidebar and the grid disagree. Both halves have partial indexes already
(`items_content_hash`, `items_similar_group`). If the planner will not use them through the
`OR`, the filter takes `DUPLICATE_FILTER`'s shape instead — `i.id IN (SELECT … UNION ALL
SELECT …)` — and a plan test pins whichever form ships.

**`Library::copy_count(id) -> usize`**: the size of the copies set above, not including the
photo. Computed as `copies_of` ∪ `similar_of` by id, so it cannot disagree with the info
panel.

**Engine**: `set_copies_view(id)`, the same shape as `set_album_view` —
`rebuild_or_restore`, arg then view.

**IPC**, three files each, and an answer in `mock.js` for each:

- `copy_count(id) -> usize`
- `set_copies_view(id)`

**`GridInfo`** gains `copies_of: Option<CopiesOf>` where `CopiesOf { id, file_name }`,
reported only while the view is `Copies` (the typed-field rule every parameterised view
follows). `file_name` is read when `GridInfo` is built; a photo no longer in the library
reports the id with an empty name, and the UI keeps the name it already had. `api.ts` mirrors
it as `copiesOf: { id: number; fileName: string } | null`, and every `GridInfo` literal in the
UI tests gains the field in the same commit.

No schema change. Membership changes arrive the usual way — a scan that purges or rehashes
rows ends in `hash_after_scan` and a grid rebuild of the current view — so the view refreshes
itself with no new path through the refresh chain.

## UI

- **`Grid.svelte`** — the menu. On opening for a one-photo selection it calls
  `api.copyCount(id)`; the answer is kept only if the menu is still open on that same id
  (a generation counter, as elsewhere in the store). The item calls `onshowcopies(id)`.
- **`folders.ts`** — `showCopies(itemId, deps)`, the same shape as `locateItem`: cancel a
  pending search, `setCopiesView(id)`, then `offsetOfItem(id)` *against the new index*, then
  select. Looking the offset up before the switch would hand the grid an offset from the
  wrong index.
- **`App.svelte`** wires `onshowcopies` to it.
- **`library.svelte.ts`** — `setCopiesView(id)`.
- **`FolderTree.svelte`** — the indented, active *Copies of …* row under Duplicates while
  `info.view === 'copies'`. The Duplicates row itself shows while either view is open, even
  if the library-wide count has fallen to zero.
- **`Grid.svelte`** empty/lone-photo line as described above.

## Testing

Each test is shown to fail with its change reverted, per the conventions.

- **Membership** (`duplicates.rs`): anchor, an identical copy, a look-alike, a row sharing
  neither, a missing identical copy, and a second anchor with NULL hashes whose view holds
  only itself — the last is the input that separates `= (SELECT …)` from a join that lets
  NULLs through.
- **Order**: a folder placed by its oldest matching photo, the Duplicates view's existing
  test repeated for this filter.
- **Plan**: both halves of the filter served by their indexes.
- **Count agrees with the panel** (`commands.rs`): a photo whose copy is both identical and
  similar counts it once, and `copy_count` equals `viewer_item(...).copies.len()`.
- **Argument**: `set_view` away and back to a non-parameterised view clears the id; a
  non-numeric argument gives an empty grid, not an error.
- **`GridInfo`**: `copies_of` is reported only in the Copies view.
- **`showCopies`** (vitest): switches before looking up the offset, and a photo the view
  does not hold selects nothing.
- **`screenshots.rs`** already fails on an IPC command without a `mock.js` answer.

The stale-answer guard on the menu's lookup is effect wiring with no seam here; it goes on
the README's smoke checklist with the rest of the menu: right-click a photo with copies and
one without; right-click one, then quickly another; choose the item and see the photo
selected; open the view, remove a copy on disk, rescan.

## Not in this

- A *Show all in grid* link in the viewer's info panel beside the copies list.
- A mark on grid tiles that have copies.
- Anything that removes a copy. photon does not delete photos.

## Addendum, 0.25.2: the frozen hash

The whole-branch review found that deleting the photo a Copies view was opened on emptied
the view at the next scan, because every branch of the filter went through the photo's own
row. 0.25.0 made the message truthful; 0.25.2 keeps the twins. `set_copies_view` reads the
photo's `content_hash` into the argument (`"<id>:<hex>"`, `CopiesArg`), and the filter uses
it **only once the photo's row is gone**. While the row exists its own current hash
decides, so a file rewritten with new bytes (hash cleared) stops matching its old twins.

Look-alikes are not frozen. A group's id is its smallest member's id, so purging that
member renumbers the group at the next pass, and a stored group id would name nothing, or
later a different group. They drop out, and the view's notice points to Duplicates.
