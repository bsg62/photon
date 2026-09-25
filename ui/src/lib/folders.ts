import type { Folder, GridView, Section } from './api';

export interface FolderRow {
  folderId: number;
  name: string;
  count: number;
  year: number;
  /** Capture time of the folder's oldest photo, in seconds. Decides both the year group and
   *  the order within it. */
  takenAtMin: number;
}

export interface YearGroup {
  year: number;
  rows: FolderRow[];
}

/** Resolved in the viewer's local time rather than UTC: a person means their own new year,
 *  so a photo taken at 23:00 on 31 December belongs to the year they experienced. */
export function yearOf(takenAtMin: number): number {
  return new Date(takenAtMin * 1000).getFullYear();
}

/** One row per folder that actually holds photos.
 *
 *  Sections exist only for folders with items, which is what excludes the empty intermediate
 *  folders the folder table still contains — the sidebar used to list those because it drew
 *  from `list_folders` instead.
 *
 *  Per folder, not per section: a view whose order is not folder-first hands back a folder
 *  as several runs — Recent orders by date across folders, so a folder reappears every time
 *  its photos are the newest again. One row per section listed such a folder once per run,
 *  each row claiming the run's handful of photos. The folder's photos in this view are the
 *  sum of its runs, and its oldest photo — the minimum, matching `Section::taken_at_min` —
 *  decides its year and its place in the list. */
export function folderRows(sections: Section[], folders: Folder[]): FolderRow[] {
  const names = new Map(folders.map((f) => [f.id, f.name]));
  const rows = new Map<number, FolderRow>();
  for (const s of sections) {
    const row = rows.get(s.folderId);
    if (row) {
      row.count += s.count;
      row.takenAtMin = Math.min(row.takenAtMin, s.takenAtMin);
      row.year = yearOf(row.takenAtMin);
      continue;
    }
    rows.set(s.folderId, {
      folderId: s.folderId,
      // A section implies an item, which implies a folder row — but a section can arrive
      // before the folder list has been refreshed, and a blank name beats throwing.
      name: names.get(s.folderId) ?? '',
      count: s.count,
      year: yearOf(s.takenAtMin),
      takenAtMin: s.takenAtMin,
    });
  }
  return [...rows.values()];
}

/** Years newest first, and within a year the folder whose oldest photo is newest. */
export function groupByYear(rows: FolderRow[]): YearGroup[] {
  const byYear = new Map<number, FolderRow[]>();
  for (const row of rows) {
    const list = byYear.get(row.year) ?? [];
    list.push(row);
    byYear.set(row.year, list);
  }
  return [...byYear.entries()]
    .sort(([a], [b]) => b - a)
    .map(([year, group]) => ({
      year,
      rows: [...group].sort((a, b) => b.takenAtMin - a.takenAtMin),
    }));
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
 *  Awaiting `setView` is what the jump itself depends on: `jump` looks the folder up in the
 *  grid's current index, and racing that lookup against an unawaited view switch can return
 *  a stale or mismatched offset (see the Important 1 writeup — awaiting here is load-bearing,
 *  not stylistic).
 *
 *  Hidden is the one view left as it is. The sidebar's folders are the current view's
 *  sections, and every other view is a subset of All, so All holds the folder jumped to; the
 *  Hidden view is disjoint from All, and a folder whose photos are all hidden is not in All
 *  at all - the jump would land the user at the top of All with nothing selected. */
export async function enterFolder(
  folderId: number,
  deps: {
    cancelSearch: () => void;
    currentView: () => GridView;
    setView: (view: GridView) => Promise<void>;
    jump: (folderId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  const view = deps.currentView();
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
  currentView: () => GridView;
  setView: (view: GridView) => Promise<void>;
  lastFolder: () => Promise<number | null>;
  jump: (folderId: number) => void;
}): Promise<void> {
  deps.cancelSearch();
  // Read *before* the switch. The grid writes the folder at its top whenever All is showing,
  // and a freshly rebuilt All sits at its first folder until the jump: read after the switch,
  // the remembered place could already have been overwritten with the library's top - the
  // very reset this exists to avoid. A folder id, unlike an offset, needs no index to be read
  // against.
  const folderId = await deps.lastFolder().catch(() => null);
  if (deps.currentView() !== 'all') await deps.setView('all');
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
    currentView: () => GridView;
    setView: (view: GridView) => Promise<void>;
    offsetOfItem: (itemId: number) => Promise<number | null>;
    /** Given the id as well as the offset: the page at `offset` is not loaded at this
     *  point, so the store needs telling which photo it is selecting. */
    select: (offset: number, itemId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  const home: GridView = hidden ? 'hidden' : 'all';
  if (deps.currentView() !== home) await deps.setView(home);
  const at = await deps.offsetOfItem(itemId);
  if (at === null) return;
  deps.select(at, itemId);
}

