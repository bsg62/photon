import type { Folder, FolderTally, GridView, Sort, SortKey } from './api';

export interface FolderRow {
  folderId: number;
  name: string;
  count: number;
  year: number;
  /** Capture time of the folder's oldest photo, in seconds. Decides both the year group and
   *  the order within it. */
  takenAtMin: number;
  /** The folder's photos in the view, in bytes. */
  bytes: number;
  /** The newest modification time among them, in milliseconds. */
  modifiedMs: number;
}

export interface YearGroup {
  /** `null` for the one headerless group a sort other than date lists every folder in. */
  year: number | null;
  rows: FolderRow[];
}

/** Resolved in the viewer's local time rather than UTC: a person means their own new year,
 *  so a photo taken at 23:00 on 31 December belongs to the year they experienced. */
export function yearOf(takenAtMin: number): number {
  return new Date(takenAtMin * 1000).getFullYear();
}

/** What a folder's header in the grid says after its name: how many of its photos the view
 *  holds, and the month its oldest one was taken - "23 photos · July 2026".
 *
 *  The oldest, not a range to the newest, for the reason the sidebar files a folder under its
 *  oldest photo's year: a photo with no date of its own is dated by its file, so one scan
 *  copied over yesterday would stretch a folder from 1998 "to" this month. The month is the
 *  viewer's own, like `yearOf`, so the header and the year the sidebar lists the folder under
 *  are the same instant read the same way. The count is the view's: in a search it is the
 *  photos that matched, which is what is under the header. */
export function folderSummary(count: number, takenAtMin: number, locale?: string): string {
  const photos = count === 1 ? '1 photo' : `${count.toLocaleString(locale)} photos`;
  const month = new Date(takenAtMin * 1000).toLocaleDateString(locale, { month: 'long', year: 'numeric' });
  return `${photos} · ${month}`;
}

/** What photon calls a folder: the user's alias, else its directory name. The one place that
 *  decides it, so the sidebar, its name sort and the grid header cannot disagree. The path,
 *  shown beside the header and as the sidebar row's tooltip, keeps the real name visible. */
export function folderLabel(f: Pick<Folder, 'name' | 'alias'>): string {
  return f.alias ?? f.name;
}

/** One row per folder that actually holds photos in the view.
 *
 *  Drawn from the index's tallies rather than `list_folders`, which is what excludes the
 *  empty intermediate folders the folder table still contains - the sidebar used to list
 *  those. One tally per folder however its photos are arranged: in Recent a folder
 *  reappears every time its photos are the newest again, and the tally already sums them. */
export function folderRows(tallies: FolderTally[], folders: Folder[]): FolderRow[] {
  const names = new Map(folders.map((f) => [f.id, folderLabel(f)]));
  return tallies.map((t) => ({
    folderId: t.folderId,
    // A tally implies an item, which implies a folder row — but a tally can arrive before
    // the folder list has been refreshed, and a blank name beats throwing.
    name: names.get(t.folderId) ?? '',
    count: t.count,
    year: yearOf(t.takenAtMin),
    takenAtMin: t.takenAtMin,
    bytes: t.bytes,
    modifiedMs: t.modifiedMs,
  }));
}

/** Years newest first, and within a year the folder whose oldest photo is newest. */
export function groupByYear(rows: FolderRow[], reverse = false): YearGroup[] {
  const sign = reverse ? -1 : 1;
  const byYear = new Map<number, FolderRow[]>();
  for (const row of rows) {
    const list = byYear.get(row.year) ?? [];
    list.push(row);
    byYear.set(row.year, list);
  }
  return [...byYear.entries()]
    .sort(([a], [b]) => sign * (b - a))
    .map(([year, group]) => ({
      year,
      rows: [...group].sort((a, b) => sign * (b.takenAtMin - a.takenAtMin)),
    }));
}

/** Folder names the way a person reads them: ignoring case, with `Trip 2` before `Trip 10`.
 *  The grid's own name sort (`sort::natural_cmp`) agrees on case and digits but not beyond:
 *  it has no collation tables, so there an accented letter sorts after `z`, while here it
 *  sorts beside its base letter. */
const byName = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** The order a key lists folders in, before `reverse`: the same direction the grid runs in
 *  for that key - A to Z, largest first, most recently modified first. */
const FOLDER_ORDER: Record<Exclude<SortKey, 'date'>, (a: FolderRow, b: FolderRow) => number> = {
  name: (a, b) => byName.compare(a.name, b.name),
  size: (a, b) => b.bytes - a.bytes,
  modified: (a, b) => b.modifiedMs - a.modifiedMs,
};

/** The sidebar's folder list under the user's sort.
 *
 *  By date it is `groupByYear`, and reversed it is that list turned over - years oldest
 *  first and each year's folders oldest first - which is the order the reversed grid reaches
 *  them in. By any other key the grid is flat and has no folder order to follow, so the list
 *  answers the key's question about folders instead (the biggest, the most recently touched)
 *  and drops the year headings, which would split a list sorted by name into pieces sorted
 *  by something else. Ties keep the order the grid first reaches each folder. */
