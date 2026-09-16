<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { locateItem } from './lib/folders';
  import { library } from './lib/library.svelte';
  import { resultsChanged } from './lib/search';
  import { searchBox } from './lib/search-box.svelte';
  import { clampSidebarWidth, SIDEBAR_DEFAULT, SIDEBAR_STEP } from './lib/sidebar';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import SearchBar from './components/SearchBar.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Viewer from './components/Viewer.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();
  let viewerAt = $state<number | null>(null);
  let sidebarWidth = $state(SIDEBAR_DEFAULT);
  let dragFrom: { x: number; width: number } | null = null;

  // Pointer capture keeps the drag alive when the pointer outruns the 5px bar or crosses
  // the grid, which would otherwise take the move events.
  function startResize(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragFrom = { x: e.clientX, width: sidebarWidth };
  }

  function moveResize(e: PointerEvent) {
    if (!dragFrom) return;
    sidebarWidth = clampSidebarWidth(dragFrom.width + e.clientX - dragFrom.x, window.innerWidth);
  }

  function endResize() {
    dragFrom = null;
  }

  function keyResize(e: KeyboardEvent) {
    const delta = e.key === 'ArrowLeft' ? -SIDEBAR_STEP : e.key === 'ArrowRight' ? SIDEBAR_STEP : 0;
    if (!delta) return;
    e.preventDefault();
    sidebarWidth = clampSidebarWidth(sidebarWidth + delta, window.innerWidth);
  }

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
      select: (offset, id) => {
        library.selectItem(offset, id);
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

<svelte:window onresize={() => (sidebarWidth = clampSidebarWidth(sidebarWidth, window.innerWidth))} />
<div class="app" style:--sidebar-width="{sidebarWidth}px">
  <div class="topbar" inert={viewerAt !== null}><SearchBar /></div>
  <aside class="sidebar" inert={viewerAt !== null}><FolderTree onjump={jump} /></aside>
  <!-- A focusable separator is a widget in WAI-ARIA (a window splitter); Svelte's a11y
       rules list `separator` as non-interactive regardless. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
  <div
    class="splitter"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize sidebar"
    aria-valuenow={sidebarWidth}
    tabindex="0"
    inert={viewerAt !== null}
    onpointerdown={startResize}
    onpointermove={moveResize}
    onpointerup={endResize}
    onpointercancel={endResize}
    onkeydown={keyResize}
  ></div>
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
    grid-template-columns: var(--sidebar-width) 5px 1fr;
    grid-template-rows: auto 1fr auto;
    height: 100%;
  }
  .sidebar {
    overflow: auto;
    background: var(--panel);
  }
  .splitter {
    cursor: col-resize;
    touch-action: none;
    background: var(--panel);
    border-left: 1px solid #0003;
  }
  .splitter:hover,
  .splitter:focus-visible {
    background: var(--accent);
    outline: none;
  }
  .content { min-width: 0; min-height: 0; }
  /* Its own grid row, so it stays put while the sidebar and the grid scroll under it. */
  .topbar { grid-column: 1 / -1; }
  .statusbar { grid-column: 1 / -1; }
</style>
