<script lang="ts">
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { buildRows, columnsFor, GAP, itemSpan, layoutSections, rowOfItem, topFolderId, totalHeight, visibleRange } from '../lib/layout';
  import { nextSelection, type NavKey } from '../lib/nav';
  import Tile from './Tile.svelte';

  let { onopen }: { onopen: (offset: number) => void } = $props();

  const NAV_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'];
  const VISIBLE_DEBOUNCE_MS = 150;

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);

  const columns = $derived(columnsFor(Math.max(0, width - 2 * GAP)));
  /** Recent is laid out as one continuous run of tiles with no folder headers; every other
   *  view keeps the index's folder sections. See `layoutSections` for why. Both the layout
   *  and the keyboard navigation read these rather than `library.info.sections`, so arrow
   *  keys move along the rows the eye sees.
   *
   *  A tile's offline dimming follows the photo's own folder for the same reason: one
   *  Recent row holds photos from several folders, so the section's folder answers for at
   *  most the first of them. */
  const sections = $derived(layoutSections(library.info.view, library.info.sections, library.info.len));
  const headers = $derived(library.info.view !== 'recent');
  const rows = $derived(buildRows(sections, columns, headers));
  const rendered = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, height * 2);
    return rows.slice(start, end);
  });
  const onScreen = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, 0);
    return itemSpan(rows.slice(start, end));
  });

  $effect(() => {
    const span = itemSpan(rendered);
    if (span) void library.ensure(span[0], span[1]);
  });

  // Jump to the folder the last session ended on, once there is something to jump to.
  //
  // Both guards are load-bearing. `len === 0` waits for the first grid: on a first run the
  // scan is still working when this mounts, and asking an empty index for a folder's offset
  // answers null, which is indistinguishable from "that folder is gone". `width === 0`
  // waits for the first layout pass: row tops are computed from the column count, so a jump
  // measured before the viewport has a width lands somewhere else once it gets one.
  //
  // It runs once. `started` guards the re-entry the `await`s open up, and `restoring` — read
  // by the effect below — stays set until the jump has actually been made.
  let started = false;
  $effect(() => {
    if (started || !library.restoring || library.info.len === 0 || width === 0) return;
    started = true;
    void (async () => {
      try {
        const folderId = await api.lastFolder();
        if (folderId === null) return;
        const offset = await api.gridOffsetOfFolder(folderId);
        // The folder survives in the library but has no section in this view (every photo
        // in it has gone missing, say). Staying at the top beats scrolling nowhere.
        if (offset === null) return;
        scrollToOffset(offset, 'start');
      } catch {
        // A failed restore is not worth a toast: the grid is simply where it already is.
      } finally {
        library.restoring = false;
      }
    })();
  });

  // Remember the folder at the top of the grid, so the next launch can come back to it.
  //
  // Only in the All view: Starred, Recent and Search are excursions, and letting one
  // overwrite this would mean closing photon from Starred lost the place the user was
  // actually browsing. Only on change, too — the folder at the top changes a handful of
  // times a session, while `scrollTop` changes on every frame of a flick, and this is a
  // database write.
  //
  // `library.restoring` gates the first write: until the restore below has run (or found
  // nothing to restore), the top of a freshly built grid is offset 0, and writing that
  // would overwrite the stored folder with the first one in the library before it could
  // ever be read.
  let remembered: number | null = null;
  $effect(() => {
    if (library.info.view !== 'all' || library.restoring) return;
    const folderId = topFolderId(rows, sections, scrollTop);
    if (folderId === null || folderId === remembered) return;
    remembered = folderId;
    api.setLastFolder(folderId).catch(() => {});
  });

  // Tell the thumbnail queue what's on screen once scrolling settles.
  $effect(() => {
    const span = onScreen;
    void library.pageTick;
    const timer = setTimeout(() => {
      if (!span) return;
      const ids: number[] = [];
      for (let o = span[0]; o < span[1]; o++) {
        const e = library.entry(o);
        if (e) ids.push(e.id);
      }
      api.setVisible(ids).catch(() => {});
    }, VISIBLE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  export function scrollToOffset(offset: number, align: 'start' | 'nearest' = 'start') {
    const i = rowOfItem(rows, offset);
    if (i < 0 || !viewport) return;
    // Before the first layout pass `height` is still 0 (e.g. Task 14 jumping to a
    // folder right after mount): 'nearest' would then over-scroll by a row, so wait
    // for a real viewport height. 'start' doesn't depend on `height` and stays exact.
    if (align === 'nearest' && height === 0) return;
    const row = rows[i];
    if (align === 'start') {
      const header = rows[i - 1];
      viewport.scrollTop = header?.kind === 'header' && header.first === row.first ? header.top : row.top;
    } else if (row.top < viewport.scrollTop) {
      viewport.scrollTop = row.top;
    } else if (row.top + row.height > viewport.scrollTop + height) {
      viewport.scrollTop = row.top + row.height - height;
    }
  }

  export function focus() {
    viewport?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    const sel = library.selected;
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) api.revealInFileManager(entry.id).catch(library.reportError);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (sel !== null) onopen(sel);
      return;
    }
    if (!NAV_KEYS.includes(e.key) || library.info.len === 0) return;
    e.preventDefault();
    const next = nextSelection(sel, e.key as NavKey, sections, columns);
    library.selected = next;
    scrollToOffset(next, 'nearest');
  }
</script>

<div
  class="viewport"
  bind:this={viewport}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = viewport.scrollTop)}
  {onkeydown}
  tabindex="0"
  role="grid"
  aria-label="Photos"
>
  {#if library.info.len === 0}
    <p class="empty">
      {#if library.info.view === 'starred'}
        No starred photos. photon reads stars from the Picasa.ini beside your photos — it
        never sets them.
      {:else if library.info.view === 'search'}
        No photos match “{library.info.searchQuery}”
      {:else}
        No photos yet. Add a folder to get started.
      {/if}
    </p>
  {/if}
  <div class="canvas" style:height="{totalHeight(rows)}px">
    {#each rendered as row (row.top)}
      {#if row.kind === 'header'}
        {@const folder = library.folderOf(sections[row.section].folderId)}
        <div class="header" style:top="{row.top}px">
          <span class="name">{folder?.name ?? ''}</span>
          <span class="path">{folder?.path ?? ''}</span>
        </div>
      {:else}
        <div class="row" style:top="{row.top}px" style:gap="{GAP}px" style:padding-left="{GAP}px">
          {#each { length: row.count } as _, i (row.first + i)}
            {@const offset = row.first + i}
            {@const entry = library.entry(offset)}
            <Tile
              {entry}
              selected={library.selected === offset}
              dimmed={!!entry && !library.isOnline(entry.folderId)}
              onselect={() => (library.selected = offset)}
              onopen={() => onopen(offset)}
            />
          {/each}
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .viewport { position: relative; height: 100%; overflow-y: auto; outline: none; }
  .canvas { position: relative; }
  .header, .row { position: absolute; left: 0; right: 0; }
  .header { display: flex; align-items: baseline; gap: 12px; height: 32px; padding: 8px 8px 0; }
  .header .name { font-weight: 600; }
  .header .path { color: var(--muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; color: var(--muted); margin: 0; }
</style>
