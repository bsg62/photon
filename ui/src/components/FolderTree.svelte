<script lang="ts">
  import { ask } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { api, type AlbumSummary, type Folder, type SavedSearch } from '../lib/api';
  import { createAlbumEditor } from '../lib/album-editor.svelte';
  import { enterFolder, folderRows, groupByYear } from '../lib/folders';
  import { sidebarTags } from '../lib/tags';
  import { library } from '../lib/library.svelte';
  import { searchBox } from '../lib/search-box.svelte';
  import Icon from './Icon.svelte';

  let { onjump, onopensettings }: { onjump: (folderId: number) => void; onopensettings: () => void } = $props();

  /** Folders that actually hold photos, grouped by the year of their newest one.
   *
   *  Drawn from the grid's sections rather than the folder table: a section exists only for
   *  a folder with items, which is what keeps empty intermediate folders out of the list.
   *  Watched roots with no photos of their own are managed from Settings instead. */
  const years = $derived(groupByYear(folderRows(library.info.sections, library.folders.folders)));
  const shownTags = $derived(sidebarTags(library.tags));

  /** Which collection groups are open. Albums start open because they are the user's own;
   *  People and Tags start closed because a real library has hundreds of each, and the years
   *  below must stay reachable. Session state, not persisted. */
  let open = $state({ albums: true, searches: true, people: false, tags: false });

  let menu = $state<{ x: number; y: number; folder: Folder } | null>(null);
  let albumMenu = $state<{ x: number; y: number; album: AlbumSummary } | null>(null);
  let searchMenu = $state<{ x: number; y: number; search: SavedSearch } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();
  let albumMenuEl = $state<HTMLDivElement | undefined>();
  let searchMenuEl = $state<HTMLDivElement | undefined>();
  let editorInput = $state<HTMLInputElement | undefined>();
  let searchEditorInput = $state<HTMLInputElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });
  $effect(() => {
    if (albumMenu) albumMenuEl?.focus();
  });
  $effect(() => {
    if (searchMenu) searchMenuEl?.focus();
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

  /** Hide folder: its photos, and any added to it later, until Unhide folder. The row then
   *  leaves this list on its own - the rows are the view's sections, and a hidden folder's
   *  photos are in no view but Hidden, unless the user unhid one by hand, which keeps the
   *  folder listed (and its menu offering Unhide folder) wherever that photo shows. */
  async function toggleFolderHidden(f: Folder) {
    menu = null;
    await library.setFolderHidden(f.id, !f.hidden).catch(library.reportError);
  }

  function closeMenus() {
    menu = null;
    albumMenu = null;
    searchMenu = null;
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

  // ---- saved searches ----

  /** A second editor instance: the sidebar shows one field at a time, and an album rename
   *  started over a search rename should replace it, but the two lists key their rows by
   *  their own ids, so one editor shared between them would open a field in both lists at
   *  once whenever the ids happened to match. `create` saves whatever the box currently
   *  holds; nothing renders it today, since saving is the bookmark button's job. */
  const searchEditor = createAlbumEditor({
    create: (name) => library.saveSearch(name, searchBox.query),
    rename: (searchId, name) => library.renameSavedSearch(searchId, name),
  });

  async function startSearchRename(search: SavedSearch) {
    searchMenu = null;
    searchEditor.startRename(search.id, search.name);
    await tick();
    searchEditorInput?.focus();
    searchEditorInput?.select();
  }

  function commitSearchEditor() {
    searchEditor.commit().catch(library.reportError);
  }

  function onSearchEditorKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitSearchEditor();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      searchEditor.cancel();
    }
  }

  function searchContextMenu(e: MouseEvent, search: SavedSearch) {
    e.preventDefault();
    menu = null;
    albumMenu = null;
    searchMenu = { x: e.clientX, y: e.clientY, search };
  }

  async function deleteSearch(search: SavedSearch) {
    searchMenu = null;
    try {
      const confirmed = await ask(`Delete the saved search “${search.name}”? Your photos are not affected.`, {
        title: 'Delete saved search',
        kind: 'warning',
      });
      if (!confirmed) return;
      await library.deleteSavedSearch(search.id);
      // Unlike an album, this leaves the grid alone: the Search view is driven by the query
      // string, so the photos on screen are still the answer to what was typed.
    } catch (e) {
      library.reportError(e);
    }
  }

  /** Runs a saved search as though it had been typed. `searchBox.search` cancels a pending
   *  debounce first, which is what stops half-typed text landing after this and replacing
   *  the grid the click just asked for. */
  function showSearch(search: SavedSearch) {
    closeMenus();
    searchBox.search(search.query);
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
    <Icon name="star" size={14} /><span class="name">Starred</span>
    <span class="count">{library.info.starredCount.toLocaleString()}</span>
  </button>

  <button
    class="root recent"
    class:active={library.info.view === 'recent'}
    onclick={() => show(() => library.setView('recent'))}
    title="The newest photos by capture date"
  >
    <Icon name="clock" size={14} /><span class="name">Recent</span>
  </button>

  <!-- Only while there is something in it, or while it is what the grid shows: most
       libraries have no duplicates, and a permanent "(0)" row is noise. -->
  {#if library.info.duplicateCount > 0 || library.info.view === 'duplicates' || library.info.view === 'copies'}
    <button
      class="root duplicates"
      class:active={library.info.view === 'duplicates'}
      onclick={() => show(() => library.setView('duplicates'))}
      title="Photos with a byte-identical copy elsewhere in the library"
    >
      <Icon name="copy" size={14} /><span class="name">Duplicates</span>
      <span class="count">{library.info.duplicateCount.toLocaleString()}</span>
    </button>
    {#if library.info.view === 'copies'}
      <!-- Not a saved place: it exists while the view is open, and leaving removes it. Not
           a `<button>`: it does nothing on click (the view is already open), so a button
           here was a dead tab stop announced as interactive with no action behind it. -->
      <div class="root copies active" aria-current="true" title={library.info.copiesOf?.fileName}>
        <span class="name">Copies of {library.info.copiesOf?.fileName || 'a photo'}</span>
      </div>
    {/if}
  {/if}

  <!-- Like Duplicates, only while there is something in it or it is showing: a library
       nobody has hidden anything in needs no reminder that hiding exists. -->
  {#if library.info.hiddenCount > 0 || library.info.view === 'hidden'}
    <button
      class="root hidden-view"
      class:active={library.info.view === 'hidden'}
      onclick={() => show(() => library.setView('hidden'))}
      title="Photos you have hidden. They stay on disk; unhide them from here"
    >
      <Icon name="eye-off" size={14} /><span class="name">Hidden</span>
      <span class="count">{library.info.hiddenCount.toLocaleString()}</span>
    </button>
  {/if}

  <!-- Albums: photon's own, so the group is editable. -->
  <button class="group" aria-expanded={open.albums} onclick={() => (open.albums = !open.albums)}>
    <span class="chevron"><Icon name={open.albums ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="folder" size={14} />
    <span class="name">Albums</span>
    <span class="count">{library.albums.length.toLocaleString()}</span>
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
          <span class="count">{album.count.toLocaleString()}</span>
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

  <!-- Saved searches: a name over a query, re-run on every visit. No count - one would
       cost a full library pass per row on every change; see library/searches.rs. -->
  {#if library.searches.length > 0}
    <button class="group" aria-expanded={open.searches} onclick={() => (open.searches = !open.searches)}>
      <span class="chevron"><Icon name={open.searches ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="bookmark" size={14} />
      <span class="name">Searches</span>
      <span class="count">{library.searches.length.toLocaleString()}</span>
    </button>
    {#if open.searches}
      {#each library.searches as search (search.id)}
        {#if searchEditor.editing(search.id)}
          <input
            class="editor"
            bind:this={searchEditorInput}
            bind:value={searchEditor.text}
            disabled={searchEditor.busy}
            aria-label="Saved search name"
            onkeydown={onSearchEditorKeydown}
            onblur={commitSearchEditor}
          />
        {:else}
          <button
            class="node"
            class:active={library.info.view === 'search' && library.info.searchQuery === search.query}
            title={search.name === search.query ? search.query : `${search.name} — ${search.query}`}
            onclick={() => showSearch(search)}
            oncontextmenu={(e) => searchContextMenu(e, search)}
          >
            <span class="name">{search.name}</span>
          </button>
        {/if}
      {/each}
    {/if}
  {/if}

  <!-- People: Picasa's contacts, read from the INI beside the photos. Read only. -->
  <button class="group" aria-expanded={open.people} onclick={() => (open.people = !open.people)}>
    <span class="chevron"><Icon name={open.people ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="user" size={14} />
    <span class="name">People</span>
    <span class="count">{library.people.length.toLocaleString()}</span>
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
        <span class="count">{person.count.toLocaleString()}</span>
      </button>
    {:else}
      <p class="empty small">No people. photon reads face names from Picasa’s .picasa.ini.</p>
    {/each}
  {/if}

  <!-- Tags: keywords read from the photos' own XMP and IPTC. Read only. -->
  <button class="group" aria-expanded={open.tags} onclick={() => (open.tags = !open.tags)}>
    <span class="chevron"><Icon name={open.tags ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="tag" size={14} />
    <span class="name">Tags</span>
    <span class="count">{shownTags.length.toLocaleString()}</span>
  </button>
  {#if open.tags}
    {#each shownTags as t (t.tag)}
      <button
        class="node"
        class:active={library.info.view === 'tag' && library.info.tag === t.tag}
        title={t.tag}
        onclick={() => show(() => library.setTagView(t.tag))}
      >
        <span class="name">{t.tag}</span>
        <span class="count">{t.count.toLocaleString()}</span>
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
        <span class="count">{row.count.toLocaleString()}</span>
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
    class="menu focus-container"
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
    <button role="menuitem" onclick={() => toggleFolderHidden(folder)}>{folder.hidden ? 'Unhide folder' : 'Hide folder'}</button>
  </div>
{/if}

{#if albumMenu}
  {@const album = albumMenu.album}
  <div
    class="menu focus-container"
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

{#if searchMenu}
  {@const search = searchMenu.search}
  <div
    class="menu focus-container"
    role="menu"
    tabindex="-1"
    bind:this={searchMenuEl}
    style:left="{searchMenu.x}px"
    style:top="{searchMenu.y}px"
    onkeydown={onMenuKeydown}
  >
    <button role="menuitem" onclick={() => startSearchRename(search)}>Rename…</button>
    <button role="menuitem" class="danger" onclick={() => deleteSearch(search)}>Delete…</button>
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding: var(--s-2) 0 var(--s-3); }
  /* Rows are inset from the panel edge so the focus ring, which sits 2px outside its
     element, is not clipped by the sidebar's overflow. */
  .root,
  .node,
  .group {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex: none;
    height: 28px;
    margin: 0 6px;
    padding: 0 var(--s-2);
    border: 0;
    border-radius: var(--r-3);
    background: none;
    text-align: left;
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .group {
    margin-top: var(--s-2);
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
  }
  .chevron { display: grid; place-items: center; width: 12px; }
  .node { padding-left: 28px; }
  /* Nested under Duplicates, the same depth as an album under its group. */
  /* Nothing happens on a click (see the markup), so it must not offer one: `.root` is
     styled for the buttons it is otherwise always on. Its hover background is already
     covered by `.copies.active`, which is declared after it. */
  .copies { padding-left: 28px; cursor: default; }
  .root:hover, .node:hover, .group:hover { background: var(--hover); }
  .starred.active, .recent.active, .duplicates.active, .hidden-view.active, .node.active, .copies.active { background: var(--accent-soft); }
  /* --text-dim does not reach 4.5:1 over --accent-soft; --text does (tokens.test.ts). */
  .active .count { color: var(--text); }
  .add-album { color: var(--text-dim); }
  .add-album:hover { color: var(--text); }
  .editor {
    height: 28px;
    margin: 0 6px 0 26px;
    padding: 0 var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: var(--surface);
    color: inherit;
    font: inherit;
  }
  /* The global ring, pulled in to hug the field rather than float 2px off it - it replaces
     the old accent border, so a focused editor gets one ring, not two. */
  .editor:focus-visible { outline-offset: 0; }
  .year {
    margin: var(--s-3) 0 2px;
    padding: 0 14px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count {
    margin-left: auto;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-variant-numeric: tabular-nums;
  }
  .empty { padding: var(--s-2) 14px; color: var(--text-dim); }
  .empty.small { margin: 0; padding: 2px 14px 6px 34px; font-size: var(--t-2); }
  .add {
    margin: 0 14px;
    padding: 6px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    cursor: pointer;
  }
  .add:hover { background: var(--field-hover); }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button {
    padding: 6px 10px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover:not(:disabled) { background: var(--hover); }
  .menu button:disabled { color: var(--text-dim); cursor: default; }
  .menu .danger { color: var(--danger); }
  @media (prefers-reduced-motion: reduce) {
    .root, .node, .group { transition: none; }
  }
</style>
