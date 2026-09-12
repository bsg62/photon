import type { Folder, Section } from './api';

export interface FolderRow {
  folderId: number;
  name: string;
  count: number;
  year: number;
  /** Capture time of the folder's newest photo, in seconds. Decides both the year group and
   *  the order within it. */
  takenAtMax: number;
}

export interface YearGroup {
  year: number;
  rows: FolderRow[];
}

/** Resolved in the viewer's local time rather than UTC: a person means their own new year,
 *  so a photo taken at 23:00 on 31 December belongs to the year they experienced. */
function yearOf(takenAtMax: number): number {
  return new Date(takenAtMax * 1000).getFullYear();
}

/** One row per folder that actually holds photos.
 *
 *  Sections exist only for folders with items, which is what excludes the empty intermediate
 *  folders the folder table still contains — the sidebar used to list those because it drew
 *  from `list_folders` instead. */
export function folderRows(sections: Section[], folders: Folder[]): FolderRow[] {
  const names = new Map(folders.map((f) => [f.id, f.name]));
  return sections.map((s) => ({
    folderId: s.folderId,
    // A section implies an item, which implies a folder row — but a section can arrive
    // before the folder list has been refreshed, and a blank name beats throwing.
    name: names.get(s.folderId) ?? '',
    count: s.count,
    year: yearOf(s.takenAtMax),
    takenAtMax: s.takenAtMax,
  }));
}

/** Years newest first, and within a year the folder whose newest photo is newest. */
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
      rows: [...group].sort((a, b) => b.takenAtMax - a.takenAtMax),
    }));
}