export function arrangeFolders(rows: FolderRow[], sort: Sort): YearGroup[] {
  // Reversed by the comparison rather than by turning the result over: the rows already
  // arrive in the reversed grid's order, and turning the list over would flip every tie
  // back against it.
  if (sort.key === 'date') return groupByYear(rows, sort.reverse);
  const order = FOLDER_ORDER[sort.key];
  const sign = sort.reverse ? -1 : 1;
  const sorted = [...rows].sort((a, b) => sign * order(a, b));
  return sorted.length === 0 ? [] : [{ year: null, rows: sorted }];
}

/** Everything a folder jump has to do before it can scroll, in the order it has to do it.
 *
 *  Both steps are load-bearing, and both were once missing from the watched-root jump while
 *  the year-row jump had them:
 *
 *  Cancelling first stops a debounced search that has been typed but not yet sent. Left
 *  running, it fires after the view switch below has landed on `all` and re-enters Search
 *  with its captured text, replacing the grid the user just navigated to.
 *
 *  The view is read once every view command already issued has landed (`currentView` is
 *  `LibraryStore.settledView`): a search sent from All just before the click would
 *  otherwise read as All, skip the switch, and then land and carry the grid into Search.
 *
 *  Awaiting `setView` is what the jump itself depends on: `jump` looks the folder up in the
 *  grid's current index, and racing that lookup against an unawaited view switch can return
 *  a stale or mismatched offset (see the Important 1 writeup — awaiting here is load-bearing,
 *  not stylistic).
 *
 *  Hidden is the one view left as it is. The sidebar's folders are the current view's
 *  folders, and every other view is a subset of All, so All holds the folder jumped to; the
 *  Hidden view is disjoint from All, and a folder whose photos are all hidden is not in All
 *  at all - the jump would land the user at the top of All with nothing selected. */
export async function enterFolder(
  folderId: number,
  deps: {
    cancelSearch: () => void;
    currentView: () => Promise<GridView>;
    setView: (view: GridView) => Promise<void>;
    jump: (folderId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  const view = await deps.currentView();
  if (view !== 'all' && view !== 'hidden') await deps.setView('all');
  deps.jump(folderId);
}

/** "All photos" in the sidebar: back to the whole library, at the folder last browsed there.
 *
 *  An excursion (Starred, Recent, an album, a search) leaves All's place alone - the grid
 *  remembers the folder at the top of All for the next launch, and only while All is showing -
 *  so that remembered folder is where the user left the gallery. Clicking a folder instead
 *  lands on that folder's top, which is what made going back feel like a reset.
 *
 *  Like `enterFolder`, a pending search is cancelled first and the jump waits for the view
 *  switch to settle, since the folder's offset is only meaningful against All's index.
 *  Nothing remembered, or a lookup that fails, opens All wherever it opens. */
export async function returnToAll(deps: {
  cancelSearch: () => void;
  currentView: () => Promise<GridView>;
  setView: (view: GridView) => Promise<void>;
  lastFolder: () => Promise<number | null>;
  /** Whether the grid is sorted by date. The remembered folder is a place in the date
   *  order, written only while All is laid out by folder; in a flat sort a jump would land
   *  on that folder's first photo wherever the key put it - somewhere, not where the user
   *  was - so All opens at its top, as it does at launch (`Grid.svelte`'s restore). */
  sortedByDate: () => boolean;
  jump: (folderId: number) => void;
}): Promise<void> {
  deps.cancelSearch();
  // Already in All: the user is at their own place, so there is nothing to go back to. Not a
  // re-read and a jump - the grid saves its place fire-and-forget as the scroll crosses a
  // folder, a read right behind that save could still see the folder before, and the jump
  // would land on a folder's top rather than where the user is.
  if ((await deps.currentView()) === 'all') return;
  // Read *before* the switch. The grid writes the folder at its top whenever All is showing,
  // and a freshly rebuilt All sits at its first folder until the jump: read after the switch,
  // the remembered place could already have been overwritten with the library's top - the
  // very reset this exists to avoid. A folder id, unlike an offset, needs no index to be read
  // against.
  const folderId = deps.sortedByDate() ? await deps.lastFolder().catch(() => null) : null;
  await deps.setView('all');
  if (folderId !== null) deps.jump(folderId);
}

/** "Locate in photon" from the viewer: lands the grid on the photo, selected and in view.
 *
 *  The same shape as `enterFolder`, for the same two reasons: a pending search must not
 *  re-enter Search behind the jump, and the photo's offset is only meaningful against the
 *  index of the view it is looked up in, so a subset view (Starred, Search, Recent) is left
 *  for All *before* the lookup. Looking it up first and then switching would hand the grid
 *  an offset from the wrong index. A photo the library no longer holds selects nothing.
 *
 *  A hidden photo is in no view but Hidden, so that is where it is looked for: All would
 *  answer "not here" and the click would silently do nothing. */
export async function locateItem(
  itemId: number,
  hidden: boolean,
  deps: {
    cancelSearch: () => void;
    currentView: () => Promise<GridView>;
    setView: (view: GridView) => Promise<void>;
    offsetOfItem: (itemId: number) => Promise<number | null>;
    /** Given the id as well as the offset: the page at `offset` is not loaded at this
     *  point, so the store needs telling which photo it is selecting. */
    select: (offset: number, itemId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  const home: GridView = hidden ? 'hidden' : 'all';
  if ((await deps.currentView()) !== home) await deps.setView(home);
  const at = await deps.offsetOfItem(itemId);
  if (at === null) return;
  deps.select(at, itemId);
}

