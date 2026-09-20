<script lang="ts">
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { buildRows, columnsFor, GAP, itemSpan, layoutSections, rowOfItem, topFolderId, totalHeight, visibleRange } from '../lib/layout';
  import { move, type NavKey } from '../lib/nav';
  import { yearMarks } from '../lib/timeline';
  import Tile from './Tile.svelte';
  import Timeline from './Timeline.svelte';

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
  const total = $derived(totalHeight(rows));
  /** The year strip. It needs folder headers to mark (so Recent, which has none, never
   *  shows it), more than one year to choose between, and something to scroll. */
  const marks = $derived(yearMarks(sections, rows));
  const scrubbable = $derived(marks.length > 1 && total > height);
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
      // One path is all a file manager takes, so this follows the same rule as the menu's
      // Reveal item: only when exactly one photo is selected, not just the lead of a wider
      // selection.
      if (library.selectionCount !== 1) return;
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) api.revealInFileManager(entry.id).catch(library.reportError);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      // preventDefault or the webview selects the chrome's own text behind the grid.
      e.preventDefault();
      library.selectAll().catch(library.reportError);
      return;
    }
    if (e.key === 'Escape') {
      // The window handler closes the menu on Escape. Clearing here as well would do both at
      // once, so the first Escape only ever dismisses the menu.
      if (menu) return;
      library.clearSelection();
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (sel !== null) onopen(sel);
      return;
    }
    if (!NAV_KEYS.includes(e.key) || library.info.len === 0) return;
    e.preventDefault();
    const next = move(sel, e.key as NavKey, sections, columns);
    library.selected = next;
    scrollToOffset(next, 'nearest');
  }
  // ---- the tile's context menu ----

  let menu = $state<{ x: number; y: number } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  /** Shift extends, Ctrl/Cmd toggles, a plain click collapses to one. Shift wins when both
   *  are held, which is what every file manager does. */
  function tileClick(e: MouseEvent, offset: number) {
    if (e.shiftKey) {
      void library.extendSelection(offset).catch(library.reportError);
    } else if (e.ctrlKey || e.metaKey) {
      library.toggleSelected(offset);
    } else {
      library.selected = offset;
    }
  }

  /** Right-clicking outside the selection selects that tile first, so what the menu acts on
   *  is always what is outlined. Inside it, the whole selection stands. */
  function tileMenu(e: MouseEvent, offset: number) {
    e.preventDefault();
    const entry = library.entry(offset);
    if (!entry) return;
    if (!library.isSelected(entry.id)) library.selected = offset;
    menu = { x: e.clientX, y: e.clientY };
  }

  function closeMenu() {
    menu = null;
  }

  /** "1 photo" / "12 photos", for a message naming a specific count. */
  function counted(n: number): string {
    return n === 1 ? '1 photo' : `${n.toLocaleString()} photos`;
  }

  const count = $derived(library.selectionCount);
  /** "photo" / "12 photos", for menu items that name what they will act on. */
  const subject = $derived(count === 1 ? 'photo' : counted(count));

  function withSelection(action: (ids: number[]) => Promise<unknown>) {
    const ids = library.selectedItemIds;
    menu = null;
    if (ids.length) action(ids).catch(library.reportError);
    focus();
  }

  /** Stars or unstars everything selected. The backend skips a folder whose `.picasa.ini`
   *  it cannot write and answers with how many landed, so a read-only folder in the
   *  selection costs the user a toast rather than the other eleven photos. */
  async function star(ids: number[], starred: boolean) {
    const done = await api.setStars(ids, starred);
    if (done < ids.length) {
      throw new Error(
        `${counted(ids.length - done)} of ${ids.length.toLocaleString()} could not be ${starred ? 'starred' : 'unstarred'}`,
      );
    }
  }
</script>

<svelte:window onclick={closeMenu} onkeydown={(e) => e.key === 'Escape' && closeMenu()} />

