<script lang="ts">
  import { untrack } from 'svelte';
  import { mediaUrl, type GridEntry } from '../lib/api';
  import { TILE } from '../lib/layout';
  import { library } from '../lib/library.svelte';

  let {
    entry,
    selected,
    dimmed,
    onselect,
    onopen,
  }: {
    entry: GridEntry | undefined;
    selected: boolean;
    dimmed: boolean;
    onselect: () => void;
    onopen: () => void;
  } = $props();

  const RETRY_MS = 2000;
  let status = $state<'loading' | 'loaded' | 'broken'>('loading');
  let attempt = $state(0);
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  const key = $derived(entry ? `${entry.id}/${entry.thumbKey}` : '');
  const src = $derived(
    entry ? mediaUrl(`thumb/${entry.id}/grid/${entry.thumbKey}`) + (attempt ? `?retry=${attempt}` : '') : undefined,
  );

  // A different item or file version starts fresh. Tiles are keyed by grid offset, not
  // photo id, so the entry can change under a live tile (e.g. a scan renumbers the
  // grid) without unmounting: a pending retry from the old photo must not fire against
  // the new one, so cancel it whenever `key` changes or the tile unmounts.
  $effect(() => {
    void key;
    status = 'loading';
    attempt = 0;
    return () => {
      clearTimeout(retryTimer);
      retryTimer = undefined;
    };
  });

  // A tile can break for reasons that later go away: a thumbnail that was still queued
  // when the tile scrolled out answers 503, and both attempts can fall in that window.
  // Nothing else resets it (`key` doesn't change when the thumbnail becomes ready, and
  // the component isn't remounted), so retry whenever the library moves on. Only a broken
  // tile is touched: resetting a loading or loaded one would flicker.
  // `status` is written here, so it is read through `untrack` — the effect depends on
  // `pageTick` alone and cannot re-trigger itself.
  $effect(() => {
    void library.pageTick;
    if (untrack(() => status) === 'broken') {
      status = 'loading';
      attempt = 0;
    }
  });

  function onerror() {
    if (attempt === 0) retryTimer = setTimeout(() => (attempt = 1), RETRY_MS);
    else status = 'broken';
  }
</script>

<button
  class="tile"
  class:selected
  class:dimmed
  style:width="{TILE}px"
  style:height="{TILE}px"
  tabindex="-1"
  onclick={onselect}
  ondblclick={onopen}
>
  {#if entry && status !== 'broken'}
    <img {src} alt="" draggable="false" decoding="async" class:loaded={status === 'loaded'} onload={() => (status = 'loaded')} {onerror} />
  {:else if status === 'broken'}
    <span class="broken" title="This photo can't be shown">⚠</span>
  {/if}
</button>

<style>
  .tile {
    position: relative;
    flex: none;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 4px;
    background: var(--panel-2);
    overflow: hidden;
    cursor: default;
  }
  .tile.selected { border-color: var(--accent); }
  .tile.dimmed { opacity: 0.4; }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 120ms ease-out;
  }
  img.loaded { opacity: 1; }
  .broken { display: grid; place-items: center; height: 100%; color: var(--muted); font-size: 28px; }
</style>
