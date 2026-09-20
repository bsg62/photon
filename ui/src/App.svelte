<script lang="ts">
  import { onMount, tick } from 'svelte';
  // `open` is already this component's name for opening the viewer.
  import { open as pickFolder } from '@tauri-apps/plugin-dialog';
  import { api } from './lib/api';
  import { theme } from './lib/app-theme.svelte';
  import { locateItem } from './lib/folders';
  import { library } from './lib/library.svelte';
  import { ownsSelectAll } from './lib/nav';
  import { resultsChanged, viewKey } from './lib/search';
  import { searchBox } from './lib/search-box.svelte';
  import type { SettingsSection } from './lib/settings';
  import { clampSidebarWidth, SIDEBAR_DEFAULT, SIDEBAR_STEP } from './lib/sidebar';
  import { createExportDialog } from './lib/export-dialog.svelte';
  import { createTagPicker } from './lib/tag-picker.svelte';
  import Icon from './components/Icon.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import Grid from './components/Grid.svelte';
  import SearchBar from './components/SearchBar.svelte';
  import Settings from './components/Settings.svelte';
  import ExportDialog from './components/ExportDialog.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import TagPicker from './components/TagPicker.svelte';
  import Toasts from './components/Toasts.svelte';
  import Viewer from './components/Viewer.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();
  let viewerAt = $state<number | null>(null);
  let settingsAt = $state<SettingsSection | null>(null);
  let gear: HTMLButtonElement | undefined = $state();
  /** The keyword dialog for the grid's selection. It lives here, not in the grid, because
   *  it is an overlay: `covered` below makes everything behind one inert, and a dialog
   *  mounted inside `<main>` would be made inert by its own opening - Settings could then
   *  be opened on top of it by tabbing to a gear that should not have been reachable. */
  const picker = createTagPicker({
    apply: (mode, tag, ids) => (mode === 'add' ? api.addItemsTag(ids, tag) : api.removeItemsTag(ids, tag)),
  });

  /** Exporting copies. Here with the other overlays, for the reason the picker gives, and
   *  the folder picker is the plugin's - photon never types a path for the user. */
  const exporter = createExportDialog({
    run: (ids, dest, applyEdits) => api.exportItems(ids, dest, applyEdits),
    pick: async () => {
      const picked = await pickFolder({ directory: true, multiple: false, title: 'Export copies to…' });
      return typeof picked === 'string' ? picked : null;
    },
    remember: (apply) => api.setExportApplyEdits(apply),
  });

  /** Everything behind an overlay is inert; the overlays never stack, because each one
   *  makes the other's opener inert. */
  const covered = $derived(
    viewerAt !== null || settingsAt !== null || picker.visible || exporter.visible,
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
    return () => {
      library.dispose();
      theme.dispose();
    };
  });

  // Spec §5: the grid returns to the top whenever the result set changes — a new view, or
  // a refined query within Search — since a scroll position from one set of photos is
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

  function closeViewer(at: number) {
    viewerAt = null;
    // Same rule, the other way round: closing on the photo the viewer was opened with
    // leaves the selection alone; closing after navigating collapses to what is on screen.
    if (library.selected !== at) library.selected = at;
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

  /** A camera or lens clicked in the viewer's info panel. Closes the viewer, since the
   *  photo it shows has no fixed place in the results, and hands the query to the search
   *  box so the box shows what the grid is filtered by. */
  function searchFrom(query: string) {
    viewerAt = null;
    searchBox.search(query);
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
    if (e.key !== 'F11') return;
    e.preventDefault();
    api
      .windowFullscreen()
      .then((on) => api.setWindowFullscreen(!on))
      .catch(library.reportError);
  }

  function openSettings(section: SettingsSection) {
    settingsAt = section;
  }

  /** The top bar is still `inert` until the DOM catches up with `settingsAt`, and focusing
   *  an inert element silently does nothing — hence the tick before handing focus back. */
  async function closeSettings() {
    settingsAt = null;
    await tick();
    gear?.focus();
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

  async function jump(folderId: number) {
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
</script>

<svelte:window onresize={() => (sidebarWidth = clampSidebarWidth(sidebarWidth, window.innerWidth))} onkeydown={onkeydown} />
<div class="app" style:--sidebar-width="{sidebarWidth}px">
  <div class="topbar" inert={covered}>
    <SearchBar />
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
    <Grid bind:this={grid} onopen={open} onkeywords={openKeywords} onexport={openExport} />
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
{#if viewerAt !== null}<Viewer offset={viewerAt} onclose={closeViewer} onlocate={locate} onsearch={searchFrom} />{/if}
{#if settingsAt !== null}<Settings section={settingsAt} onclose={closeSettings} />{/if}
<TagPicker {picker} onclosed={closeKeywords} />
<ExportDialog dialog={exporter} onclosed={closeExport} />
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
  .content { min-width: 0; min-height: 0; }
  /* Its own grid row, so it stays put while the sidebar and the grid scroll under it. */
  .topbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: var(--s-2);
    background: var(--chrome);
    border-bottom: 1px solid var(--line);
  }
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
</style>
