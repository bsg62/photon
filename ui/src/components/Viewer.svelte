<script lang="ts">
  import { untrack } from 'svelte';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { api, errorMessage, mediaUrl, type ViewerItem } from '../lib/api';
  import { formatCaption } from '../lib/caption';
  import { createCopyFeedback } from '../lib/copied.svelte';
  import { library } from '../lib/library.svelte';
  import { MAX_ZOOM, MIN_ZOOM, clampPan, clampZoom, closesViewer, positionInView, wheelStep } from '../lib/nav';

  let {
    offset,
    onclose,
    onlocate,
  }: {
    offset: number;
    onclose: (offset: number) => void;
    /** "Locate in photon": the viewer closes and the grid lands on this photo. */
    onlocate: (itemId: number) => void;
  } = $props();

  const PRELOAD_RADIUS = 2;
  let current = $state(untrack(() => offset));
  let item = $state<ViewerItem | null>(null);
  let fullSrc = $state<string | null>(null);
  let error = $state<string | null>(null);
  let zoom = $state(MIN_ZOOM);
  let pan = $state({ x: 0, y: 0 });
  let dragging = $state(false);
  let stage = $state<HTMLDivElement | null>(null);
  /** Deliberately not `$state`: nothing renders from a partial wheel total, and making it
   *  reactive would re-run effects on every wheel event of a flick. */
  let wheelTotal = 0;
  let dragFrom = { x: 0, y: 0, panX: 0, panY: 0 };

  /** Photos are numbered within their own folder, not across the library. In the All view
   *  that count matches what the file manager shows for that directory; in Starred or
   *  Search it is the folder's position among the current view's results instead, since
   *  those views only show a subset of the folder's photos. Recent has no folder runs to
   *  count within and is numbered flat — see `positionInView`. */
  const position = $derived(positionInView(library.info.view, library.info.sections, current, library.info.len));
  const caption = $derived(item ? formatCaption(item, position) : '');

  // Click-to-copy on the caption. The clipboard goes through the Tauri plugin rather than
  // `navigator.clipboard`, which needs a secure context and answers differently in the
  // three webviews photon ships in.
  const copy = createCopyFeedback(writeText);
  $effect(() => () => copy.dispose());

  function copyName() {
    if (item) copy.copy(item.fileName).catch(library.reportError);
  }

  let menu = $state<{ x: number; y: number } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  function oncontextmenu(e: MouseEvent) {
    e.preventDefault();
    if (item) menu = { x: e.clientX, y: e.clientY };
  }

  function closeMenu() {
    menu = null;
  }

  function locate() {
    if (!item) return;
    closeMenu();
    onlocate(item.id);
  }

  function reveal() {
    if (!item) return;
    closeMenu();
    api.revealInFileManager(item.id).catch(library.reportError);
  }

  function viewport(): { width: number; height: number } {
    return { width: stage?.clientWidth ?? 0, height: stage?.clientHeight ?? 0 };
  }

  function goto(next: number) {
    const last = library.info.len - 1;
    if (last < 0) return;
    current = Math.min(last, Math.max(0, next));
  }

  /** An offset the rebind below has already resolved, so the loader can tell "the same photo,
   *  renumbered" from "a different photo". Deliberately not `$state`: writing it must not
   *  wake anything, and it is always set immediately before the `current` that does. */
  let rebound: number | null = null;

  // `current` is an index into a grid that is rebuilt whole whenever anything changes, so it
  // stops meaning "the photo the user opened" the moment a scan indexes something ahead of
  // it: one photo copied into an earlier folder shifts every later offset by one, and the
  // viewer would go on showing the *next* photo under the same caption, silently. Clamping
  // alone only catches the case where the offset falls off the end.
  //
  // So the photo is re-found by id after every rebuild. Only when nothing is loaded yet —
  // the first paint, or after the photo has gone — is there an id to work from, and staying
  // in range is then all that can be done.
  $effect(() => {
    void library.info.version;
    const last = library.info.len - 1;
    const showing = untrack(() => item?.id);
    if (showing === undefined) {
      if (last >= 0 && untrack(() => current) > last) current = last;
      return;
    }
    void (async () => {
      const at = await api.gridOffsetOfItem(showing);
      // The user navigated while this was in flight; that move is the newer truth.
      if (untrack(() => item?.id) !== showing) return;
      if (at === null) {
        error = 'This photo is no longer available.';
        return;
      }
      if (at === untrack(() => current)) return;
      rebound = at;
      current = at;
    })();
  });

  $effect(() => {
    const at = current;
    // A renumbering, not a navigation: the photo on screen is already the right one, so
    // reloading it would blank it and throw away the zoom and pan for nothing.
    if (rebound === at) {
      rebound = null;
      return;
    }
    let cancelled = false;
    item = null;
    fullSrc = null;
    error = null;
    // Every photo opens fitted to the window: arriving at the next one already at 400% and
    // panned into a corner leaves you lost.
    zoom = MIN_ZOOM;
    pan = { x: 0, y: 0 };
    (async () => {
      // `untrack`, because `ensure` reads `library.info.len` and this call is still inside
      // the effect's tracked window. `refresh()` assigns a new `info` object on every
      // library-changed event, so without it any background scan finishing - or any single
      // watched file changing - re-runs this effect, blanking the photo on screen and
      // throwing away the zoom and pan the user set, although nothing about that photo
      // changed. The one dependency this effect wants is `current`.
      await untrack(() => library.ensure(at, at + 1));
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
    // An open menu takes Escape first, the way any menu does; the viewer is next.
    if (e.key === 'Escape' && menu) {
      e.preventDefault();
      closeMenu();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose(current);
      return;
    }
    // While the zoom slider has focus the arrow keys belong to it, which is how a range
    // input is expected to behave. Navigation stays available everywhere else.
    if (e.target instanceof HTMLInputElement) return;
    // Backspace closes as well, like the back button. Deliberately placed here: after the
    // input guard, so a text field gets its character deleted rather than the viewer
    // slammed shut; but before the empty-library check below, so it still closes when a
    // scan has emptied the grid underneath an open viewer — the case where the viewer
    // shows "This photo is no longer available" and Escape must still work.
    if (e.key === 'Backspace') {
      e.preventDefault();
      onclose(current);
      return;
    }
    const last = library.info.len - 1;
    if (last < 0) return;
    const next =
      e.key === 'ArrowLeft' ? current - 1
      : e.key === 'ArrowRight' ? current + 1
      : e.key === 'Home' ? 0
      : e.key === 'End' ? last
      : null;
    if (next !== null) {
      e.preventDefault();
      goto(next);
    }
  }

  /** The mouse's back button closes the viewer, like Escape, Backspace and the ✕.
   *
   *  On the window rather than the viewer element so a press anywhere counts, including on
   *  the zoom slider. `preventDefault` stops the webview treating it as history navigation;
   *  there is nowhere to go back to, but the press would otherwise be handled twice. The pan
   *  handler below is unaffected — it already ignores every button but the left one. */
  function onbackbutton(e: PointerEvent) {
    if (!closesViewer(e.button)) return;
    e.preventDefault();
    onclose(current);
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    const stepped = wheelStep(wheelTotal, e.deltaY);
    wheelTotal = stepped.accumulated;
    if (stepped.step !== 0) goto(current + stepped.step);
  }

  function onzoom(e: Event & { currentTarget: HTMLInputElement }) {
    zoom = clampZoom(Number(e.currentTarget.value));
    const { width, height } = viewport();
    // Zooming back out shrinks how far the photo may travel, so a pan that was legal at 4x
    // has to be pulled back in rather than left hanging off the edge.
    pan = clampPan(pan.x, pan.y, zoom, width, height);
  }

  function onpointerdown(e: PointerEvent) {
    // The zoom slider and the close button sit on the same surface: a press on either is
    // theirs, not the start of a pan.
    if ((e.target as HTMLElement).closest('.zoom, .close')) return;
    // Left button only. Without this every button panned, which is why the right button
    // looked like the pan control: the left one was being swallowed by the browser's native
    // image drag before the pointer stream could produce a move.
    if (e.button !== 0) return;
    if (zoom === MIN_ZOOM) return;
    dragging = true;
    dragFrom = { x: e.clientX, y: e.clientY, panX: pan.x, panY: pan.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    const { width, height } = viewport();
    pan = clampPan(
      dragFrom.panX + (e.clientX - dragFrom.x),
      dragFrom.panY + (e.clientY - dragFrom.y),
      zoom,
      width,
      height,
    );
  }

  function onpointerup(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
  }
</script>

<svelte:window {onkeydown} onpointerdown={onbackbutton} onclick={closeMenu} />

<!-- The pan handlers live here rather than on the stage below: this element already carries
     a role, and dragging anywhere in the viewer is easier to hit than the photo alone. -->
<div
  class="viewer"
  role="dialog"
  aria-modal="true"
  aria-label="Photo viewer"
  tabindex="-1"
  {onwheel}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  {oncontextmenu}
>
  {#if error}
    <p class="error">{error}</p>
  {:else if item}
    <div
      class="stage"
      class:grabbable={zoom > MIN_ZOOM}
      class:grabbing={dragging}
      style="transform: translate({pan.x}px, {pan.y}px) scale({zoom})"
      bind:this={stage}
    >
      <img class="preview" src={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)} alt="" draggable="false" class:hidden={!!fullSrc} />
      {#if fullSrc}
        <img class="full" src={fullSrc} alt={item.fileName} draggable="false" />
      {/if}
    </div>
  {/if}
  <!-- A button, because a click copies the file name. The confirmation replaces the whole
       line for a moment rather than appending to it, so the line does not jump in width. -->
  <button class="caption" onclick={copyName} disabled={!item} title="Click to copy the file name">
    {copy.copied ? 'Copied' : caption}
  </button>
  {#if menu && item}
    <div
      class="menu"
      role="menu"
      tabindex="-1"
      bind:this={menuEl}
      style:left="{menu.x}px"
      style:top="{menu.y}px"
    >
      <button role="menuitem" onclick={locate}>Locate in photon</button>
      <button role="menuitem" onclick={reveal}>Reveal in file manager</button>
    </div>
  {/if}
  <div class="zoom">
    <input
      type="range"
      min={MIN_ZOOM}
      max={MAX_ZOOM}
      step="0.05"
      value={zoom}
      oninput={onzoom}
      aria-label="Zoom"
    />
    <span class="level">{Math.round(zoom * 100)}%</span>
  </div>
  <button class="close" onclick={() => onclose(current)} aria-label="Close viewer">✕</button>
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #000; overflow: hidden; }
  .stage { position: absolute; inset: 0; transform-origin: center; will-change: transform; }
  .grabbable { cursor: grab; }
  .grabbing { cursor: grabbing; }
  /* `draggable="false"` covers the drag itself; these stop WebKit — which is the webview on
     both Linux and macOS — from starting its own image drag or selecting the image instead
     of panning. */
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; image-orientation: from-image; user-select: none; -webkit-user-drag: none; }
  .hidden { visibility: hidden; }
  .caption { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); padding: 4px 10px; border: 0; background: #0009; border-radius: 4px; color: var(--muted); font-size: 12px; white-space: nowrap; cursor: pointer; }
  .caption:hover:not(:disabled) { color: var(--text); }
  .caption:disabled { cursor: default; }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    background: var(--panel-2);
    border-radius: 6px;
    box-shadow: 0 6px 24px #0008;
  }
  .menu button { padding: 6px 10px; border: 0; background: none; text-align: left; cursor: pointer; border-radius: 4px; }
  .menu button:hover { background: #ffffff14; }
  .zoom { position: absolute; bottom: 12px; right: 12px; display: flex; align-items: center; gap: 8px; padding: 4px 10px; background: #0009; border-radius: 4px; }
  .zoom input { width: 120px; }
  .level { color: var(--muted); font-size: 12px; min-width: 38px; text-align: right; }
  .close { position: absolute; top: 12px; right: 12px; width: 32px; height: 32px; border: 0; border-radius: 50%; background: #0009; cursor: pointer; }
  .error { color: var(--muted); }
</style>
