# The grid index, views, sort and search

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

### The grid index is the spine

`Engine` holds one in-memory `GridIndex` (`grid.rs`) built from a single ordered query, plus a
version number. Everything downstream - folder sections, viewer navigation, the selection,
the sidebar - reads from that one index, which is why a "view" is just a different
row set rather than a different code path.

The refresh chain is worth knowing end to end, because a change that alters rows without
travelling it will update the database while the UI shows stale data:

```
scan/mutation → Engine::refresh_grid() → snapshot (view, query, epoch, seq)
              → query + GridIndex::build(), no engine lock held
              → publish_if_current(): discarded if the view changed (epoch) or a
                later-stamped rebuild already published (seq); else bumps version
              → events::library_changed → the window's channel (`events.rs`)
              → Engine::published → App::adopt → drawn
```

Rebuilds run unlocked so a view switch never waits behind a scan's rebuild; the two publish
checks are what make that safe, and both are needed - each has a test that fails only when
that guard is removed, which is worth keeping that way: the epoch's own window (the state
moved, its rebuild not yet landed) went untested for months while the `seq` stamp quietly
covered every scenario written for it. A setter's own rebuild is the authority
for a view change. A discarded rebuild never loses rows: every commit is followed on its own
thread by a rebuild stamped after it, and the highest stamp always publishes.

`Engine::open` does not build the grid: it publishes an empty index at `NOT_BUILT` (version 0,
which no real publish reaches, since each adds one) so opening the window, before its first
frame, costs a library open, not a whole-library query. `startup` builds it first thing. The
window treats that version as "not known yet" (`empty.rs`): no empty-library notice, no photo
count, and no last-folder restore.
`open` still fails on a library the grid query cannot run against, through
`check_grid_query`, which prepares the query without running it. A first build that fails
anyway is retried (`FIRST_GRID_BACKOFF`) and then published *empty* (`build_first_grid`):
left at `NOT_BUILT` the window drew nothing, and an unchanged library rebuilt only on a view
switch. The empty grid carries the error (`GridInfo::build_error`), so the UI says the
library could not be read instead of "No photos yet"; it is never published over a grid something
else built meanwhile, and the next successful publish clears it.

