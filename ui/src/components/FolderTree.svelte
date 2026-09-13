<script lang="ts">
  import { untrack } from 'svelte';
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { api, type Folder, type WatchedFolder } from '../lib/api';
  import { folderRows, groupByYear } from '../lib/folders';
  import { library } from '../lib/library.svelte';
  import { debounce, SEARCH_DEBOUNCE_MS, shouldAdoptBackendQuery } from '../lib/search';

  let { onjump }: { onjump: (folderId: number) => void } = $props();

  let query = $state(library.info.searchQuery);
  /** How many `setSearchQuery` calls of ours have been sent but not yet settled.
   *  `library.setSearchQuery` serialises calls, so more than one can be outstanding at once
   *  (a second send already queued behind a first that's still in flight); while any is
   *  outstanding, an echo arriving on `library.info.searchQuery` might belong to an older,
   *  since-superseded send rather than the latest one, so the sync effect below must not
   *  adopt anything until the count reaches zero. `$state` so the effect re-runs once it
   *  does. */
  let outstanding = $state(0);

  /** The last value we sent. The backend echoes it back on `library.info.searchQuery`, and
   *  the sync effect below has to tell that echo from a change made anywhere else. Plain,
   *  not `$state`: only the effect reads it, and always alongside a change it is already
   *  waking for. */
  let lastSent: string | null = null;

  /** Sends `q` to the backend, tracking it as outstanding for as long as it takes. */
  function send(q: string): Promise<void> {
    lastSent = q;
    outstanding++;
    return library.setSearchQuery(q).finally(() => outstanding--);
  }

  const runSearch = debounce((q: string) => void send(q), SEARCH_DEBOUNCE_MS);

  function clearSearch() {
    // Cancel first: a pending debounced call would otherwise land after the clear and put
    // the backend straight back into the search view. Cancelling only stops a call that
    // hasn't fired yet; a call already dispatched can't be cancelled, which is why
    // `outstanding` exists — the sync effect declines every echo until all dispatched calls,
    // including this one, have settled in order.
    runSearch.cancel();
    query = '';
    void send('');
  }

  // The backend is the source of truth for the active query (spec §5): clicking Starred or
  // a folder clears it server-side, and without this the box would keep displaying text
  // that no longer filters anything. shouldAdoptBackendQuery is what tells an external
  // change (adopt it) from our own echo of a keystroke (must not be adopted — doing so
  // would snap the box back to stale text while the user is still typing ahead of it).
  // Both `lastSent` and `outstanding` are needed: the count alone says every send has
  // landed, not that what landed came from anywhere but us, and shipping only the count is
  // what made the box swallow characters.
  //
  // `query` is read via `untrack` rather than directly: a direct read would make `query`
  // itself a dependency of this effect, so every keystroke (which writes `query` via
  // `bind:value`) would re-run it — and since the effect can also write `query`, that write
  // would immediately re-trigger the effect it happened inside. It would still settle rather
  // than loop (the second run sees backend === query and stops), but there is no reason to
  // pay for it: `outstanding` reaching zero is the only signal this effect needs to act on.
  $effect(() => {
    const backend = library.info.searchQuery;
    if (shouldAdoptBackendQuery(backend, untrack(() => query), lastSent, outstanding)) query = backend;
  });

  /** Folders that actually hold photos, grouped by the year of their newest one.
   *
   *  Drawn from the grid's sections rather than the folder table: a section exists only for
   *  a folder with items, which is what keeps empty intermediate folders out of the list. */
  const years = $derived(groupByYear(folderRows(library.info.sections, library.folders.folders)));

  /** Watched roots stay pinned above the year groups. They are the only place to rescan or
   *  remove a folder, and a root whose photos all live in subfolders — or whose drive is
   *  offline, or which has never been scanned — has no section of its own, so it would
   *  otherwise vanish along with any way to manage it. */
  const roots = $derived(library.folders.watched);

  type MenuTarget = { kind: 'folder'; folder: Folder } | { kind: 'watched'; watched: WatchedFolder };

  let menu = $state<{ x: number; y: number; target: MenuTarget } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  /** Rebuilt only when the folder list changes: `folderById` is called for every row's title
   *  and again from the context menu, so a linear scan per row would be quadratic in a
   *  sidebar holding hundreds of folders. */
  const foldersById = $derived(new Map(library.folders.folders.map((f) => [f.id, f])));
  const folderById = (id: number) => foldersById.get(id);

  /** Jumps to a watched root's own folder row when it has one. A root whose drive is offline
   *  or which has never been scanned has no row — and so nothing to scroll to — which is why
   *  this can't simply pass the watched id. */
  function jumpToRoot(w: WatchedFolder) {
    const root = library.folders.folders.find((f) => f.watchedId === w.id && f.parentId === null);
    if (root) onjump(root.id);
  }

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
    const watched =
      target.kind === 'folder'
        ? library.folders.watched.find((w) => w.id === target.folder.watchedId)
        : target.watched;
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

  /** A folder row's context menu still offers Rescan and Reveal, which act on the watched
   *  folder it belongs to. Removing stays on the roots above. */
  function folderMenu(e: MouseEvent, folderId: number) {
    const folder = folderById(folderId);
    if (folder) openMenu(e, { kind: 'folder', folder });
  }

  /** Jumping to a folder from the Starred or Search view has to leave that view first: the
   *  jump looks up the offset in the grid's current index, and racing that lookup against
   *  an unawaited view switch can return a stale or mismatched result (see the Important 1
   *  writeup — awaiting here is load-bearing, not stylistic). */
  async function jumpToFolder(folderId: number) {
    // Cancel a pending debounced search first: otherwise it can fire after the view switch
    // below has already landed on `all` and re-enter Search with its captured text,
    // replacing the grid the user just navigated to.
    runSearch.cancel();
    if (library.info.view !== 'all') await library.setView('all');
    onjump(folderId);
  }
