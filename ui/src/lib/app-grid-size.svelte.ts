import { api } from './api';
import { library } from './library.svelte';
import { createGridSize } from './grid-size.svelte';

/** The app's grid tile size. The logic is `createGridSize`'s; this is the wiring it is
 *  injected with, in its own module so importing `createGridSize` in a test touches no
 *  Tauri. */
export const gridSize = createGridSize({
  load: () => api.gridTile(),
  save: (size) => api.setGridTile(size),
  onerror: (e) => library.reportError(e),
});
