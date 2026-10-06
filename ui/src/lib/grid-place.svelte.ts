/** Where the grid is, for the parts of the window that are not the grid.
 *
 *  `folderId` is the folder whose photos are at the top of the grid while it runs folder by
 *  folder, and null otherwise: a date grouping, a flat sort, an empty grid. The grid writes
 *  it (`topFolderId`, the same answer its pinned header draws and its last-place restore
 *  stores); the sidebar reads it to mark that folder's row, which is what makes the list an
 *  index of the grid while scrolling and not only on a click. */
export const gridPlace = $state<{ folderId: number | null }>({ folderId: null });
