import { TILE_WIDTH, type TileSize } from './layout';

export interface GridSizeDeps {
  load(): Promise<TileSize>;
  save(size: TileSize): Promise<void>;
  onerror(e: unknown): void;
}

/** How large the grid draws its tiles. One store behind both controls - the top bar's and
 *  Settings' - so they cannot disagree. */
export function createGridSize(deps: GridSizeDeps) {
  let size = $state<TileSize>('medium');
  /** Bumped by dispose() and by every init(), so a load started under an earlier generation
   *  can tell it is no longer current. In production this is a module singleton that
   *  App.svelte disposes on unmount and re-inits on remount (HMR), the same lifecycle
   *  library.svelte.ts documents for `LibraryStore` and `createTheme` solves the same way -
   *  a one-way `disposed` boolean would never let the singleton come back to life. */
  let generation = 0;
  /** True once `set()` has been called since the current init's load started, so the
   *  load's eventual answer does not clobber a choice made while it was still pending. */
  let setDuringLoad = false;

  return {
    get size() {
      return size;
    },
    get width() {
      return TILE_WIDTH[size];
    },

    async init() {
      const myGeneration = ++generation;
      setDuringLoad = false;
      try {
        const loaded = await deps.load();
        if (myGeneration === generation && !setDuringLoad) size = loaded;
      } catch (e) {
        if (myGeneration === generation) deps.onerror(e);
      }
    },

    /** Applies first and saves second, so the click is answered at once. A failed save
     *  keeps the size for this session: reverting it would punish the user for a disk
     *  error with a reflow. */
    async set(next: TileSize) {
      size = next;
      setDuringLoad = true;
      try {
        await deps.save(next);
      } catch (e) {
        deps.onerror(e);
      }
    },

    dispose() {
      generation++;
    },
  };
}

export type GridSize = ReturnType<typeof createGridSize>;
