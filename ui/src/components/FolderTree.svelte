<script lang="ts">
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { api, type Folder, type WatchedFolder } from '../lib/api';
  import { library } from '../lib/library.svelte';

  let { onjump }: { onjump: (folderId: number) => void } = $props();

  // Assumes the backend lists parents before children and never reports a cycle;
  // a folder whose parent hasn't arrived yet (or a cycle) silently drops its subtree.
  const children = $derived.by(() => {
    const map = new Map<number | null, Folder[]>();
    for (const f of library.folders.folders) {
      const list = map.get(f.parentId) ?? [];
      list.push(f);
      map.set(f.parentId, list);
    }
    return map;
  });

  /** A watched folder with no root `Folder` row yet: `scan_watched` returns before
   *  calling `upsert_folder` when the root is offline or hasn't been scanned, so
   *  such a folder would otherwise be invisible and unmanageable. */
  const watchedOnly = $derived.by(() => {
    const rootsByWatched = new Set(
      library.folders.folders.filter((f) => f.parentId === null).map((f) => f.watchedId),
    );
    return library.folders.watched.filter((w) => !rootsByWatched.has(w.id));
  });

  type MenuTarget = { kind: 'folder'; folder: Folder } | { kind: 'watched'; watched: WatchedFolder };

  let menu = $state<{ x: number; y: number; target: MenuTarget } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  const watchedOf = (f: Folder) => library.folders.watched.find((w) => w.id === f.watchedId);

  function lastSegment(path: string): string {
    const parts = path.split(/[/\\]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }

  async function addFolder() {
    const path = await open({ directory: true, multiple: false, title: 'Add a folder to photon' });
    if (typeof path !== 'string') return;
    try {
      await api.addFolder(path);
      await library.refreshFolders();
    } catch (e) {
      library.reportError(e);
    }
  }

  async function rescan(target: MenuTarget) {
    menu = null;
    const watchedId = target.kind === 'folder' ? target.folder.watchedId : target.watched.id;
    await api.rescanFolder(watchedId).catch(library.reportError);
  }

  async function reveal(f: Folder) {
    menu = null;
    await api.revealFolder(f.id).catch(library.reportError);
  }

  async function remove(target: MenuTarget) {
    menu = null;
    const watched = target.kind === 'folder' ? watchedOf(target.folder) : target.watched;
    if (!watched) return;
    try {
      const confirmed = await ask(`Remove “${watched.path}” from photon? Your files stay where they are.`, {
        title: 'Remove folder',
        kind: 'warning',
      });
      if (!confirmed) return;
      await api.removeFolder(watched.id).catch(library.reportError);
      await library.refreshFolders();
    } catch (e) {
      library.reportError(e);
    }
  }

  function openMenu(e: MouseEvent, target: MenuTarget) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, target };
  }

  function closeMenu() {
    menu = null;
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') closeMenu();
  }
</script>

<svelte:window onclick={closeMenu} onkeydown={(e) => e.key === 'Escape' && closeMenu()} />

<nav class="tree" aria-label="Folders">
  <div class="toolbar">
    <button class="add" onclick={addFolder}>Add folder…</button>
  </div>

  {#snippet node(f: Folder, depth: number)}
    {@const watched = watchedOf(f)}
    <button
      class="node"
      class:offline={watched && !watched.online}
      style:padding-left="{8 + depth * 14}px"
      title={f.path}
      onclick={() => onjump(f.id)}
      oncontextmenu={(e) => openMenu(e, { kind: 'folder', folder: f })}
    >
      <span class="name">{f.name}</span>
      {#if depth === 0 && library.isScanning(f.watchedId)}
        <span class="spinner" aria-label="Scanning"></span>
      {/if}
    </button>
    {#each children.get(f.id) ?? [] as child (child.id)}
      {@render node(child, depth + 1)}
    {/each}
  {/snippet}

  {#each children.get(null) ?? [] as root (root.id)}
    {@render node(root, 0)}
  {/each}

  {#each watchedOnly as w (w.id)}
    <button
      class="node offline"
      title={w.path}
      onclick={() => {}}
      oncontextmenu={(e) => openMenu(e, { kind: 'watched', watched: w })}
    >
      <span class="name">{lastSegment(w.path)}</span>
    </button>
  {/each}

  {#if library.folders.watched.length === 0}
    <p class="empty">No folders yet.</p>
  {/if}
</nav>

{#if menu}
  {@const target = menu.target}
  {@const watchedId = target.kind === 'folder' ? target.folder.watchedId : target.watched.id}
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
      disabled={library.isScanning(watchedId)}
      title={library.isScanning(watchedId) ? 'This folder is being scanned' : undefined}
      onclick={() => rescan(target)}>Rescan</button
    >
    {#if target.kind === 'folder'}
      <button role="menuitem" onclick={() => reveal(target.folder)}>Reveal in file manager</button>
    {/if}
    {#if target.kind === 'watched' || target.folder.parentId === null}
      <button role="menuitem" class="danger" onclick={() => remove(target)}>Remove from photon</button>
    {/if}
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding-bottom: 12px; }
  .toolbar { padding: 8px; }
  .add { width: 100%; padding: 6px; border: 1px solid #fff2; border-radius: 4px; background: var(--panel-2); cursor: pointer; }
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
  .node:hover { background: #ffffff0d; }
  .node.offline { opacity: 0.45; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid var(--muted);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
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
  .menu .danger { color: var(--danger); }
</style>
