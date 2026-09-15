<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { locateItem } from './lib/folders';
  import { library } from './lib/library.svelte';
  import { resultsChanged } from './lib/search';
  import { searchBox } from './lib/search-box.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import SearchBar from './components/SearchBar.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Viewer from './components/Viewer.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();
  let viewerAt = $state<number | null>(null);

  onMount(() => {
    library.init().catch(library.reportError);
    return () => library.dispose();
  });

  // Spec §5: the grid returns to the top whenever the result set changes — a new view, or
  // a refined query within Search — since a scroll position from one set of photos is
  // arbitrary against another's. Tracked here rather than in FolderTree because App owns
  // the `grid` binding and its scroll helper.
  let last = { view: library.info.view, query: library.info.searchQuery };
  $effect(() => {
    const next = { view: library.info.view, query: library.info.searchQuery };
    if (resultsChanged(last, next)) {
      last = next;
      grid?.scrollToOffset(0, 'start');
    }
  });

  function open(offset: number) {
    library.selected = offset;
    viewerAt = offset;
  }

  function closeViewer(at: number) {
    viewerAt = null;
    library.selected = at;
    grid?.scrollToOffset(at, 'nearest');
    grid?.focus();
  }

  /** "Locate in photon" from the viewer. Closes it first so the grid is what lands on the
   *  photo; the view switch and the lookup order are `locateItem`'s. */
  async function locate(itemId: number) {
    viewerAt = null;
    await locateItem(itemId, {
      cancelSearch: () => searchBox.cancel(),
      currentView: () => library.info.view,
      setView: (view) => library.setView(view),
      offsetOfItem: (id) => api.gridOffsetOfItem(id).catch(() => null),
      select: (offset) => {
        library.selected = offset;
        grid?.scrollToOffset(offset, 'nearest');
        grid?.focus();
      },
    });
  }

  async function jump(folderId: number) {
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
</script>

<div class="app">
  <div class="topbar" inert={viewerAt !== null}><SearchBar /></div>
  <aside class="sidebar" inert={viewerAt !== null}><FolderTree onjump={jump} /></aside>
  <main class="content" inert={viewerAt !== null}>
    <Grid bind:this={grid} onopen={open} />
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
{#if viewerAt !== null}<Viewer offset={viewerAt} onclose={closeViewer} onlocate={locate} />{/if}
<Toasts />

<style>
  .app {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto 1fr auto;
    height: 100%;
  }
  .sidebar {
    width: 260px;
    min-width: 160px;
    max-width: 50vw;
    resize: horizontal;
    overflow: auto;
    background: var(--panel);
    border-right: 1px solid #0003;
  }
  .content { min-width: 0; min-height: 0; }
  /* Its own grid row, so it stays put while the sidebar and the grid scroll under it. */
  .topbar { grid-column: 1 / -1; }
  .statusbar { grid-column: 1 / -1; }
</style>
