<script lang="ts">
  import { untrack } from 'svelte';
  import { api, errorMessage, mediaUrl, type ViewerItem } from '../lib/api';
  import { library } from '../lib/library.svelte';

  let { offset, onclose }: { offset: number; onclose: (offset: number) => void } = $props();

  const PRELOAD_RADIUS = 2;
  let current = $state(untrack(() => offset));
  let item = $state<ViewerItem | null>(null);
  let fullSrc = $state<string | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const at = current;
    let cancelled = false;
    item = null;
    fullSrc = null;
    error = null;
    (async () => {
      await library.ensure(at, at + 1);
      const entry = library.entry(at);
      if (cancelled) return;
      if (!entry) {
        error = 'This photo is no longer available.';
        return;
      }
      const it = await api.viewerItem(entry.id);
      if (cancelled) return;
      item = it;
      if (it.thumbState === 'failed') {
        error = it.thumbError ?? "This photo can't be shown.";
        return;
      }
      const url = mediaUrl(`image/${it.id}`);
      const full = new Image();
      full.src = url;
      full.decode().then(
        () => {
          if (!cancelled) fullSrc = url;
        },
        () => {},
      );
      const near = await api.neighbours(it.id, PRELOAD_RADIUS);
      if (cancelled) return;
      for (const id of near) new Image().src = mediaUrl(`image/${id}`);
    })().catch((e) => {
      if (!cancelled) error = errorMessage(e);
    });
    return () => {
      cancelled = true;
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose(current);
      return;
    }
    const last = library.info.len - 1;
    if (last < 0) return;
    const next =
      e.key === 'ArrowLeft' ? Math.max(0, current - 1)
      : e.key === 'ArrowRight' ? Math.min(last, current + 1)
      : e.key === 'Home' ? 0
      : e.key === 'End' ? last
      : null;
    if (next !== null) {
      e.preventDefault();
      current = next;
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="viewer" role="dialog" aria-modal="true" aria-label="Photo viewer">
  {#if error}
    <p class="error">{error}</p>
  {:else if item}
    <img class="preview" src={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)} alt="" class:hidden={!!fullSrc} />
    {#if fullSrc}
      <img class="full" src={fullSrc} alt={item.fileName} />
    {/if}
  {/if}
  <div class="caption">{item?.fileName ?? ''} · {current + 1} / {library.info.len}</div>
  <button class="close" onclick={() => onclose(current)} aria-label="Close viewer">✕</button>
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #000; }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; image-orientation: from-image; }
  .hidden { visibility: hidden; }
  .caption { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); padding: 4px 10px; background: #0009; border-radius: 4px; color: var(--muted); font-size: 12px; }
  .close { position: absolute; top: 12px; right: 12px; width: 32px; height: 32px; border: 0; border-radius: 50%; background: #0009; cursor: pointer; }
  .error { color: var(--muted); }
</style>
