# Grouping the grid

2026-10-06

## What

A **Group by** control in the top bar, beside the sort: **Folder** (the default, and what photon
has always done), **Day**, **Month**, **Year** and **None**. It decides where the grid's headers
fall, in every view, and is remembered across launches (`grid_group` in the `settings` table).

The request was for the grouping to be configurable with the folder staying the default. The
four other choices, the rule that grouping needs the Date taken sort, and newest-first order
under a date grouping were each asked about and chosen; everything else below is a call made
here and listed under "Calls made here" so it can be vetoed.

## Semantics

- **Folder** is today's grid, untouched: folders by their oldest photo, newest first, each read
  oldest to newest, under the folder's header. Recent stays flat.
- **Day, Month, Year** are one timeline across folders, newest photo first, with a header
  wherever the day, month or year changes: "Sunday, 4 October 2026 · 12 photos",
  "October 2026 · 312 photos", "2026 · 4,210 photos". The three and None show the *same order*
  and differ only in where the headers fall, so switching between them moves no photo past
  another.
- **None** is that timeline with no headers: one continuous run, as Recent is.
- **Reverse** turns the whole list over, as it does everywhere: oldest first, the periods with
  it.
- **Grouping needs the Date taken sort.** By Date modified, Name or Size every photo is sorted
  together and the grid is flat, exactly as now; the Group control is shown disabled with its
  stored choice, which applies again when the sort returns to Date taken.
- **One setting for every view.** Starred, a search, an album, a person, Duplicates and Hidden
  all follow it. Recent, which is already newest first, takes date headers under Day, Month and
  Year and is flat under Folder and None.
- **Which day a photo belongs to** is read from `taken_at` as the camera's wall-clock time
  (`civil_from_unix`), in Rust: the reading search's `2024-06` and Statistics already use, so a
  month's header and a search for that month hold the same photos.

### What follows the headers

- **The year strip** marks the years of a date grouping as it marks a folder grouping's. None
  has no headers to mark and no strip, like today's flat views.
- **The viewer's counter** ("3 / 312") counts within the section, as it always has: the folder,
  or now the day, month or year. Under None it counts the whole view.
- **Ctrl+A in All** selects what is under the lead photo's header: the folder, or the day, month
  or year. With no headers (None, or a flat sort) it selects the lead photo's folder, as a flat
  sort does today.
- **The sidebar** keeps its folder list in year groups under every grouping: the sort is still
  Date taken, and the list is still in date order. Under a date grouping or None a folder's
  photos are no longer one run, so a click lands on the first of them the grid reaches (its
  newest), as under a flat sort.
- **The place in the library** (the folder at the top, restored at launch and by "All photos")
  is remembered and restored only while the grid is grouped by folder. It is a place in the
  folder order; under any other arrangement the grid opens at its top.
- **Changing the grouping** scrolls to the top and clears the selection, as a sort change does.

## How

### Core

- `photon_core::sort::Grouping { Folder, Day, Month, Year, None }`, and `Sort` gains a third
  field: `Sort { key, reverse, group }`. The grouping is part of the order - a date grouping
  re-sorts the view - so it belongs to the value that already names the order, and it then
  travels everything the sort does with no second mechanism: `ViewState` and the epoch guard,
  `rebuild_or_restore`'s rollback, `set_sort`, `GridInfo.sort`, and the UI's "change in flight"
  (`LibraryStore.sort`), so a sort change and a grouping change made close together compose.
  There is **no new IPC command**.
- `Sort::arrange`: by date under `Folder` it does nothing, as now. By date under any other
  grouping it is a stable sort by `taken_at` descending over the rows the view's query returned;
  ties keep the view's own order, so the grid does not reshuffle between rebuilds. The other
  keys are unchanged and ignore the grouping. `reverse` still turns the result over last.
- `Sort::layout(view)`: flat for a key other than date; else by grouping - `Folder` is the
  view's own layout (`GridView::layout`, so Recent stays flat), `Day`/`Month`/`Year` are
  `Layout::Periods(unit)` in every view, `None` is flat.
- `grid::Section` gains `period: Option<Period>`, `Period { year, month: Option<u32>,
  day: Option<u32> }` (a year group has neither, a month group no day). `GridIndex::build`
  starts a new section when the entry's key differs from the last section's: the folder under
  `Folders`, the period under `Periods`, nothing under `Flat`. A section has a header when it has
  a folder or a period. `offset_of_folder` answers a section only under `Folders` and the first
  matching entry otherwise.
