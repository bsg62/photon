<script lang="ts">
  import { api, type Folder } from '../lib/api';
  import { enterFolder, folderRows, groupByYear } from '../lib/folders';
  import { library } from '../lib/library.svelte';
  import { searchBox } from '../lib/search-box.svelte';

  let { onjump, onopensettings }: { onjump: (folderId: number) => void; onopensettings: () => void } = $props();

  /** Folders that actually hold photos, grouped by the year of their newest one.
   *
   *  Drawn from the grid's sections rather than the folder table: a section exists only for
   *  a folder with items, which is what keeps empty intermediate folders out of the list.
   *  Watched roots with no photos of their own are managed from Settings instead. */
  const years = $derived(groupByYear(folderRows(library.info.sections, library.folders.folders)));

  let menu = $state<{ x: number; y: number; folder: Folder } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  /** Rebuilt only when the folder list changes: `folderById` is called for every row's title
   *  and again from the context menu, so a linear scan per row would be quadratic in a
   *  sidebar holding hundreds of folders. */
  const foldersById = $derived(new Map(library.folders.folders.map((f) => [f.id, f])));
  const folderById = (id: number) => foldersById.get(id);

  async function rescan(f: Folder) {
    menu = null;
    await api.rescanFolder(f.watchedId).catch(library.reportError);
  }

  async function reveal(f: Folder) {
    menu = null;
    await api.revealFolder(f.id).catch(library.reportError);
  }

  function closeMenu() {
    menu = null;
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') closeMenu();
  }

  /** A folder row's context menu offers Rescan and Reveal, which act on the watched folder
   *  it belongs to. Adding and removing watched folders live in Settings. */
  function folderMenu(e: MouseEvent, folderId: number) {
    e.preventDefault();
    const folder = folderById(folderId);
    if (folder) menu = { x: e.clientX, y: e.clientY, folder };
  }

  /** The order here — cancel, then await the view switch, then scroll — is explained on
   *  `enterFolder`. */
  function jumpToFolder(folderId: number): Promise<void> {
    return enterFolder(folderId, {
      cancelSearch: () => searchBox.cancel(),
      currentView: () => library.info.view,
      setView: (view) => library.setView(view),
      jump: onjump,
    });
  }
</script>

<svelte:window onclick={closeMenu} onkeydown={(e) => e.key === 'Escape' && closeMenu()} />

<nav class="tree" aria-label="Folders">
  <button
    class="root starred"
    class:active={library.info.view === 'starred'}
    onclick={() => {
      // Same hazard as jumpToFolder: an orphaned debounced search could otherwise fire
      // after this and re-enter Search, replacing the Starred grid the user just asked for.
      searchBox.cancel();
      void library.setView('starred');
    }}
    title="Photos rated in another program"
  >
    <span class="name">★ Starred</span>
    <span class="count">({library.info.starredCount})</span>
  </button>

  <button
    class="root recent"
    class:active={library.info.view === 'recent'}
    onclick={() => {
      // Same hazard as the Starred button above: cancel any pending search first.
      searchBox.cancel();
      void library.setView('recent');
    }}
    title="The newest photos by capture date"
  >
    <span class="name">🕘 Recent</span>
  </button>

  {#each years as group (group.year)}
    <h2 class="year">{group.year}</h2>
    {#each group.rows as row (row.folderId)}
      <button
        class="node"
        title={folderById(row.folderId)?.path}
        onclick={() => jumpToFolder(row.folderId)}
        oncontextmenu={(e) => folderMenu(e, row.folderId)}
      >
        <span class="name">{row.name}</span>
        <span class="count">({row.count})</span>
      </button>
    {/each}
  {/each}

  {#if library.folders.watched.length === 0}
    <p class="empty">No folders yet.</p>
    <button class="add" onclick={onopensettings}>Add a folder in Settings…</button>
  {/if}
</nav>

{#if menu}
  {@const folder = menu.folder}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style:left="{menu.x}px"
    style:top="{menu.y}px"
    onkeydown={onMenuKeydown}
  >
    <!-- `rescan_folder` is a no-op while a scan of that folder is running, and reports
         nothing back, so don't offer it. -->
    <button
      role="menuitem"
      disabled={library.isScanning(folder.watchedId)}
      title={library.isScanning(folder.watchedId) ? 'This folder is being scanned' : undefined}
      onclick={() => rescan(folder)}>Rescan</button
    >
    <button role="menuitem" onclick={() => reveal(folder)}>Reveal in file manager</button>
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding: 8px 0 12px; }
  .add { margin: 0 8px; padding: 6px; border: 1px solid #fff2; border-radius: 4px; background: var(--panel-2); cursor: pointer; }
  .root,
  .node {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .root { font-weight: 600; }
  .node { padding-left: 18px; }
  .root:hover, .node:hover { background: #ffffff0d; }
  .starred.active, .recent.active { background: #ffffff14; }
  .year {
    margin: 10px 0 2px;
    padding: 0 8px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { color: var(--muted); font-size: 11px; }
  .empty { padding: 8px 12px; color: var(--muted); }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    background: var(--panel-2);
    border-radius: 6px;
    box-shadow: 0 6px 24px #0008;
  }
  .menu button { padding: 6px 10px; border: 0; background: none; text-align: left; cursor: pointer; border-radius: 4px; }
  .menu button:hover:not(:disabled) { background: #ffffff14; }
  .menu button:disabled { color: var(--muted); cursor: default; }
</style>