**An empty library says why, and only what the window knows** (the window's side is
"The launch" and "The empty library's panel", below). "No folder is watched" is the rare
state: `startup` watches the Pictures folder by itself when nothing is watched, so most
first runs are a watched folder with a scan in it, or with nothing in it. **A full scan
reports once before it walks**, with nothing seen, as soon as it has found its root
(`scan_watched`): the walk's own first report is 64 photos in, a flushed batch, or its end,
and an empty library said photon had found no photos in a folder it was a minute into
reading. Not a root that is not there (an unplugged drive is polled twice a minute), not one
last found offline (`watched.online`: an unmounted volume leaves an empty mount point, polled
the same way, and telling it from a live folder reads every known row first - the scan's
line came and went with each poll; the price is that the first scan after the drive is back
goes without the report), and not a subtree scan (the watcher runs one for every directory a
file changed in, each over in milliseconds: announced, each would flash the scan's line).
"Has found no photos" is never worded as a finished search ("looked and found none"): a
folder photon may not read is scanned and reported like an empty one. Hidden photos come
before all of it: with any, the library is not empty.

`LibraryChanged::data_changed` tells the UI whether to refetch the sidebar's collections
(albums, people, tags - the tag counts alone are ~220ms at 300k photos carrying three keywords
each; `tag_counts_100k` measures a third of that library). Every `refresh_grid`
marks the engine-wide `data_dirty` before it snapshots; the view setters' rebuilds
(`rebuild_or_restore`) do not, and neither does `refresh_grid_derived`, the rebuild after a
stored poster frame and after the hashing passes in `hash_after_scan` - a thumbnail state, a
content hash or a look-alike group is read by the grid and `GridInfo` (the Duplicates count),
never by an album, person, tag, tag rule or folder count, and frames arrive up to once a
second. Whichever rebuild *publishes* next swaps the flag back and sends it - except a failed
first build's empty stand-in, which announces a data change without taking the flag, so the
build that finally succeeds still carries it. Engine-wide
rather than per rebuild, because a data rebuild discarded by a view switch would otherwise
take its flag with it. A new writer calls `refresh_grid`; `refresh_grid_derived` only for a
write that no collection or Settings query reads, checked against those queries. A command
that has committed its write rebuilds through `refresh_after_write`, which logs a failed
rebuild rather than returning it: returned, the UI undid its side of a write that stands (a
star flipped back, a selection kept for a retry), and the flag the failed rebuild left set
has the next publish announce the write anyway.

`grid_info` sends the sections and folders only when their `layout_gen` - published with the
grid, moved only when a publish changes them - is not the one the UI names, and reads its four
counts from a cache keyed by `counts_epoch`. `data_snapshot` moves the epoch beside
`data_dirty`, and `hash_after_scan` moves it before each of its rebuilds (the Duplicates count
reads both `content_hash` and `similar_group`); a new writer that
changes something a count reads without going through `refresh_grid` must move it too, or the
sidebar shows a stale count until the next data write.

`ScanReport::touched_rows` gates the end-of-scan refresh on whether a scan actually moved
rows. A change that alters data by some *other* means must add its own counter to `ScanReport`
and fold it into `touched_rows`, or the grid silently never rebuilds. Today the counters
beyond the obvious four are `restarred`, `refaced` (Picasa faces), `rehidden` (Picasa's
`hidden=yes`), `realbumed` (Picasa's albums), `enriched` (the metadata backfill) and `moved` (a
row re-pointed to its file's new path, below).

**Grid order** (`items.rs`, `GRID_ORDER`) is the folder's oldest photo descending, then each
folder's photos oldest to newest. While the grid is grouped by folder the sidebar groups by the same value, so the list is an
index of the grid (under a date grouping it is not: see the grouping, below). Changing one without the other splits them onto different axes. `GRID_ORDER` reads
columns only `folder_order(shown, filter)` supplies, and `grid_query(select, shown, filter)` is the one place
the two are paired: in a filtered view (Starred) the driver's filter must equal the outer
`WHERE`, so a folder is placed by its oldest *matching* photo, which is what keeps the sidebar
and the grid agreeing. A query assembled by hand with `GRID_ORDER` and no driver compiles and
fails at `prepare`, only when that view is opened.

**The user's sort** (`photon_core::sort`, `ViewState.sort`, the `grid_sort` setting) is applied
*after* the view's query, in Rust, by `Library::sorted_entries`. Date is the view's own order
(`GRID_ORDER`, or Recent's), untouched; `reverse` turns the whole list over, so folder runs stay
contiguous. Modified, name and size sort every photo together and lay the grid out flat
(`Sort::layout`), which has no headers, no timeline, and a folder jump that lands on the
folder's first photo (`offset_of_folder`); the sidebar then drops its year groups and orders
folders by `FolderTally`'s `bytes`/`modified_ms` or by name (`arrange_folders`). The sort is part
of the `ViewState` a rebuild snapshots, so a sort change is guarded by the epoch like a view
switch; do not read it at query time instead. `GridEntry` carries `size`/`mtime_ms` unserialized
for it, and a name sort reads the names in a side query, because `GridEntry` is `Copy`.

**The grouping** (`sort::Grouping`, the `grid_group` setting) is a field of that `Sort`, not a
value beside it: by date it decides the order as well as the headers. `Folder` is the view's own
order and layout; `Day`, `Month` and `Year` are one timeline across folders, newest first
(`arrange`: a stable sort by `taken_at` over the view's rows), laid out as `Layout::Periods`,
in every view, Recent included; `None` is that timeline flat. Under any other key it is kept and
ignored, so the sort control changes a `Sort` by spreading it - built from its two fields, it
dropped the grouping. A `Section` carries its `period` as numbers (`grid::Period`, read with
`civil_from_unix` like search's `2024-06`), and the window formats those (`period_label`),
never `taken_at_min`, which it would read again in another zone. A section has a header when it
has a folder or a period (`has_header`); code that means "flat" asks that, not "no folder",
which a period section has too - the Svelte `selectAll` did, and under a month header selected
a folder's scattered photos. The place photon remembers in the library is a folder, so its
restore and the way back to All photos ask `laid_out_by_folder`, not the sort key. Under a date grouping or
none a folder has no header, so a sidebar click lands on the first of its photos the grid
reaches (`offset_of_folder`, the flat answer) and the sidebar is no longer an index of the
grid.

**Hidden photos** (`library/hidden.rs`, schema 13) are in no view but `GridView::Hidden`.
`grid_query` takes a `Shown` argument (`Visible`, `Hidden`, or `Either` for bookkeeping like the
thumbnail queue) so every caller has to say which set it wants; the queries that do not go
through it - Recent, every sidebar count, `duplicate_ids!`, `copies_of`, `similar_of` - each
carry `hidden = 0` themselves, and `library/hidden.rs`'s tests pin every one. A new
user-visible query must filter on it too, or photos the user put away turn up again. Hidden
photos are still scanned, thumbnailed and hashed, so unhiding is instant. **Hide folder** (schema
16) is `folders.hidden` plus `items.hidden` on its photos, and `insert_items` gives a new row its
folder's flag - so visibility stays that one column and no query joins `folders` to decide it.
Any other writer that creates item rows must inherit the flag the same way, and
`set_folder_hidden` writes missing rows too, since `update_items` revives a row without touching
`hidden`. `move_items` (below) does: it writes `MAX(hidden, the destination folder's)`, so a
hidden folder hides what arrives in it and a photo the user hid never comes back out by moving.
**It also forgets `picasa_hidden` for a row that is hidden once that write is done.** The
column is the INI's answer for the name the file had in the folder it was in, and the hidden
pass follows a *change* in it (`apply_folder_hidden`): renamed, or moved without its INI, the
photo has no line where it is now, and against a kept "yes" that read as Picasa un-hiding it -
a photo hidden in Picasa came back visible by being renamed
(`a_photo_hidden_by_picasa_stays_hidden_when_it_is_renamed`,
`a_photo_hidden_by_picasa_stays_hidden_when_it_is_moved_alone`), and a visible one moved into a
hidden folder was un-hidden there
(`a_photo_moved_into_a_hidden_folder_is_not_unhidden_by_the_ini_it_left`, the reason it is the
flag *after* the write that is asked, spelled out a second time in the SQL because a SET reads
the row as it was). Forgotten, the pass makes a first read, which follows a `hidden=yes` and
ignores a missing line. A row that stays visible keeps its answer, which is the whole record of
an unhide made in photon: forgotten too, a folder renamed with its INI hid the photo again
(`a_photo_unhidden_in_photon_stays_visible_when_its_folder_is_renamed_with_its_ini`;
`a_moved_row_forgets_picasas_answer_only_when_it_is_hidden` is the column's own test). The cost:
a photo hidden in Picasa, then renamed and un-hidden there between two scans of photon, stays
hidden until it is unhidden in photon.

**Parameterised views.** `GridView::Search`, `Person`, `Album` and `Tag` are selected by an
argument held beside the view in `ViewState.arg` (the query, a person's key, an album id,
a keyword). `entries_for(view, arg)` interprets it per view and binds it as a SQL
parameter; `GridInfo` reports it in typed fields (`search_query`, `person`, `album`, `tag`). `set_view` clears the argument unless it is re-entering the
same parameterised view, so a query can never be read as a person's key. The membership
views filter the driver as well as the outer `WHERE`, the same way Starred does, or the
sidebar's year groups and the grid disagree.

**Search** is one view, not a family of them. `search_entries` builds the haystacks — file
and folder name, make, model, lens, `50mm`/`f/1.8`/`iso400`, keywords through `EFFECTIVE_TAGS`, the caption,
and the capture date as `YYYY-MM-DD` — and `search::Query` holds the grammar: words AND,
capitals-only `OR`/`AND`, quotes, `camera:`/`lens:`/`tag:`/`person:`/`album:`/`folder:` each
confined to its own field, `is:`/`has:`/`near:`/`on:`/`faces:` asking about the photo rather than its text
(`has:face` and `faces:N`/`faces:N+` count its faces, below), `size:`/`iso:`/`aperture:`/`focal:`/`mp:`
comparing a number of the photo's (`Term::Number`, inclusive bounds in whole units, so every
spelling of one comparison is one term), a
leading `-` negating a token, dangling pieces ignored. A fact a term reads is a field of
`Haystacks` set per photo in `search_entries` and cleared in `Haystacks::truncate`; one set
only inside an `if let` (the caption's) and not cleared there carries over to the next photo. People, albums and the face counts are read from side
tables only when a query names one (`Query::needs`). A new searchable fact is a haystack there, not a view; a new *filter* is a
prefixed term. **The grammar is listed for the user in `search_help.rs`**, which the `?` in
the search box draws: its tests read `Query::terms` out of `search.rs` and fail on a prefix, an
`is:` or a `has:` the list lacks, and on one it offers that the parser would drop. It finds them by shape - `prefixed("x:")`, and the `"word" =>` arms
between `prefixed("is:")` and the next prefix - so a parser rearranged away from that shape
changes the test with it; the wording and the examples nothing checks. The query string is
the whole interface, so the window's links (the info panel's camera and lens) go through
`App::go`, which tells the search box first.

**A grid offset is only meaningful against one index version.** Indexing a photo into a
folder that sorts earlier shifts every later offset, so anything holding an offset across a
rebuild - the viewer, the grid selection - must re-find its photo by id
(`GridIndex::position_of`). Clamping catches only the offset falling off the end; in range the
consumer silently shows a different photo.

