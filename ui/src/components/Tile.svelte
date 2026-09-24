<script lang="ts">
  import { untrack } from 'svelte';
  import { mediaUrl, type GridEntry } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { createThumbRequest } from '../lib/thumb-request.svelte';
  import { createTileRetry } from '../lib/tile-retry.svelte';
  import { copiesMarkShown } from '../lib/copies';
  import Icon from './Icon.svelte';

  let {
    entry,
    selected,
    dimmed,
    onselect,
    onopen,
    onmenu,
    tile,
  }: {
    entry: GridEntry | undefined;
    selected: boolean;
    dimmed: boolean;
    /** The click that selects. The event travels because the grid, not the tile, decides
     *  what Ctrl and Shift mean. */
    onselect: (e: MouseEvent) => void;
    onopen: () => void;
    /** Right-click. The grid owns the menu, since it knows the view and the albums. */
    onmenu: (e: MouseEvent) => void;
    /** The tile's side in pixels, chosen by the user. The grid passes it because the same
     *  number decides the row layout there: a tile that sized itself would be free to
     *  disagree with the box the row reserved for it. */
    tile: number;
  } = $props();

  const key = $derived(entry ? `thumb/${entry.id}/grid/${entry.thumbKey}` : '');
  const request = createThumbRequest();
  const retry = createTileRetry();
  const src = $derived(
    request.requested
      ? mediaUrl(request.requested) + (retry.attempt ? `?retry=${retry.attempt}` : '')
      : undefined,
  );

  // Tiles are keyed by grid offset, not photo id, so the entry changes under a live tile
  // without it unmounting - every frame of a fast scroll. `request` is what decides when
  // that becomes an actual round trip; see `createThumbRequest` for why asking for each one
  // starves the tiles that finally stop on screen. The cancel covers both a key that moves
  // on again before it settles and the tile unmounting.
  $effect(() => {
    const assigned = key;
    if (assigned) request.show(assigned);
    return () => request.cancel();
  });

  // A different item or file version starts fresh. This tracks the photo actually
  // requested, not the one assigned: `status` describes whatever `src` currently points at,
  // and resetting it for a key the tile has not asked for yet would blank a loaded
  // thumbnail - including when a scroll comes straight back to the photo already showing,
  // where no new `src` is set and so no `onload` would ever arrive to clear it again.
  // A pending retry belongs to the old photo, so it is cancelled here too - `retry.reset`
  // itself covers that; see `createTileRetry`.
  $effect(() => {
    void request.requested;
    retry.reset();
    return () => retry.cancel();
  });

  // A tile can break for reasons that later go away: a thumbnail that was still queued
  // when the tile scrolled out answers 503, and both attempts can fall in that window - the
  // common case `createTileRetry`'s own quick retry already covers. This effect is a second,
  // independent path back to loading, for the terminal `'broken'` state only - `retry`
  // already keeps retrying on its own schedule while it's `'retrying'`, so there's nothing
  // for this to do there, and touching a loading or loaded tile would flicker it. Once
  // `retry` has given up (`TILE_BROKEN_RETRY_ATTEMPTS` exhausted), this is what still
  // brings a tile back once an unrelated library change happens to land - `key` doesn't
  // change when the thumbnail becomes ready, and the component isn't remounted.
  // `retry.status` is written here (via `reset`), so it is read through `untrack` — the
  // effect depends on `pageTick` alone and cannot re-trigger itself.
  $effect(() => {
    void library.pageTick;
    if (untrack(() => retry.status) === 'broken') {
      retry.reset();
    }
  });

  function onerror() {
    retry.failed();
  }
</script>

<button
  class="tile"
  class:selected
  class:dimmed
  style:width="{tile}px"
  style:height="{tile}px"
  tabindex="-1"
  onclick={onselect}
  ondblclick={onopen}
  oncontextmenu={onmenu}
>
  <!-- Mounted for every status, never removed for 'retrying' or 'broken': a background
       retry still needs a live <img> to actually reissue the request (`retry.attempt`
       changing `src`), and it stays invisible (opacity 0) behind the icon below until an
       `onload` promotes it. Before `createTileRetry` grew a slow background retry, a
       broken tile never had a request in flight to protect, and this same markup swapped
       the <img> out for the icon on 'broken'; once retries started firing behind that
       icon, each one's timer setting `status` back to `'loading'` (the bug `TileRetry`'s
       own doc on `failed` describes) was what this unmount-on-broken markup then showed -
       a blank <img> - until the next `onerror` swapped the icon back in. -->
  {#if entry && src}
    <img
      {src}
      alt=""
      draggable="false"
      decoding="async"
      class:loaded={retry.status === 'loaded'}
      onload={() => retry.loaded()}
      {onerror}
    />
  {/if}
  {#if retry.status === 'broken' || retry.status === 'retrying'}
    <span class="broken" title="This photo can't be shown"><Icon name="triangle-alert" size={28} /></span>
  {/if}
  {#if entry?.starred}
    <span class="star" aria-label="Starred"><Icon name="star" size={14} filled /></span>
  {/if}
  <!-- A mark, not a control: the tile is itself a button, which cannot hold another. The
       way to the copies is the menu's "Show duplicates", which this tells you is there. -->
  {#if entry && copiesMarkShown(entry.hasCopies, library.info.view)}
    <span class="copies" aria-label="Has copies" title="Has copies"><Icon name="copy" size={14} /></span>
  {/if}
</button>

<style>
  .tile {
    position: relative;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    /* What shows while the thumbnail loads, and behind a broken one. */
    background: var(--field);
    overflow: hidden;
    cursor: default;
  }
  /* Inside the box, not an outline around it: the grid scrolls a row flush to the top of
     its container on ArrowUp, and Recent's first row starts at 0, so anything outside the
     tile is clipped there. The thin surface-coloured line inside the accent keeps the ring
     legible over a photo of the accent's own blue. From the class and not from focus - a
     tile is tabindex="-1" and never script-focused, so it deliberately carries no
     `.focus-container` (tokens.css scopes the focus-ring suppression to that class): a
     tile relies on never being focused, not on a ring being hidden after the fact. */
  .tile.selected::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: inset 0 0 0 2px var(--accent), inset 0 0 0 3px var(--surface);
    pointer-events: none;
  }
  .tile.dimmed { opacity: 0.4; }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 120ms ease-out;
  }
  img.loaded { opacity: 1; }
  /* Absolute, not a flow sibling: the <img> stays mounted (invisible) behind it during
     'retrying' and 'broken' so a background retry can still fetch - see the template
     comment above the <img>. */
  .broken {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-dim);
  }
  /* The shadow keeps an amber star legible on a bright or amber photo. */
  .star {
    position: absolute;
    right: 5px;
    bottom: 5px;
    color: var(--star);
    filter: drop-shadow(0 0 2px var(--shadow-ink));
    pointer-events: none;
  }
  /* The star's opposite corner, so a photo can carry both. Drawn onto the photo, like a
     face box, so the same unthemed line colour, with the star's shadow to hold it on a
     white one. */
  .copies {
    position: absolute;
    left: 5px;
    bottom: 5px;
    color: var(--photo-line);
    filter: drop-shadow(0 0 2px var(--shadow-ink));
    pointer-events: none;
  }
</style>