<div class="grid">
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
          No starred photos. Star one in the viewer, or in Picasa.
        {:else if library.info.view === 'search'}
          No photos match “{library.info.searchQuery}”
        {:else if library.info.view === 'album'}
          “{library.albumName(library.info.album)}” is empty. Right-click a photo to add it.
        {:else if library.info.view === 'person'}
          No photos of {library.personName(library.info.person)}.
        {:else if library.info.view === 'duplicates'}
          No duplicates. Every photo in the library is the only copy of itself.
        {:else if library.info.view === 'tag'}
          No photos tagged “{library.info.tag}”.
        {:else}
          No photos yet. Add a folder to get started.
        {/if}
      </p>
    {/if}
    <div class="canvas" style:height="{total}px">
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
                selected={library.isSelectedTile(offset, entry?.id)}
                dimmed={!!entry && !library.isOnline(entry.folderId)}
                onselect={(e) => tileClick(e, offset)}
                onopen={() => onopen(offset)}
                onmenu={(e) => tileMenu(e, offset)}
              />
            {/each}
          </div>
        {/if}
      {/each}
    </div>
  </div>
  {#if scrubbable}
    <Timeline {marks} {total} viewport={height} {scrollTop} onscrub={(top) => (viewport.scrollTop = top)} />
  {/if}
</div>

{#if menu}
  {@const albumId = library.info.view === 'album' ? library.info.album : null}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style:left="{menu.x}px"
    style:top="{menu.y}px"
  >
    {#if count === 1}
      <button role="menuitem" onclick={() => withSelection((ids) => api.revealInFileManager(ids[0]))}>
        Reveal in file manager
      </button>
    {/if}
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, true))}>Star {subject}</button>
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, false))}>Unstar {subject}</button>
    {#if albumId !== null}
      <button role="menuitem" onclick={() => withSelection((ids) => library.removeFromAlbum(albumId, ids))}>
        Remove {subject} from “{library.albumName(albumId)}”
      </button>
    {/if}
    <div class="heading">Add to album</div>
    {#each library.albums as album (album.id)}
      <!-- Adding is idempotent, so the album the photos are already in is not filtered out
           here: the grid rows do not know their memberships, and asking per photo for a
           menu would be a round trip for nothing. -->
      <button role="menuitem" class="album" onclick={() => withSelection((ids) => library.addToAlbum(album.id, ids))}>
        {album.name}
      </button>
    {:else}
      <div class="none">No albums yet — create one in the sidebar.</div>
    {/each}
  </div>
{/if}

<style>
  .grid { display: flex; height: 100%; background: var(--surface); }
  .viewport { position: relative; flex: 1; min-width: 0; height: 100%; overflow-y: auto; outline: none; }
  /* The grid is in the tab order (tabindex="0"), so tabbing into it must show something -
     with nothing selected there is no tile ring to stand in for it. Drawn inside, like the
     tile's ring and for the same reason: the viewport scrolls a row flush to its own top
     edge, which clips anything outside the box. */
  .viewport:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .canvas { position: relative; }
  .header, .row { position: absolute; left: 0; right: 0; }
  /* 32px is layout.ts's HEADER: every row below is placed by it, so the type fits the box
     rather than the box growing to the type. */
  .header { display: flex; align-items: baseline; gap: var(--s-3); height: 32px; padding: 7px var(--s-2) 0; }
  /* min-width: 0 and overflow: hidden so a folder name wider than the grid ellipsises
     instead of forcing a horizontal scrollbar, which would change the viewport's measured
     clientHeight. max-width caps the name so a long one cannot squeeze .path (flex: 1 1 auto,
     not 1 1 0, so the path keeps its own room rather than starting from nothing) down to
     zero. overflow: hidden moves a flex item's baseline to its own bottom edge, not its
     text's; giving .name and .path the same line-height puts both bottom edges - and so both
     baselines - on the same line, which plain `align-items: baseline` alone no longer does
     once either child clips its own overflow. */
  .header .name { flex: 0 1 auto; max-width: 70%; min-width: 0; overflow: hidden; font-size: var(--t-4); font-weight: 600; line-height: 20px; white-space: nowrap; text-overflow: ellipsis; }
  .header .path { flex: 1 1 auto; min-width: 0; color: var(--text-dim); font-size: var(--t-1); line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; color: var(--text-dim); margin: 0; }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button { padding: 6px 10px; border: 0; border-radius: var(--r-2); background: none; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--hover); }
  .menu .album { padding-left: 18px; }
  .menu .heading {
    margin-top: var(--s-1);
    padding: 6px 10px 2px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
    border-top: 1px solid var(--line);
  }
  .menu .none { padding: 4px 18px 6px; color: var(--text-dim); font-size: var(--t-2); }
</style>