</script>

<svelte:window onclick={closeMenu} onkeydown={(e) => e.key === 'Escape' && closeMenu()} />

<nav class="tree" aria-label="Folders">
  <div class="toolbar">
    <button class="add" onclick={addFolder}>Add folder…</button>
    <input
      class="search"
      type="search"
      placeholder="Search"
      aria-label="Search photos by file or folder name"
      bind:value={query}
      oninput={() => runSearch(query)}
      onkeydown={(e) => {
        // An empty box with Search not active has nothing to clear: unconditionally
        // clearing here would call setSearchQuery('') regardless, which is a no-op query
        // but still forces the view to All — kicking the user out of Starred with a
        // keystroke that cleared nothing.
        if (e.key === 'Escape' && (query !== '' || library.info.view === 'search')) clearSearch();
      }}
    />
  </div>

  <button
    class="root starred"
    class:active={library.info.view === 'starred'}
    onclick={() => {
      // Same hazard as jumpToFolder: an orphaned debounced search could otherwise fire
      // after this and re-enter Search, replacing the Starred grid the user just asked for.
      runSearch.cancel();
      void library.setView('starred');
    }}
    title="Photos rated in another program"
  >
    <span class="name">★ Starred</span>
    <span class="count">({library.info.starredCount})</span>
  </button>

  {#each roots as w (w.id)}
    <button
      class="root"
      class:offline={!w.online}
      title={w.path}
      onclick={() => jumpToRoot(w)}
      oncontextmenu={(e) => openMenu(e, { kind: 'watched', watched: w })}
    >
      <span class="name">{lastSegment(w.path)}</span>
      {#if library.isScanning(w.id)}
        <span class="spinner" aria-label="Scanning"></span>
      {/if}
    </button>
  {/each}

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
    {#if target.kind === 'watched'}
      <button role="menuitem" class="danger" onclick={() => remove(target)}>Remove from photon</button>
    {/if}
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding-bottom: 12px; }
  .toolbar { display: flex; flex-direction: column; gap: 6px; padding: 8px; }
  .add { width: 100%; padding: 6px; border: 1px solid #fff2; border-radius: 4px; background: var(--panel-2); cursor: pointer; }
  .search {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 8px;
    border: 1px solid #fff2;
    border-radius: 4px;
    background: var(--panel-2);
    color: inherit;
  }
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
  .root.offline { opacity: 0.45; }
  .starred.active { background: #ffffff14; }
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
