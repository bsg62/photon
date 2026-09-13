<script lang="ts">
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { buildRows, columnsFor, GAP, itemSpan, rowOfItem, totalHeight, visibleRange } from '../lib/layout';
  import { move, type NavKey } from '../lib/nav';
  import Tile from './Tile.svelte';

  let { onopen }: { onopen: (offset: number) => void } = $props();

  const NAV_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'];
  const VISIBLE_DEBOUNCE_MS = 150;

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);

  const columns = $derived(columnsFor(Math.max(0, width - 2 * GAP)));
  const rows = $derived(buildRows(library.info.sections, columns));
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
    const next = sel === null ? 0 : move(sel, e.key as NavKey, library.info.sections, columns);
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
      {@const folderId = library.info.sections[row.section].folderId}
      {#if row.kind === 'header'}
        {@const folder = library.folderOf(folderId)}
        <div class="header" style:top="{row.top}px">
          <span class="name">{folder?.name ?? ''}</span>
          <span class="path">{folder?.path ?? ''}</span>
        </div>
      {:else}
        <div class="row" style:top="{row.top}px" style:gap="{GAP}px" style:padding-left="{GAP}px">
          {#each { length: row.count } as _, i (row.first + i)}
            {@const offset = row.first + i}
            <Tile
              entry={library.entry(offset)}
              selected={library.selected === offset}
              dimmed={!library.isOnline(folderId)}
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
