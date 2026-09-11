<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { library } from './lib/library.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();

  onMount(() => {
    library.init().catch(library.reportError);
    return () => library.dispose();
  });

  function open(offset: number) {
    library.selected = offset;
  }

  async function jump(folderId: number) {
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
</script>

<div class="app">
  <aside class="sidebar"><FolderTree onjump={jump} /></aside>
  <main class="content">
    <Grid bind:this={grid} onopen={open} />
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
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
