<script lang="ts">
  import { onMount, tick } from 'svelte';
  // `open` is already this component's name for opening the viewer.
  import { open as pickFolder } from '@tauri-apps/plugin-dialog';
  import { api, events, type NamedPerson } from './lib/api';
  import { gridSize } from './lib/app-grid-size.svelte';
  import { theme } from './lib/app-theme.svelte';
  import { showCopies } from './lib/copies';
  import { locateItem } from './lib/folders';
  import { library } from './lib/library.svelte';
  import { mainPage } from './lib/main-page.svelte';
  import { ownsSelectAll } from './lib/nav';
  import { openFacePhoto } from './lib/people';
  import { resultsChanged, viewKey } from './lib/search';
  import { searchBox } from './lib/search-box.svelte';
  import { focusesSearch, opensShortcuts } from './lib/shortcuts';
  import type { SettingsSection } from './lib/settings';
  import { isMac } from './lib/url';
  import { clampSidebarWidth, SIDEBAR_DEFAULT, SIDEBAR_STEP } from './lib/sidebar';
  import { createExportDialog } from './lib/export-dialog.svelte';
  import { createFolderDrop } from './lib/folder-drop.svelte';
  import { createPersonPicker, type PickerTarget } from './lib/person-picker.svelte';
  import { createTagPicker } from './lib/tag-picker.svelte';
  import { grabPoster } from './lib/video-grab';
  import { videoState } from './lib/video-state.svelte';
  import { createVideoThumbnailer } from './lib/video-thumbnailer.svelte';
  import { mediaSupported, videoUrl, PREVIEW_MAX_EDGE } from './lib/video';
  import Icon from './components/Icon.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import PeoplePage from './components/PeoplePage.svelte';
  import PersonPicker from './components/PersonPicker.svelte';
  import SearchBar from './components/SearchBar.svelte';
  import ShortcutSheet from './components/ShortcutSheet.svelte';
  import Compare from './components/Compare.svelte';
  import Settings from './components/Settings.svelte';
  import SizeControl from './components/SizeControl.svelte';
  import SortControl from './components/SortControl.svelte';
  import ExportDialog from './components/ExportDialog.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import TagPicker from './components/TagPicker.svelte';
  import Toasts from './components/Toasts.svelte';
  import Viewer from './components/Viewer.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();
  let peoplePage: ReturnType<typeof PeoplePage> | undefined = $state();
  let viewer: ReturnType<typeof Viewer> | undefined = $state();
  let viewerAt = $state<number | null>(null);
  let settingsAt = $state<SettingsSection | null>(null);
  /** Counts Settings closing, for the People page: Find faces may have been switched there
   *  in a way the page has no other word of (`PeoplePage`'s switch read says which). */
  let settingsClosed = $state(0);
  let compareIds = $state<number[] | null>(null);
  let compare: ReturnType<typeof Compare> | undefined = $state();
  /** The shortcut sheet (`?`). Unlike the other overlays it opens over the viewer and over
   *  compare as well as over the grid, since those are where most of the keys are. */
  let shortcutsOpen = $state(false);
  /** What held focus when the sheet opened, to hand it back to. */
  let shortcutsFrom: HTMLElement | null = null;
  let gear: HTMLButtonElement | undefined = $state();
  let searchBar: ReturnType<typeof SearchBar> | undefined = $state();
  /** The keyword dialog for the grid's selection. It lives here, not in the grid, because
   *  it is an overlay: `covered` below makes everything behind one inert, and a dialog
   *  mounted inside `<main>` would be made inert by its own opening - Settings could then
   *  be opened on top of it by tabbing to a gear that should not have been reachable. */
  const picker = createTagPicker({
    apply: (mode, tag, ids) => (mode === 'add' ? api.addItemsTag(ids, tag) : api.removeItemsTag(ids, tag)),
  });

  /** The named people the person dialog lists and joins, read each time it opens: the
   *  sidebar's list (`library.people`) leaves out a person with no visible photo, and the
   *  dialog would then call their name a new person while the backend joins them. */
  let namedPeople = $state.raw<NamedPerson[]>([]);
  /** Numbered, so a read from an earlier opening landing late cannot replace a newer one. */
  let namedRead = 0;

  /** Naming a face (from the viewer) or adding photos to a person (from the grid). An
   *  overlay like the keyword dialog, so here for the same reason; `nameOf` reads the
   *  stored spelling, so the toast says "Anna" when "anna" was typed for her. */
  const personPicker = createPersonPicker({
    nameFaces: api.nameFaces,
    nameItems: api.nameItems,
    nameOf: (id) => namedPeople.find((p) => p.id === id)?.name,
  });
  /** Where the person dialog hands focus back to: the viewer it was opened over, or the grid. */
  let personPickerFrom: 'grid' | 'viewer' = 'grid';

  /** Exporting copies. Here with the other overlays, for the reason the picker gives, and
   *  the folder picker is the plugin's - photon never types a path for the user. */
  const exporter = createExportDialog({
    run: (ids, dest, applyEdits, maxEdge) => api.exportItems(ids, dest, applyEdits, maxEdge),
    pick: async () => {
      const picked = await pickFolder({ directory: true, multiple: false, title: 'Export copies to…' });
      return typeof picked === 'string' ? picked : null;
    },
    check: (dest) => api.checkExportDest(dest),
    remember: (apply) => api.setExportApplyEdits(apply),
  });

  /** Folders dragged in from a file manager are watched. Not an overlay in `covered`'s
   *  sense: it takes no focus and no clicks, it only says what letting go will do. */
  const folderDrop = createFolderDrop({
    add: (path) => api.addFolder(path),
    refresh: () => library.refreshFolders(),
    notify: library.notify,
    reportError: library.reportError,
  });

  /** Everything behind an overlay is inert; the overlays never stack, because each one
   *  makes the other's opener inert. */
  const covered = $derived(
    viewerAt !== null ||
      settingsAt !== null ||
      compareIds !== null ||
      picker.visible ||
      personPicker.visible ||
      exporter.visible ||
      shortcutsOpen,
  );
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
    // `init` reports its own failures; theme-boot.js has already set the first frame.
    void theme.init();
    // Same lifecycle as the theme, and for the same reason: a module singleton that has to
    // come back to life when App remounts.
    void gridSize.init();
    // Videos: where they are served and whether this webview can play them, then the
    // session that tells the backend so - on Linux without GStreamer's plugins, playing one
    // would take the window down, and without a session no thumbnail request waits on us.
    // `disposed` is this mount's own guard, the same pattern `theme.svelte.ts` uses for its
    // generation: the setup below awaits two round trips before it ever calls
    // `thumbnailer.start()`, so `onMount`'s cleanup can run first, and a `videoSessionStart`
    // landing after that would tell a backend nobody is left to poll on behalf of.
    // `createVideoThumbnailer` guards `start()` itself too, since `stop()` is a disposal -
    // this mount creates a fresh thumbnailer, never reusing a stopped one.
    let disposed = false;
    const thumbnailer = createVideoThumbnailer({
      nextJob: api.nextVideoJob,
      put: api.putVideoFrame,
      fail: api.videoFrameFailed,
      url: (id) => videoUrl(videoState.base ?? '', id),
      grab: (url, signal) => grabPoster(url, signal, PREVIEW_MAX_EDGE),
    });
    void (async () => {
      const base = await api.mediaBase().catch(() => null);
      if (disposed) return;
      videoState.base = base;
      videoState.supported = base !== null && mediaSupported((t) => document.createElement('video').canPlayType(t));
      await api.videoSessionStart(videoState.supported).catch(() => {});
      if (disposed) return;
      if (videoState.supported) thumbnailer.start();
    })();
    // The listener arrives a round trip later; one that lands after this mount has gone
    // is dropped at once, like the video session above.
    let stopDrag: (() => void) | undefined;
    void events
      .onFileDrag((e) => void folderDrop.handle(e).catch(library.reportError))
      .then((unlisten) => {
        if (disposed) unlisten();
        else stopDrag = unlisten;
      })
      .catch(library.reportError);
    return () => {
      library.dispose();
      theme.dispose();
      gridSize.dispose();
      disposed = true;
      thumbnailer.stop();
      stopDrag?.();
    };
  });

  // Spec §5: the grid returns to the top whenever the result set changes — a new view, a
  // refined query within Search, or a new sort — since a scroll position from one set of photos is
  // arbitrary against another's. Tracked here rather than in FolderTree because App owns
  // the `grid` binding and its scroll helper.
  let last = viewKey(library.info);
  $effect(() => {
    const next = viewKey(library.info);
    if (resultsChanged(last, next)) {
      last = next;
      grid?.scrollToOffset(0, 'start');
    }
  });

  function open(offset: number) {
    // Only when it is not already the lead: assigning collapses a multi-selection, and
    // Enter on a selection of twelve should open one photo without throwing the other
    // eleven away. (A double-click collapses anyway — the click lands first.)
    if (library.selected !== offset) library.selected = offset;
    viewerAt = offset;
  }

  async function closeViewer(at: number, itemId: number | null) {
    viewerAt = null;
    // Same rule, the other way round: closing on the photo the viewer was opened with
    // leaves the selection alone; closing after navigating collapses to what is on screen.
    // By id, because the page holding `at` may be long gone; see `closeViewerOn`.
    library.closeViewerOn(at, itemId);
    // Behind the People page too: a face's photo may have switched the grid to All, and
    // the hidden grid then holds the photo the user last saw rather than All's first folder,
    // as "Locate in photon" leaves it.
    grid?.scrollToOffset(at, 'nearest');
    if (mainPage.current === 'people') {
      // Opened from a face: the page is where the user was, and the grid under it is
      // inert. After `tick`, for `closeSettings`' reason - `<main>` is inert until the DOM
      // catches up with `covered`.
      await tick();
      peoplePage?.focus();
      return;
    }
    grid?.focus();
  }

  /** A face double-clicked on the People page: its photo in the viewer, over the page. The
   *  grid behind is switched to All photos when its view does not hold the photo, as "Locate
   *  in photon" does; the page stays, so closing the viewer lands back on it. */
  function openFace(itemId: number) {
    void openFacePhoto(itemId, {
      offsetOf: (id) => api.gridOffsetOfItem(id).catch(() => null),
      cancelSearch: () => searchBox.cancel(),
      showAll: () => library.setView('all'),
      open,
      notify: library.notify,
    });
  }

  /** "Locate in photon" from the viewer. Closes it first so the grid is what lands on the
   *  photo; the view switch and the lookup order are `locateItem`'s. `hidden` is the photo's
   *  own flag - a copy listed in the info panel is never hidden, since hidden copies are not
   *  listed. */
  async function locate(itemId: number, hidden = false) {
    viewerAt = null;
    mainPage.showGrid();
    await locateItem(itemId, hidden, {
      cancelSearch: () => searchBox.cancel(),
      currentView: () => library.settledView(),
      setView: (view) => library.setView(view),
      offsetOfItem: (id) => api.gridOffsetOfItem(id).catch(() => null),
      select: (offset, id) => {
        library.selectItem(offset, id);
        grid?.scrollToOffset(offset, 'nearest');
        grid?.focus();
      },
    });
  }

  /** "Show duplicates" from the tile menu; the switch-then-lookup order is `showCopies`'s. */
  async function showCopiesOf(itemId: number) {
    mainPage.showGrid();
    await showCopies(itemId, {
      cancelSearch: () => searchBox.cancel(),
      setCopiesView: (id) => library.setCopiesView(id),
      offsetOfItem: (id) => api.gridOffsetOfItem(id).catch(() => null),
      select: (offset, id) => {
        library.selectItem(offset, id);
        grid?.scrollToOffset(offset, 'nearest');
        grid?.focus();
      },
    }).catch(library.reportError);
  }

  /** A camera or lens clicked in the viewer's info panel. Closes the viewer, since the
   *  photo it shows has no fixed place in the results, and hands the query to the search
   *  box so the box shows what the grid is filtered by. */
  /** The info panel's "Show N duplicates in the grid". Closed first, like `locate`: the
   *  viewer is an overlay, and the grid behind it is inert until it goes, so the selection
   *  `showCopiesOf` lands would otherwise be focused into nothing. */
  function showCopiesFromViewer(itemId: number) {
    viewerAt = null;
    void showCopiesOf(itemId);
  }

  /** After `tick`: `<main>`, and from the People page the grid's own layer, stay inert until
   *  the DOM catches up (`closeSettings`). */
  async function searchFrom(query: string) {
    viewerAt = null;
    mainPage.showGrid();
    searchBox.search(query);
    await tick();
    grid?.focus();
  }

  /** A name clicked in the viewer's info panel: that person's view, as the sidebar's People
   *  list opens it. The viewer closes first, for `searchFrom`'s reason, and a pending search
   *  is cancelled for the sidebar's: its debounced send would otherwise re-enter Search behind
   *  the switch. After `tick`, `<main>` is no longer inert. */
  async function showPerson(key: string) {
    viewerAt = null;
    searchBox.cancel();
    mainPage.showGrid();
    void library.setPersonView(key);
    await tick();
    grid?.focus();
  }

  /** F11 toggles fullscreen, everywhere. It exists for its own sake and as the way out of a
   *  trap: the window's fullscreen state is remembered across launches, so quitting in the
   *  middle of a slideshow reopens photon fullscreen, with no title bar to leave it by. */
  function onkeydown(e: KeyboardEvent) {
    // Ctrl/Cmd+A is photon's, not the webview's. Unprevented, the webview runs its own
    // select-all and paints the whole window in selection highlight — the way a browser
    // treats a page. The grid's handler has already run by the time this does, so a
    // Ctrl+A over the grid has selected its photos and this only stops the default that
    // would otherwise follow; everywhere else the key does nothing at all, which is the
    // fix. A text entry keeps it, because there it means "select this field's text".
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a' && ownsSelectAll(e.target as HTMLElement | null)) {
      e.preventDefault();
      return;
    }
    // The `/` that focused the search box, still held: its repeats arrive in the box, where
    // the key is a character, and the first of them would replace the selected search with
    // a slash and run it.
    if (slashHeld && e.key === '/' && e.repeat) {
      e.preventDefault();
      return;
    }
    if (focusesSearch(e, e.target as HTMLElement | null, mac)) {
      // Prevented whether or not it is acted on: Ctrl+F is photon's for Ctrl+A's reason - a
      // webview with a find bar of its own would open it over the app - and `/` must not be
      // typed into the box it has just focused. (The shortcut sheet and the person dialog
      // stop every key before it gets here, so under those two the chord is still the
      // webview's.) Under an overlay the top bar is inert, and the key does nothing: the
      // viewer or a dialog is what the user is in. Nor while the grid holds a rubber band,
      // for `canShowShortcuts`' reason: the band's release takes the focus back to the grid,
      // and what was meant for the box would be typed at the photos - `h` hides them.
      e.preventDefault();
      if (!covered && !grid?.dragging()) {
        slashHeld = e.key === '/';
        searchBar?.focus();
      }
      return;
    }
    if (opensShortcuts(e, e.target as HTMLElement | null)) {
      // The sheet's own handler closes it and stops the key there; this is the `?` that
      // arrives with focus on `<body>`.
      if (shortcutsOpen) {
        e.preventDefault();
        void closeShortcuts();
      } else if (canShowShortcuts()) {
        e.preventDefault();
        openShortcuts();
      }
      return;
    }
    if (e.key !== 'F11') return;
    e.preventDefault();
    api
      .windowFullscreen()
      .then((on) => api.setWindowFullscreen(!on))
      .catch(library.reportError);
  }

  const mac = isMac();
  /** A `/` keydown focused the search box and the key has not come up yet. */
  let slashHeld = false;

  function openSettings(section: SettingsSection) {
    settingsAt = section;
  }

  /** Not over another dialog: each of those is a question being answered, and Settings
   *  lists the keys itself. And not while a rubber band is held in the grid, which an
   *  overlay would leave running behind it (`Grid.dragging`). */
  function canShowShortcuts(): boolean {
    const dialog = settingsAt !== null || picker.visible || personPicker.visible || exporter.visible;
    return !dialog && !grid?.dragging();
  }

  function openShortcuts() {
    const active = document.activeElement;
    // `<body>` is what "nothing has focus" reads as; handing focus back to it would leave
    // the grid's keys dead, so that case takes the fallback in `closeShortcuts`.
    shortcutsFrom = active instanceof HTMLElement && active !== document.body ? active : null;
    shortcutsOpen = true;
  }

  /** Focus goes back to what had it, after `tick` for `closeSettings`' reason: everything
   *  behind the sheet is inert until the DOM catches up. If that element has gone meanwhile
   *  (a scan can replace the tile), or nothing had focus, whatever is in front takes it. */
  async function closeShortcuts() {
    shortcutsOpen = false;
    const from = shortcutsFrom;
    shortcutsFrom = null;
    await tick();
    if (from?.isConnected) from.focus();
    else if (viewerAt !== null) viewer?.focus();
    else if (compareIds !== null) compare?.focus();
    else if (mainPage.current === 'people') peoplePage?.focus();
    else grid?.focus();
  }

  /** Opens the compare overlay for a selection. Called by the grid (Task 5). */
  function openCompare(ids: number[]) {
    compareIds = ids;
  }

  /** As `closeSettings`: `<main>` is still inert until the DOM catches up with `covered`,
   *  and focusing an inert element silently does nothing. */
  async function closeCompare() {
    compareIds = null;
    await tick();
    grid?.focus();
  }

  /** Compare's Enter key: open the focused pane's photo in the viewer. Compare addresses a
   *  photo by item id, the viewer by grid offset, so this is the same lookup `locate` does.
   *  Unlike `locate`, compare never leaves the current view first - its panes are already
   *  drawn from a selection in that view, so the photo is always findable in it. A lookup
   *  that still comes back null (the photo was deleted from under the open overlay) opens
   *  nothing; compare has already closed via its own `onclose`, so the grid is what is left
   *  on screen, which is a reasonable place to land. */
  async function openFromCompare(itemId: number) {
    const at = await api.gridOffsetOfItem(itemId).catch(() => null);
    if (at === null) return;
    open(at);
  }

  /** The top bar is still `inert` until the DOM catches up with `settingsAt`, and focusing
   *  an inert element silently does nothing — hence the tick before handing focus back. */
  async function closeSettings() {
    settingsAt = null;
    settingsClosed++;
    await tick();
    gear?.focus();
  }

  /** A year, camera or lens clicked in Settings' Statistics: the dialog closes onto that
   *  search. Focus goes to the grid, where the answer is, rather than back to the gear;
   *  after `tick`, for `closeSettings`' reason. */
  async function searchFromSettings(query: string) {
    settingsAt = null;
    mainPage.showGrid();
    searchBox.search(query);
    await tick();
    grid?.focus();
  }

  /** The selection is captured now, not read when the dialog writes: the dialog takes
   *  focus, and a scan landing while it is open can rebind what the grid has selected. What
   *  the user was told the dialog would act on is what it acts on. */
  function openKeywords(mode: 'add' | 'remove') {
    const ids = library.selectedItemIds;
    if (ids.length) picker.show(mode, ids);
  }

  /** The grid keeps its own keyboard handling on its viewport, so a dialog that closes
   *  without handing focus back leaves the arrow keys dead until the user clicks. The
   *  `tick` is `closeSettings`' reason: `<main>` is still inert until the DOM catches up
   *  with `covered`, and focusing an inert element silently does nothing. */
  async function closeKeywords() {
    await tick();
    grid?.focus();
  }

  /** The person dialog, for photos in the grid (`items`) or one face in the viewer (`face`).
   *  The target is captured by `show`, for `openKeywords`' reason. */
  function openPersonPicker(target: PickerTarget, from: 'grid' | 'viewer') {
    personPickerFrom = from;
    // The sidebar's named people stand in until the read lands, and stay if it fails.
    namedPeople = library.people
      .filter((p) => p.key.startsWith('p:'))
      .map((p) => ({ id: Number(p.key.slice(2)), name: p.name }));
    const read = ++namedRead;
    api
      .namedPeople()
      .then((people) => {
        if (read === namedRead) namedPeople = people;
      })
      .catch(library.reportError);
    personPicker.show(target);
  }

  /** Focus goes back where the dialog was opened from, after `tick` for `closeKeywords`'
   *  reason - and the viewer, too, is inert while the dialog is over it. A viewer that has
   *  gone meanwhile leaves the grid. */
  async function closePersonPicker() {
    await tick();
    if (personPickerFrom === 'viewer' && viewer) viewer.focus();
    else grid?.focus();
  }

  /** The remembered checkbox is read when the dialog opens rather than held in the UI: it
   *  lives in the library, and Settings is not the only thing that can change it. A read
   *  that fails must not cost the user the export, so it falls back to rendering edits -
   *  the default, and what the dialog says it does. */
  async function openExport() {
    const ids = library.selectedItemIds;
    if (!ids.length) return;
    const applyEdits = await api.exportApplyEdits().catch(() => true);
    exporter.show(ids, applyEdits);
  }

  /** As `closeKeywords`: the grid's keys live on the grid, and `<main>` is inert until the
   *  DOM catches up with `covered`. */
  async function closeExport() {
    await tick();
    grid?.focus();
  }

  /** Enter in the search box, or Escape on an empty one: on to the photos, or to the People
   *  page while it is what the main area shows and the grid behind it is inert. */
  function leaveSearch() {
    if (mainPage.current === 'people') peoplePage?.focus();
    else grid?.focus();
  }

  async function jump(folderId: number) {
    mainPage.showGrid();
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
</script>

<svelte:window
  onresize={() => (sidebarWidth = clampSidebarWidth(sidebarWidth, window.innerWidth))}
  onkeydown={onkeydown}
  onkeyup={(e) => {
    if (e.key === '/') slashHeld = false;
  }}
  onblur={() => (slashHeld = false)}
/>
<div class="app" style:--sidebar-width="{sidebarWidth}px">
  <div class="topbar" inert={covered}>
    <SearchBar bind:this={searchBar} onleave={leaveSearch} />
    <!-- The grid's own controls: under the People page they would sort and size a grid no
         one can see. Hidden rather than removed, so the search box and the gear keep their
         places; `visibility` takes them out of the tab order too. -->
    <div class="grid-controls" class:away={mainPage.current !== 'grid'}>
      <SortControl />
      <SizeControl />
    </div>
    <button class="gear" bind:this={gear} aria-label="Settings" title="Settings" onclick={() => openSettings('folders')}
      ><Icon name="settings" size={18} /></button
    >
  </div>
  <aside class="sidebar" inert={covered}>
    <FolderTree onjump={jump} onopensettings={() => openSettings('folders')} />
  </aside>
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
    inert={covered}
    onpointerdown={startResize}
    onpointermove={moveResize}
    onpointerup={endResize}
    onpointercancel={endResize}
    onkeydown={keyResize}
  ></div>
  <main class="content" inert={covered}>
    <!-- The People page has no rows, so it is not a grid view (main-page.svelte.ts): it is
         drawn over the grid, which stays mounted beneath it, keeping its view, scroll,
         selection and layout. Not unmounted: a remounted grid starts at the top with the
         launch restore long done, and its "remember the folder at the top" effect then
         overwrote the user's place with the first folder on every return from the page.
         Not `display: none` either: that drops the layout box, resets `scrollTop` and shows
         the grid's ResizeObserver a zero width, whose relayout trips the same write.
         `visibility: hidden` keeps the box; `inert` keeps the grid's keys and tiles out of
         reach. Its rubber band cannot be running when the page opens: the page opens from
         a sidebar click, and the viewport holds the pointer captured until the pointerup
         or pointercancel that ends a band. -->
    <div class="grid-layer" class:behind={mainPage.current === 'people'} inert={mainPage.current === 'people'}>
      <Grid
        bind:this={grid}
        onopen={open}
        onkeywords={openKeywords}
        onexport={openExport}
        onnameperson={(ids) => openPersonPicker({ kind: 'items', items: ids }, 'grid')}
        oncompare={openCompare}
        onshowcopies={showCopiesOf}
      />
    </div>
    {#if mainPage.current === 'people'}
      <div class="page-layer">
        <PeoplePage bind:this={peoplePage} {settingsClosed} onopen={openFace} onopensettings={() => openSettings('people')} />
      </div>
    {/if}
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
<!-- Inert under the person dialog, which is opened over it to name a face, and under the
     shortcut sheet: `aria-modal` alone does not keep Tab from walking out of the dialog into
     the viewer's controls. No box of its own, so the viewer is placed exactly as before. -->
<div class="under-dialog" inert={personPicker.visible || shortcutsOpen}>
  {#if viewerAt !== null}<Viewer
      bind:this={viewer}
      offset={viewerAt}
      onclose={closeViewer}
      onlocate={locate}
      onsearch={searchFrom}
      onshowcopies={showCopiesFromViewer}
      onnameface={(faceId) => openPersonPicker({ kind: 'face', face: faceId }, 'viewer')}
      onperson={showPerson}
      paused={personPicker.visible || shortcutsOpen}
    />{/if}
</div>
{#if settingsAt !== null}<Settings section={settingsAt} onclose={closeSettings} onsearch={searchFromSettings} />{/if}
<!-- As the viewer above: compare's keys live on its own element, which `inert` puts out of
     reach while the shortcut sheet is over it. -->
<div class="under-dialog" inert={shortcutsOpen}>
  {#if compareIds !== null}<Compare bind:this={compare} ids={compareIds} onclose={closeCompare} onopen={openFromCompare} />{/if}
</div>
{#if shortcutsOpen}<ShortcutSheet onclose={closeShortcuts} />{/if}
<TagPicker {picker} onclosed={closeKeywords} />
<PersonPicker picker={personPicker} people={namedPeople} onclosed={closePersonPicker} />
<ExportDialog dialog={exporter} onclosed={closeExport} />
<Toasts />
{#if folderDrop.hovering}
  <!-- Says what letting go will do. It takes no pointer events: the drag is the system's,
       and the webview reports the drop wherever it lands. -->
  <div class="drop" aria-hidden="true">
    <div class="drop-card"><Icon name="folder" size={20} /> Drop folders to add them to photon</div>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: var(--sidebar-width) 5px 1fr;
    grid-template-rows: auto 1fr auto;
    height: 100%;
  }
  .sidebar {
    overflow: auto;
    background: var(--chrome);
  }
  .splitter {
    cursor: col-resize;
    touch-action: none;
    background: var(--chrome);
    border-left: 1px solid var(--line);
  }
  /* Its own focus treatment rather than the global ring: a 5px bar cannot hold one. */
  .splitter:hover,
  .splitter:focus-visible {
    background: var(--accent);
    outline: none;
  }
  .content { position: relative; min-width: 0; min-height: 0; }
  .grid-layer { height: 100%; }
  .grid-layer.behind { visibility: hidden; }
  /* Opaque, since the grid's box is still there beneath it. */
  .page-layer { position: absolute; inset: 0; background: var(--surface); }
  /* Its own grid row, so it stays put while the sidebar and the grid scroll under it. */
  .topbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    /* The size control sits between the search bar and the gear, and `space-between` alone
       would leave it touching the gear: the gap is what keeps the three apart. */
    gap: var(--s-2);
    padding-right: var(--s-2);
    background: var(--chrome);
    border-bottom: 1px solid var(--line);
  }
  /* No box of its own: the two controls stay items of the top bar's flex row, spaced by its
     gap, as they were before they were wrapped. */
  .grid-controls { display: contents; }
  .grid-controls.away > :global(*) { visibility: hidden; }
  .gear {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--r-3);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .gear:hover { color: var(--text); background: var(--hover); }
  @media (prefers-reduced-motion: reduce) { .gear { transition: none; } }
  .statusbar { grid-column: 1 / -1; }
  .under-dialog { display: contents; }
  .drop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    background: var(--scrim);
    pointer-events: none;
  }
  .drop-card {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-4) var(--s-5);
    background: var(--surface);
    color: var(--text);
    border-radius: var(--r-4);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-dialog);
    font-size: var(--t-4);
  }
</style>
