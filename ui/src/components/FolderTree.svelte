<script lang="ts">
  import { ask } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { api, type AlbumSummary, type Folder } from '../lib/api';
  import { createAlbumEditor } from '../lib/album-editor.svelte';
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

  /** Which collection groups are open. Albums start open because they are the user's own;
   *  People and Tags start closed because a real library has hundreds of each, and the years
   *  below must stay reachable. Session state, not persisted. */
  let open = $state({ albums: true, people: false, tags: false });

  let menu = $state<{ x: number; y: number; folder: Folder } | null>(null);
  let albumMenu = $state<{ x: number; y: number; album: AlbumSummary } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();
  let albumMenuEl = $state<HTMLDivElement | undefined>();
  let editorInput = $state<HTMLInputElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });
  $effect(() => {
    if (albumMenu) albumMenuEl?.focus();
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

  function closeMenus() {
    menu = null;
    albumMenu = null;
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') closeMenus();
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

  /** Every collection click cancels a pending search first, for the reason on the Starred
   *  button: an orphaned debounced send would otherwise re-enter Search behind it. */
  function show(switchView: () => Promise<void>) {
    searchBox.cancel();
    void switchView();
  }

  // ---- albums ----

  const editor = createAlbumEditor({
    create: (name) => library.createAlbum(name),
    rename: (albumId, name) => library.renameAlbum(albumId, name),
  });

  /** The field appears in the DOM a tick after the editor opens; focusing it then is what
   *  lets the user type straight away. */
  async function startNew() {
    editor.startNew();
    await tick();
    editorInput?.focus();
  }

  async function startRename(album: AlbumSummary) {
    albumMenu = null;
    editor.startRename(album.id, album.name);
    await tick();
    editorInput?.focus();
    editorInput?.select();
  }

  function commitEditor() {
    editor.commit().catch(library.reportError);
  }

  function onEditorKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitEditor();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      editor.cancel();
    }
  }

  function albumContextMenu(e: MouseEvent, album: AlbumSummary) {
    e.preventDefault();
    menu = null;
    albumMenu = { x: e.clientX, y: e.clientY, album };
  }

  async function deleteAlbum(album: AlbumSummary) {
    albumMenu = null;
    try {
      const confirmed = await ask(`Delete the album “${album.name}”? The photos stay in your library.`, {
        title: 'Delete album',
        kind: 'warning',
      });
      if (!confirmed) return;
      await library.deleteAlbum(album.id);
      // Deleting the album on screen leaves the grid on a view with nothing to show; the
      // backend has already emptied it, and All is the sensible place to land.
      if (library.info.view === 'album' && library.info.album === album.id) await library.setView('all');
    } catch (e) {
      library.reportError(e);
    }
  }
</script>

<svelte:window onclick={closeMenus} onkeydown={(e) => e.key === 'Escape' && closeMenus()} />

<nav class="tree" aria-label="Folders">
  <button
    class="root starred"
    class:active={library.info.view === 'starred'}
    onclick={() => show(() => library.setView('starred'))}
    title="Starred photos"
  >
    <span class="name">★ Starred</span>
    <span class="count">({library.info.starredCount})</span>
  </button>

  <button
    class="root recent"
    class:active={library.info.view === 'recent'}
    onclick={() => show(() => library.setView('recent'))}
    title="The newest photos by capture date"
  >
    <span class="name">🕘 Recent</span>
  </button>

  <!-- Only while there is something in it, or while it is what the grid shows: most
       libraries have no duplicates, and a permanent "(0)" row is noise. -->
  {#if library.info.duplicateCount > 0 || library.info.view === 'duplicates'}
    <button
      class="root duplicates"
      class:active={library.info.view === 'duplicates'}
      onclick={() => show(() => library.setView('duplicates'))}
      title="Photos with a byte-identical copy elsewhere in the library"
    >
      <span class="name">⧉ Duplicates</span>
      <span class="count">({library.info.duplicateCount})</span>
    </button>
  {/if}

  <!-- Albums: photon's own, so the group is editable. -->
  <button class="group" aria-expanded={open.albums} onclick={() => (open.albums = !open.albums)}>
    <span class="chevron">{open.albums ? '▾' : '▸'}</span>
    <span class="name">Albums</span>
    <span class="count">({library.albums.length})</span>
  </button>
  {#if open.albums}
    {#each library.albums as album (album.id)}
      {#if editor.editing(album.id)}
        <input
          class="editor"
          bind:this={editorInput}
          bind:value={editor.text}
          disabled={editor.busy}
          aria-label="Album name"
          onkeydown={onEditorKeydown}
          onblur={commitEditor}
        />
      {:else}
        <button
          class="node"
          class:active={library.info.view === 'album' && library.info.album === album.id}
          title={album.name}
          onclick={() => show(() => library.setAlbumView(album.id))}
          oncontextmenu={(e) => albumContextMenu(e, album)}
        >
          <span class="name">{album.name}</span>
          <span class="count">({album.count})</span>
        </button>
      {/if}
    {/each}
    {#if editor.editing()}
      <input
        class="editor"
        bind:this={editorInput}
        bind:value={editor.text}
        disabled={editor.busy}
        placeholder="Album name"
        aria-label="New album name"
        onkeydown={onEditorKeydown}
        onblur={commitEditor}
      />
    {:else}
      <button class="node add-album" onclick={startNew}>New album…</button>
    {/if}
  {/if}

  <!-- People: Picasa's contacts, read from the INI beside the photos. Read only. -->
  <button class="group" aria-expanded={open.people} onclick={() => (open.people = !open.people)}>
    <span class="chevron">{open.people ? '▾' : '▸'}</span>
    <span class="name">People</span>
    <span class="count">({library.people.length})</span>
  </button>
  {#if open.people}
    {#each library.people as person (person.hash)}
      <button
        class="node"
        class:active={library.info.view === 'person' && library.info.person === person.hash}
        title={person.name}
        onclick={() => show(() => library.setPersonView(person.hash))}
      >
        <span class="name">{person.name}</span>
        <span class="count">({person.count})</span>
      </button>
    {:else}
      <p class="empty small">No people. photon reads face names from Picasa’s .picasa.ini.</p>
    {/each}
  {/if}

  <!-- Tags: keywords read from the photos' own XMP and IPTC. Read only. -->
  <button class="group" aria-expanded={open.tags} onclick={() => (open.tags = !open.tags)}>
    <span class="chevron">{open.tags ? '▾' : '▸'}</span>
    <span class="name">Tags</span>
    <span class="count">({library.tags.length})</span>
  </button>
  {#if open.tags}
    {#each library.tags as t (t.tag)}
      <button
        class="node"
        class:active={library.info.view === 'tag' && library.info.tag === t.tag}
        title={t.tag}
        onclick={() => show(() => library.setTagView(t.tag))}
      >
        <span class="name">{t.tag}</span>
        <span class="count">({t.count})</span>
      </button>
    {:else}
      <p class="empty small">No keywords. photon reads them from the photos themselves.</p>
    {/each}
  {/if}

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

{#if albumMenu}
  {@const album = albumMenu.album}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={albumMenuEl}
    style:left="{albumMenu.x}px"
    style:top="{albumMenu.y}px"
    onkeydown={onMenuKeydown}
  >
    <button role="menuitem" onclick={() => startRename(album)}>Rename…</button>
    <button role="menuitem" class="danger" onclick={() => deleteAlbum(album)}>Delete…</button>
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding: 8px 0 12px; }
  .add { margin: 0 8px; padding: 6px; border: 1px solid #fff2; border-radius: 4px; background: var(--panel-2); cursor: pointer; }
  .root,
  .node,
  .group {
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
  .group { margin-top: 6px; font-weight: 600; }
  .chevron { width: 10px; color: var(--muted); font-size: 11px; }
  .node { padding-left: 18px; }
  .root:hover, .node:hover, .group:hover { background: #ffffff0d; }
  .starred.active, .recent.active, .duplicates.active, .node.active { background: #ffffff14; }
  .add-album { color: var(--muted); }
  .add-album:hover { color: var(--text); }
  .editor {
    margin: 2px 8px 2px 18px;
    padding: 3px 6px;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: var(--panel-2);
    color: inherit;
    font: inherit;
  }
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
  .empty.small { margin: 0; padding: 2px 18px 6px; font-size: 12px; }
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
