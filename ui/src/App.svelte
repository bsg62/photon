<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { library } from './lib/library.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Viewer from './components/Viewer.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();
  let viewerAt = $state<number | null>(null);

  onMount(() => {
    library.init().catch(library.reportError);
    return () => library.dispose();
  });

  // Spec §5: switching views returns the grid to the top, since a scroll position from
  // one view's set of photos is arbitrary against another's. Tracked here rather than in
  // FolderTree because App owns the `grid` binding and its scroll helper.
  let lastView = library.info.view;
  $effect(() => {
    const view = library.info.view;
    if (view !== lastView) {
      lastView = view;
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

  async function jump(folderId: number) {
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
</script>

<div class="app">
  <aside class="sidebar" inert={viewerAt !== null}><FolderTree onjump={jump} /></aside>
  <main class="content" inert={viewerAt !== null}>
    <Grid bind:this={grid} onopen={open} />
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
{#if viewerAt !== null}<Viewer offset={viewerAt} onclose={closeViewer} />{/if}
<Toasts />

<style>
  .app {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: 1fr auto;
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
  .statusbar { grid-column: 1 / -1; }
</style>