- The setting: `grid_group` holds `folder`, `day`, `month`, `year` or `none`; anything else
  reads as `folder`. `grid_sort` keeps its form, so an older photon opening the library reads
  the sort it knows and ignores the grouping. `Library::grid_sort`/`set_grid_sort` read and
  write both. No schema change.

### UI

- `api.ts`: `Grouping`, `Sort.group`, `Section.period`. Hand-written mirror, same commit.
- `GroupControl.svelte` beside `SortControl`: a `Select` labelled "Group by" calling
  `library.setSort({ ...sort, group })`. `Select` gains a `disabled` prop.
- `layout.ts`: a header row when `folderId !== null || period !== null`.
- `Grid.svelte`'s header: a folder section is drawn as now; a period section shows the period's
  label and its count, with no path. The label is built from the period's numbers
  (`new Date(year, month - 1, day)` formatted with `Intl`), never from a timestamp read in the
  viewer's zone, so it cannot drift from the day Rust put the photos in.
- `timeline.ts`: a period section's year is `period.year`; a folder section's stays
  `yearOf(takenAtMin)`.
- `viewKey`'s `order` carries the grouping, which is what scrolls a grouping change to the top.
- One predicate, "laid out by folder" (date key and `folder` grouping), replaces the three
  `sort.key === 'date'` tests that guard the last-folder restore, its write, and
  `returnToAll`. `arrangeFolders` keeps testing the key alone.
- `LibraryStore.selectAll`: "flat" becomes "the first section has no header", not
  `folderId === null`, which a period section also has.

### Screenshots and docs

- `mock.js` answers a grouped `grid_info` (period sections); one new shot, the main window
  grouped by month, so the count in `CLAUDE.md` becomes thirty-eight.
- `CLAUDE.md`'s "The user's sort" paragraph gains the grouping; `docs/smoke-checklist.md`
  gains its items.

## Testing

Each new test is shown to fail with its change reverted.

- `sort.rs`: `arrange` under each grouping (newest first across folders, ties in the view's
  order, reversed); the grouping ignored under each non-date key; `layout` for every
  (key, grouping, view), Recent included; the setting's round trip and its fallback.
- `grid.rs`: sections per unit - two photos either side of midnight on 31 December are two days,
  two months and two years, and a second apart on the same day are one; a period run's
  `offset`/`count`; `offset_of_folder` under `Periods`; the period's JSON shape.
- `items.rs`: `sorted_entries` under a date grouping in a filtered view (Starred), so the
  driver's folder order is shown not to leak into it.
- `engine.rs`: a grouping change rebuilds, is reported in `GridInfo`, is stored only after the
  rebuild succeeded, and is restored at the next open.
- UI (`vitest`): header rows for period sections, the three labels, year marks from periods,
  `viewKey` differing by grouping, the "laid out by folder" predicate, `selectAll` under a date
  grouping (a range) and under None (the folder's ids).
- A bench case in `grid` for the date-grouped build at 300k, to put a number on the extra sort.

## Calls made here

1. **Ctrl+A in All selects the period under a date grouping**, the folder otherwise. The
   alternative is the folder always, which under a month header selects photos scattered
   through other months.
2. **A grouping change does not keep the place.** Day, Month, Year and None share one order, so
   re-finding the top photo by id after the change would be natural there; it is new scroll
   machinery and is left out.
3. **The grouping rides in `Sort`**, not in a value and command of its own (the design as first
   presented named a `set_grouping` command). Same behaviour, one less thing to keep in step.

## Known limits

- A photo with no capture date is dated by its file's modification time, which is a UTC
  instant; read as wall-clock time like the rest, it can fall on the neighbouring day. Search's
  `YYYY-MM-DD` already reads it so.
- The sidebar's year groups and a folder header's month read `takenAtMin` in the viewer's local
  zone, the period headers as wall-clock time. A folder whose oldest photo lies within hours of
  New Year can be listed under a year other than the one its photos are grouped in. This
  mismatch exists today against search and is not changed here.
- By day, a large library has thousands of headers, and Recent can have one per photo. That is
  what the grouping asks for.
- The view's query still orders by folder before a date grouping re-sorts its rows.

## Not in this

- Sorting inside groups by Name, Size or Date modified.
- A grouping per view.
- A year strip for None.
- Grouping by anything but the folder and the date (camera, person, keyword).
- A shortcut for the control.
