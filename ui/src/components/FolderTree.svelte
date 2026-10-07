<script lang="ts">
  import { ask } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { api, type AlbumSummary, type Folder, type SavedSearch } from '../lib/api';
  import { createAlbumEditor } from '../lib/album-editor.svelte';
  import { arrangeFolders, enterFolder, folderLabel, folderRows, returnToAll } from '../lib/folders';
  import { browserStore, storedOpenGroups, storeOpenGroups, type OpenGroups } from '../lib/sidebar';
  import { sidebarTags } from '../lib/tags';
  import { gridPlace } from '../lib/grid-place.svelte';
  import { laidOutByFolder } from '../lib/grouping';
  import { library } from '../lib/library.svelte';
  import { mainPage } from '../lib/main-page.svelte';
  import { searchBox } from '../lib/search-box.svelte';
  import { onThisDayLabel, onThisDayQuery } from '../lib/searches';
  import FolderMenu from './FolderMenu.svelte';
  import Icon from './Icon.svelte';
  import Menu from './Menu.svelte';

  let { onjump, onopensettings }: { onjump: (folderId: number) => void; onopensettings: () => void } = $props();

  /** Folders that actually hold photos, in the user's sort: grouped by the year of their
   *  oldest one by date, one headerless list otherwise (`arrangeFolders`).
   *
   *  Drawn from the index's folder tallies rather than the folder table: a tally exists only
   *  for a folder with items, which is what keeps empty intermediate folders out of the list.
   *  Watched roots with no photos of their own are managed from Settings instead. */
  /** One formatter for every count in the list, rather than a `toLocaleString()` per row,
   *  each of which looks the locale up again: the folder list alone can be thousands long. */
  const counted = new Intl.NumberFormat();
  /** A row's height, as `.node` and `.editor` draw it below: what a year group that has not
   *  been laid out yet is assumed to be tall (`contain-intrinsic-block-size`). */
  const ROW = 28;

  /** Whether the grid is what the main area shows. While the People page is up no grid view
   *  is on screen, so none of their rows may look selected. */
  const onGrid = $derived(mainPage.current === 'grid');

  const years = $derived(arrangeFolders(folderRows(library.info.folders, library.folders.folders), library.info.sort));
  const shownTags = $derived(sidebarTags(library.tags));

  /** Which collection groups are open, as they were left on this machine (`sidebar.ts`,
   *  where the defaults are). */
  const store = browserStore();
  let open = $state(storedOpenGroups(store));

  /** Opens or folds a group and remembers it. Stored here, where the user says so, not in
   *  an effect on `open`: that would also run on mount and write back what it had just read. */
  function setOpen(group: keyof OpenGroups, value: boolean) {
    open[group] = value;
    storeOpenGroups(store, open);
  }

  /** Whether the focus has nowhere to be: on `<body>`, which no key reaches anything from. */
  function focusLost(): boolean {
    const at = document.activeElement;
    return !at || at === document.body;
  }

  /** A rename field closed from the keyboard - Escape, or Enter once the name is stored -
   *  hands the focus to the row it stood in for (`selector`). The field is removed while it
   *  holds the focus, which otherwise falls to `<body>`. Only then: a field closed by a click
   *  elsewhere has lost the focus to what was clicked, and a rename of another row started
   *  meanwhile holds it. `elsewhere` is for a rename that was not started from its row: the
   *  focus goes back where it was started from. */
  async function backToRow(selector: string, elsewhere?: (() => void) | null) {
    await tick();
    if (!focusLost()) return;
    if (elsewhere) elsewhere();
    else tree?.querySelector<HTMLElement>(selector)?.focus();
  }

  /** The keys of a rename field. Enter stores the name; a name the backend refuses leaves
   *  the field open (`commit` rejects) with the caret in it.
   *
   *  While a write runs the field is read-only, not disabled. A disabled field cannot hold
   *  the focus, and one editor serves every row of its list: a rename of a second folder
   *  begun while the first was still being written opened its field disabled, the focus
   *  stayed on the grid the header menu had handed it to, and what was typed for the name
   *  went to the photos - `h` hid the selected one. */
  function editorKeydown(
    e: KeyboardEvent,
    field: { commit(): Promise<boolean>; cancel(): void },
    selector: string,
    input: () => HTMLInputElement | undefined,
    elsewhere?: () => (() => void) | null,
  ) {
    if (e.key === 'Enter') {
      e.preventDefault();
      field.commit().then(
        () => backToRow(selector, elsewhere?.()),
        async (error) => {
          library.reportError(error);
          await tick();
          if (focusLost()) input()?.focus();
        },
      );
    } else if (e.key === 'Escape') {
      e.preventDefault();
      field.cancel();
      void backToRow(selector, elsewhere?.());
    }
  }

  /** The folder the grid is in (`gridPlace`), while the grid is what the main area shows:
   *  its row is marked, so the list says where the grid is as it scrolls. A mark of its own,
   *  not `.active`: the view row above (All photos, an album) is the current *view* and stays
   *  filled, and this is the place inside it. */
  const here = $derived(onGrid ? gridPlace.folderId : null);
  let tree = $state<HTMLElement | undefined>();

  // The list follows the grid: the marked row is kept in view, by the least movement, as
  // the grid scrolls through folders the list has scrolled past. Not while the pointer is
  // over the list or a name is being typed in it - then it is the user's, and a list that
  // moved under the pointer would put another row under a click already on its way.
  $effect(() => {
    const folderId = here;
    const nav = tree;
    if (folderId === null || !nav) return;
    if (nav.matches(':hover') || nav.contains(document.activeElement?.closest('input') ?? null)) return;
    nav.querySelector(`[data-folder="${folderId}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  // A held Enter does not press a row twice. Enter in a rename field hands the focus to the
  // field's row once the name is stored, and the key, still down, repeats there: the album
  // just renamed was opened, the folder jumped to, "New album…" opened its field again.
  // Nothing in the list is worth doing again for a key held down. A listener rather than an
  // `onkeydown` on the `<nav>`, which is not a control.
  $effect(() => {
    const nav = tree;
    if (!nav) return;
    const held = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && e.repeat && e.target instanceof HTMLButtonElement) e.preventDefault();
    };
    nav.addEventListener('keydown', held);
    return () => nav.removeEventListener('keydown', held);
  });

  let menu = $state<{ x: number; y: number; folder: Folder } | null>(null);
  let albumMenu = $state<{ x: number; y: number; album: AlbumSummary } | null>(null);
  let searchMenu = $state<{ x: number; y: number; search: SavedSearch } | null>(null);
  let editorInput = $state<HTMLInputElement | undefined>();
  let searchEditorInput = $state<HTMLInputElement | undefined>();
  let folderEditorInput = $state<HTMLInputElement | undefined>();

  /** Rebuilt only when the folder list changes: `folderById` is called for every row's title
   *  and again from the context menu, so a linear scan per row would be quadratic in a
   *  sidebar holding hundreds of folders. */
  const foldersById = $derived(new Map(library.folders.folders.map((f) => [f.id, f])));
  const folderById = (id: number) => foldersById.get(id);

  // ---- folder names ----

  /** A third editor, for the same reason as `searchEditor`: folder ids collide with album
   *  and search ids. `blankClears`, because an emptied field is how the user asks for the
   *  directory's own name back; the backend also stores the directory's name as no alias,
   *  so committing the pre-filled field unchanged leaves nothing behind. Nothing creates a
   *  folder from here. */
  const folderEditor = createAlbumEditor({
    create: async () => {},
    rename: (folderId, name) => library.setFolderAlias(folderId, name),
    blankClears: true,
  });

  /** Where the focus goes when the folder's rename field closes, for a rename asked for from
   *  outside the list (`renameFolder`); null for one started from the row's own menu. */
  let folderRenameBack: (() => void) | null = null;

  async function startFolderRename(f: Folder, back: (() => void) | null = null) {
    menu = null;
    folderRenameBack = back;
    folderEditor.startRename(f.id, folderLabel(f));
    await tick();
    folderEditorInput?.focus();
    folderEditorInput?.select();
  }

  /** "Rename in photon…" on the folder's header in the grid: the same field, in the folder's
   *  row here, which focusing scrolls into view. A folder the list no longer holds has no
   *  row to open it in. `back` takes the focus when the field is closed from the keyboard:
   *  the rename was begun in the grid, and the keys go back to the photos. */
  export function renameFolder(folderId: number, back: () => void) {
    const folder = folderById(folderId);
    if (folder && years.some((group) => group.rows.some((row) => row.folderId === folderId))) void startFolderRename(folder, back);
  }

  function commitFolderEditor() {
    folderEditor.commit().catch(library.reportError);
  }

  /** Exported for App, which closes them when the sidebar is hidden with one open. */
  export function closeMenus() {
    menu = null;
    albumMenu = null;
    searchMenu = null;
  }

  function folderMenu(e: MouseEvent, folderId: number) {
    e.preventDefault();
    const folder = folderById(folderId);
    if (folder) menu = { x: e.clientX, y: e.clientY, folder };
  }

  /** The order here — cancel, then await the view switch, then scroll — is explained on
   *  `enterFolder`. */
  function jumpToFolder(folderId: number): Promise<void> {
    mainPage.showGrid();
    return enterFolder(folderId, {
      cancelSearch: () => searchBox.cancel(),
      currentView: () => library.settledView(),
      setView: (view) => library.setView(view),
      jump: onjump,
    });
  }

  function showAll(): Promise<void> {
    mainPage.showGrid();
    return returnToAll({
      cancelSearch: () => searchBox.cancel(),
      currentView: () => library.settledView(),
      setView: (view) => library.setView(view),
      lastFolder: () => api.lastFolder(),
      byFolder: () => laidOutByFolder(library.info.sort),
      jump: onjump,
    });
  }

  /** Every collection click cancels a pending search first, for the reason on the Starred
   *  button: an orphaned debounced send would otherwise re-enter Search behind it. A click
   *  on a view is also the user asking for the grid, so it leaves the People page. */
  function show(switchView: () => Promise<void>) {
    mainPage.showGrid();
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
  /** Today, as the "On this day" row means it. Derived from the grid's version so that a
   *  window left open past midnight catches up the next time anything in the library
   *  moves; the click reads the clock itself, so it is never a day behind. */
  const today = $derived.by(() => {
    void library.info.version;
    const now = new Date();
    return { query: onThisDayQuery(now), label: onThisDayLabel(now) };
  });

  function showOnThisDay() {
    closeMenus();
    mainPage.showGrid();
    searchBox.search(onThisDayQuery(new Date()));
  }

  function showSearch(search: SavedSearch) {
    closeMenus();
    mainPage.showGrid();
    searchBox.search(search.query);
  }

  /** The People page, from the label beside the People chevron. A pending search is
   *  cancelled for the reason on `show`: landing behind the page, it would switch the grid
   *  to Search where nobody is looking. The list opens too, so the names are beside the page
   *  that makes them. */
  function showPeople() {
    searchBox.cancel();
    mainPage.showPeople();
    setOpen('people', true);
  }
</script>

<svelte:window onclick={closeMenus} onkeydown={(e) => e.key === 'Escape' && closeMenus()} />

<nav class="tree" aria-label="Folders" bind:this={tree}>
  <!-- The way back from an excursion without losing your place: a folder click lands on that
       folder's top, this lands where the gallery was left (returnToAll). No count: the grid
       reports none for the whole library, and Recent has none either. -->
  <button
    class="root all"
    class:active={onGrid && library.info.view === 'all'}
    onclick={showAll}
    title="Every photo, back where you left the gallery"
  >
    <Icon name="layout-grid" size={14} /><span class="name">All photos</span>
  </button>

  <button
    class="root starred"
    class:active={onGrid && library.info.view === 'starred'}
    onclick={() => show(() => library.setView('starred'))}
    title="Starred photos"
  >
    <Icon name="star" size={14} /><span class="name">Starred</span>
    <span class="count">{counted.format(library.info.starredCount)}</span>
  </button>

  <button
    class="root recent"
    class:active={onGrid && library.info.view === 'recent'}
    onclick={() => show(() => library.setView('recent'))}
    title="The newest photos by capture date"
  >
    <Icon name="clock" size={14} /><span class="name">Recent</span>
  </button>

  <!-- A search, not a view: what was taken on today's date in any year. No count, as a
       saved search has none: it would mean running the search on every library change. -->
  <button
    class="root on-this-day"
    class:active={onGrid && library.info.view === 'search' && library.info.searchQuery === today.query}
    onclick={showOnThisDay}
    title="Photos taken on {today.label}, in any year"
  >
    <Icon name="calendar" size={14} /><span class="name">On this day</span>
  </button>

  <!-- Only while the library has a video, or while the view is showing: a library of photos
       alone would otherwise carry a permanent "0" row, the reason Duplicates hides too. -->
  {#if library.info.videoCount > 0 || library.info.view === 'videos'}
    <button
      class="root videos"
      class:active={onGrid && library.info.view === 'videos'}
      onclick={() => show(() => library.setView('videos'))}
      title="Every video in the library"
    >
      <Icon name="play" size={14} /><span class="name">Videos</span>
      <span class="count">{counted.format(library.info.videoCount)}</span>
    </button>
  {/if}

  <!-- Only while there is something in it, or while it is what the grid shows: most
       libraries have no duplicates, and a permanent "(0)" row is noise. -->
  {#if library.info.duplicateCount > 0 || library.info.view === 'duplicates' || library.info.view === 'copies'}
    <button
      class="root duplicates"
      class:active={onGrid && library.info.view === 'duplicates'}
      onclick={() => show(() => library.setView('duplicates'))}
      title="Photos with a byte-identical copy elsewhere in the library"
    >
      <Icon name="copy" size={14} /><span class="name">Duplicates</span>
      <span class="count">{counted.format(library.info.duplicateCount)}</span>
    </button>
    {#if library.info.view === 'copies'}
      <!-- Not a saved place: it exists while the view is open, and leaving removes it. Not
           a `<button>`: it does nothing on click (the view is already open), so a button
           here was a dead tab stop announced as interactive with no action behind it. -->
      <div class="root copies" class:active={onGrid} aria-current={onGrid ? 'true' : undefined} title={library.info.copiesOf?.fileName}>
        <span class="name">Copies of {library.info.copiesOf?.fileName || 'a photo'}</span>
      </div>
    {/if}
  {/if}

  <!-- Like Duplicates, only while there is something in it or it is showing: a library
       nobody has hidden anything in needs no reminder that hiding exists. -->
  {#if library.info.hiddenCount > 0 || library.info.view === 'hidden'}
    <button
      class="root hidden-view"
      class:active={onGrid && library.info.view === 'hidden'}
      onclick={() => show(() => library.setView('hidden'))}
      title="Photos you have hidden. They stay on disk; unhide them from here"
    >
      <Icon name="eye-off" size={14} /><span class="name">Hidden</span>
      <span class="count">{counted.format(library.info.hiddenCount)}</span>
    </button>
  {/if}

  <!-- Albums: photon's own, editable, and Picasa's, mirrored from its INI and read-only. -->
  <button class="group" aria-expanded={open.albums} onclick={() => setOpen('albums', !open.albums)}>
    <span class="chevron"><Icon name={open.albums ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="folder" size={14} />
    <span class="name">Albums</span>
    <span class="count">{counted.format(library.albums.length)}</span>
  </button>
  {#if open.albums}
    {#each library.albums as album (album.id)}
      {#if editor.editing(album.id)}
        <input
          class="editor"
          bind:this={editorInput}
          bind:value={editor.text}
          readonly={editor.busy}
          aria-label="Album name"
          onkeydown={(e) => editorKeydown(e, editor, `[data-album="${album.id}"]`, () => editorInput)}
          onblur={commitEditor}
        />
      {:else}
        <button
          class="node"
          class:active={onGrid && library.info.view === 'album' && library.info.album === album.id}
          data-album={album.id}
          title={album.name}
          onclick={() => show(() => library.setAlbumView(album.id))}
          oncontextmenu={(e) => (album.picasa ? e.preventDefault() : albumContextMenu(e, album))}
        >
          <span class="name">{album.name}</span>
          {#if album.picasa}
            <!-- Picasa's own album, mirrored from its INI: no menu, since Rename and Delete
                 are all it would hold and the backend refuses both. -->
            <span class="picasa" role="img" aria-label="From Picasa" title="From Picasa. Change it in Picasa."
              ><Icon name="images" size={12} /></span
            >
          {/if}
          <span class="count">{counted.format(album.count)}</span>
        </button>
      {/if}
    {/each}
    {#if editor.editing()}
      <input
        class="editor"
        bind:this={editorInput}
        bind:value={editor.text}
        readonly={editor.busy}
        placeholder="Album name"
        aria-label="New album name"
        onkeydown={(e) => editorKeydown(e, editor, '.add-album', () => editorInput)}
        onblur={commitEditor}
      />
    {:else}
      <button class="node add-album" onclick={startNew}>New album…</button>
    {/if}
  {/if}

  <!-- Saved searches: a name over a query, re-run on every visit. No count - one would
       cost a full library pass per row on every change; see library/searches.rs. -->
  {#if library.searches.length > 0}
    <button class="group" aria-expanded={open.searches} onclick={() => setOpen('searches', !open.searches)}>
      <span class="chevron"><Icon name={open.searches ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="bookmark" size={14} />
      <span class="name">Searches</span>
      <span class="count">{counted.format(library.searches.length)}</span>
    </button>
    {#if open.searches}
      {#each library.searches as search (search.id)}
        {#if searchEditor.editing(search.id)}
          <input
            class="editor"
            bind:this={searchEditorInput}
            bind:value={searchEditor.text}
            readonly={searchEditor.busy}
            aria-label="Saved search name"
            onkeydown={(e) => editorKeydown(e, searchEditor, `[data-search="${search.id}"]`, () => searchEditorInput)}
            onblur={commitSearchEditor}
          />
        {:else}
          <button
            class="node"
            class:active={onGrid && library.info.view === 'search' && library.info.searchQuery === search.query}
            data-search={search.id}
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

  <!-- People: the people the user named among the faces photon found, and Picasa's contacts
       no such person is linked to, read from the INI beside the photos. Named on the People
       page, which the label opens; the chevron only folds the list, as the other groups'
       whole row does. -->
  <div class="group-row">
    <button
      class="fold"
      aria-expanded={open.people}
      aria-label={open.people ? 'Hide people' : 'Show people'}
      title={open.people ? 'Hide people' : 'Show people'}
      onclick={() => setOpen('people', !open.people)}
    >
      <span class="chevron"><Icon name={open.people ? 'chevron-down' : 'chevron-right'} size={12} /></span>
    </button>
    <button
      class="group people"
      class:active={mainPage.current === 'people'}
      aria-current={mainPage.current === 'people' ? 'page' : undefined}
      title="Name the faces photon found"
      onclick={showPeople}
    >
      <Icon name="user" size={14} />
      <span class="name">People</span>
      {#if library.toName > 0}
        <span class="count to-name" title="Groups of faces waiting for a name">{counted.format(library.toName)} to name</span>
      {:else}
        <span class="count">{counted.format(library.people.length)}</span>
      {/if}
    </button>
  </div>
  {#if open.people}
    {#each library.people as person (person.key)}
      <button
        class="node"
        class:active={onGrid && library.info.view === 'person' && library.info.person === person.key}
        title={person.name}
        onclick={() => show(() => library.setPersonView(person.key))}
      >
        <span class="name">{person.name}</span>
        <span class="count">{counted.format(person.count)}</span>
      </button>
    {:else}
      <p class="empty small">No named people yet. Name the faces photon found on the People page; names Picasa recorded are listed here too.</p>
    {/each}
  {/if}

  <!-- Tags: keywords read from the photos' own XMP and IPTC. Read only. -->
  <button class="group" aria-expanded={open.tags} onclick={() => setOpen('tags', !open.tags)}>
    <span class="chevron"><Icon name={open.tags ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="tag" size={14} />
    <span class="name">Tags</span>
    <span class="count">{counted.format(shownTags.length)}</span>
  </button>
  {#if open.tags}
    {#each shownTags as t (t.tag)}
      <button
        class="node"
        class:active={onGrid && library.info.view === 'tag' && library.info.tag === t.tag}
        title={t.tag}
        onclick={() => show(() => library.setTagView(t.tag))}
      >
        <span class="name">{t.tag}</span>
        <span class="count">{counted.format(t.count)}</span>
      </button>
    {:else}
      <p class="empty small">No keywords. photon reads them from the photos themselves.</p>
    {/each}
  {/if}

  {#each years as group (group.year)}
    {#if group.year !== null}<h2 class="year">{group.year}</h2>{/if}
    <div class="folders" style:contain-intrinsic-block-size="auto {group.rows.length * ROW}px">
      {#each group.rows as row (row.folderId)}
        {#if folderEditor.editing(row.folderId)}
          <input
            class="editor"
            bind:this={folderEditorInput}
            bind:value={folderEditor.text}
            readonly={folderEditor.busy}
            aria-label="Folder name in photon"
            title={folderById(row.folderId)?.path}
            onkeydown={(e) => editorKeydown(e, folderEditor, `[data-folder="${row.folderId}"]`, () => folderEditorInput, () => folderRenameBack)}
            onblur={commitFolderEditor}
          />
        {:else}
          <button
            class="node"
            class:here={here === row.folderId}
            aria-current={here === row.folderId ? 'location' : undefined}
            data-folder={row.folderId}
            title={folderById(row.folderId)?.path}
            onclick={() => jumpToFolder(row.folderId)}
            oncontextmenu={(e) => folderMenu(e, row.folderId)}
          >
            <span class="name">{row.name}</span>
            <span class="count">{counted.format(row.count)}</span>
          </button>
        {/if}
      {/each}
    </div>
  {/each}

  <!-- Once the list has been read: before that an empty one is not "no folders". -->
  {#if library.foldersKnown && library.folders.watched.length === 0}
    <p class="empty">No folders yet.</p>
    <button class="add" onclick={onopensettings}>Add a folder in Settings…</button>
  {/if}
</nav>

{#if menu}
  <FolderMenu at={menu} folder={menu.folder} onclose={() => (menu = null)} onrename={(f) => startFolderRename(f)} />
{/if}

{#if albumMenu}
  {@const album = albumMenu.album}
  <Menu at={albumMenu}>
    <button role="menuitem" onclick={() => startRename(album)}>Rename…</button>
    <button role="menuitem" class="danger" onclick={() => deleteAlbum(album)}>Delete…</button>
  </Menu>
{/if}

{#if searchMenu}
  {@const search = searchMenu.search}
  <Menu at={searchMenu}>
    <button role="menuitem" onclick={() => startSearchRename(search)}>Rename…</button>
    <button role="menuitem" class="danger" onclick={() => deleteSearch(search)}>Delete…</button>
  </Menu>
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
  /* People's header is two buttons, the fold and the page, laid out to sit exactly where
     a one-button group row puts its chevron and icon: the row takes the group's margins,
     the fold its left padding and half the gap, the label the other half. */
  .group-row { display: flex; flex: none; margin: var(--s-2) 6px 0; }
  .group-row .group { flex: 1; min-width: 0; margin: 0; padding-left: var(--s-1); }
  .fold {
    display: flex;
    align-items: center;
    flex: none;
    height: 28px;
    padding: 0 var(--s-1) 0 var(--s-2);
    border: 0;
    border-radius: var(--r-3);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .node { position: relative; padding-left: 28px; }
  /* Where the grid is: a bar in the row's indent, beside the name. Not the fill `.active`
     has, which says which view the grid shows - both are on screen at once. */
  .node.here::before {
    content: '';
    position: absolute;
    left: 12px;
    top: 7px;
    bottom: 7px;
    width: 3px;
    border-radius: 2px;
    background: var(--accent);
  }
  /* A library can have thousands of folders, and the list is laid out again on every frame
     of a splitter drag. `content-visibility: auto` skips the layout and paint of whatever is
     off screen while keeping every row in the accessibility tree and focusable - unlike
     `display: none` or `hidden` - and the intrinsic size keeps the scroll height exact, since
     every row is 28px (ROW in the script). Measured in headless Chromium on 5000 folders in
     25 years, a width change and its layout: 35-42ms with neither, 4-6ms per row alone,
     0.2ms with the year groups too - the scroll height and every row's position unchanged.

     Both, because each covers what the other cannot: a flat sort has one group holding every
     folder, always on screen, and People and Tags are `.node` lists with no group at all.

     The group's containment clips paint to its own box, and the global focus ring sits 4px
     outside a row (a 2px outline, 2px off): the padding makes room for the first and last
     rows' rings, and the negative margin takes it back out of the layout. */
  .node { content-visibility: auto; contain-intrinsic-block-size: auto 28px; }
  .folders {
    display: flex;
    flex-direction: column;
    content-visibility: auto;
    padding-block: 4px;
    margin-block: -4px;
  }
  /* Nested under Duplicates, the same depth as an album under its group. */
  /* Nothing happens on a click (see the markup), so it must not offer one: `.root` is
     styled for the buttons it is otherwise always on. Its hover background is already
     covered by `.copies.active`, which is declared after it. */
  .copies { padding-left: 28px; cursor: default; }
  .root:hover, .node:hover, .group:hover, .fold:hover { background: var(--hover); }
  /* Every row that can be the current place: `.root` is all of the top rows, so a new one
     is covered without being named. Listed one by one, this rule had left out All photos and
     Videos, and neither showed that it was where the grid was. */
  .root.active, .node.active, .people.active { background: var(--accent-soft); }
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
  .picasa { display: inline-flex; flex: none; color: var(--text-dim); margin-left: 4px; }
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
  @media (prefers-reduced-motion: reduce) {
    .root, .node, .group, .fold { transition: none; }
  }
</style>
